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
use tools::{execute_action, tool_system_prompt};

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

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// ─── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn confirm_write(
    state: tauri::State<'_, PendingWriteState>,
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

    let tool_prompt = tool_system_prompt(project_path.is_some(), db_connection_string.is_some());
    let effective_system = match &system {
        Some(s) if !s.is_empty() => format!("{}\n\n{}", tool_prompt, s),
        _ => tool_prompt,
    };

    let (local_url, local_key, local_model_name) = if use_local_model {
        (
            config["local_model_url"].as_str().unwrap_or("http://localhost:11434/v1").to_string(),
            config["local_model_api_key"].as_str().map(|s| s.to_string()),
            config["local_model_name"].as_str().unwrap_or("llama3").to_string(),
        )
    } else {
        (String::new(), None, String::new())
    };

    let root = project_path.as_deref().unwrap_or("").to_string();
    let db_conn = db_connection_string.clone();
    let pending = app.state::<PendingWriteState>().0.clone();
    let client = Client::new();

    let mut conv: Vec<serde_json::Value> = messages.iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();

    loop {
        let mut interceptor = TagInterceptor::new();

        let visible = if use_local_model {
            let mut msgs = vec![json!({"role": "system", "content": effective_system})];
            msgs.extend_from_slice(&conv);
            stream_turn_openai_compat(&app, &client, &local_url, local_key.as_deref(), &local_model_name, &msgs, &mut interceptor).await?
        } else if use_local_claude {
            stream_turn_local_claude(&app, &conv, &model, &effective_system, &mut interceptor).await?
        } else {
            stream_turn_claude_api(&app, &client, &api_key, &model, &effective_system, &conv, &mut interceptor).await?
        };

        let actions = interceptor.take_actions();

        if actions.is_empty() {
            let _ = app.emit("claude:done", ());
            return Ok(());
        }

        conv.push(json!({"role": "assistant", "content": visible}));

        let mut results: Vec<String> = Vec::new();
        for action in &actions {
            let _ = app.emit("claude:tool_use", action.to_tool_use_event());
            let result = execute_action(action, &root, db_conn.as_deref(), &app, pending.clone()).await;
            results.push(format!("[locus:{}]\n{}", action.name, result));
        }
        conv.push(json!({"role": "user", "content": results.join("\n\n")}));
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
