use std::collections::BTreeMap;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

use super::CancelToken;
use super::interceptor::{NativeCall, TagInterceptor};
use super::tools::native_tool_to_action;

/// Returned as the error when the user pressed Stop; the caller exits quietly.
pub(crate) const CANCELLED: &str = "__CANCELLED__";

pub(crate) struct TurnResult {
    pub visible: String,
    pub calls: Vec<NativeCall>,
    /// Claude API only: the assistant content blocks exactly as received.
    /// Replaying them verbatim (thinking blocks included) keeps the model's
    /// reasoning valid across tool-use turns.
    pub content: Vec<serde_json::Value>,
}

fn fail(app: &AppHandle, msg: String) -> String {
    let _ = app.emit("claude:error", &msg);
    msg
}

/// Pull complete `data:` lines out of an SSE buffer.
fn drain_sse_lines(buf: &mut String) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(pos) = buf.find('\n') {
        let line = buf[..pos].trim().to_string();
        buf.drain(..=pos);
        if let Some(d) = line.strip_prefix("data:") {
            out.push(d.trim_start().to_string());
        }
    }
    out
}

// ─── Claude API (native tool use) ─────────────────────────────────────────────

pub(crate) async fn stream_turn_claude_api(
    app: &AppHandle,
    client: &Client,
    api_key: &str,
    model: &str,
    effort: Option<&str>,
    system: &str,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    cancel: &CancelToken,
) -> Result<TurnResult, String> {
    let is_haiku = model.starts_with("claude-haiku");
    let mut body = json!({
        "model": model,
        "max_tokens": if is_haiku { 32000 } else { 64000 },
        "stream": true,
        "system": system,
        "messages": messages,
    });
    if !tools.is_empty() {
        // Stream large tool inputs (whole files) as they're generated.
        let tools: Vec<_> = tools.iter().cloned().map(|mut t| { t["eager_input_streaming"] = json!(true); t }).collect();
        body["tools"] = json!(tools);
    }
    let mut req = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");
    if !is_haiku {
        if let Some(e) = effort { body["output_config"] = json!({ "effort": e }); }
        // If a safety classifier declines, let the API retry on a fallback model.
        body["fallbacks"] = json!("default");
        req = req.header("anthropic-beta", "server-side-fallback-2026-07-01");
    }

    let response = req.json(&body).send().await
        .map_err(|e| fail(app, format!("Could not reach the Claude API: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<serde_json::Value>(&text).ok()
            .and_then(|j| j["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or(text);
        let msg = match status {
            401 => "Invalid API key. Check your settings.".to_string(),
            429 => "Rate limit exceeded. Please wait and try again.".to_string(),
            529 => "Claude is overloaded right now. Please try again shortly.".to_string(),
            _ => format!("API error {}: {}", status, detail),
        };
        return Err(fail(app, msg));
    }

    // Blocks are rebuilt from their start event plus deltas, keyed by index.
    let mut blocks: BTreeMap<usize, serde_json::Value> = BTreeMap::new();
    let mut tool_json: BTreeMap<usize, String> = BTreeMap::new();
    let mut visible = String::new();
    let mut stop_reason = String::new();
    let mut stream = response.bytes_stream();
    let mut sse_buf = String::new();

    'outer: while let Some(chunk) = stream.next().await {
        if cancel.is_cancelled() { return Err(CANCELLED.into()); }
        let bytes = chunk.map_err(|e| fail(app, format!("Stream interrupted: {}", e)))?;
        sse_buf.push_str(&String::from_utf8_lossy(&bytes));

        for data in drain_sse_lines(&mut sse_buf) {
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            match ev["type"].as_str() {
                Some("content_block_start") => {
                    let idx = ev["index"].as_u64().unwrap_or(0) as usize;
                    let mut block = ev["content_block"].clone();
                    match block["type"].as_str() {
                        Some("tool_use") => { block["input"] = json!({}); tool_json.insert(idx, String::new()); }
                        Some("thinking") | Some("redacted_thinking") => { let _ = app.emit("claude:thinking", ()); }
                        _ => {}
                    }
                    blocks.insert(idx, block);
                }
                Some("content_block_delta") => {
                    let idx = ev["index"].as_u64().unwrap_or(0) as usize;
                    let delta = &ev["delta"];
                    let Some(block) = blocks.get_mut(&idx) else { continue };
                    match delta["type"].as_str() {
                        Some("text_delta") => if let Some(t) = delta["text"].as_str() {
                            let cur = block["text"].as_str().unwrap_or("").to_string();
                            block["text"] = json!(cur + t);
                            visible.push_str(t);
                            let _ = app.emit("claude:delta", t);
                        },
                        Some("thinking_delta") => if let Some(t) = delta["thinking"].as_str() {
                            let cur = block["thinking"].as_str().unwrap_or("").to_string();
                            block["thinking"] = json!(cur + t);
                        },
                        Some("signature_delta") => if let Some(s) = delta["signature"].as_str() {
                            block["signature"] = json!(s);
                        },
                        Some("input_json_delta") => if let Some(p) = delta["partial_json"].as_str() {
                            tool_json.entry(idx).or_default().push_str(p);
                        },
                        _ => {}
                    }
                }
                Some("message_delta") => {
                    if let Some(r) = ev["delta"]["stop_reason"].as_str() { stop_reason = r.to_string(); }
                }
                Some("error") => {
                    let msg = ev["error"]["message"].as_str().unwrap_or("unknown error");
                    return Err(fail(app, format!("Claude API error: {}", msg)));
                }
                Some("message_stop") => break 'outer,
                _ => {}
            }
        }
    }

    if stop_reason == "refusal" {
        return Err(fail(app, "Claude declined to continue with this request.".to_string()));
    }
    if stop_reason == "max_tokens" {
        let note = "\n\n*(Response stopped at the output limit.)*";
        visible.push_str(note);
        let _ = app.emit("claude:delta", note);
    }

    let mut calls = Vec::new();
    for (idx, block) in blocks.iter_mut() {
        if block["type"] != "tool_use" { continue; }
        let args: serde_json::Value = tool_json.get(idx)
            .and_then(|s| if s.trim().is_empty() { Some(json!({})) } else { serde_json::from_str(s).ok() })
            .unwrap_or(serde_json::Value::Null);
        let name = block["name"].as_str().unwrap_or("").to_string();
        let id = block["id"].as_str().unwrap_or("").to_string();
        let action = native_tool_to_action(&name, &args);
        if args.is_object() { block["input"] = args; }
        calls.push(NativeCall { id, action });
    }

    Ok(TurnResult { visible, calls, content: blocks.into_values().collect() })
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
    cancel: &CancelToken,
) -> Result<TurnResult, String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut body = json!({ "model": model, "messages": messages, "stream": true });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto");
    }

    let mut req = client.post(&url).header("content-type", "application/json");
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    let response = req.json(&body).send().await
        .map_err(|e| fail(app, format!("Could not reach local model at {}: {}", url, e)))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        return Err(fail(app, format!("Local model error {}: {}", status, text)));
    }

    let mut stream = response.bytes_stream();
    let mut sse_buf = String::new();
    let mut visible = String::new();
    // index -> (id, name, accumulated_arguments)
    let mut partial_calls: BTreeMap<usize, (String, String, String)> = BTreeMap::new();

    'outer: while let Some(chunk) = stream.next().await {
        if cancel.is_cancelled() { return Err(CANCELLED.into()); }
        let bytes = chunk.map_err(|e| fail(app, format!("Stream interrupted: {}", e)))?;
        sse_buf.push_str(&String::from_utf8_lossy(&bytes));

        for data in drain_sse_lines(&mut sse_buf) {
            if data == "[DONE]" { break 'outer; }
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            let Some(choice) = ev["choices"].as_array().and_then(|a| a.first()) else { continue };
            let delta = &choice["delta"];

            if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                visible.push_str(text);
                let _ = app.emit("claude:delta", text);
            }

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

    let calls = partial_calls.into_iter().map(|(idx, (id, name, args_str))| {
        let args: serde_json::Value = if args_str.trim().is_empty() { json!({}) }
            else { serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null) };
        // Some local servers omit call ids; the follow-up `tool` message needs one.
        let id = if id.is_empty() { format!("call_{}", idx) } else { id };
        NativeCall { id, action: native_tool_to_action(&name, &args) }
    }).collect();

    Ok(TurnResult { visible, calls, content: Vec::new() })
}

// ─── Local Claude CLI (tag-based) ─────────────────────────────────────────────

/// A headless `claude -p` call with Claude Code's own tools, MCP servers, and
/// customizations switched off. Locus supplies the system prompt and handles
/// tools itself via locus tags. The prompt is written to stdin, which avoids
/// Windows' command-line length limit on long conversations.
pub(crate) fn cli_command(model: &str, effort: Option<&str>, system: &str, output_format: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("claude");
    cmd.arg("-p").arg("--output-format").arg(output_format);
    if output_format == "stream-json" { cmd.arg("--verbose").arg("--include-partial-messages"); }
    cmd.arg("--tools").arg("")
        .arg("--strict-mcp-config")
        .arg("--no-session-persistence")
        .arg("--safe-mode");
    if !model.is_empty() { cmd.arg("--model").arg(model); }
    if let Some(e) = effort.filter(|_| !model.starts_with("claude-haiku")) { cmd.arg("--effort").arg(e); }
    if !system.is_empty() { cmd.arg("--system-prompt").arg(system); }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

pub(crate) fn cli_launch_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "Claude Code isn't installed or isn't on your PATH. Install it from claude.com/code, then try again.".to_string()
    } else {
        format!("Failed to launch Claude Code: {}", e)
    }
}

pub(crate) fn cli_auth_hint(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("authenticate") || lower.contains("log in") || lower.contains("login") || lower.contains("oauth") {
        format!("{}\n\nClaude Code isn't signed in. Open a terminal, run `claude`, and use /login — then try again.", msg)
    } else {
        msg.to_string()
    }
}

pub(crate) async fn stream_turn_claude_cli(
    app: &AppHandle,
    messages: &[serde_json::Value],
    model: &str,
    effort: Option<&str>,
    system: &str,
    interceptor: &mut TagInterceptor,
    cancel: &CancelToken,
) -> Result<TurnResult, String> {
    // The CLI runs one turn per process, so earlier turns go in as a transcript.
    let mut prompt = String::new();
    if messages.len() > 1 {
        prompt.push_str("<conversation_history>\n");
        for msg in &messages[..messages.len() - 1] {
            let role = if msg["role"] == "user" { "User" } else { "Assistant" };
            prompt.push_str(&format!("{}: {}\n\n", role, msg["content"].as_str().unwrap_or("")));
        }
        prompt.push_str("</conversation_history>\n\n");
    }
    prompt.push_str(messages.last().and_then(|m| m["content"].as_str()).unwrap_or(""));

    let mut child = cli_command(model, effort, system, "stream-json").spawn()
        .map_err(|e| fail(app, cli_launch_error(e)))?;

    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    stdin.write_all(prompt.as_bytes()).await.map_err(|e| fail(app, format!("Failed to send prompt to Claude Code: {}", e)))?;
    drop(stdin);

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr_handle = child.stderr.take();
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut visible = String::new();
    let mut error: Option<String> = None;
    let mut stopped_early = false;

    while let Some(line) = reader.next_line().await.map_err(|e| fail(app, e.to_string()))? {
        if cancel.is_cancelled() { return Err(CANCELLED.into()); }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line.trim()) else { continue };
        match event["type"].as_str() {
            Some("stream_event") => {
                let ev = &event["event"];
                match ev["type"].as_str() {
                    Some("content_block_start") if ev["content_block"]["type"] == "thinking" => {
                        let _ = app.emit("claude:thinking", ());
                    }
                    Some("content_block_delta") if ev["delta"]["type"] == "text_delta" => {
                        let text = ev["delta"]["text"].as_str().unwrap_or("");
                        let to_emit = interceptor.process(text);
                        if !to_emit.is_empty() {
                            visible.push_str(&to_emit);
                            let _ = app.emit("claude:delta", to_emit);
                        }
                        // A complete tag means the model wants a tool result; stop
                        // this process instead of letting it keep generating.
                        if interceptor.has_actions() {
                            stopped_early = true;
                            let _ = child.kill().await;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Some("assistant") if !event["error"].is_null() => {
                let text = event["message"]["content"][0]["text"].as_str().unwrap_or("Claude Code reported an error.");
                error = Some(text.to_string());
            }
            Some("result") if event["is_error"].as_bool() == Some(true) => {
                if error.is_none() {
                    error = Some(event["result"].as_str().unwrap_or("Claude Code reported an error.").to_string());
                }
            }
            _ => {}
        }
    }

    if let Some(e) = error {
        return Err(fail(app, cli_auth_hint(&e)));
    }
    if !stopped_early {
        let status = child.wait().await.map_err(|e| e.to_string())?;
        if !status.success() {
            let mut err = String::new();
            if let Some(mut se) = stderr_handle { let _ = se.read_to_string(&mut err).await; }
            let msg = if err.trim().is_empty() { "Claude Code exited with an error.".to_string() } else { err.trim().to_string() };
            return Err(fail(app, cli_auth_hint(&msg)));
        }
    }

    let tail = interceptor.flush();
    if !tail.is_empty() {
        visible.push_str(&tail);
        let _ = app.emit("claude:delta", tail);
    }

    let calls = interceptor.take_actions().into_iter().enumerate()
        .map(|(i, action)| NativeCall { id: format!("local_{}", i), action })
        .collect();

    Ok(TurnResult { visible, calls, content: Vec::new() })
}
