use std::path::{Path, PathBuf};
use serde::Serialize;

// Tasks come from the task-master CLI (`npm i -g task-master-ai`), run inside each
// project folder that has a `.taskmaster/` directory.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmTask {
    id: String,
    title: String,
    description: String,
    details: String,
    /// pending | in-progress | review | done | blocked | deferred | cancelled
    status: String,
    priority: Option<String>,
    dependencies: Vec<String>,
    test_strategy: String,
    subtasks: Vec<TmSubtask>,
    subtasks_total: usize,
    subtasks_done: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmSubtask {
    id: String,
    title: String,
    description: String,
    details: String,
    status: String,
}

fn tm_command(cwd: &Path) -> tokio::process::Command {
    // npm installs a `.cmd` shim on Windows; std quotes batch-file arguments safely.
    #[cfg(windows)]
    let mut cmd = tokio::process::Command::new("task-master.cmd");
    #[cfg(not(windows))]
    let mut cmd = tokio::process::Command::new("task-master");
    cmd.current_dir(cwd);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

fn tasks_file(root: &Path) -> PathBuf {
    root.join(".taskmaster").join("tasks").join("tasks.json")
}

fn project_root(path: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(path.trim());
    if !root.is_dir() {
        return Err(format!("Folder not found: {}", path));
    }
    Ok(root)
}

async fn run_tm(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = tm_command(root)
        .args(args)
        .output()
        .await
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
            "TM_NOT_INSTALLED".to_string()
        } else {
            format!("Failed to run task-master: {}", e)
        })?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let err = if err.is_empty() { String::from_utf8_lossy(&output.stdout).trim().to_string() } else { err };
        return Err(if err.is_empty() { "task-master reported an error.".to_string() } else { err });
    }
    Ok(output.stdout)
}

/// Task and subtask ids look like `3` or `3.1`.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit() || c == '.')
}

const STATUSES: &[&str] = &["pending", "in-progress", "review", "done", "blocked", "deferred", "cancelled"];

fn as_string(v: &serde_json::Value) -> String {
    v.as_str().map(|s| s.to_string()).unwrap_or_else(|| if v.is_null() { String::new() } else { v.to_string() })
}

/// Lists the top-level tasks of the current tag. Errors with `TM_NOT_INITIALIZED`
/// when the folder has no `.taskmaster/` directory.
#[tauri::command]
pub async fn get_taskmaster_tasks(path: String) -> Result<Vec<TmTask>, String> {
    let root = project_root(&path)?;
    if !root.join(".taskmaster").is_dir() {
        return Err("TM_NOT_INITIALIZED".to_string());
    }
    // Initialized but no tasks yet — the CLI errors in this case.
    if !tasks_file(&root).is_file() {
        return Ok(Vec::new());
    }

    let stdout = run_tm(&root, &["list", "--json"]).await?;
    let text = String::from_utf8_lossy(&stdout);
    // Skip anything printed before the JSON (update notices and the like).
    let start = text.find('{').ok_or_else(|| "Unexpected output from task-master.".to_string())?;
    let json: serde_json::Value = serde_json::from_str(&text[start..])
        .map_err(|e| format!("Unexpected output from task-master: {}", e))?;

    Ok(json["tasks"].as_array().map(|arr| arr.iter().map(|t| {
        let subtasks = t["subtasks"].as_array();
        TmTask {
            id: as_string(&t["id"]),
            title: as_string(&t["title"]),
            description: as_string(&t["description"]),
            details: as_string(&t["details"]),
            status: t["status"].as_str().unwrap_or("pending").to_string(),
            priority: t["priority"].as_str().map(|s| s.to_string()),
            dependencies: t["dependencies"].as_array().map(|d| d.iter().map(as_string).collect()).unwrap_or_default(),
            test_strategy: as_string(&t["testStrategy"]),
            subtasks: subtasks.map(|s| s.iter().map(|st| TmSubtask {
                id: as_string(&st["id"]),
                title: as_string(&st["title"]),
                description: as_string(&st["description"]),
                details: as_string(&st["details"]),
                status: st["status"].as_str().unwrap_or("pending").to_string(),
            }).collect()).unwrap_or_default(),
            subtasks_total: subtasks.map(|s| s.len()).unwrap_or(0),
            subtasks_done: subtasks.map(|s| s.iter().filter(|st| st["status"] == "done").count()).unwrap_or(0),
        }
    }).collect()).unwrap_or_default())
}

#[tauri::command]
pub async fn set_taskmaster_status(path: String, id: String, status: String) -> Result<(), String> {
    let root = project_root(&path)?;
    if !valid_id(&id) {
        return Err(format!("Invalid task id: {}", id));
    }
    if !STATUSES.contains(&status.as_str()) {
        return Err(format!("Invalid task status: {}", status));
    }
    run_tm(&root, &["set-status", &format!("--id={}", id), &format!("--status={}", status)]).await?;
    Ok(())
}

/// Adds a task by hand (no AI) to the current tag.
#[tauri::command]
pub async fn add_taskmaster_task(path: String, title: String, description: String) -> Result<(), String> {
    let root = project_root(&path)?;
    let title = title.trim();
    if title.is_empty() {
        return Err("A task needs a title.".to_string());
    }
    let file = tasks_file(&root);
    if !file.is_file() {
        // `add-task` refuses to run until the tasks file exists.
        std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&file, r#"{"master":{"tasks":[],"metadata":{}}}"#).map_err(|e| e.to_string())?;
    }
    let description = if description.trim().is_empty() { title } else { description.trim() };
    run_tm(&root, &["add-task", "--title", title, "--description", description]).await?;
    Ok(())
}

/// Runs `task-master init` in a folder with defaults and Claude Code rules.
#[tauri::command]
pub async fn init_taskmaster(path: String) -> Result<(), String> {
    let root = project_root(&path)?;
    run_tm(&root, &["init", "--yes", "--rules", "claude", "--no-aliases", "--no-git", "--skip-install"]).await?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmFolder {
    path: String,
    /// The `origin` remote URL, used to match the folder to a project's repo.
    remote: Option<String>,
}

/// Reads `remote "origin"` → `url` from `.git/config`. Worktrees (where `.git` is a file) are skipped.
fn origin_remote(root: &Path) -> Option<String> {
    let config = std::fs::read_to_string(root.join(".git").join("config")).ok()?;
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line == r#"[remote "origin"]"#;
        } else if in_origin {
            if let Some(url) = line.strip_prefix("url").map(str::trim_start).and_then(|l| l.strip_prefix('=')) {
                return Some(url.trim().to_string());
            }
        }
    }
    None
}

/// Finds folders with a `.taskmaster/` directory: each root itself and its direct children.
#[tauri::command]
pub async fn find_taskmaster_folders(roots: Vec<String>) -> Result<Vec<TmFolder>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for root in roots.iter().map(|r| PathBuf::from(r.trim())).filter(|r| r.is_dir()) {
        let children = std::fs::read_dir(&root).map(|rd| rd.flatten().map(|e| e.path()).collect::<Vec<_>>()).unwrap_or_default();
        for dir in std::iter::once(root.clone()).chain(children) {
            if !dir.join(".taskmaster").is_dir() {
                continue;
            }
            let path = dir.to_string_lossy().to_string();
            if seen.insert(path.to_lowercase()) {
                out.push(TmFolder { remote: origin_remote(&dir), path });
            }
        }
    }
    out.sort_by_key(|f| f.path.to_lowercase());
    Ok(out)
}
