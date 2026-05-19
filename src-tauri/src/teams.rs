use base64::{engine::general_purpose::{URL_SAFE_NO_PAD, STANDARD}, Engine};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};

use crate::settings::{read_config, write_config};

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TeamInfo {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TeamsAttachment {
    pub id: String,
    pub content_type: String,
    pub content: Option<String>,
    pub content_url: Option<String>,
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TeamsMessage {
    pub id: String,
    pub from_name: Option<String>,
    pub from_user_id: Option<String>,
    pub body: String,
    pub content_type: String,
    pub created_at: String,
    pub attachments: Vec<TeamsAttachment>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TeamsStatus {
    pub connected: bool,
    pub user_name: Option<String>,
    pub user_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatInfo {
    pub id: String,
    pub topic: Option<String>,
    pub chat_type: String,
    pub members: Vec<String>,
    pub member_ids: Vec<String>,
    pub last_message_preview: Option<String>,
}

// ─── PKCE helpers ────────────────────────────────────────────────────────────

fn code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge(verifier: &str) -> String {
    let mut h = Sha256::new();
    h.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(h.finalize())
}

// ─── Config helpers ──────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn get_teams_config(app: &AppHandle) -> serde_json::Value {
    read_config(app)
}

fn save_tokens(
    app: &AppHandle,
    access: &str,
    refresh: &str,
    expires_in: u64,
    user_name: &str,
    user_id: &str,
) -> Result<(), String> {
    let mut cfg = read_config(app);
    cfg["teams_access_token"] = access.into();
    cfg["teams_refresh_token"] = refresh.into();
    cfg["teams_token_expiry"] = (now_secs() + expires_in - 60).into();
    cfg["teams_user_name"] = user_name.into();
    cfg["teams_user_id"] = user_id.into();
    write_config(app, cfg)
}

// ─── Token management ────────────────────────────────────────────────────────

async fn get_valid_token(app: &AppHandle) -> Result<String, String> {
    let cfg = get_teams_config(app);
    let access = cfg["teams_access_token"].as_str().unwrap_or("").to_string();
    let refresh = cfg["teams_refresh_token"].as_str().unwrap_or("").to_string();
    let expiry = cfg["teams_token_expiry"].as_u64().unwrap_or(0);
    let client_id = cfg["teams_client_id"].as_str().unwrap_or("").to_string();
    let tenant_id = cfg["teams_tenant_id"].as_str().unwrap_or("").to_string();

    if access.is_empty() || refresh.is_empty() {
        return Err("Not connected to Teams. Please authenticate in Settings.".into());
    }

    // Token still valid
    if now_secs() < expiry {
        return Ok(access);
    }

    // Refresh
    let client = Client::new();
    let scopes = graph_scopes();
    let resp = client
        .post(format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"
        ))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", &client_id),
            ("refresh_token", &refresh),
            ("scope", &scopes),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token refresh failed: {body}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let new_access = json["access_token"].as_str().unwrap_or("").to_string();
    let new_refresh = json["refresh_token"].as_str().unwrap_or(&refresh).to_string();
    let expires_in = json["expires_in"].as_u64().unwrap_or(3600);
    let cfg = get_teams_config(app);
    let user_name = cfg["teams_user_name"].as_str().unwrap_or("").to_string();
    let user_id = cfg["teams_user_id"].as_str().unwrap_or("").to_string();

    save_tokens(app, &new_access, &new_refresh, expires_in, &user_name, &user_id)?;
    Ok(new_access)
}

fn graph_scopes() -> String {
    // ChannelMessage.Read.All requires org admin consent — excluded by default.
    // User.ReadBasic.All allows fetching other users' profile photos.
    "offline_access User.Read User.ReadBasic.All Team.ReadBasic.All Channel.ReadBasic.All Chat.Read Chat.ReadBasic ChatMessage.Send ChannelMessage.Send".to_string()
}

// ─── OAuth callback server ───────────────────────────────────────────────────

async fn wait_for_code(listener: TcpListener) -> Result<String, String> {
    let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
    let request = String::from_utf8_lossy(&buf[..n]).to_string();

    let code = parse_code_from_request(&request)?;

    let html = r#"<!DOCTYPE html><html><head><meta charset="utf-8"><style>
      body{font-family:-apple-system,sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;background:#09090b;color:#fafafa}
      .card{text-align:center;padding:40px;border-radius:16px;border:1px solid #3f3f46;background:#18181b}
      h2{margin:0 0 8px;color:#fafafa} p{color:#71717a;margin:0}
    </style></head><body>
    <div class="card"><h2>✓ Connected to Microsoft Teams</h2><p>You can close this tab and return to Locus.</p></div>
    </body></html>"#;

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    stream.write_all(response.as_bytes()).await.ok();
    stream.flush().await.ok();

    Ok(code)
}

fn parse_code_from_request(request: &str) -> Result<String, String> {
    let first_line = request.lines().next().ok_or("empty HTTP request")?;
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or("missing path in request")?;
    let query = path.split('?').nth(1).ok_or("no query string in callback")?;

    for param in query.split('&') {
        if let Some(code) = param.strip_prefix("code=") {
            return Ok(urlencoding::decode(code)
                .map_err(|e| e.to_string())?
                .into_owned());
        }
    }

    // Check for error
    for param in query.split('&') {
        if let Some(err) = param.strip_prefix("error_description=") {
            return Err(urlencoding::decode(err)
                .map_err(|e| e.to_string())?
                .into_owned());
        }
    }

    Err("No authorization code received".into())
}

// ─── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_teams_auth(
    app: AppHandle,
    client_id: String,
    tenant_id: String,
) -> Result<(), String> {
    // Save credentials first
    let mut cfg = read_config(&app);
    cfg["teams_client_id"] = client_id.clone().into();
    cfg["teams_tenant_id"] = tenant_id.clone().into();
    write_config(&app, cfg)?;

    let verifier = code_verifier();
    let challenge = code_challenge(&verifier);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://localhost:{port}");

    let scopes = urlencoding::encode(&graph_scopes()).to_string();
    let redirect_enc = urlencoding::encode(&redirect_uri).to_string();

    let auth_url = format!(
        "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/authorize\
         ?client_id={client_id}\
         &response_type=code\
         &redirect_uri={redirect_enc}\
         &scope={scopes}\
         &code_challenge={challenge}\
         &code_challenge_method=S256\
         &prompt=select_account"
    );

    open::that(&auth_url).map_err(|e| format!("Failed to open browser: {e}"))?;

    // Wait up to 5 minutes for the callback
    let code = timeout(Duration::from_secs(300), wait_for_code(listener))
        .await
        .map_err(|_| "Authentication timed out — please try again".to_string())??;

    // Exchange code for tokens
    let client = Client::new();
    let resp = client
        .post(format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"
        ))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", &code),
            ("redirect_uri", &redirect_uri),
            ("code_verifier", &verifier),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed: {body}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let access = json["access_token"].as_str().unwrap_or("").to_string();
    let refresh = json["refresh_token"].as_str().unwrap_or("").to_string();
    let expires_in = json["expires_in"].as_u64().unwrap_or(3600);

    // Fetch user display name
    let user_resp = client
        .get("https://graph.microsoft.com/v1.0/me")
        .bearer_auth(&access)
        .send()
        .await
        .ok();
    let (user_name, user_id) = if let Some(r) = user_resp {
        r.json::<serde_json::Value>()
            .await
            .ok()
            .map(|v| (
                v["displayName"].as_str().map(String::from).unwrap_or_default(),
                v["id"].as_str().map(String::from).unwrap_or_default(),
            ))
            .unwrap_or_default()
    } else {
        (String::new(), String::new())
    };

    save_tokens(&app, &access, &refresh, expires_in, &user_name, &user_id)?;
    let _ = app.emit("teams:connected", &user_name);
    Ok(())
}

#[tauri::command]
pub fn get_teams_status(app: AppHandle) -> TeamsStatus {
    let cfg = get_teams_config(&app);
    let has_token = cfg["teams_access_token"]
        .as_str()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let has_refresh = cfg["teams_refresh_token"]
        .as_str()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    TeamsStatus {
        connected: has_token && has_refresh,
        user_name: cfg["teams_user_name"].as_str().filter(|s| !s.is_empty()).map(String::from),
        user_id: cfg["teams_user_id"].as_str().filter(|s| !s.is_empty()).map(String::from),
    }
}

#[tauri::command]
pub fn get_teams_credentials(app: AppHandle) -> (String, String) {
    let cfg = get_teams_config(&app);
    (
        cfg["teams_client_id"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        cfg["teams_tenant_id"]
            .as_str()
            .unwrap_or("")
            .to_string(),
    )
}

#[tauri::command]
pub fn disconnect_teams(app: AppHandle) -> Result<(), String> {
    let mut cfg = read_config(&app);
    cfg["teams_access_token"] = "".into();
    cfg["teams_refresh_token"] = "".into();
    cfg["teams_token_expiry"] = 0u64.into();
    cfg["teams_user_name"] = "".into();
    write_config(&app, cfg)
}

#[tauri::command]
pub async fn get_teams_list(app: AppHandle) -> Result<Vec<TeamInfo>, String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .get("https://graph.microsoft.com/v1.0/me/joinedTeams")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Graph API error {status}: {body}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let teams = json["value"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|t| TeamInfo {
            id: t["id"].as_str().unwrap_or("").to_string(),
            display_name: t["displayName"].as_str().unwrap_or("").to_string(),
            description: t["description"].as_str().map(String::from),
        })
        .collect();

    Ok(teams)
}

#[tauri::command]
pub async fn get_team_channels(app: AppHandle, team_id: String) -> Result<Vec<ChannelInfo>, String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .get(format!(
            "https://graph.microsoft.com/v1.0/teams/{team_id}/channels"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Graph API error {status}: {body}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let channels = json["value"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|c| ChannelInfo {
            id: c["id"].as_str().unwrap_or("").to_string(),
            display_name: c["displayName"].as_str().unwrap_or("").to_string(),
            description: c["description"].as_str().map(String::from),
        })
        .collect();

    Ok(channels)
}

#[tauri::command]
pub async fn get_channel_messages(
    app: AppHandle,
    team_id: String,
    channel_id: String,
) -> Result<Vec<TeamsMessage>, String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .get(format!(
            "https://graph.microsoft.com/v1.0/teams/{team_id}/channels/{channel_id}/messages?$top=50&$orderby=createdDateTime desc"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        let msg = if status == 403 {
            "Permission denied — your admin may need to grant ChannelMessage.Read.All consent for this app.".to_string()
        } else {
            format!("Graph API error {status}: {body}")
        };
        return Err(msg);
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let messages = json["value"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter(|m| m["messageType"].as_str() == Some("message"))
        .map(|m| TeamsMessage {
            id: m["id"].as_str().unwrap_or("").to_string(),
            from_name: m["from"]["user"]["displayName"].as_str()
                .or_else(|| m["from"]["application"]["displayName"].as_str())
                .map(String::from),
            from_user_id: m["from"]["user"]["id"].as_str().map(String::from),
            body: m["body"]["content"].as_str().unwrap_or("").to_string(),
            content_type: m["body"]["contentType"]
                .as_str()
                .unwrap_or("text")
                .to_string(),
            created_at: m["createdDateTime"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            attachments: m["attachments"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|a| TeamsAttachment {
                    id: a["id"].as_str().unwrap_or("").to_string(),
                    content_type: a["contentType"].as_str().unwrap_or("").to_string(),
                    content: a["content"].as_str().map(String::from),
                    content_url: a["contentUrl"].as_str().filter(|s| !s.is_empty()).map(String::from),
                    name: a["name"].as_str().filter(|s| !s.is_empty()).map(String::from),
                })
                .collect(),
        })
        .collect();

    Ok(messages)
}

#[tauri::command]
pub async fn send_chat_message(
    app: AppHandle,
    chat_id: String,
    content: String,
) -> Result<(), String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .post(format!(
            "https://graph.microsoft.com/v1.0/me/chats/{chat_id}/messages"
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "body": { "content": content, "contentType": "text" } }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to send ({status}): {body}"));
    }
    Ok(())
}

#[tauri::command]
pub async fn send_channel_message(
    app: AppHandle,
    team_id: String,
    channel_id: String,
    content: String,
) -> Result<(), String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .post(format!(
            "https://graph.microsoft.com/v1.0/teams/{team_id}/channels/{channel_id}/messages"
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "body": { "content": content, "contentType": "text" } }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to send ({status}): {body}"));
    }
    Ok(())
}

#[tauri::command]
pub async fn get_chats(app: AppHandle) -> Result<Vec<ChatInfo>, String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .get("https://graph.microsoft.com/v1.0/me/chats?$expand=members&$top=50")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Graph API error {status}: {body}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let chats = json["value"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|c| {
            let members_arr = c["members"].as_array().cloned().unwrap_or_default();
            let members: Vec<String> = members_arr
                .iter()
                .filter_map(|m| m["displayName"].as_str().map(String::from))
                .collect();
            let member_ids: Vec<String> = members_arr
                .iter()
                .filter_map(|m| m["userId"].as_str().map(String::from))
                .collect();

            ChatInfo {
                id: c["id"].as_str().unwrap_or("").to_string(),
                topic: c["topic"].as_str().filter(|s| !s.is_empty()).map(String::from),
                chat_type: c["chatType"].as_str().unwrap_or("oneOnOne").to_string(),
                members,
                member_ids,
                last_message_preview: None,
            }
        })
        .collect();

    Ok(chats)
}

#[tauri::command]
pub async fn get_chat_messages(
    app: AppHandle,
    chat_id: String,
) -> Result<Vec<TeamsMessage>, String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .get(format!(
            "https://graph.microsoft.com/v1.0/me/chats/{chat_id}/messages?$top=50"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Graph API error {status}: {body}"));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let messages = json["value"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter(|m| m["messageType"].as_str() == Some("message"))
        .map(|m| TeamsMessage {
            id: m["id"].as_str().unwrap_or("").to_string(),
            from_name: m["from"]["user"]["displayName"].as_str()
                .or_else(|| m["from"]["application"]["displayName"].as_str())
                .map(String::from),
            from_user_id: m["from"]["user"]["id"].as_str().map(String::from),
            body: m["body"]["content"].as_str().unwrap_or("").to_string(),
            content_type: m["body"]["contentType"]
                .as_str()
                .unwrap_or("text")
                .to_string(),
            created_at: m["createdDateTime"].as_str().unwrap_or("").to_string(),
            attachments: m["attachments"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|a| TeamsAttachment {
                    id: a["id"].as_str().unwrap_or("").to_string(),
                    content_type: a["contentType"].as_str().unwrap_or("").to_string(),
                    content: a["content"].as_str().map(String::from),
                    content_url: a["contentUrl"].as_str().filter(|s| !s.is_empty()).map(String::from),
                    name: a["name"].as_str().filter(|s| !s.is_empty()).map(String::from),
                })
                .collect(),
        })
        .collect();

    Ok(messages)
}

#[tauri::command]
pub async fn get_user_photo(app: AppHandle, user_id: String) -> Result<String, String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .get(format!(
            "https://graph.microsoft.com/v1.0/users/{user_id}/photo/$value"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err("no photo".into());
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(format!("data:{content_type};base64,{}", STANDARD.encode(&bytes)))
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_teams_image(app: AppHandle, url: String) -> Result<String, String> {
    let token = get_valid_token(&app).await?;
    let client = Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .to_string();

    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(format!("data:{};base64,{}", content_type, STANDARD.encode(&bytes)))
}
