use futures_util::{future::FutureExt as _, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tiberius::{Client as SqlClient, Config};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

// ─── Pending Write State ───────────────────────────────────────────────────────

pub(crate) struct PendingWriteEntry {
    path: PathBuf,
    content: String,
}

pub struct PendingWriteState(pub Arc<Mutex<HashMap<String, PendingWriteEntry>>>);

impl PendingWriteState {
    pub fn new() -> Self {
        PendingWriteState(Arc::new(Mutex::new(HashMap::new())))
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// ─── SQL Server Tool ───────────────────────────────────────────────────────────

fn is_read_only_sql(query: &str) -> bool {
    let upper = query.trim().to_uppercase();
    // Must start with SELECT or WITH (CTEs)
    if !upper.starts_with("SELECT") && !upper.starts_with("WITH") {
        return false;
    }
    // Reject anything containing write/DDL keywords
    let blocked = [
        "INSERT ", "UPDATE ", "DELETE ", "DROP ", "CREATE ", "ALTER ",
        "TRUNCATE ", "EXEC ", "EXECUTE ", "MERGE ", "BULK ",
    ];
    !blocked.iter().any(|kw| upper.contains(kw))
}

/// Extract the primary table name from a simple FROM clause.
/// Returns None for subqueries or anything too complex to safely parse.
fn extract_primary_table(query: &str) -> Option<String> {
    let upper = query.to_uppercase();
    let from_pos = upper.find(" FROM ")? + 6;
    let after_from = query[from_pos..].trim_start();
    if after_from.starts_with('(') {
        return None; // subquery — can't safely determine table
    }
    let end = after_from
        .find(|c: char| c.is_whitespace() || c == ',' || c == ')')
        .unwrap_or(after_from.len());
    let raw = after_from[..end].trim_matches(|c| c == '[' || c == ']' || c == '"');
    raw.split('.')
        .last()
        .map(|s| s.trim_matches(|c| c == '[' || c == ']' || c == '"').to_string())
        .filter(|s| !s.is_empty())
}

/// Rewrite `SELECT [TOP N] * FROM …` to use explicit safe columns.
/// Returns None if the SELECT list isn't a simple star that can be replaced.
fn rewrite_select_star(query: &str, cols: &[String]) -> Option<String> {
    let upper = query.to_uppercase();
    let from_pos = upper.find(" FROM ")?;
    let select_clause = query[..from_pos].trim();
    let from_onward = &query[from_pos..];
    let upper_clause = select_clause.to_uppercase();

    let col_list = cols
        .iter()
        .map(|c| format!("[{}]", c.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");

    // Plain SELECT *
    if upper_clause == "SELECT *" {
        return Some(format!("SELECT {}{}", col_list, from_onward));
    }

    // SELECT TOP N * or SELECT TOP(N) *
    if upper_clause.starts_with("SELECT TOP") && select_clause.trim_end().ends_with('*') {
        let star_pos = select_clause.rfind('*')?;
        let prefix = select_clause[..star_pos].trim_end(); // e.g. "SELECT TOP 5"
        let after_top = prefix["SELECT TOP".len()..]
            .trim()
            .trim_matches(|c| c == '(' || c == ')');
        if after_top.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("{} {}{}", prefix, col_list, from_onward));
        }
    }

    None
}

/// Convert any SQL Server column value to a display string without panicking.
/// `simple_query` stores typed ColumnData; calling get::<&str> on an INT panics,
/// so we walk the common types via try_get and return the first that succeeds.
fn cell_to_string(row: &tiberius::Row, idx: usize) -> String {
    macro_rules! try_as {
        ($t:ty) => {
            if let Ok(v) = row.try_get::<$t, _>(idx) {
                return v.map(|n| n.to_string()).unwrap_or_else(|| "NULL".to_string());
            }
        };
    }
    // String types
    if let Ok(v) = row.try_get::<&str, _>(idx) {
        return v.unwrap_or("NULL").to_string();
    }
    // Numeric types (try widest first so narrower conversions don't silently truncate)
    try_as!(i64);
    try_as!(i32);
    try_as!(i16);
    try_as!(u8);
    try_as!(f64);
    try_as!(f32);
    // Boolean (BIT)
    try_as!(bool);
    // Unrecognised / unsupported type (e.g. geography, hierarchyid)
    "?".to_string()
}

/// Called when a query panics on a UDT column. Automatically fetches safe column
/// names via query_sqlserver (same proven path), rewrites SELECT *, and retries.
async fn auto_recover_udt(connection_string: &str, original_query: &str) -> String {
    let table_name = match extract_primary_table(original_query) {
        Some(t) => t,
        None => return "Error: query hit an unsupported column type (UDT/geometry/geography). \
                        Could not auto-detect the table — try selecting specific columns by name."
            .to_string(),
    };

    let cols_query = format!(
        "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_NAME = N'{}' \
         AND DATA_TYPE NOT IN ('geometry','geography','hierarchyid','sql_variant') \
         ORDER BY ORDINAL_POSITION",
        table_name.replace('\'', "''")
    );

    let cols_output = Box::pin(query_sqlserver(connection_string, &cols_query)).await;

    if cols_output.starts_with("Error") || cols_output == "Query returned 0 rows" {
        return format!(
            "Error: table '{}' contains an unsupported column type and safe columns could not be retrieved.",
            table_name
        );
    }

    // Output format: "COLUMN_NAME\n-----------\nCol1\nCol2\n...\n\n(N rows)"
    let safe_cols: Vec<String> = cols_output
        .lines()
        .skip(2) // header + divider
        .map(|l| l.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with('(') && !s.starts_with('-'))
        .collect();

    if safe_cols.is_empty() {
        return format!("Error: no supported columns found for table '{}'.", table_name);
    }

    match rewrite_select_star(original_query, &safe_cols) {
        Some(rewritten) => Box::pin(query_sqlserver(connection_string, &rewritten)).await,
        None => format!(
            "Could not auto-rewrite the query. The supported columns for '{}' are: {}",
            table_name,
            safe_cols
                .iter()
                .map(|c| format!("[{}]", c))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

async fn query_sqlserver(connection_string: &str, query: &str) -> String {
    if !is_read_only_sql(query) {
        return "Error: only SELECT and WITH (CTE) queries are allowed".to_string();
    }

    let config = match Config::from_ado_string(connection_string) {
        Ok(c) => c,
        Err(e) => return format!("Error: invalid connection string — {}", e),
    };
    let addr = config.get_addr();
    let tcp = match TcpStream::connect(addr).await {
        Ok(t) => t,
        Err(e) => return format!("Error: could not reach server — {}", e),
    };
    let _ = tcp.set_nodelay(true);
    let mut client = match SqlClient::connect(config, tcp.compat_write()).await {
        Ok(c) => c,
        Err(e) => return format!("Error: connection failed — {}", e),
    };

    // tiberius panics (not errors) when it encounters UDT/geometry/geography columns because
    // column metadata is parsed eagerly inside query() itself, before into_first_result().
    // Wrap both calls; on panic, auto_recover_udt rewrites the query using safe columns.
    let params: &[&dyn tiberius::ToSql] = &[];

    let stream = match std::panic::AssertUnwindSafe(client.query(query, params))
        .catch_unwind()
        .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return format!("Error executing query: {}", e),
        Err(_) => return auto_recover_udt(connection_string, query).await,
    };

    let rows = match std::panic::AssertUnwindSafe(stream.into_first_result())
        .catch_unwind()
        .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return format!("Error reading results: {}", e),
        Err(_) => return auto_recover_udt(connection_string, query).await,
    };

    if rows.is_empty() {
        return "Query returned 0 rows".to_string();
    }

    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    let col_widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            rows.iter()
                .take(200)
                .map(|r| cell_to_string(r, i).len())
                .max()
                .unwrap_or(0)
                .max(name.len())
                .min(60)
        })
        .collect();

    let header: String = columns
        .iter()
        .zip(&col_widths)
        .map(|(name, w)| format!("{:<width$}", name, width = w))
        .collect::<Vec<_>>()
        .join("  ");

    let divider: String = col_widths
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("  ");

    let max_rows = 200;
    let truncated = rows.len() > max_rows;

    let body: String = rows
        .iter()
        .take(max_rows)
        .map(|row| {
            (0..columns.len())
                .zip(&col_widths)
                .map(|(i, w)| {
                    let val = cell_to_string(row, i);
                    let trimmed = &val[..val.len().min(60)];
                    format!("{:<width$}", trimmed, width = w)
                })
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut out = format!("{}\n{}\n{}", header, divider, body);
    if truncated {
        out.push_str(&format!("\n\n... showing {} of {} rows", max_rows, rows.len()));
    } else {
        out.push_str(&format!(
            "\n\n({} row{})",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ));
    }
    out
}

async fn get_schema_sqlserver(connection_string: &str) -> String {
    let schema_query = "\
        SELECT \
            t.TABLE_SCHEMA, \
            t.TABLE_NAME, \
            c.COLUMN_NAME, \
            c.DATA_TYPE, \
            c.CHARACTER_MAXIMUM_LENGTH, \
            c.IS_NULLABLE, \
            c.COLUMN_DEFAULT \
        FROM INFORMATION_SCHEMA.TABLES t \
        JOIN INFORMATION_SCHEMA.COLUMNS c \
            ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME \
        WHERE t.TABLE_TYPE = 'BASE TABLE' \
        ORDER BY t.TABLE_SCHEMA, t.TABLE_NAME, c.ORDINAL_POSITION";

    query_sqlserver(connection_string, schema_query).await
}

// ─── Tool Definitions ──────────────────────────────────────────────────────────

fn build_tools(has_files: bool, has_db: bool) -> serde_json::Value {
    let mut tools: Vec<serde_json::Value> = Vec::new();

    // write_file and read_file are always available so Claude can read then edit any file.
    // When a project path is set, use a relative path; otherwise use an absolute path.
    tools.push(json!({
        "name": "write_file",
        "description": "Write or create a file. Use a path relative to the project root when a project is configured, or an absolute path otherwise. Call this tool immediately whenever you want to make any code or text change — do NOT describe the change in chat or say 'go ahead and approve'. The user will see a Yes/No button in the UI. Always provide the complete new file content.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Relative path from project root, or absolute path if no project is set" },
                "content": { "type": "string", "description": "Complete new file content to write" }
            },
            "required": ["path", "content"]
        }
    }));
    tools.push(json!({
        "name": "read_file",
        "description": "Read the contents of a file. Use a relative path from the project root when a project is configured, or an absolute path otherwise. Always read a file before calling write_file so you can provide the complete updated content.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Relative path from project root, or absolute path if no project is configured" }
            },
            "required": ["path"]
        }
    }));

    if has_files {
        tools.push(json!({
            "name": "list_files",
            "description": "List files in the project directory. Returns relative paths of all non-ignored source files.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Subdirectory relative to project root. Defaults to root." }
                },
                "required": []
            }
        }));
        tools.push(json!({
            "name": "search_code",
            "description": "Search for a string pattern across project files. Returns matching lines with file paths and line numbers.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "String to search for (case-insensitive)" },
                    "path": { "type": "string", "description": "Subdirectory to search in. Defaults to root." }
                },
                "required": ["pattern"]
            }
        }));
    }

    if has_db {
        tools.push(json!({
            "name": "get_database_schema",
            "description": "Get all tables and columns in the database. Always call this first before querying to understand the schema.",
            "input_schema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }));
        tools.push(json!({
            "name": "query_database",
            "description": "Execute a read-only SELECT query against the project database. Only SELECT and WITH (CTE) statements are permitted.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "SQL SELECT or WITH query to execute" }
                },
                "required": ["query"]
            }
        }));
    }

    serde_json::Value::Array(tools)
}

// ─── Tool Execution ────────────────────────────────────────────────────────────

const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    ".git",
    ".svn",
    "build",
    "__pycache__",
    ".next",
    ".nuxt",
    "vendor",
    "coverage",
    ".turbo",
    "out",
];

const BINARY_EXTENSIONS: &[&str] = &[
    "exe", "dll", "so", "dylib", "bin", "obj", "o", "a", "lib", "pdb", "png", "jpg", "jpeg",
    "gif", "ico", "webp", "bmp", "tiff", "woff", "woff2", "ttf", "eot", "otf", "zip", "tar",
    "gz", "7z", "rar", "pdf", "lock", "mp4", "mp3", "wav", "ogg", "svg",
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

/// Resolves a project-relative path and verifies it stays inside the project root.
fn safe_path(project_root: &str, relative: &str) -> Result<PathBuf, String> {
    let base = fs::canonicalize(project_root)
        .map_err(|e| format!("Invalid project path: {}", e))?;
    let full = base.join(relative);
    let canonical = fs::canonicalize(&full)
        .map_err(|_| format!("'{}' does not exist", relative))?;
    if canonical.starts_with(&base) {
        Ok(canonical)
    } else {
        Err("Path is outside the project directory".to_string())
    }
}

fn read_file_tool(project_root: &str, path_str: &str) -> String {
    let path = if project_root.is_empty() {
        let p = PathBuf::from(path_str);
        if !p.is_absolute() {
            return "Error: no project is configured — provide an absolute file path".to_string();
        }
        p
    } else {
        match safe_path(project_root, path_str) {
            Ok(p) => p,
            Err(e) => return format!("Error: {}", e),
        }
    };
    match fs::metadata(&path) {
        Ok(m) if m.len() > 200_000 => {
            return format!(
                "File is too large ({} bytes). Only files under 200 KB are readable.",
                m.len()
            )
        }
        Err(e) => return format!("Error: {}", e),
        _ => {}
    }
    match fs::read_to_string(&path) {
        Ok(content) => {
            let numbered = content
                .lines()
                .enumerate()
                .map(|(i, line)| format!("{:4}\t{}", i + 1, line))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{}\n{}", path_str, numbered)
        }
        Err(_) => "Error: file is not valid UTF-8 (binary file)".to_string(),
    }
}

/// Like safe_path but allows the target file to not yet exist.
/// Only requires that the parent directory chain eventually resolves inside the project root.
fn safe_path_for_write(project_root: &str, relative: &str) -> Result<PathBuf, String> {
    if relative.contains("..") {
        return Err("Path traversal not allowed".to_string());
    }
    let base = fs::canonicalize(project_root)
        .map_err(|e| format!("Invalid project path: {}", e))?;
    let full = base.join(relative);
    // Walk upward until we find an existing ancestor we can canonicalize.
    let mut check = full.clone();
    loop {
        if check.exists() {
            let canonical = fs::canonicalize(&check).map_err(|e| e.to_string())?;
            if !canonical.starts_with(&base) {
                return Err("Path is outside the project directory".to_string());
            }
            break;
        }
        match check.parent() {
            Some(p) if p != check => check = p.to_path_buf(),
            _ => return Err("Cannot resolve path inside project directory".to_string()),
        }
    }
    Ok(full)
}

fn write_file_tool(
    app: &AppHandle,
    pending: Arc<Mutex<HashMap<String, PendingWriteEntry>>>,
    tool_use_id: &str,
    project_root: &str,
    file_path: &str,
    content: &str,
) -> String {
    if file_path.is_empty() {
        return "Error: path is required".to_string();
    }
    if file_path.contains("..") {
        return "Error: path traversal not allowed".to_string();
    }
    let path = if project_root.is_empty() {
        PathBuf::from(file_path)
    } else {
        match safe_path_for_write(project_root, file_path) {
            Ok(p) => p,
            Err(e) => return format!("Error: {}", e),
        }
    };

    let current_content = fs::read_to_string(&path).unwrap_or_default();

    // Store the pending write — the actual write happens only after user approves.
    pending.lock().unwrap().insert(
        tool_use_id.to_string(),
        PendingWriteEntry { path, content: content.to_string() },
    );

    // Notify the frontend. Returns immediately — no blocking wait.
    let _ = app.emit("claude:edit_request", json!({
        "toolUseId": tool_use_id,
        "filePath": file_path,
        "currentContent": current_content,
        "newContent": content
    }));

    format!("Edit proposed for {}. Waiting for user approval in the UI.", file_path)
}

#[tauri::command]
pub async fn confirm_write(
    state: tauri::State<'_, PendingWriteState>,
    tool_use_id: String,
    approved: bool,
) -> Result<(), String> {
    let entry = {
        let mut map = state.0.lock().unwrap();
        map.remove(&tool_use_id)
    };
    if let Some(e) = entry {
        if approved {
            if let Some(parent) = e.path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&e.path, &e.content).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn collect_files(dir: &Path, project_root: &Path, entries: &mut Vec<String>, depth: usize) {
    if entries.len() >= 300 || depth > 8 {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };

    let mut items: Vec<_> = read.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| {
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        // dirs first, then alphabetical
        (!is_dir, e.file_name().to_string_lossy().to_lowercase())
    });

    for entry in items {
        if entries.len() >= 300 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            collect_files(&path, project_root, entries, depth + 1);
        } else {
            if is_binary(&path) {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(project_root) {
                entries.push(normalize_slashes(rel));
            }
        }
    }
}

fn list_files_tool(project_root: &str, relative_path: &str) -> String {
    let start = if relative_path.is_empty() || relative_path == "." {
        match fs::canonicalize(project_root) {
            Ok(p) => p,
            Err(e) => return format!("Error: {}", e),
        }
    } else {
        match safe_path(project_root, relative_path) {
            Ok(p) => p,
            Err(e) => return format!("Error: {}", e),
        }
    };

    let root = match fs::canonicalize(project_root) {
        Ok(p) => p,
        Err(e) => return format!("Error: {}", e),
    };

    let mut entries: Vec<String> = Vec::new();
    collect_files(&start, &root, &mut entries, 0);

    if entries.is_empty() {
        return "No files found".to_string();
    }
    let label = if relative_path.is_empty() { "." } else { relative_path };
    let truncated = entries.len() == 300;
    let mut out = format!("Files in {}:\n{}", label, entries.join("\n"));
    if truncated {
        out.push_str("\n... (truncated at 300 files)");
    }
    out
}

fn search_recursive(
    dir: &Path,
    project_root: &Path,
    pattern: &str,
    matches: &mut Vec<String>,
) {
    if matches.len() >= 50 {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut items: Vec<_> = read.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());

    for entry in items {
        if matches.len() >= 50 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            search_recursive(&path, project_root, pattern, matches);
        } else {
            if is_binary(&path) {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                let rel = path
                    .strip_prefix(project_root)
                    .map(normalize_slashes)
                    .unwrap_or_else(|_| name.clone());
                for (i, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(pattern) {
                        matches.push(format!("{}:{}: {}", rel, i + 1, line.trim()));
                        if matches.len() >= 50 {
                            break;
                        }
                    }
                }
            }
        }
    }
}

fn search_code_tool(project_root: &str, relative_path: &str, pattern: &str) -> String {
    if pattern.is_empty() {
        return "Error: pattern is required".to_string();
    }
    let search_dir = if relative_path.is_empty() || relative_path == "." {
        match fs::canonicalize(project_root) {
            Ok(p) => p,
            Err(e) => return format!("Error: {}", e),
        }
    } else {
        match safe_path(project_root, relative_path) {
            Ok(p) => p,
            Err(e) => return format!("Error: {}", e),
        }
    };

    let root = match fs::canonicalize(project_root) {
        Ok(p) => p,
        Err(e) => return format!("Error: {}", e),
    };

    let mut matches: Vec<String> = Vec::new();
    search_recursive(&search_dir, &root, &pattern.to_lowercase(), &mut matches);

    if matches.is_empty() {
        return format!("No matches found for '{}'", pattern);
    }
    let truncated = matches.len() == 50;
    let mut out = format!("Matches for '{}':\n{}", pattern, matches.join("\n"));
    if truncated {
        out.push_str("\n... (truncated at 50 matches)");
    }
    out
}

async fn execute_tool(
    name: &str,
    input: &serde_json::Value,
    project_root: &str,
    db_connection_string: Option<&str>,
    app: &AppHandle,
    pending: Arc<Mutex<HashMap<String, PendingWriteEntry>>>,
    tool_use_id: &str,
) -> String {
    match name {
        "read_file" => read_file_tool(project_root, input["path"].as_str().unwrap_or("")),
        "list_files" => list_files_tool(project_root, input["path"].as_str().unwrap_or(".")),
        "search_code" => search_code_tool(
            project_root,
            input["path"].as_str().unwrap_or("."),
            input["pattern"].as_str().unwrap_or(""),
        ),
        "write_file" => write_file_tool(
            app,
            pending,
            tool_use_id,
            project_root,
            input["path"].as_str().unwrap_or(""),
            input["content"].as_str().unwrap_or(""),
        ),
        "get_database_schema" => match db_connection_string {
            Some(conn) => get_schema_sqlserver(conn).await,
            None => "Error: no database connection configured for this project".to_string(),
        },
        "query_database" => match db_connection_string {
            Some(conn) => query_sqlserver(conn, input["query"].as_str().unwrap_or("")).await,
            None => "Error: no database connection configured for this project".to_string(),
        },
        _ => format!("Unknown tool: {}", name),
    }
}

// ─── SSE Stream ────────────────────────────────────────────────────────────────

#[derive(Default)]
struct BlockBuilder {
    block_type: String,
    id: String,
    name: String,
    text: String,
    input_json: String,
}

/// Stream one request. Emits `claude:delta` for text chunks.
/// Returns (content blocks, stop_reason) when the stream ends.
async fn do_stream(
    app: &AppHandle,
    client: &Client,
    api_key: &str,
    body: serde_json::Value,
) -> Result<(Vec<BlockBuilder>, String), String> {
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

    let mut stream = response.bytes_stream();
    let mut buf = String::new();
    let mut blocks: HashMap<usize, BlockBuilder> = HashMap::new();
    let mut stop_reason = String::from("end_turn");

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

            let Ok(event) = serde_json::from_str::<serde_json::Value>(&data) else {
                continue;
            };

            match event["type"].as_str() {
                Some("content_block_start") => {
                    let idx = event["index"].as_u64().unwrap_or(0) as usize;
                    let cb = &event["content_block"];
                    let mut b = BlockBuilder {
                        block_type: cb["type"].as_str().unwrap_or("").to_string(),
                        ..Default::default()
                    };
                    if b.block_type == "tool_use" {
                        b.id = cb["id"].as_str().unwrap_or("").to_string();
                        b.name = cb["name"].as_str().unwrap_or("").to_string();
                    }
                    blocks.insert(idx, b);
                }
                Some("content_block_delta") => {
                    let idx = event["index"].as_u64().unwrap_or(0) as usize;
                    let delta = &event["delta"];
                    if let Some(b) = blocks.get_mut(&idx) {
                        match delta["type"].as_str() {
                            Some("text_delta") => {
                                if let Some(text) = delta["text"].as_str() {
                                    b.text.push_str(text);
                                    let _ = app.emit("claude:delta", text.to_string());
                                }
                            }
                            Some("input_json_delta") => {
                                if let Some(partial) = delta["partial_json"].as_str() {
                                    b.input_json.push_str(partial);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some("message_delta") => {
                    if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                        stop_reason = reason.to_string();
                    }
                }
                _ => {}
            }
        }
    }

    let mut ordered: Vec<(usize, BlockBuilder)> = blocks.into_iter().collect();
    ordered.sort_by_key(|(i, _)| *i);
    Ok((ordered.into_iter().map(|(_, b)| b).collect(), stop_reason))
}

// ─── Local Claude CLI ──────────────────────────────────────────────────────────

async fn stream_message_local(
    app: &AppHandle,
    messages: Vec<Message>,
    model: String,
    system: Option<String>,
    project_path: Option<String>,
    db_connection_string: Option<String>,
) -> Result<(), String> {
    let mut system_text = system.unwrap_or_default();

    // File editing is not available via the CLI path — override any instructions
    // that tell Claude to use write_file or promise to apply file changes.
    let no_write_note = "File editing tools are not available in this session. When asked to make any code or text changes, show the complete updated file content inside a code block so the user can apply it manually. Do NOT say 'grant permission', 'when the prompt appears', or promise to write files.";
    if system_text.is_empty() {
        system_text = no_write_note.to_string();
    } else {
        system_text.push_str("\n\n");
        system_text.push_str(no_write_note);
    }

    // Inject DB schema when conversation references the database
    if let Some(ref conn) = db_connection_string {
        let recent_text: String = messages
            .iter()
            .rev()
            .take(4)
            .map(|m| m.content.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");

        let db_kw = [
            "database", "table", "query", "sql", "schema", "record",
            "column", "select", "where", "data",
        ];
        if db_kw.iter().any(|kw| recent_text.contains(kw)) {
            let _ = app.emit("claude:tool_use", json!({"name": "get_database_schema", "input": {}}));
            let schema = get_schema_sqlserver(conn).await;
            if !schema.starts_with("Error") {
                if !system_text.is_empty() {
                    system_text.push_str("\n\n");
                }
                system_text.push_str("Database schema:\n");
                system_text.push_str(&schema);
            }
        }
    }

    // Include prior turns in system prompt so claude -p stays stateless
    if messages.len() > 1 {
        if !system_text.is_empty() {
            system_text.push_str("\n\n");
        }
        system_text.push_str("Previous conversation:\n");
        for msg in messages.iter().take(messages.len() - 1) {
            let role = if msg.role == "user" { "User" } else { "Assistant" };
            system_text.push_str(&format!("{}: {}\n\n", role, msg.content));
        }
    }

    let current_msg = messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_default();

    let mut cmd = tokio::process::Command::new("claude");
    cmd.arg("--print")
        .arg("--verbose")
        .arg("--output-format")
        .arg("stream-json");

    if !model.is_empty() {
        cmd.arg("--model").arg(&model);
    }
    if !system_text.is_empty() {
        cmd.arg("--system-prompt").arg(&system_text);
    }
    if let Some(ref path) = project_path {
        cmd.current_dir(path);
    }

    cmd.arg(&current_msg)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        let msg = format!(
            "Failed to launch claude CLI: {}. Ensure 'claude' is installed and you are logged in.",
            e
        );
        let _ = app.emit("claude:error", &msg);
        msg
    })?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr_handle = child.stderr.take();
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut last_text_len = 0usize;

    while let Some(line) = reader.next_line().await.map_err(|e| e.to_string())? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
            match event["type"].as_str() {
                // Anthropic Messages API delta format
                Some("content_block_delta") => {
                    if let Some(text) = event["delta"]["text"].as_str() {
                        let _ = app.emit("claude:delta", text.to_string());
                    }
                }
                // Claude Code CLI stream-json format — emit only the new portion
                Some("assistant") => {
                    if let Some(content) = event["message"]["content"].as_array() {
                        for block in content {
                            if block["type"].as_str() == Some("text") {
                                if let Some(full_text) = block["text"].as_str() {
                                    if full_text.len() > last_text_len {
                                        let delta = &full_text[last_text_len..];
                                        let _ = app.emit("claude:delta", delta.to_string());
                                        last_text_len = full_text.len();
                                    }
                                }
                            }
                        }
                    }
                }
                Some("system") | Some("user") | Some("result") => {}
                _ => {}
            }
        }
    }

    let status = child.wait().await.map_err(|e| e.to_string())?;

    if !status.success() {
        let mut err_text = String::new();
        if let Some(mut se) = stderr_handle {
            let _ = se.read_to_string(&mut err_text).await;
        }
        let msg = if !err_text.trim().is_empty() {
            format!("Claude CLI error: {}", err_text.trim())
        } else {
            "Claude CLI failed. Make sure you are logged in with 'claude login'.".to_string()
        };
        let _ = app.emit("claude:error", &msg);
        return Err(msg);
    }

    let _ = app.emit("claude:done", ());
    Ok(())
}

async fn generate_title_local(user_message: &str, assistant_message: &str) -> Result<String, String> {
    let preview = &assistant_message[..assistant_message.len().min(500)];
    let prompt = format!(
        "Generate a concise 3-5 word title for this conversation. Respond with ONLY the title, no punctuation, no quotes, no explanation.\n\nUser: {}\n\nAssistant: {}",
        user_message, preview
    );

    let output = tokio::process::Command::new("claude")
        .arg("--print")
        .arg("--output-format")
        .arg("text")
        .arg(&prompt)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if title.is_empty() {
        return Err("empty title".to_string());
    }
    Ok(title)
}

// ─── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn stream_message(
    app: AppHandle,
    api_key: String,
    messages: Vec<Message>,
    model: String,
    system: Option<String>,
    project_path: Option<String>,
    db_connection_string: Option<String>,
) -> Result<(), String> {
    let use_local = crate::settings::read_config(&app)["use_local_claude"]
        .as_bool()
        .unwrap_or(false);

    if use_local {
        return stream_message_local(&app, messages, model, system, project_path, db_connection_string).await;
    }

    let client = Client::new();
    let tools = build_tools(project_path.is_some(), db_connection_string.is_some());

    // Always tell Claude to use write_file directly, never describe edits in text.
    let write_file_instruction = "IMPORTANT — FILE EDITING RULES:\n\
1. When you need to change any file (fix typos, edit code, update content), call the write_file tool immediately.\n\
2. Use read_file first to get the current content, then call write_file with the complete corrected content.\n\
3. Do NOT output the corrected content in your text response.\n\
4. Do NOT list what you changed. Do NOT describe the edits. Do NOT explain what you are about to do.\n\
5. Just call the tools — the application shows the diff and Yes/No buttons to the user automatically.\n\
Outputting file content or change summaries in chat instead of calling write_file is incorrect behavior.";
    let effective_system = match &system {
        Some(s) if !s.is_empty() => format!("{}\n\n{}", write_file_instruction, s),
        _ => write_file_instruction.to_string(),
    };

    let mut api_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();

    // After read_file executes, force the next turn to call a tool so Claude
    // can't respond with a text description instead of calling write_file.
    let mut force_tool_use = false;

    loop {
        let mut body = json!({
            "model": model,
            "max_tokens": 8096,
            "stream": true,
            "messages": api_messages,
            "system": effective_system,
        });
        body["tools"] = tools.clone();
        if force_tool_use {
            body["tool_choice"] = json!({"type": "any"});
            force_tool_use = false;
        }

        let (blocks, stop_reason) = do_stream(&app, &client, &api_key, body).await?;

        if stop_reason != "tool_use" {
            let _ = app.emit("claude:done", ());
            return Ok(());
        }

        // Append assistant turn with full content blocks (text + tool_use)
        let assistant_content: Vec<serde_json::Value> = blocks
            .iter()
            .map(|b| {
                if b.block_type == "text" {
                    json!({"type": "text", "text": b.text})
                } else {
                    let input: serde_json::Value =
                        serde_json::from_str(&b.input_json).unwrap_or(json!({}));
                    json!({"type": "tool_use", "id": b.id, "name": b.name, "input": input})
                }
            })
            .collect();
        api_messages.push(json!({"role": "assistant", "content": assistant_content}));

        // After read_file, force the next response to use a tool (prevents Claude
        // from responding with a text description of the file instead of write_file).
        if blocks.iter().any(|b| b.block_type == "tool_use" && b.name == "read_file") {
            force_tool_use = true;
        }

        // Execute each tool call and collect results
        let root = project_path.as_deref().unwrap_or("");
        let db_conn = db_connection_string.as_deref();
        let pending = app.state::<PendingWriteState>().0.clone();
        let mut tool_results: Vec<serde_json::Value> = Vec::new();
        for b in blocks.iter().filter(|b| b.block_type == "tool_use") {
            let input: serde_json::Value =
                serde_json::from_str(&b.input_json).unwrap_or(json!({}));
            let _ = app.emit("claude:tool_use", json!({"name": b.name, "input": input}));
            let result = execute_tool(&b.name, &input, root, db_conn, &app, pending.clone(), &b.id).await;
            tool_results.push(json!({
                "type": "tool_result",
                "tool_use_id": b.id,
                "content": result,
            }));
        }
        api_messages.push(json!({"role": "user", "content": tool_results}));
    }
}

#[tauri::command]
pub async fn generate_title(
    app: AppHandle,
    api_key: String,
    user_message: String,
    assistant_message: String,
) -> Result<String, String> {
    if assistant_message.is_empty() {
        return Err("empty assistant message".to_string());
    }

    let use_local = crate::settings::read_config(&app)["use_local_claude"]
        .as_bool()
        .unwrap_or(false);

    if use_local {
        return generate_title_local(&user_message, &assistant_message).await;
    }

    let client = Client::new();
    let preview = &assistant_message[..assistant_message.len().min(500)];
    let prompt = format!(
        "Generate a concise 3-5 word title for this conversation. Respond with ONLY the title, no punctuation, no quotes, no explanation.\n\nUser: {}\n\nAssistant: {}",
        user_message, preview
    );

    let body = json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 20,
        "messages": [{ "role": "user", "content": prompt }]
    });

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
        return Err(format!("API error {}", response.status().as_u16()));
    }

    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let title = json["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if title.is_empty() {
        return Err("empty title".to_string());
    }
    Ok(title)
}

#[tauri::command]
pub async fn pick_folder() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Select Project Folder")
        .pick_folder()
        .await
        .map(|f| f.path().to_string_lossy().to_string())
}
