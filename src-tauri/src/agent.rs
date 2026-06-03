mod interceptor;
mod stream;
mod tools;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use interceptor::TagInterceptor;
use stream::{stream_turn_claude_api, stream_turn_local_claude, stream_turn_openai_compat};
use tools::{action_to_input, api_system_prompt, anthropic_tools_schema, execute_action, openai_tools_schema, tool_system_prompt};

// ─── Shared State ─────────────────────────────────────────────────────────────

pub(crate) struct PendingWriteEntry {
    pub path: PathBuf,
    pub content: String,
}

pub struct PendingWriteState(pub Arc<Mutex<HashMap<String, PendingWriteEntry>>>);

impl PendingWriteState {
    pub fn new() -> Self {
        PendingWriteState(Arc::new(Mutex::new(HashMap::new())))
    }
}

// Each write action creates a oneshot channel. write_file_tool awaits the
// receiver; confirm_write fires the sender. This blocks the agent loop until
// the user approves or rejects — enforcing one change at a time.
pub struct ApprovalWaiter(
    pub Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
);

impl ApprovalWaiter {
    pub fn new() -> Self {
        ApprovalWaiter(Arc::new(Mutex::new(HashMap::new())))
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// ─── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn confirm_write(
    state: tauri::State<'_, PendingWriteState>,
    waiter: tauri::State<'_, ApprovalWaiter>,
    tool_use_id: String,
    approved: bool,
) -> Result<(), String> {
    let entry = { state.0.lock().unwrap().remove(&tool_use_id) };
    if let Some(e) = entry {
        if approved {
            if let Some(parent) = e.path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&e.path, &e.content).map_err(|e| e.to_string())?;
        }
    }
    // Unblock the waiting execute_action
    if let Some(tx) = waiter.0.lock().unwrap().remove(&tool_use_id) {
        let _ = tx.send(approved);
    }
    Ok(())
}

#[tauri::command]
pub async fn stream_message(
    app: AppHandle,
    api_key: String,
    messages: Vec<Message>,
    model: String,
    system: Option<String>,
    project_path: Option<String>,
    db_connection_string: Option<String>,
) -> Result<(), String> {
    let config = crate::settings::read_config(&app);
    let use_local_model = config["use_local_model"].as_bool().unwrap_or(false);
    let use_local_claude = config["use_local_claude"].as_bool().unwrap_or(false);

    let has_files = project_path.is_some();
    let has_db    = db_connection_string.is_some();

    // System prompts: API paths use a simple prose prompt (tools sent natively);
    // local Claude CLI keeps the tag-based prompt.
    let cli_system = {
        let tag_prompt = tool_system_prompt(has_files, has_db);
        match &system {
            Some(s) if !s.is_empty() => format!("{}\n\n{}", tag_prompt, s),
            _ => tag_prompt,
        }
    };
    let api_sys = api_system_prompt(system.as_deref());

    let (local_url, local_key, local_model_name) = if use_local_model {
        let url  = config["local_model_url"].as_str().unwrap_or("http://localhost:11434/v1").to_string();
        let key  = config["local_model_api_key"].as_str().map(|s| s.to_string());
        let name = config["local_model_name"].as_str().unwrap_or("llama3").to_string();
        (url, key, name)
    } else {
        (String::new(), None, String::new())
    };

    let root    = project_path.as_deref().unwrap_or("").to_string();
    let db_conn = db_connection_string.clone();
    let pending = app.state::<PendingWriteState>().0.clone();
    let waiter  = app.state::<ApprovalWaiter>().0.clone();
    let client  = Client::new();

    // Tool schemas for native tool-calling paths.
    let oai_tools = openai_tools_schema(has_files, has_db);
    let ant_tools = anthropic_tools_schema(has_files, has_db);

    let mut conv: Vec<serde_json::Value> = messages.iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();

    let mut first_turn = true;
    loop {
        if !first_turn { let _ = app.emit("claude:new_turn", ()); }
        first_turn = false;

        let (visible, calls) = if use_local_model {
            // Prepend system message for OpenAI-compat format.
            let mut msgs = vec![json!({"role": "system", "content": api_sys})];
            msgs.extend_from_slice(&conv);
            stream_turn_openai_compat(&app, &client, &local_url, local_key.as_deref(), &local_model_name, &msgs, &oai_tools).await?
        } else if use_local_claude {
            let mut interceptor = TagInterceptor::new();
            stream_turn_local_claude(&app, &conv, &model, &cli_system, &mut interceptor).await?
        } else {
            stream_turn_claude_api(&app, &client, &api_key, &model, &api_sys, &conv, &ant_tools).await?
        };

        if calls.is_empty() {
            let _ = app.emit("claude:done", ());
            return Ok(());
        }

        // Push assistant message in provider-specific format so the model
        // receives proper context on the next turn.
        if use_local_model {
            let tool_calls_json: Vec<serde_json::Value> = calls.iter().map(|c| json!({
                "id": c.id,
                "type": "function",
                "function": {
                    "name": format!("locus_{}", c.action.name),
                    "arguments": serde_json::to_string(&action_to_input(&c.action)).unwrap_or_default()
                }
            })).collect();
            conv.push(json!({
                "role": "assistant",
                "content": if visible.is_empty() { serde_json::Value::Null } else { json!(visible) },
                "tool_calls": tool_calls_json
            }));
        } else if use_local_claude {
            conv.push(json!({"role": "assistant", "content": visible}));
        } else {
            // Claude API: content array with text + tool_use blocks.
            let mut content: Vec<serde_json::Value> = Vec::new();
            if !visible.is_empty() { content.push(json!({"type": "text", "text": visible})); }
            for c in &calls {
                content.push(json!({
                    "type": "tool_use",
                    "id": c.id,
                    "name": format!("locus_{}", c.action.name),
                    "input": action_to_input(&c.action)
                }));
            }
            conv.push(json!({"role": "assistant", "content": content}));
        }

        // Execute each tool call and collect results.
        let mut results: Vec<String> = Vec::new();
        for call in &calls {
            let _ = app.emit("claude:tool_use", call.action.to_tool_use_event());
            let result = execute_action(
                &call.action, &root, db_conn.as_deref(), &app,
                pending.clone(), waiter.clone(),
            ).await;
            if result == "__REJECTED__" {
                let _ = app.emit("claude:done", ());
                return Ok(());
            }
            let label = match call.action.name.as_str() {
                "read"   => format!("Contents of {}", call.action.content.trim().lines().next().unwrap_or("file")),
                "list"   => "Project files".to_string(),
                "search" => format!("Search results for \"{}\"", call.action.content.trim()),
                "schema" => "Database schema".to_string(),
                "query"  => "Query results".to_string(),
                other    => format!("{} result", other),
            };
            results.push(format!("=== {} ===\n{}", label, result));
        }

        // Inject tool results in provider-specific format.
        if use_local_model {
            // One `tool` role message per result.
            for (call, result) in calls.iter().zip(&results) {
                conv.push(json!({"role": "tool", "tool_call_id": call.id, "content": result}));
            }
        } else if use_local_claude {
            conv.push(json!({"role": "user", "content": results.join("\n\n")}));
        } else {
            // Claude API: single user message with tool_result blocks.
            let tool_results: Vec<serde_json::Value> = calls.iter().zip(&results)
                .map(|(c, r)| json!({"type": "tool_result", "tool_use_id": c.id, "content": r}))
                .collect();
            conv.push(json!({"role": "user", "content": tool_results}));
        }
    }
}

#[tauri::command]
pub async fn generate_title(
    app: AppHandle,
    api_key: String,
    user_message: String,
    assistant_message: String,
) -> Result<String, String> {
    if assistant_message.is_empty() { return Err("empty assistant message".to_string()); }

    let config = crate::settings::read_config(&app);
    let use_local_model = config["use_local_model"].as_bool().unwrap_or(false);
    let use_local_claude = config["use_local_claude"].as_bool().unwrap_or(false);

    let preview = &assistant_message[..assistant_message.len().min(500)];
    let prompt = format!(
        "Generate a concise 3-5 word title for this conversation. Respond with ONLY the title, no punctuation, no quotes.\n\nUser: {}\n\nAssistant: {}",
        user_message, preview
    );

    if use_local_model {
        let base_url = config["local_model_url"].as_str().unwrap_or("http://localhost:11434/v1").to_string();
        let local_key = config["local_model_api_key"].as_str().map(|s| s.to_string());
        let local_model = config["local_model_name"].as_str().unwrap_or("llama3").to_string();
        let client = Client::new();
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let body = json!({ "model": local_model, "messages": [{"role": "user", "content": prompt}], "max_tokens": 20, "stream": false });
        let mut req = client.post(&url).header("content-type", "application/json");
        if let Some(k) = &local_key { if !k.is_empty() { req = req.header("Authorization", format!("Bearer {}", k)); } }
        let resp = req.json(&body).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() { return Err(format!("error {}", resp.status())); }
        let j: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let t = j["choices"][0]["message"]["content"].as_str().unwrap_or("").trim().to_string();
        return if t.is_empty() { Err("empty title".to_string()) } else { Ok(t) };
    }

    if use_local_claude {
        let output = tokio::process::Command::new("claude")
            .arg("--print").arg("--output-format").arg("text").arg(&prompt)
            .output().await.map_err(|e| e.to_string())?;
        let t = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return if t.is_empty() { Err("empty title".to_string()) } else { Ok(t) };
    }

    let client = Client::new();
    let body = json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 20,
        "messages": [{"role": "user", "content": prompt}],
    });
    let resp = client.post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() { return Err(format!("API error {}", resp.status())); }
    let j: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let t = j["content"][0]["text"].as_str().unwrap_or("").trim().to_string();
    if t.is_empty() { return Err("empty title".to_string()); }
    Ok(t)
}

#[tauri::command]
pub async fn pick_folder() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Select Project Folder")
        .pick_folder()
        .await
        .map(|f| f.path().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn fetch_local_models(base_url: String) -> Result<Vec<String>, String> {
    let client = Client::new();
    let base = base_url.trim_end_matches('/').to_string();

    if let Ok(resp) = client.get(format!("{}/models", base)).header("content-type", "application/json").send().await {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                let names: Vec<String> = json["data"].as_array().unwrap_or(&vec![])
                    .iter().filter_map(|m| m["id"].as_str().map(|s| s.to_string())).collect();
                if !names.is_empty() { return Ok(names); }
            }
        }
    }

    let ollama_base = base.trim_end_matches("/v1").trim_end_matches("/V1").to_string();
    if let Ok(resp) = client.get(format!("{}/api/tags", ollama_base)).send().await {
        if resp.status().is_success() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                let names: Vec<String> = json["models"].as_array().unwrap_or(&vec![])
                    .iter().filter_map(|m| m["name"].as_str().map(|s| s.to_string())).collect();
                if !names.is_empty() { return Ok(names); }
            }
        }
    }

    Err("No models found. Make sure the server is running and the URL is correct.".to_string())
}
