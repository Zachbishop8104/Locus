mod interceptor;
mod stream;
mod tools;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

use interceptor::TagInterceptor;
use stream::{cli_command, cli_auth_hint, cli_launch_error, stream_turn_claude_api, stream_turn_claude_cli, stream_turn_openai_compat, CANCELLED};
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

// Bumped by every new stream and by cancel_stream. A running loop holds the
// value it started with and stops as soon as the counter moves on.
pub struct StreamGeneration(pub Arc<AtomicU64>);

impl StreamGeneration {
    pub fn new() -> Self {
        StreamGeneration(Arc::new(AtomicU64::new(0)))
    }
}

// The permission mode can change while a response is running (e.g. switching
// to auto-accept after the first edit), so tool execution reads it live.
pub struct LiveMode(pub Arc<AtomicU8>);

impl LiveMode {
    pub fn new() -> Self {
        LiveMode(Arc::new(AtomicU8::new(Mode::Ask as u8)))
    }
}

#[derive(Clone)]
pub(crate) struct CancelToken {
    generation: Arc<AtomicU64>,
    mine: u64,
}

impl CancelToken {
    pub fn is_cancelled(&self) -> bool {
        self.generation.load(Ordering::SeqCst) != self.mine
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provider {
    ClaudeApi,
    ClaudeCli,
    Local,
}

impl Provider {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "claude_api" => Ok(Provider::ClaudeApi),
            "claude_cli" => Ok(Provider::ClaudeCli),
            "local"      => Ok(Provider::Local),
            other        => Err(format!("Unknown provider: {}", other)),
        }
    }
}

/// Permission modes, matching Claude Code: ask before each edit, apply edits
/// without asking, or plan (read-only until the user approves a plan).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Mode {
    Ask = 0,
    AcceptEdits = 1,
    Plan = 2,
}

impl Mode {
    fn parse(s: &str) -> Self {
        match s {
            "edits" => Mode::AcceptEdits,
            "plan"  => Mode::Plan,
            _       => Mode::Ask,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Mode::AcceptEdits,
            2 => Mode::Plan,
            _ => Mode::Ask,
        }
    }

    pub fn allows_write(self) -> bool {
        self != Mode::Plan
    }
}

struct LocalEndpoint {
    url: String,
    key: Option<String>,
}

fn local_endpoint(app: &AppHandle) -> LocalEndpoint {
    let config = crate::settings::read_config(app);
    LocalEndpoint {
        url: config["local_model_url"].as_str().unwrap_or("http://localhost:11434/v1").to_string(),
        key: config["local_model_api_key"].as_str().map(|s| s.to_string()),
    }
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
pub fn set_mode(live: tauri::State<'_, LiveMode>, mode: String) {
    live.0.store(Mode::parse(&mode) as u8, Ordering::SeqCst);
}

/// Stop button: ends the running agent loop and rejects any pending edit.
#[tauri::command]
pub fn cancel_stream(
    generation: tauri::State<'_, StreamGeneration>,
    state: tauri::State<'_, PendingWriteState>,
    waiter: tauri::State<'_, ApprovalWaiter>,
) {
    generation.0.fetch_add(1, Ordering::SeqCst);
    state.0.lock().unwrap().clear();
    for (_, tx) in waiter.0.lock().unwrap().drain() {
        let _ = tx.send(false);
    }
}

#[tauri::command]
pub async fn stream_message(
    app: AppHandle,
    provider: String,
    api_key: String,
    messages: Vec<Message>,
    model: String,
    effort: Option<String>,
    mode: String,
    system: Option<String>,
    project_path: Option<String>,
    db_connection_string: Option<String>,
) -> Result<(), String> {
    let provider = Provider::parse(&provider)?;
    let mode = Mode::parse(&mode);
    let live_mode = app.state::<LiveMode>().0.clone();
    live_mode.store(mode as u8, Ordering::SeqCst);
    let effort = effort.filter(|e| !e.is_empty());

    let generation = app.state::<StreamGeneration>().0.clone();
    let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let cancel = CancelToken { generation, mine };

    let has_files = project_path.is_some();
    let has_db    = db_connection_string.is_some();
    let allow_write = mode.allows_write();

    let cli_system = {
        let tag_prompt = tool_system_prompt(has_files, has_db, mode);
        match &system {
            Some(s) if !s.is_empty() => format!("{}\n\n{}", tag_prompt, s),
            _ => tag_prompt,
        }
    };
    let api_sys = api_system_prompt(system.as_deref(), mode);
    let local = local_endpoint(&app);

    let root    = project_path.as_deref().unwrap_or("").to_string();
    let db_conn = db_connection_string.clone();
    let pending = app.state::<PendingWriteState>().0.clone();
    let waiter  = app.state::<ApprovalWaiter>().0.clone();
    let client  = Client::new();

    let oai_tools = openai_tools_schema(has_files, has_db, allow_write);
    let ant_tools = anthropic_tools_schema(has_files, has_db, allow_write);

    let mut conv: Vec<serde_json::Value> = messages.iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();

    let mut first_turn = true;
    loop {
        if cancel.is_cancelled() { return Ok(()); }
        if !first_turn { let _ = app.emit("claude:new_turn", ()); }
        first_turn = false;

        let turn = match provider {
            Provider::Local => {
                let mut msgs = vec![json!({"role": "system", "content": api_sys})];
                msgs.extend_from_slice(&conv);
                stream_turn_openai_compat(&app, &client, &local.url, local.key.as_deref(), &model, &msgs, &oai_tools, &cancel).await
            }
            Provider::ClaudeCli => {
                let mut interceptor = TagInterceptor::new();
                stream_turn_claude_cli(&app, &conv, &model, effort.as_deref(), &cli_system, &mut interceptor, &cancel).await
            }
            Provider::ClaudeApi => {
                stream_turn_claude_api(&app, &client, &api_key, &model, effort.as_deref(), &api_sys, &conv, &ant_tools, &cancel).await
            }
        };
        let turn = match turn {
            Ok(t) => t,
            Err(e) if e == CANCELLED => return Ok(()),
            Err(e) => return Err(e),
        };

        if turn.calls.is_empty() {
            let _ = app.emit("claude:done", ());
            return Ok(());
        }

        // Push the assistant message in provider-specific format so the model
        // receives proper context on the next turn.
        match provider {
            Provider::Local => {
                let tool_calls_json: Vec<serde_json::Value> = turn.calls.iter().map(|c| json!({
                    "id": c.id,
                    "type": "function",
                    "function": {
                        "name": format!("locus_{}", c.action.name),
                        "arguments": serde_json::to_string(&action_to_input(&c.action)).unwrap_or_default()
                    }
                })).collect();
                conv.push(json!({
                    "role": "assistant",
                    "content": if turn.visible.is_empty() { serde_json::Value::Null } else { json!(turn.visible) },
                    "tool_calls": tool_calls_json
                }));
            }
            Provider::ClaudeCli => conv.push(json!({"role": "assistant", "content": turn.visible})),
            // Verbatim replay: thinking, text, and tool_use blocks exactly as received.
            Provider::ClaudeApi => conv.push(json!({"role": "assistant", "content": turn.content})),
        }

        // Execute each tool call and collect results.
        let mut results: Vec<String> = Vec::new();
        for call in &turn.calls {
            let _ = app.emit("claude:tool_use", call.action.to_tool_use_event());
            let result = execute_action(
                &call.action, &root, db_conn.as_deref(), &app,
                pending.clone(), waiter.clone(), Mode::from_u8(live_mode.load(Ordering::SeqCst)),
            ).await;
            if cancel.is_cancelled() { return Ok(()); }
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
        match provider {
            Provider::Local => {
                for (call, result) in turn.calls.iter().zip(&results) {
                    conv.push(json!({"role": "tool", "tool_call_id": call.id, "content": result}));
                }
            }
            Provider::ClaudeCli => conv.push(json!({"role": "user", "content": results.join("\n\n")})),
            Provider::ClaudeApi => {
                let tool_results: Vec<serde_json::Value> = turn.calls.iter().zip(&results)
                    .map(|(c, r)| json!({"type": "tool_result", "tool_use_id": c.id, "content": r}))
                    .collect();
                conv.push(json!({"role": "user", "content": tool_results}));
            }
        }
    }
}

#[tauri::command]
pub async fn generate_title(
    app: AppHandle,
    provider: String,
    api_key: String,
    model: String,
    user_message: String,
    assistant_message: String,
) -> Result<String, String> {
    if assistant_message.is_empty() { return Err("empty assistant message".to_string()); }
    let provider = Provider::parse(&provider)?;

    let preview: String = assistant_message.chars().take(500).collect();
    let user_preview: String = user_message.chars().take(1000).collect();
    let prompt = format!(
        "Generate a concise 3-5 word title for this conversation. Respond with ONLY the title, no punctuation, no quotes.\n\nUser: {}\n\nAssistant: {}",
        user_preview, preview
    );

    let title = match provider {
        Provider::Local => {
            let local = local_endpoint(&app);
            let url = format!("{}/chat/completions", local.url.trim_end_matches('/'));
            let body = json!({ "model": model, "messages": [{"role": "user", "content": prompt}], "max_tokens": 20, "stream": false });
            let mut req = Client::new().post(&url).header("content-type", "application/json");
            if let Some(k) = local.key.filter(|k| !k.is_empty()) { req = req.header("Authorization", format!("Bearer {}", k)); }
            let resp = req.json(&body).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() { return Err(format!("error {}", resp.status())); }
            let j: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            j["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string()
        }
        Provider::ClaudeCli => {
            let mut child = cli_command("claude-haiku-4-5", None, "", "json").spawn().map_err(cli_launch_error)?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(prompt.as_bytes()).await.map_err(|e| e.to_string())?;
            }
            let output = child.wait_with_output().await.map_err(|e| e.to_string())?;
            let j: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
            if j["is_error"].as_bool() == Some(true) { return Err("Claude Code error".to_string()); }
            j["result"].as_str().unwrap_or("").to_string()
        }
        Provider::ClaudeApi => {
            let body = json!({
                "model": "claude-haiku-4-5",
                "max_tokens": 32,
                "messages": [{"role": "user", "content": prompt}],
            });
            let resp = Client::new().post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() { return Err(format!("API error {}", resp.status())); }
            let j: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            j["content"][0]["text"].as_str().unwrap_or("").to_string()
        }
    };

    let title = title.trim().trim_matches('"').to_string();
    if title.is_empty() { Err("empty title".to_string()) } else { Ok(title) }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCliStatus {
    installed: bool,
    version: Option<String>,
    logged_in: bool,
    auth_method: Option<String>,
    error: Option<String>,
}

/// Checks whether Claude Code is installed and signed in. Doesn't call the model.
#[tauri::command]
pub async fn check_claude_cli() -> ClaudeCliStatus {
    let mut status = ClaudeCliStatus { installed: false, version: None, logged_in: false, auth_method: None, error: None };

    let mut version_cmd = tokio::process::Command::new("claude");
    version_cmd.arg("--version");
    #[cfg(windows)]
    version_cmd.creation_flags(0x0800_0000);
    match version_cmd.output().await {
        Ok(out) => {
            status.installed = true;
            status.version = Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
        Err(e) => {
            status.error = Some(cli_launch_error(e));
            return status;
        }
    }

    let mut auth_cmd = tokio::process::Command::new("claude");
    auth_cmd.args(["auth", "status", "--json"]);
    #[cfg(windows)]
    auth_cmd.creation_flags(0x0800_0000);
    match auth_cmd.output().await {
        Ok(out) => match serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            Ok(j) => {
                status.logged_in = j["loggedIn"].as_bool().unwrap_or(false);
                status.auth_method = j["authMethod"].as_str().map(|s| s.to_string());
            }
            Err(_) => status.error = Some(cli_auth_hint(String::from_utf8_lossy(&out.stderr).trim())),
        },
        Err(e) => status.error = Some(e.to_string()),
    }
    status
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
