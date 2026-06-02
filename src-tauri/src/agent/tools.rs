use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter};
use serde_json::json;
use tiberius::{Client as SqlClient, Config};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use futures_util::future::FutureExt as _;

use super::{PendingWriteEntry};
use super::interceptor::ParsedAction;

static WRITE_ID: AtomicU64 = AtomicU64::new(0);
pub(crate) fn next_write_id() -> String {
    format!("locus_write_{}", WRITE_ID.fetch_add(1, Ordering::SeqCst))
}

// ─── Tool System Prompt ───────────────────────────────────────────────────────

pub(crate) fn tool_system_prompt(has_files: bool, has_db: bool) -> String {
    let mut s = String::from(
        "You have access to tools. Use them by placing these XML tags in your response. \
The application intercepts them, executes the action, and provides the result in the \
next message before you continue.\n\n",
    );
    s.push_str("<locus:read>path/to/file</locus:read>\n  Reads a file and returns its content.\n\n");
    s.push_str(
        "<locus:write>path/to/file\nCOMPLETE NEW FILE CONTENT HERE\n</locus:write>\n\
  Proposes an edit shown as a diff for user approval.\n\
  First line = file path. Everything after = complete new file content.\n\
  Always read the file first so you can provide the full updated content.\n\n",
    );
    if has_files {
        s.push_str("<locus:list>optional/subdir</locus:list>\n  Lists files in the project.\n\n");
        s.push_str("<locus:search>pattern</locus:search>\n  Searches for text across all project files.\n\n");
    }
    if has_db {
        s.push_str("<locus:query>SELECT ...</locus:query>\n  Runs a read-only SQL query against the project database.\n\n");
    }
    s.push_str(
        "Rules:\n\
- Always read a file before writing it.\n\
- Never paste full file content in chat when locus:write is available — use the tag.\n\
- After each tool tag, stop and wait. The result arrives in the next message.\n",
    );
    s
}

// ─── Action Execution ─────────────────────────────────────────────────────────

pub(crate) async fn execute_action(
    action: &ParsedAction,
    project_root: &str,
    db_conn: Option<&str>,
    app: &AppHandle,
    pending: Arc<Mutex<HashMap<String, PendingWriteEntry>>>,
) -> String {
    match action.name.as_str() {
        "read"   => read_file_tool(project_root, action.content.trim()),
        "list"   => list_files_tool(project_root, action.content.trim()),
        "search" => search_code_tool(project_root, ".", action.content.trim()),
        "write"  => {
            let (path, content) = action.write_parts();
            write_file_tool(app, pending, &next_write_id(), project_root, path, content)
        }
        "query" => match db_conn {
            Some(conn) => query_sqlserver(conn, action.content.trim()).await,
            None => "Error: no database configured for this project".to_string(),
        },
        other => format!("Unknown tool: {}", other),
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

pub(crate) fn read_file_tool(project_root: &str, path_str: &str) -> String {
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
            format!("{}\n{}", path_str, numbered)
        }
        Err(_) => "Error: file is not valid UTF-8 (binary file)".to_string(),
    }
}

pub(crate) fn write_file_tool(
    app: &AppHandle,
    pending: Arc<Mutex<HashMap<String, PendingWriteEntry>>>,
    tool_use_id: &str,
    project_root: &str,
    file_path: &str,
    content: &str,
) -> String {
    if file_path.is_empty() { return "Error: path is required".to_string(); }
    if file_path.contains("..") { return "Error: path traversal not allowed".to_string(); }
    let path = if project_root.is_empty() {
        PathBuf::from(file_path)
    } else {
        match safe_path_for_write(project_root, file_path) { Ok(p) => p, Err(e) => return format!("Error: {}", e) }
    };
    let current_content = fs::read_to_string(&path).unwrap_or_default();
    pending.lock().unwrap().insert(
        tool_use_id.to_string(),
        PendingWriteEntry { path, content: content.to_string() },
    );
    let _ = app.emit("claude:edit_request", json!({
        "toolUseId": tool_use_id,
        "filePath": file_path,
        "currentContent": current_content,
        "newContent": content
    }));
    format!("Edit proposed for {}. Waiting for user approval.", file_path)
}

fn collect_files(dir: &Path, project_root: &Path, entries: &mut Vec<String>, depth: usize) {
    if entries.len() >= 300 || depth > 8 { return; }
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut items: Vec<_> = read.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| {
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        (!is_dir, e.file_name().to_string_lossy().to_lowercase())
    });
    for entry in items {
        if entries.len() >= 300 { break; }
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

pub(crate) fn list_files_tool(project_root: &str, relative_path: &str) -> String {
    let start = if relative_path.is_empty() || relative_path == "." {
        match fs::canonicalize(project_root) { Ok(p) => p, Err(e) => return format!("Error: {}", e) }
    } else {
        match safe_path(project_root, relative_path) { Ok(p) => p, Err(e) => return format!("Error: {}", e) }
    };
    let root = match fs::canonicalize(project_root) { Ok(p) => p, Err(e) => return format!("Error: {}", e) };
    let mut entries: Vec<String> = Vec::new();
    collect_files(&start, &root, &mut entries, 0);
    if entries.is_empty() { return "No files found".to_string(); }
    let label = if relative_path.is_empty() { "." } else { relative_path };
    let truncated = entries.len() == 300;
    let mut out = format!("Files in {}:\n{}", label, entries.join("\n"));
    if truncated { out.push_str("\n... (truncated at 300 files)"); }
    out
}

fn search_recursive(dir: &Path, project_root: &Path, pattern: &str, matches: &mut Vec<String>) {
    if matches.len() >= 50 { return; }
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut items: Vec<_> = read.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for entry in items {
        if matches.len() >= 50 { break; }
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
                        if matches.len() >= 50 { break; }
                    }
                }
            }
        }
    }
}

pub(crate) fn search_code_tool(project_root: &str, relative_path: &str, pattern: &str) -> String {
    if pattern.is_empty() { return "Error: pattern is required".to_string(); }
    let search_dir = if relative_path.is_empty() || relative_path == "." {
        match fs::canonicalize(project_root) { Ok(p) => p, Err(e) => return format!("Error: {}", e) }
    } else {
        match safe_path(project_root, relative_path) { Ok(p) => p, Err(e) => return format!("Error: {}", e) }
    };
    let root = match fs::canonicalize(project_root) { Ok(p) => p, Err(e) => return format!("Error: {}", e) };
    let mut matches: Vec<String> = Vec::new();
    search_recursive(&search_dir, &root, &pattern.to_lowercase(), &mut matches);
    if matches.is_empty() { return format!("No matches found for '{}'", pattern); }
    let truncated = matches.len() == 50;
    let mut out = format!("Matches for '{}':\n{}", pattern, matches.join("\n"));
    if truncated { out.push_str("\n... (truncated at 50 matches)"); }
    out
}

// ─── SQL Tool ─────────────────────────────────────────────────────────────────

fn is_read_only_sql(query: &str) -> bool {
    let upper = query.trim().to_uppercase();
    if !upper.starts_with("SELECT") && !upper.starts_with("WITH") { return false; }
    let blocked = ["INSERT ", "UPDATE ", "DELETE ", "DROP ", "CREATE ", "ALTER ", "TRUNCATE ", "EXEC ", "EXECUTE ", "MERGE ", "BULK "];
    !blocked.iter().any(|kw| upper.contains(kw))
}

fn extract_primary_table(query: &str) -> Option<String> {
    let upper = query.to_uppercase();
    let from_pos = upper.find(" FROM ")? + 6;
    let after_from = query[from_pos..].trim_start();
    if after_from.starts_with('(') { return None; }
    let end = after_from.find(|c: char| c.is_whitespace() || c == ',' || c == ')').unwrap_or(after_from.len());
    let raw = after_from[..end].trim_matches(|c| c == '[' || c == ']' || c == '"');
    raw.split('.').last()
        .map(|s| s.trim_matches(|c| c == '[' || c == ']' || c == '"').to_string())
        .filter(|s| !s.is_empty())
}

fn rewrite_select_star(query: &str, cols: &[String]) -> Option<String> {
    let upper = query.to_uppercase();
    let from_pos = upper.find(" FROM ")?;
    let select_clause = query[..from_pos].trim();
    let from_onward = &query[from_pos..];
    let upper_clause = select_clause.to_uppercase();
    let col_list = cols.iter().map(|c| format!("[{}]", c.replace(']', "]]"))).collect::<Vec<_>>().join(", ");
    if upper_clause == "SELECT *" { return Some(format!("SELECT {}{}", col_list, from_onward)); }
    if upper_clause.starts_with("SELECT TOP") && select_clause.trim_end().ends_with('*') {
        let star_pos = select_clause.rfind('*')?;
        let prefix = select_clause[..star_pos].trim_end();
        let after_top = prefix["SELECT TOP".len()..].trim().trim_matches(|c| c == '(' || c == ')');
        if after_top.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("{} {}{}", prefix, col_list, from_onward));
        }
    }
    None
}

fn cell_to_string(row: &tiberius::Row, idx: usize) -> String {
    macro_rules! try_as { ($t:ty) => { if let Ok(v) = row.try_get::<$t, _>(idx) { return v.map(|n| n.to_string()).unwrap_or_else(|| "NULL".to_string()); } }; }
    if let Ok(v) = row.try_get::<&str, _>(idx) { return v.unwrap_or("NULL").to_string(); }
    try_as!(i64); try_as!(i32); try_as!(i16); try_as!(u8); try_as!(f64); try_as!(f32); try_as!(bool);
    "?".to_string()
}

async fn auto_recover_udt(connection_string: &str, original_query: &str) -> String {
    let table_name = match extract_primary_table(original_query) {
        Some(t) => t,
        None => return "Error: query hit an unsupported column type. Try selecting specific columns.".to_string(),
    };
    let cols_query = format!(
        "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_NAME = N'{}' \
         AND DATA_TYPE NOT IN ('geometry','geography','hierarchyid','sql_variant') ORDER BY ORDINAL_POSITION",
        table_name.replace('\'', "''")
    );
    let cols_output = Box::pin(query_sqlserver(connection_string, &cols_query)).await;
    if cols_output.starts_with("Error") || cols_output == "Query returned 0 rows" {
        return format!("Error: table '{}' contains an unsupported column type and safe columns could not be retrieved.", table_name);
    }
    let safe_cols: Vec<String> = cols_output.lines().skip(2)
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('(') && !s.starts_with('-'))
        .collect();
    if safe_cols.is_empty() { return format!("Error: no supported columns found for table '{}'.", table_name); }
    match rewrite_select_star(original_query, &safe_cols) {
        Some(rewritten) => Box::pin(query_sqlserver(connection_string, &rewritten)).await,
        None => format!("Could not auto-rewrite query. Supported columns for '{}': {}", table_name, safe_cols.iter().map(|c| format!("[{}]", c)).collect::<Vec<_>>().join(", ")),
    }
}

pub(crate) async fn query_sqlserver(connection_string: &str, query: &str) -> String {
    if !is_read_only_sql(query) { return "Error: only SELECT and WITH (CTE) queries are allowed".to_string(); }
    let config = match Config::from_ado_string(connection_string) { Ok(c) => c, Err(e) => return format!("Error: invalid connection string — {}", e) };
    let addr = config.get_addr();
    let tcp = match TcpStream::connect(addr).await { Ok(t) => t, Err(e) => return format!("Error: could not reach server — {}", e) };
    let _ = tcp.set_nodelay(true);
    let mut client = match SqlClient::connect(config, tcp.compat_write()).await { Ok(c) => c, Err(e) => return format!("Error: connection failed — {}", e) };
    let params: &[&dyn tiberius::ToSql] = &[];
    let stream = match std::panic::AssertUnwindSafe(client.query(query, params)).catch_unwind().await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return format!("Error executing query: {}", e),
        Err(_) => return auto_recover_udt(connection_string, query).await,
    };
    let rows = match std::panic::AssertUnwindSafe(stream.into_first_result()).catch_unwind().await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return format!("Error reading results: {}", e),
        Err(_) => return auto_recover_udt(connection_string, query).await,
    };
    if rows.is_empty() { return "Query returned 0 rows".to_string(); }
    let columns: Vec<String> = rows[0].columns().iter().map(|c| c.name().to_string()).collect();
    let col_widths: Vec<usize> = columns.iter().enumerate()
        .map(|(i, name)| rows.iter().take(200).map(|r| cell_to_string(r, i).len()).max().unwrap_or(0).max(name.len()).min(60))
        .collect();
    let header = columns.iter().zip(&col_widths).map(|(n, w)| format!("{:<width$}", n, width = w)).collect::<Vec<_>>().join("  ");
    let divider = col_widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("  ");
    let max_rows = 200;
    let truncated = rows.len() > max_rows;
    let body = rows.iter().take(max_rows)
        .map(|row| (0..columns.len()).zip(&col_widths).map(|(i, w)| { let v = cell_to_string(row, i); format!("{:<width$}", &v[..v.len().min(60)], width = w) }).collect::<Vec<_>>().join("  "))
        .collect::<Vec<_>>().join("\n");
    let mut out = format!("{}\n{}\n{}", header, divider, body);
    out.push_str(&if truncated { format!("\n\n... showing {} of {} rows", max_rows, rows.len()) } else { format!("\n\n({} row{})", rows.len(), if rows.len() == 1 { "" } else { "s" }) });
    out
}

pub(crate) async fn get_schema_sqlserver(connection_string: &str) -> String {
    query_sqlserver(connection_string, "\
        SELECT t.TABLE_SCHEMA, t.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, \
               c.CHARACTER_MAXIMUM_LENGTH, c.IS_NULLABLE, c.COLUMN_DEFAULT \
        FROM INFORMATION_SCHEMA.TABLES t \
        JOIN INFORMATION_SCHEMA.COLUMNS c \
            ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME \
        WHERE t.TABLE_TYPE = 'BASE TABLE' \
        ORDER BY t.TABLE_SCHEMA, t.TABLE_NAME, c.ORDINAL_POSITION").await
}
