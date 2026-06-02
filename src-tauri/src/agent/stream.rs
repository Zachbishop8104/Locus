use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

use super::interceptor::TagInterceptor;

// ─── Claude API ───────────────────────────────────────────────────────────────

pub(crate) async fn stream_turn_claude_api(
    app: &AppHandle,
    client: &Client,
    api_key: &str,
    model: &str,
    system: &str,
    messages: &[serde_json::Value],
    interceptor: &mut TagInterceptor,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "max_tokens": 8096,
        "stream": true,
        "system": system,
        "messages": messages,
    });

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        let msg = match status {
            401 => "Invalid API key. Check your settings.".to_string(),
            429 => "Rate limit exceeded. Please wait and try again.".to_string(),
            _ => format!("API error {}: {}", status, text),
        };
        let _ = app.emit("claude:error", &msg);
        return Err(msg);
    }

    process_sse(app, response, interceptor, |event| {
        if event["type"].as_str() == Some("content_block_delta") {
            event["delta"]["text"].as_str().map(|s| s.to_string())
        } else {
            None
        }
    })
    .await
}

// ─── OpenAI-compatible ────────────────────────────────────────────────────────

pub(crate) async fn stream_turn_openai_compat(
    app: &AppHandle,
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    messages: &[serde_json::Value],
    interceptor: &mut TagInterceptor,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({ "model": model, "messages": messages, "stream": true });

    let mut req = client.post(&url).header("content-type", "application/json");
    if let Some(key) = api_key {
        if !key.is_empty() { req = req.header("Authorization", format!("Bearer {}", key)); }
    }

    let response = req.json(&body).send().await.map_err(|e| {
        let msg = format!("Could not reach local model at {}: {}", url, e);
        let _ = app.emit("claude:error", &msg);
        msg
    })?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        let msg = format!("Local model error {}: {}", status, text);
        let _ = app.emit("claude:error", &msg);
        return Err(msg);
    }

    process_sse(app, response, interceptor, |event| {
        event["choices"].as_array()?.first()
            .and_then(|c| c["delta"]["content"].as_str())
            .map(|s| s.to_string())
    })
    .await
}

// ─── Local Claude CLI ─────────────────────────────────────────────────────────

pub(crate) async fn stream_turn_local_claude(
    app: &AppHandle,
    messages: &[serde_json::Value],
    model: &str,
    system: &str,
    interceptor: &mut TagInterceptor,
) -> Result<String, String> {
    // Encode conversation history into the system prompt (CLI is single-turn).
    let mut context = system.to_string();
    if messages.len() > 1 {
        context.push_str("\n\nConversation history:\n");
        for msg in messages.iter().take(messages.len() - 1) {
            let role = if msg["role"] == "user" { "User" } else { "Assistant" };
            context.push_str(&format!("{}: {}\n\n", role, msg["content"].as_str().unwrap_or("")));
        }
    }
    let current = messages.last().and_then(|m| m["content"].as_str()).unwrap_or("");

    let mut cmd = tokio::process::Command::new("claude");
    cmd.arg("--print").arg("--output-format").arg("stream-json");
    if !model.is_empty() { cmd.arg("--model").arg(model); }
    if !context.is_empty() { cmd.arg("--system-prompt").arg(&context); }
    cmd.arg(current)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        let msg = format!("Failed to launch claude CLI: {}. Ensure 'claude' is installed and logged in.", e);
        let _ = app.emit("claude:error", &msg);
        msg
    })?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr_handle = child.stderr.take();
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut last_len = 0usize;
    let mut visible = String::new();

    while let Some(line) = reader.next_line().await.map_err(|e| e.to_string())? {
        let line = line.trim().to_string();
        if line.is_empty() { continue; }
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
            let text_opt: Option<String> = match event["type"].as_str() {
                Some("content_block_delta") => event["delta"]["text"].as_str().map(|s| s.to_string()),
                Some("assistant") => {
                    event["message"]["content"].as_array().and_then(|blocks| {
                        blocks.iter().find(|b| b["type"] == "text")
                            .and_then(|b| b["text"].as_str())
                            .map(|full| {
                                let delta = if full.len() > last_len { full[last_len..].to_string() } else { String::new() };
                                last_len = full.len();
                                delta
                            })
                    })
                }
                _ => None,
            };
            if let Some(text) = text_opt {
                if !text.is_empty() {
                    let to_emit = interceptor.process(&text);
                    if !to_emit.is_empty() {
                        visible.push_str(&to_emit);
                        let _ = app.emit("claude:delta", to_emit);
                    }
                }
            }
        }
    }

    let status = child.wait().await.map_err(|e| e.to_string())?;
    if !status.success() {
        let mut err = String::new();
        if let Some(mut se) = stderr_handle { let _ = se.read_to_string(&mut err).await; }
        let msg = if !err.trim().is_empty() {
            format!("Claude CLI error: {}", err.trim())
        } else {
            "Claude CLI failed. Make sure you are logged in with 'claude login'.".to_string()
        };
        let _ = app.emit("claude:error", &msg);
        return Err(msg);
    }

    emit_tail(app, interceptor, &mut visible);
    Ok(visible)
}

// ─── Shared SSE loop ──────────────────────────────────────────────────────────

async fn process_sse(
    app: &AppHandle,
    response: reqwest::Response,
    interceptor: &mut TagInterceptor,
    extract: impl Fn(&serde_json::Value) -> Option<String>,
) -> Result<String, String> {
    let mut stream = response.bytes_stream();
    let mut buf = String::new();
    let mut visible = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| e.to_string())?;
        buf.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf = buf[pos + 1..].to_string();
            let data = match line.strip_prefix("data: ") {
                Some(d) if d != "[DONE]" => d.to_string(),
                _ => continue,
            };
            let Ok(event) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            if let Some(text) = extract(&event) {
                if !text.is_empty() {
                    let to_emit = interceptor.process(&text);
                    if !to_emit.is_empty() {
                        visible.push_str(&to_emit);
                        let _ = app.emit("claude:delta", to_emit);
                    }
                }
            }
        }
    }

    emit_tail(app, interceptor, &mut visible);
    Ok(visible)
}

fn emit_tail(app: &AppHandle, interceptor: &mut TagInterceptor, visible: &mut String) {
    let tail = interceptor.flush();
    if !tail.is_empty() {
        visible.push_str(&tail);
        let _ = app.emit("claude:delta", tail);
    }
}
