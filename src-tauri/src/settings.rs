use std::fs;
use tauri::{AppHandle, Manager};

fn config_path(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .expect("no app data dir")
        .join("config.json")
}

pub fn read_config(app: &AppHandle) -> serde_json::Value {
    let path = config_path(app);
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(json) = serde_json::from_str(&content) {
            return json;
        }
    }
    serde_json::json!({})
}

pub fn write_config(app: &AppHandle, config: serde_json::Value) -> Result<(), String> {
    let path = config_path(app);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(path, config.to_string()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_api_key(app: AppHandle) -> String {
    read_config(&app)["api_key"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

#[tauri::command]
pub fn set_api_key(app: AppHandle, key: String) -> Result<(), String> {
    let mut config = read_config(&app);
    config["api_key"] = serde_json::Value::String(key);
    write_config(&app, config)
}
