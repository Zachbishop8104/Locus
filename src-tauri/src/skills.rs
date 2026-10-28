use std::collections::HashSet;
use std::path::{Path, PathBuf};
use serde::Serialize;

// Skills and slash commands come from the same places Claude Code reads them:
//   ~/.claude/skills/<name>/SKILL.md          user skills
//   ~/.claude/commands/**/*.md                user commands
//   <folder>/.claude/skills|commands/...      project skills and commands
//   installed plugins' skills/ and commands/  named `plugin:name`
// Locus expands the chosen one into the message itself, so it works with every provider.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    name: String,
    description: String,
    /// user | project | plugin
    source: String,
    /// skill (a SKILL.md folder) | command (a single .md file, takes $ARGUMENTS)
    kind: String,
    path: String,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// Splits `---\nkey: value\n---\nbody` into (frontmatter, body).
fn split_frontmatter(text: &str) -> (&str, &str) {
    let t = text.trim_start_matches('\u{feff}');
    if let Some(rest) = t.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let body = &rest[end + 4..];
            return (&rest[..end], body.strip_prefix('\n').or(body.strip_prefix("\r\n")).unwrap_or(body));
        }
    }
    ("", t)
}

/// A top-level frontmatter value. Handles block values (`key: >` or `key: |`
/// followed by indented lines) by joining the lines with spaces.
fn field(frontmatter: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = frontmatter.lines().collect();
    let (i, v) = lines.iter().enumerate().find_map(|(i, l)| {
        let v = l.strip_prefix(key)?.trim_start().strip_prefix(':')?.trim();
        Some((i, v))
    })?;
    let value = if v.starts_with('>') || v.starts_with('|') {
        lines[i + 1..].iter()
            .take_while(|l| l.trim().is_empty() || l.starts_with(' ') || l.starts_with('\t'))
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        v.trim_matches(|c| c == '"' || c == '\'').to_string()
    };
    Some(value).filter(|v| !v.is_empty())
}

/// Frontmatter description, or the first line of text for plain command files.
fn describe(text: &str) -> String {
    let (fm, body) = split_frontmatter(text);
    field(fm, "description").unwrap_or_else(|| {
        body.lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("").to_string()
    })
}

fn push_skills(dir: &Path, prefix: &str, source: &str, out: &mut Vec<Skill>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let file = entry.path().join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let (fm, _) = split_frontmatter(&text);
        let name = field(fm, "name").unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
        out.push(Skill {
            name: format!("{}{}", prefix, name),
            description: describe(&text),
            source: source.to_string(),
            kind: "skill".to_string(),
            path: file.to_string_lossy().to_string(),
        });
    }
}

/// Commands in subfolders are namespaced: `commands/tm/add-task.md` → `tm:add-task`.
fn push_commands(dir: &Path, prefix: &str, source: &str, out: &mut Vec<Skill>) {
    fn walk(dir: &Path, ns: &str, prefix: &str, source: &str, out: &mut Vec<Skill>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if path.is_dir() {
                walk(&path, &format!("{}{}:", ns, entry.file_name().to_string_lossy()), prefix, source, out);
            } else if path.extension().is_some_and(|e| e == "md") {
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                out.push(Skill {
                    name: format!("{}{}{}", prefix, ns, stem),
                    description: describe(&text),
                    source: source.to_string(),
                    kind: "command".to_string(),
                    path: path.to_string_lossy().to_string(),
                });
            }
        }
    }
    walk(dir, "", prefix, source, out);
}

/// Install folders of enabled plugins, keyed by plugin name.
fn plugin_dirs(claude: &Path) -> Vec<(String, PathBuf)> {
    let Ok(text) = std::fs::read_to_string(claude.join("plugins").join("installed_plugins.json")) else { return Vec::new() };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    let disabled: HashSet<String> = std::fs::read_to_string(claude.join("settings.json")).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|s| s["enabledPlugins"].as_object().map(|m| m.iter()
            .filter(|(_, v)| v.as_bool() == Some(false)).map(|(k, _)| k.clone()).collect()))
        .unwrap_or_default();

    json["plugins"].as_object().map(|plugins| plugins.iter()
        .filter(|(id, _)| !disabled.contains(*id))
        .filter_map(|(id, installs)| {
            let path = installs.as_array()?.first()?["installPath"].as_str()?;
            Some((id.split('@').next().unwrap_or(id).to_string(), PathBuf::from(path)))
        })
        .collect()
    ).unwrap_or_default()
}

/// Every skill and command available to a chat working in `project_paths`.
/// Project entries come first so they win over user ones with the same name.
#[tauri::command]
pub async fn list_skills(project_paths: Vec<String>) -> Vec<Skill> {
    let mut out = Vec::new();
    for p in project_paths.iter().filter(|p| !p.trim().is_empty()) {
        let dir = Path::new(p.trim()).join(".claude");
        push_skills(&dir.join("skills"), "", "project", &mut out);
        push_commands(&dir.join("commands"), "", "project", &mut out);
    }
    if let Some(claude) = home().map(|h| h.join(".claude")) {
        push_skills(&claude.join("skills"), "", "user", &mut out);
        push_commands(&claude.join("commands"), "", "user", &mut out);
        for (plugin, dir) in plugin_dirs(&claude) {
            let prefix = format!("{}:", plugin);
            push_skills(&dir.join("skills"), &prefix, "plugin", &mut out);
            push_commands(&dir.join("commands"), &prefix, "plugin", &mut out);
        }
    }

    let mut seen = HashSet::new();
    out.retain(|s| seen.insert(s.name.to_lowercase()));
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// The instructions of a skill or command (its file without the frontmatter).
#[tauri::command]
pub async fn load_skill(path: String) -> Result<String, String> {
    let path = PathBuf::from(path);
    if path.extension().map_or(true, |e| e != "md") {
        return Err("Not a skill file.".to_string());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read {}: {}", path.display(), e))?;
    Ok(split_frontmatter(&text).1.trim().to_string())
}
