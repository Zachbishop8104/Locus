use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter};
use serde_json::json;
use tokio::sync::oneshot;

use super::{Mode, PendingWriteEntry};
use super::interceptor::{EditSpec, ParsedAction};

type WaiterMap = Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>;

static WRITE_ID: AtomicU64 = AtomicU64::new(0);
pub(crate) fn next_write_id() -> String {
    format!("locus_write_{}", WRITE_ID.fetch_add(1, Ordering::SeqCst))
}

// ─── Tool Schemas & Prompts ───────────────────────────────────────────────────

const PLAN_MODE_RULES: &str = "\
## Plan mode is ON

You cannot change files right now. Investigate first: list, read, and search whatever you \
need to understand the code. Then reply with a concise implementation plan: the files you \
would change and what each change does. Do not write full file contents. End by asking the \
user to approve the plan. Once they approve, the editing tools become available.";

/// System prompt for API paths (Claude API, OpenAI-compat).
/// Tool descriptions live in the tools array — no tag syntax needed here.
pub(crate) fn api_system_prompt(user_system: Option<&str>, mode: Mode) -> String {
    let mut s = String::from("\
You are Locus, a coding assistant working inside the user's project. You have tools to list, \
read, and search project files, and (outside plan mode) to edit and write files.

- Answer questions directly. Short code snippets in chat are fine when explaining something.
- To change part of an existing file, call locus_edit with an exact old_string and its \
replacement. To create a file or rewrite most of one, call locus_write with the complete \
content. The user sees a diff and approves or rejects it — describing a change in prose does \
not apply it.
- Read a file before changing it. Never guess at contents.
- Make one file change per response. After the result comes back, continue with the next \
change until the task is done, then summarize what changed in a sentence or two.
- If a change is rejected, stop and wait for the user's direction.");
    if !mode.allows_write() {
        s.push_str("\n\n");
        s.push_str(PLAN_MODE_RULES);
    }
    if let Some(u) = user_system.filter(|u| !u.is_empty()) {
        s.push_str("\n\n");
        s.push_str(u);
    }
    s
}

pub(crate) fn openai_tools_schema(has_files: bool, allow_write: bool) -> Vec<serde_json::Value> {
    anthropic_tools_schema(has_files, allow_write).into_iter().map(|t| json!({
        "type": "function",
        "function": {
            "name": t["name"],
            "description": t["description"],
            "parameters": t["input_schema"],
        }
    })).collect()
}

pub(crate) fn anthropic_tools_schema(has_files: bool, allow_write: bool) -> Vec<serde_json::Value> {
    let mut tools = vec![
        json!({"name":"locus_read","description":"Read a file's full contents","input_schema":{"type":"object","properties":{"path":{"type":"string","description":"Path relative to project root"}},"required":["path"]}}),
    ];
    if allow_write {
        tools.push(json!({"name":"locus_edit","description":"Replace an exact string in an existing file. Prefer this over locus_write for changes to existing files. old_string must match the file exactly, including whitespace and indentation, without the line-number prefixes that locus_read adds. It must appear exactly once unless replace_all is true; include enough surrounding lines to make it unique.","input_schema":{"type":"object","properties":{"path":{"type":"string","description":"Path relative to project root"},"old_string":{"type":"string","description":"Exact text to replace"},"new_string":{"type":"string","description":"Replacement text"},"replace_all":{"type":"boolean","description":"Replace every occurrence (default false)"}},"required":["path","old_string","new_string"]}}));
        tools.push(json!({"name":"locus_write","description":"Create a new file, or overwrite one when most of it changes. Read it first if it already exists.","input_schema":{"type":"object","properties":{"path":{"type":"string","description":"Path relative to project root"},"content":{"type":"string","description":"Complete new file content"}},"required":["path","content"]}}));
    }
    if has_files {
        tools.push(json!({"name":"locus_list","description":"List files in the project","input_schema":{"type":"object","properties":{"path":{"type":"string","description":"Subdirectory to list (omit for root)"}},"required":[]}}));
        tools.push(json!({"name":"locus_search","description":"Search for text across all project files","input_schema":{"type":"object","properties":{"pattern":{"type":"string","description":"Text to search for"}},"required":["pattern"]}}));
    }
    tools
}

/// Map a native tool function name + JSON args back to a ParsedAction.
/// Malformed calls become an "invalid" action so the model still gets a
/// tool result for every tool call it made.
pub(crate) fn native_tool_to_action(fn_name: &str, args: &serde_json::Value) -> ParsedAction {
    let parsed = match fn_name {
        "locus_read"   => args["path"].as_str().map(|p| ParsedAction::new("read", p)),
        "locus_list"   => Some(ParsedAction::new("list", args["path"].as_str().unwrap_or(""))),
        "locus_search" => args["pattern"].as_str().map(|p| ParsedAction::new("search", p)),
        "locus_write"  => match (args["path"].as_str(), args["content"].as_str()) {
            (Some(p), Some(c)) => Some(ParsedAction::new("write", format!("{}\n{}", p, c))),
            _ => None,
        },
        "locus_edit"   => match (args["path"].as_str(), args["old_string"].as_str(), args["new_string"].as_str()) {
            (Some(p), Some(_), Some(_)) => Some(ParsedAction { args: args.clone(), ..ParsedAction::new("edit", p) }),
            _ => None,
        },
        _ => None,
    };
    parsed.unwrap_or_else(|| ParsedAction::new(
        "invalid",
        format!("Error: {} was called with missing or malformed arguments. Retry with valid arguments.", fn_name),
    ))
}

/// Reconstruct the JSON input object for a ParsedAction (used when storing
/// native tool calls back into the conversation history).
pub(crate) fn action_to_input(action: &ParsedAction) -> serde_json::Value {
    match action.name.as_str() {
        "read"   => json!({"path": action.content.trim()}),
        "list"   => json!({"path": action.content.trim()}),
        "search" => json!({"pattern": action.content.trim()}),
        "write"  => { let (p, c) = action.write_parts(); json!({"path": p, "content": c}) }
        "edit"   => match action.edit_parts() {
            Ok(spec) if spec.hunks.len() == 1 => json!({
                "path": spec.path, "old_string": spec.hunks[0].old,
                "new_string": spec.hunks[0].new, "replace_all": spec.replace_all,
            }),
            _ => json!({"path": action.write_parts().0}),
        },
        _        => json!({}),
    }
}

// ─── Tag-based System Prompt (local Claude CLI only) ─────────────────────────

pub(crate) fn tool_system_prompt(has_files: bool, mode: Mode) -> String {
    let mut s = String::from(
"You are Locus, a coding assistant working inside the user's project. You act on the project \
by writing locus tags in your reply. The app intercepts each tag, runs it, and sends you the \
result in the next message.

## Tools\n\n");

    s.push_str("<locus:read>path/to/file</locus:read>\n  Returns the file's contents. Use it whenever you need to see a file — never ask the user to paste it.\n\n");
    if has_files {
        s.push_str("<locus:list>optional/subdir</locus:list>\n  Lists files in the project.\n\n");
        s.push_str("<locus:search>pattern</locus:search>\n  Searches for text across all project files.\n\n");
    }
    if mode.allows_write() {
        s.push_str(
"<locus:write>path/to/file
COMPLETE FILE CONTENT
</locus:write>
  Creates or overwrites a file. First line is the path; everything after is the full new \
content. The user sees a diff and approves or rejects it. Putting file content in a markdown \
code block does NOT change the file — only this tag does.\n\n");
        s.push_str(
"<locus:edit>path/to/file
<<<<<<< SEARCH
exact existing lines
=======
replacement lines
>>>>>>> REPLACE
</locus:edit>
  Changes part of an existing file. Prefer this over locus:write for existing files. SEARCH \
must match the file exactly, including indentation, without the line-number prefixes that \
locus:read adds, and must appear only once — include enough surrounding lines to make it \
unique. One tag may contain several SEARCH/REPLACE blocks for the same file.\n\n");
    }

    s.push_str(
"## Rules

- Answer questions directly. Short code snippets in chat are fine when explaining something.
- Read a file before changing it.
- After a tool tag, stop writing — the result arrives in the next message.
- Use at most one locus:edit or locus:write per reply. Continue with the next change after the \
result comes back, and summarize the changes in a sentence or two when done.
- If a change is rejected, stop and wait for the user's direction.\n");

    if !mode.allows_write() {
        s.push('\n');
        s.push_str(PLAN_MODE_RULES);
        s.push('\n');
    }
    s
}

// ─── Action Execution ─────────────────────────────────────────────────────────

pub(crate) async fn execute_action(
    action: &ParsedAction,
    workspace: &Workspace,
    app: &AppHandle,
    pending: Arc<Mutex<HashMap<String, PendingWriteEntry>>>,
    waiter: WaiterMap,
    mode: Mode,
) -> String {
    match action.name.as_str() {
        "read"   => {
            let path = action.content.trim();
            match workspace.split(path) {
                Ok((root, _, rel)) => read_file_tool(root, rel, path),
                Err(e) => e,
            }
        }
        "list"   => workspace.list(action.content.trim()),
        "search" => workspace.search(action.content.trim()),
        "write" | "edit" if !mode.allows_write() => {
            "Error: plan mode is on, so files cannot be changed. Present your plan and ask the user to approve it.".to_string()
        }
        "write"  => {
            let (display, content) = action.write_parts();
            let resolved = workspace.split(display).and_then(|(root, _, rel)| resolve_write_path(root, rel));
            match resolved {
                Err(e) => e,
                Ok(path) if mode == Mode::AcceptEdits => write_file_direct(app, path, display, content),
                Ok(path) => write_file_tool(app, pending, waiter, &next_write_id(), path, display, content).await,
            }
        }
        "edit"   => {
            let edited = action.edit_parts().and_then(|mut spec| {
                let display = spec.path.clone();
                let (root, _, rel) = workspace.split(&display)?;
                spec.path = rel.to_string();
                let content = apply_edit(root, &spec)?;
                Ok((resolve_write_path(root, rel)?, display.clone(), content))
            });
            match edited {
                Err(e) => e,
                Ok((path, display, content)) if mode == Mode::AcceptEdits => write_file_direct(app, path, &display, &content),
                Ok((path, display, content)) => write_file_tool(app, pending, waiter, &next_write_id(), path, &display, &content).await,
            }
        }
        "invalid" => action.content.clone(),
        other => format!("Unknown tool: {}", other),
    }
}

// ─── Workspace ────────────────────────────────────────────────────────────────

struct Root {
    name: String,
    path: String,
}

/// The folders a chat works in. With one folder, paths are relative to it.
/// With several, every path starts with a folder's name (e.g. `api/src/main.rs`).
pub(crate) struct Workspace {
    roots: Vec<Root>,
}

impl Workspace {
    pub fn new(paths: &[String]) -> Self {
        let mut roots: Vec<Root> = Vec::new();
        for path in paths.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            let base = Path::new(path).file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "folder".to_string());
            let mut name = base.clone();
            let mut n = 2;
            while roots.iter().any(|r| r.name.eq_ignore_ascii_case(&name)) {
                name = format!("{}-{}", base, n);
                n += 1;
            }
            roots.push(Root { name, path: path.to_string() });
        }
        Workspace { roots }
    }

    pub fn has_files(&self) -> bool {
        !self.roots.is_empty()
    }

    /// Explains the folder prefixes to the model. None with fewer than two folders.
    pub fn prompt_note(&self) -> Option<String> {
        if self.roots.len() < 2 { return None; }
        let list = self.roots.iter().map(|r| format!("- {}/ ({})", r.name, r.path)).collect::<Vec<_>>().join("\n");
        Some(format!(
            "## Project folders\n\nThis chat works across {} folders. Start every file path with the folder \
             name, e.g. `{}/src/main.rs`. Listing with no path shows all of them.\n\n{}",
            self.roots.len(), self.roots[0].name, list,
        ))
    }

    /// Split a model-supplied path into (root folder, display prefix, path relative to that root).
    fn split<'a>(&'a self, path: &'a str) -> Result<(&'a str, String, &'a str), String> {
        match self.roots.as_slice() {
            [] => Ok(("", String::new(), path)),
            [only] => Ok((&only.path, String::new(), path)),
            roots => {
                let trimmed = path.trim_start_matches("./");
                let (head, rest) = trimmed.split_once(['/', '\\']).unwrap_or((trimmed, ""));
                match roots.iter().find(|r| r.name.eq_ignore_ascii_case(head)) {
                    Some(r) => Ok((&r.path, format!("{}/", r.name), rest)),
                    None => Err(format!(
                        "Error: '{}' does not start with a project folder. Start paths with one of: {}",
                        path, roots.iter().map(|r| format!("{}/", r.name)).collect::<Vec<_>>().join(", "),
                    )),
                }
            }
        }
    }

    fn list(&self, path: &str) -> String {
        let mut entries: Vec<String> = Vec::new();
        let result = if self.roots.len() > 1 && (path.is_empty() || path == ".") {
            self.roots.iter().try_for_each(|r| list_into(&r.path, "", &format!("{}/", r.name), &mut entries))
        } else {
            self.split(path).and_then(|(root, prefix, rel)| list_into(root, rel, &prefix, &mut entries))
        };
        if let Err(e) = result { return e; }
        if entries.is_empty() { return "No files found".to_string(); }
        let label = if path.is_empty() { "." } else { path };
        let mut out = format!("Files in {}:\n{}", label, entries.join("\n"));
        if entries.len() >= MAX_LIST { out.push_str("\n... (truncated at 300 files)"); }
        out
    }

    fn search(&self, pattern: &str) -> String {
        if pattern.is_empty() { return "Error: pattern is required".to_string(); }
        let roots: Vec<(&str, String)> = match self.roots.as_slice() {
            [] => vec![("", String::new())],
            [only] => vec![(only.path.as_str(), String::new())],
            roots => roots.iter().map(|r| (r.path.as_str(), format!("{}/", r.name))).collect(),
        };
        let mut matches: Vec<String> = Vec::new();
        for (root, prefix) in roots {
            if let Err(e) = search_into(root, &prefix, &pattern.to_lowercase(), &mut matches) { return e; }
        }
        if matches.is_empty() { return format!("No matches found for '{}'", pattern); }
        let mut out = format!("Matches for '{}':\n{}", pattern, matches.join("\n"));
        if matches.len() >= MAX_MATCHES { out.push_str("\n... (truncated at 50 matches)"); }
        out
    }
}

// ─── File Tools ───────────────────────────────────────────────────────────────

const IGNORED_DIRS: &[&str] = &[
    "node_modules", "target", "dist", ".git", ".svn", "build",
    "__pycache__", ".next", ".nuxt", "vendor", "coverage", ".turbo", "out",
];

const BINARY_EXTENSIONS: &[&str] = &[
    "exe", "dll", "so", "dylib", "bin", "obj", "o", "a", "lib", "pdb",
    "png", "jpg", "jpeg", "gif", "ico", "webp", "bmp", "tiff",
    "woff", "woff2", "ttf", "eot", "otf",
    "zip", "tar", "gz", "7z", "rar", "pdf", "lock",
    "mp4", "mp3", "wav", "ogg", "svg",
];

fn normalize_slashes(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn is_binary(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| BINARY_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn safe_path(project_root: &str, relative: &str) -> Result<PathBuf, String> {
    let base = fs::canonicalize(project_root).map_err(|e| format!("Invalid project path: {}", e))?;
    let full = base.join(relative);
    let canonical = fs::canonicalize(&full).map_err(|_| format!("'{}' does not exist", relative))?;
    if canonical.starts_with(&base) { Ok(canonical) } else { Err("Path is outside the project directory".to_string()) }
}

fn safe_path_for_write(project_root: &str, relative: &str) -> Result<PathBuf, String> {
    if relative.contains("..") { return Err("Path traversal not allowed".to_string()); }
    let base = fs::canonicalize(project_root).map_err(|e| format!("Invalid project path: {}", e))?;
    let full = base.join(relative);
    let mut check = full.clone();
    loop {
        if check.exists() {
            let canonical = fs::canonicalize(&check).map_err(|e| e.to_string())?;
            if !canonical.starts_with(&base) { return Err("Path is outside the project directory".to_string()); }
            break;
        }
        match check.parent() {
            Some(p) if p != check => check = p.to_path_buf(),
            _ => return Err("Cannot resolve path inside project directory".to_string()),
        }
    }
    Ok(full)
}

fn read_file_tool(project_root: &str, path_str: &str, display: &str) -> String {
    let path = if project_root.is_empty() {
        let p = PathBuf::from(path_str);
        if !p.is_absolute() { return "Error: no project is configured — provide an absolute file path".to_string(); }
        p
    } else {
        match safe_path(project_root, path_str) { Ok(p) => p, Err(e) => return format!("Error: {}", e) }
    };
    match fs::metadata(&path) {
        Ok(m) if m.len() > 200_000 => return format!("File is too large ({} bytes). Only files under 200 KB are readable.", m.len()),
        Err(e) => return format!("Error: {}", e),
        _ => {}
    }
    match fs::read_to_string(&path) {
        Ok(content) => {
            let numbered = content.lines().enumerate()
                .map(|(i, line)| format!("{:4}\t{}", i + 1, line))
                .collect::<Vec<_>>().join("\n");
            format!("{}\n{}", display, numbered)
        }
        Err(_) => "Error: file is not valid UTF-8 (binary file)".to_string(),
    }
}

fn resolve_write_path(project_root: &str, file_path: &str) -> Result<PathBuf, String> {
    if file_path.is_empty() { return Err("Error: path is required".to_string()); }
    if file_path.contains("..") { return Err("Error: path traversal not allowed".to_string()); }
    if project_root.is_empty() { return Ok(PathBuf::from(file_path)); }
    safe_path_for_write(project_root, file_path).map_err(|e| format!("Error: {}", e))
}

/// Apply exact-match replacements to a file and return the new content.
/// Nothing is written here; the result goes through the normal approval flow.
fn apply_edit(project_root: &str, spec: &EditSpec) -> Result<String, String> {
    let path = if project_root.is_empty() {
        let p = PathBuf::from(&spec.path);
        if !p.is_absolute() { return Err("Error: no project is configured — provide an absolute file path".to_string()); }
        p
    } else {
        safe_path(project_root, &spec.path).map_err(|e| format!("Error: {}", e))?
    };
    let mut content = fs::read_to_string(&path)
        .map_err(|e| format!("Error reading {}: {}. To create a new file, use the write tool.", spec.path, e))?;
    let crlf = content.contains("\r\n");

    for (i, hunk) in spec.hunks.iter().enumerate() {
        let which = if spec.hunks.len() > 1 { format!(" (block {})", i + 1) } else { String::new() };
        if hunk.old.is_empty() {
            return Err(format!("Error{}: old_string is empty. Use the write tool to create files.", which));
        }
        if hunk.old == hunk.new {
            return Err(format!("Error{}: old_string and new_string are identical.", which));
        }
        // Models write \n; match Windows line endings when the file uses them.
        let (old, new) = if crlf && !content.contains(&hunk.old) && !hunk.old.contains('\r') {
            (hunk.old.replace('\n', "\r\n"), hunk.new.replace('\n', "\r\n"))
        } else {
            (hunk.old.clone(), hunk.new.clone())
        };
        match content.matches(&old).count() {
            0 => return Err(format!(
                "Error{}: old_string was not found in {}. Read the file again and copy the text exactly, \
                 including indentation and without line-number prefixes.", which, spec.path)),
            1 => content = content.replacen(&old, &new, 1),
            _ if spec.replace_all => content = content.replace(&old, &new),
            n => return Err(format!(
                "Error{}: old_string matches {} places in {}. Include more surrounding lines so it is unique, \
                 or set replace_all to change every occurrence.", which, n, spec.path)),
        }
    }
    Ok(content)
}

/// Auto-accept mode: write immediately and tell the UI what changed.
fn write_file_direct(app: &AppHandle, path: PathBuf, file_path: &str, content: &str) -> String {
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) { return format!("Error: {}", e); }
    }
    match fs::write(&path, content) {
        Ok(()) => {
            let _ = app.emit("claude:file_written", json!({ "filePath": file_path }));
            format!("{} was written successfully.", file_path)
        }
        Err(e) => format!("Error writing {}: {}", file_path, e),
    }
}

pub(crate) async fn write_file_tool(
    app: &AppHandle,
    pending: Arc<Mutex<HashMap<String, PendingWriteEntry>>>,
    waiter: WaiterMap,
    tool_use_id: &str,
    path: PathBuf,
    file_path: &str,
    content: &str,
) -> String {
    let current_content = fs::read_to_string(&path).unwrap_or_default();
    pending.lock().unwrap().insert(
        tool_use_id.to_string(),
        PendingWriteEntry { path, content: content.to_string() },
    );

    // Register a oneshot channel so confirm_write can unblock this await.
    let (tx, rx) = oneshot::channel::<bool>();
    waiter.lock().unwrap().insert(tool_use_id.to_string(), tx);

    let _ = app.emit("claude:edit_request", json!({
        "toolUseId": tool_use_id,
        "filePath": file_path,
        "currentContent": current_content,
        "newContent": content
    }));

    // Block until the user accepts or rejects.
    match rx.await {
        Ok(true)  => format!("{} was written successfully.", file_path),
        Ok(false) => "__REJECTED__".to_string(),
        Err(_)    => "__REJECTED__".to_string(), // channel dropped = stream cancelled
    }
}

const MAX_LIST: usize = 300;
const MAX_MATCHES: usize = 50;

fn collect_files(dir: &Path, project_root: &Path, entries: &mut Vec<String>, depth: usize) {
    if entries.len() >= MAX_LIST || depth > 8 { return; }
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut items: Vec<_> = read.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| {
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        (!is_dir, e.file_name().to_string_lossy().to_lowercase())
    });
    for entry in items {
        if entries.len() >= MAX_LIST { break; }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if IGNORED_DIRS.contains(&name.as_str()) { continue; }
            collect_files(&path, project_root, entries, depth + 1);
        } else {
            if is_binary(&path) { continue; }
            if let Ok(rel) = path.strip_prefix(project_root) { entries.push(normalize_slashes(rel)); }
        }
    }
}

/// Append files under `root/rel` to `entries`, each path prefixed with `prefix`.
fn list_into(root: &str, rel: &str, prefix: &str, entries: &mut Vec<String>) -> Result<(), String> {
    let base = fs::canonicalize(root).map_err(|e| format!("Error: {}", e))?;
    let start = if rel.is_empty() || rel == "." { base.clone() } else { safe_path(root, rel).map_err(|e| format!("Error: {}", e))? };
    let from = entries.len();
    collect_files(&start, &base, entries, 0);
    for e in &mut entries[from..] { e.insert_str(0, prefix); }
    Ok(())
}

fn search_recursive(dir: &Path, project_root: &Path, pattern: &str, matches: &mut Vec<String>) {
    if matches.len() >= MAX_MATCHES { return; }
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut items: Vec<_> = read.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for entry in items {
        if matches.len() >= MAX_MATCHES { break; }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if IGNORED_DIRS.contains(&name.as_str()) { continue; }
            search_recursive(&path, project_root, pattern, matches);
        } else {
            if is_binary(&path) { continue; }
            if let Ok(content) = fs::read_to_string(&path) {
                let rel = path.strip_prefix(project_root).map(normalize_slashes).unwrap_or_else(|_| name.clone());
                for (i, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(pattern) {
                        matches.push(format!("{}:{}: {}", rel, i + 1, line.trim()));
                        if matches.len() >= MAX_MATCHES { break; }
                    }
                }
            }
        }
    }
}

/// Append matches for a lowercase `pattern` under `root` to `matches`, each path prefixed with `prefix`.
fn search_into(root: &str, prefix: &str, pattern: &str, matches: &mut Vec<String>) -> Result<(), String> {
    let base = fs::canonicalize(root).map_err(|e| format!("Error: {}", e))?;
    let from = matches.len();
    search_recursive(&base, &base, pattern, matches);
    for m in &mut matches[from..] { m.insert_str(0, prefix); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::interceptor::TagInterceptor;

    fn project_with(name: &str, content: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("locus_edit_test_{}_{}", name, std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("f.txt"), content).unwrap();
        let root = dir.to_string_lossy().to_string();
        (dir, root)
    }

    fn native_edit(old: &str, new: &str, replace_all: bool) -> ParsedAction {
        native_tool_to_action("locus_edit", &json!({
            "path": "f.txt", "old_string": old, "new_string": new, "replace_all": replace_all,
        }))
    }

    #[test]
    fn native_edit_replaces_unique_match() {
        let (dir, root) = project_with("unique", "fn a() {}\nfn b() {}\n");
        let spec = native_edit("fn b() {}", "fn b() { 1 }", false).edit_parts().unwrap();
        assert_eq!(apply_edit(&root, &spec).unwrap(), "fn a() {}\nfn b() { 1 }\n");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn ambiguous_match_errors_unless_replace_all() {
        let (dir, root) = project_with("ambiguous", "x = 1\nx = 1\n");
        let spec = native_edit("x = 1", "x = 2", false).edit_parts().unwrap();
        assert!(apply_edit(&root, &spec).unwrap_err().contains("matches 2 places"));
        let spec = native_edit("x = 1", "x = 2", true).edit_parts().unwrap();
        assert_eq!(apply_edit(&root, &spec).unwrap(), "x = 2\nx = 2\n");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_text_errors() {
        let (dir, root) = project_with("missing", "hello\n");
        let spec = native_edit("goodbye", "hi", false).edit_parts().unwrap();
        assert!(apply_edit(&root, &spec).unwrap_err().contains("was not found"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn crlf_files_match_lf_edits() {
        let (dir, root) = project_with("crlf", "line one\r\nline two\r\nline three\r\n");
        let spec = native_edit("line one\nline two", "line 1\nline 2", false).edit_parts().unwrap();
        assert_eq!(apply_edit(&root, &spec).unwrap(), "line 1\r\nline 2\r\nline three\r\n");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn tag_edit_with_multiple_blocks() {
        let (dir, root) = project_with("tag", "const a = 1;\n  const b = 2;\nconst c = 3;\n");
        let mut i = TagInterceptor::new();
        let shown = i.process(
            "Updating.<locus:edit>f.txt\n<<<<<<< SEARCH\nconst a = 1;\n=======\nconst a = 10;\n>>>>>>> REPLACE\n\
             <<<<<<< SEARCH\n  const b = 2;\n=======\n  const b = 20;\n>>>>>>> REPLACE\n</locus:edit>",
        );
        assert_eq!(shown, "Updating.");
        let action = i.take_actions().pop().unwrap();
        assert_eq!(action.name, "edit");
        let spec = action.edit_parts().unwrap();
        assert_eq!(spec.path, "f.txt");
        assert_eq!(spec.hunks.len(), 2);
        assert_eq!(apply_edit(&root, &spec).unwrap(), "const a = 10;\n  const b = 20;\nconst c = 3;\n");
        assert_eq!(action_to_input(&action), json!({"path": "f.txt"}));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn malformed_tag_edit_errors() {
        let action = ParsedAction::new("edit", "f.txt\njust some text");
        assert!(action.edit_parts().is_err());
    }

    #[test]
    fn malformed_native_edit_becomes_invalid() {
        let action = native_tool_to_action("locus_edit", &json!({"path": "f.txt"}));
        assert_eq!(action.name, "invalid");
    }

    #[test]
    fn multi_folder_paths_use_folder_prefix() {
        let (dir_a, root_a) = project_with("multi_a", "alpha\n");
        let (dir_b, root_b) = project_with("multi_b", "beta\n");
        let ws = Workspace::new(&[root_a.clone(), root_b.clone()]);
        let name_a = dir_a.file_name().unwrap().to_string_lossy().to_string();
        let name_b = dir_b.file_name().unwrap().to_string_lossy().to_string();

        let path_b = format!("{}/f.txt", name_b);
        let (root, prefix, rel) = ws.split(&path_b).unwrap();
        assert_eq!((root, prefix.as_str(), rel), (root_b.as_str(), format!("{}/", name_b).as_str(), "f.txt"));
        assert!(ws.split("f.txt").is_err());

        let listing = ws.list("");
        assert!(listing.contains(&format!("{}/f.txt", name_a)) && listing.contains(&format!("{}/f.txt", name_b)));
        assert!(ws.search("beta").contains(&format!("{}/f.txt:1: beta", name_b)));

        // A single folder keeps plain relative paths.
        let single = Workspace::new(&[root_a.clone()]);
        assert_eq!(single.split("f.txt").unwrap().2, "f.txt");
        assert!(single.list("").contains("\nf.txt"));

        fs::remove_dir_all(dir_a).ok();
        fs::remove_dir_all(dir_b).ok();
    }
}
