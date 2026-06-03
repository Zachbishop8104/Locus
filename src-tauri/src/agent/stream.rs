use std::collections::HashMap;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

use super::interceptor::{NativeCall, TagInterceptor};
use super::tools::native_tool_to_action;

// ─── Claude API (native tool use) ─────────────────────────────────────────────

pub(crate) async fn stream_turn_claude_api(
    app: &AppHandle,
    client: &Client,
    api_key: &str,
    model: &str,
    system: &str,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
) -> Result<(String, Vec<NativeCall>), String> {
    let mut body = json!({
        "model": model,
        "max_tokens": 8096,
        "stream": true,
        "system": system,
        "messages": messages,
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }

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

    // Per-block state: "text" blocks stream to the user; "tool_use" blocks
    // accumulate JSON that we parse at the end.
    struct Block { kind: String, id: String, name: String, buf: String }
    let mut blocks: HashMap<usize, Block> = HashMap::new();
    let mut visible = String::new();
    let mut stream = response.bytes_stream();
    let mut sse_buf = String::new();

    'outer: while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| e.to_string())?;
        sse_buf.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(pos) = sse_buf.find('\n') {
            let line = sse_buf[..pos].trim().to_string();
            sse_buf = sse_buf[pos + 1..].to_string();
            let data = match line.strip_prefix("data: ") {
                Some(d) if d == "[DONE]" => break 'outer,
                Some(d) => d.to_string(),
                _ => continue,
            };
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(&data) else { continue };

            match ev["type"].as_str() {
                Some("content_block_start") => {
                    let idx  = ev["index"].as_u64().unwrap_or(0) as usize;
                    let cb   = &ev["content_block"];
                    let kind = cb["type"].as_str().unwrap_or("text").to_string();
                    let id   = cb["id"].as_str().unwrap_or("").to_string();
                    let name = cb["name"].as_str().unwrap_or("").to_string();
                    blocks.insert(idx, Block { kind, id, name, buf: String::new() });
                }
                Some("content_block_delta") => {
                    let idx = ev["index"].as_u64().unwrap_or(0) as usize;
                    let delta = &ev["delta"];
                    if let Some(block) = blocks.get_mut(&idx) {
                        match block.kind.as_str() {
                            "text" => {
                                if let Some(text) = delta["text"].as_str() {
                                    block.buf.push_str(text);
                                    visible.push_str(text);
                                    let _ = app.emit("claude:delta", text);
                                }
                            }
                            "tool_use" => {
                                if let Some(partial) = delta["partial_json"].as_str() {
                                    block.buf.push_str(partial);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some("message_stop") => break 'outer,
                _ => {}
            }
        }
    }

    let calls: Vec<NativeCall> = blocks.into_values()
        .filter(|b| b.kind == "tool_use")
        .filter_map(|b| {
            let args: serde_json::Value = serde_json::from_str(&b.buf).unwrap_or(json!({}));
            native_tool_to_action(&b.name, &args).map(|action| NativeCall { id: b.id, action })
        })
        .collect();

    Ok((visible, calls))
}

// ─── OpenAI-compatible (Ollama, LM Studio, etc.) ─────────────────────────────

pub(crate) async fn stream_turn_openai_compat(
    app: &AppHandle,
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
) -> Result<(String, Vec<NativeCall>), String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut body = json!({ "model": model, "messages": messages, "stream": true });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto");
    }

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

    let mut stream = response.bytes_stream();
    let mut sse_buf = String::new();
    let mut visible = String::new();
    // index -> (id, name, accumulated_arguments)
    let mut partial_calls: HashMap<usize, (String, String, String)> = HashMap::new();

    'outer: while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| e.to_string())?;
        sse_buf.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(pos) = sse_buf.find('\n') {
            let line = sse_buf[..pos].trim().to_string();
            sse_buf = sse_buf[pos + 1..].to_string();
            let data = match line.strip_prefix("data: ") {
                Some(d) if d == "[DONE]" => break 'outer,
                Some(d) => d.to_string(),
                _ => continue,
            };
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            let Some(choice) = ev["choices"].as_array().and_then(|a| a.first()) else { continue };
            let delta = &choice["delta"];

            // Text content
            if let Some(text) = delta["content"].as_str() {
                if !text.is_empty() {
                    visible.push_str(text);
                    let _ = app.emit("claude:delta", text);
                }
            }

            // Tool call deltas — accumulate by index
            if let Some(tc_arr) = delta["tool_calls"].as_array() {
                for tc in tc_arr {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    let entry = partial_calls.entry(idx).or_default();
                    if let Some(id)   = tc["id"].as_str()                   { entry.0 = id.to_string(); }
                    if let Some(name) = tc["function"]["name"].as_str()      { entry.1 = name.to_string(); }
                    if let Some(args) = tc["function"]["arguments"].as_str() { entry.2.push_str(args); }
                }
            }
        }
    }

    let mut sorted: Vec<_> = partial_calls.into_iter().collect();
    sorted.sort_by_key(|(idx, _)| *idx);

    let calls: Vec<NativeCall> = sorted.into_iter().filter_map(|(_, (id, name, args_str))| {
        let args: serde_json::Value = serde_json::from_str(&args_str).unwrap_or(json!({}));
        native_tool_to_action(&name, &args).map(|action| NativeCall { id, action })
    }).collect();

    Ok((visible, calls))
}

// ─── Local Claude CLI (tag-based fallback) ────────────────────────────────────

pub(crate) async fn stream_turn_local_claude(
    app: &AppHandle,
    messages: &[serde_json::Value],
    model: &str,
    system: &str,
    interceptor: &mut TagInterceptor,
) -> Result<(String, Vec<NativeCall>), String> {
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
                    if interceptor.has_actions() { break; }
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

    // Flush any buffered non-tag text
    let tail = interceptor.flush();
    if !tail.is_empty() {
        visible.push_str(&tail);
        let _ = app.emit("claude:delta", tail);
    }

    // Wrap tag-based actions as NativeCalls with synthetic IDs
    let calls: Vec<NativeCall> = interceptor.take_actions().into_iter().enumerate()
        .map(|(i, action)| NativeCall { id: format!("local_{}", i), action })
        .collect();

    Ok((visible, calls))
}
