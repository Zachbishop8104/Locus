use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[tauri::command]
pub async fn stream_message(
    app: AppHandle,
    api_key: String,
    messages: Vec<Message>,
    model: String,
    system: Option<String>,
) -> Result<(), String> {
    let client = Client::new();

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": 8096,
        "stream": true,
        "messages": messages,
    });

    if let Some(sys) = system {
        if !sys.is_empty() {
            body["system"] = sys.into();
        }
    }

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let msg = if status == 401 {
            "Invalid API key. Check your settings.".to_string()
        } else if status == 429 {
            "Rate limit exceeded. Please wait and try again.".to_string()
        } else {
            format!("API error {}: {}", status, body)
        };
        let _ = app.emit("claude:error", msg.clone());
        return Err(msg);
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer = buffer[pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            let _ = app.emit("claude:done", ());
                            return Ok(());
                        }

                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                            if event["type"] == "content_block_delta"
                                && event["delta"]["type"] == "text_delta"
                            {
                                if let Some(text) = event["delta"]["text"].as_str() {
                                    let _ = app.emit("claude:delta", text.to_string());
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let _ = app.emit("claude:error", e.to_string());
                return Err(e.to_string());
            }
        }
    }

    let _ = app.emit("claude:done", ());
    Ok(())
}

#[tauri::command]
pub async fn pick_folder() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Select Project Folder")
        .pick_folder()
        .await
        .map(|f| f.path().to_string_lossy().to_string())
}
