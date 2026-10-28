use serde::Serialize;

// Jira issues come from an Atlassian CLI that is already signed in on the user's
// machine — Locus never stores a Jira token. `twg` is preferred; the official
// `acli` is the fallback.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssue {
    key: String,
    project: String,
    project_name: Option<String>,
    summary: String,
    status: String,
    /// new | indeterminate | done — Jira's fixed status categories.
    status_category: String,
    issue_type: Option<String>,
    priority: Option<String>,
    assignee: Option<String>,
    updated: Option<String>,
    url: Option<String>,
}

#[derive(Clone, Copy)]
enum Cli {
    Twg,
    Acli,
}

fn command(cli: Cli) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(match cli {
        Cli::Twg => "twg",
        Cli::Acli => "acli",
    });
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

/// Project keys are uppercase letters, digits and underscores. Anything else is dropped
/// so it can't change the meaning of the JQL.
fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Runs the CLI. `Ok(None)` means it isn't installed.
async fn run(cli: Cli, args: &[&str]) -> Result<Option<Vec<u8>>, String> {
    let output = match command(cli).args(args).output().await {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Failed to run the Jira CLI: {}", e)),
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let err = if err.is_empty() { String::from_utf8_lossy(&output.stdout).trim().to_string() } else { err };
        let lower = err.to_lowercase();
        if lower.contains("auth login") || lower.contains("twg login") || lower.contains("not logged")
            || lower.contains("unauthorized") || lower.contains("not authenticated") {
            return Err("JIRA_NOT_AUTHENTICATED".to_string());
        }
        return Err(if err.is_empty() { "The Jira CLI reported an error.".to_string() } else { err });
    }
    Ok(Some(output.stdout))
}

fn parse_json(stdout: &[u8]) -> Result<serde_json::Value, String> {
    serde_json::from_slice(stdout).map_err(|e| format!("Unexpected response from the Jira CLI: {}", e))
}

/// `https://site.atlassian.net/rest/api/3/issue/10001` → `https://site.atlassian.net/browse/KEY-1`
fn browse_url(self_url: Option<&str>, key: &str) -> Option<String> {
    let s = self_url?;
    let idx = s.find("/rest/")?;
    Some(format!("{}/browse/{}", &s[..idx], key))
}

/// Reads both twg's flat shape and the Jira REST shape (`fields.*`) that acli returns.
fn parse_issue(n: &serde_json::Value) -> Option<JiraIssue> {
    let key = n["key"].as_str()?.to_string();
    let f = if n["fields"].is_object() { &n["fields"] } else { n };
    let str_at = |v: &serde_json::Value| v.as_str().map(|s| s.to_string());
    Some(JiraIssue {
        project: str_at(&f["project"]["key"]).unwrap_or_else(|| key.split('-').next().unwrap_or("").to_string()),
        project_name: str_at(&f["project"]["name"]),
        summary: f["summary"].as_str().unwrap_or("").to_string(),
        status: f["status"]["name"].as_str().unwrap_or("").to_string(),
        status_category: f["status"]["statusCategory"]["key"].as_str().unwrap_or("new").to_string(),
        issue_type: str_at(&f["issueType"]["name"]).or_else(|| str_at(&f["issuetype"]["name"])),
        priority: str_at(&f["priority"]["name"]),
        assignee: str_at(&f["assignee"]["displayName"]),
        updated: str_at(&f["updated"]),
        url: str_at(&n["url"]).or_else(|| browse_url(n["self"].as_str(), &key)),
        key,
    })
}

/// Issues in the given projects, plus (with `include_mine`) anything assigned to you
/// anywhere. Done issues only show for two weeks after their last update.
#[tauri::command]
pub async fn get_jira_issues(projects: Vec<String>, include_mine: bool, mine_only: bool) -> Result<Vec<JiraIssue>, String> {
    let keys: Vec<String> = projects.iter().map(|k| k.trim().to_uppercase()).filter(|k| valid_key(k)).collect();

    let mut scope: Vec<String> = Vec::new();
    if !keys.is_empty() {
        scope.push(format!("project in ({})", keys.join(", ")));
    }
    if include_mine {
        scope.push("assignee = currentUser()".to_string());
    }
    if scope.is_empty() {
        return Ok(Vec::new());
    }
    let mut jql = format!("({}) AND (statusCategory != Done OR updated >= -14d)", scope.join(" OR "));
    if mine_only {
        jql.push_str(" AND assignee = currentUser()");
    }
    jql.push_str(" ORDER BY updated DESC");

    let fields = "summary,status,issuetype,priority,assignee,project,updated";
    let json = if let Some(out) = run(Cli::Twg, &[
        "jira", "workitem", "query", "--jql", &jql, "--fields", fields,
        "--limit", "100", "--output", "json", "--output-summary", "none",
    ]).await? {
        parse_json(&out)?
    } else if let Some(out) = run(Cli::Acli, &[
        "jira", "workitem", "search", "--jql", &jql, "--fields", &format!("key,{}", fields),
        "--limit", "100", "--json",
    ]).await? {
        parse_json(&out)?
    } else {
        return Err("JIRA_NOT_INSTALLED".to_string());
    };

    // twg wraps results in `data.issues`; acli prints a bare array or `{ issues }`.
    let list = if json.is_array() {
        &json
    } else if json["data"]["issues"].is_array() {
        &json["data"]["issues"]
    } else {
        &json["issues"]
    };
    Ok(list.as_array().map(|arr| arr.iter().filter_map(parse_issue).collect()).unwrap_or_default())
}

fn is_review(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("review") || n.contains("qa") || n.contains("test")
}

/// Picks the transition that lands an issue in a board column, given twg's list of
/// `{ id, name, toName, category }`. Workflows name statuses freely, so this goes by
/// status category, using the name only to tell review apart from other in-progress states.
fn pick_transition(transitions: &[serde_json::Value], column: &str) -> Option<String> {
    let to = |t: &serde_json::Value| t["toName"].as_str().or(t["name"].as_str()).unwrap_or("").to_string();
    let category = |t: &serde_json::Value| t["category"].as_str().unwrap_or("").to_lowercase();
    let found = transitions.iter().find(|t| {
        let (name, cat) = (to(t), category(t));
        match column {
            "todo" => cat == "to do" || cat == "new",
            "in_progress" => (cat == "in progress" || cat == "indeterminate") && !is_review(&name) && !name.to_lowercase().contains("block"),
            "review" => is_review(&name),
            "done" => cat == "done",
            _ => false,
        }
    });
    found.and_then(|t| t["id"].as_str().map(|s| s.to_string()))
}

/// Moves an issue into a board column (todo | in_progress | review | done) using
/// whichever transition its workflow offers from the current status.
#[tauri::command]
pub async fn set_jira_column(key: String, column: String) -> Result<(), String> {
    if !valid_key(&key) {
        return Err(format!("Invalid issue key: {}", key));
    }

    // Without --transition-id, twg lists the available transitions without changing anything.
    if let Some(out) = run(Cli::Twg, &[
        "jira", "workitem", "transition", "--id", &key, "--output", "json", "--output-summary", "none",
    ]).await? {
        let json = parse_json(&out)?;
        let transitions = json["data"]["transitions"].as_array().cloned().unwrap_or_default();
        let Some(id) = pick_transition(&transitions, &column) else {
            let options: Vec<String> = transitions.iter().filter_map(|t| t["toName"].as_str().map(|s| s.to_string())).collect();
            return Err(format!(
                "its workflow has no move to that column from here (available: {})",
                if options.is_empty() { "none".to_string() } else { options.join(", ") },
            ));
        };
        run(Cli::Twg, &["jira", "workitem", "transition", "--id", &key, "--transition-id", &id, "--output", "json"]).await?;
        return Ok(());
    }

    // acli only takes a target status name, so guess the common ones.
    let status = match column.as_str() {
        "todo" => "To Do",
        "in_progress" => "In Progress",
        "review" => "In Review",
        "done" => "Done",
        _ => return Err(format!("Unknown column: {}", column)),
    };
    match run(Cli::Acli, &["jira", "workitem", "transition", "--key", &key, "--status", status, "--yes"]).await? {
        Some(_) => Ok(()),
        None => Err("JIRA_NOT_INSTALLED".to_string()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueDetail {
    key: String,
    /// Markdown, converted from Jira's document format.
    description: String,
    issue_type: Option<String>,
    reporter: Option<String>,
    created: Option<String>,
    labels: Vec<String>,
    parent: Option<JiraIssueLink>,
    subtasks: Vec<JiraIssueLink>,
    comments: Vec<JiraComment>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueLink {
    key: String,
    summary: String,
    status: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraComment {
    author: Option<String>,
    created: Option<String>,
    /// Markdown.
    body: String,
}

/// Converts an Atlassian Document Format node to Markdown. Plain strings (older
/// Jira, wiki markup) pass through as they are.
fn adf_to_markdown(node: &serde_json::Value) -> String {
    if let Some(s) = node.as_str() {
        return s.to_string();
    }
    blocks(node, "").trim().to_string()
}

fn children(node: &serde_json::Value) -> &[serde_json::Value] {
    node["content"].as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

/// Block nodes, separated by blank lines. `indent` prefixes every line after the first,
/// for content nested in list items.
fn blocks(node: &serde_json::Value, indent: &str) -> String {
    children(node).iter().map(|n| block(n, indent)).filter(|s| !s.is_empty())
        .collect::<Vec<_>>().join(&format!("\n\n{}", indent))
}

fn block(n: &serde_json::Value, indent: &str) -> String {
    let attr = |k: &str| n["attrs"][k].as_str().unwrap_or("").to_string();
    match n["type"].as_str().unwrap_or("") {
        "paragraph" => inlines(n).replace('\n', &format!("\n{}", indent)),
        "heading" => {
            let level = n["attrs"]["level"].as_u64().unwrap_or(2).clamp(1, 6) as usize;
            format!("{} {}", "#".repeat(level), inlines(n))
        }
        "bulletList" | "orderedList" | "taskList" => {
            let ordered = n["type"] == "orderedList";
            let start = n["attrs"]["order"].as_u64().unwrap_or(1);
            children(n).iter().enumerate().map(|(i, item)| {
                let marker = if ordered {
                    format!("{}. ", start + i as u64)
                } else if item["type"] == "taskItem" {
                    if item["attrs"]["state"] == "DONE" { "- [x] ".to_string() } else { "- [ ] ".to_string() }
                } else {
                    "- ".to_string()
                };
                let inner = format!("{}{}", indent, " ".repeat(marker.len()));
                // Task items hold inline content directly; list items hold blocks.
                let body = if item["type"] == "taskItem" { inlines(item) } else { blocks(item, &inner) };
                format!("{}{}", marker, body)
            }).collect::<Vec<_>>().join(&format!("\n{}", indent))
        }
        "codeBlock" => {
            let code: String = children(n).iter().filter_map(|t| t["text"].as_str()).collect();
            format!("```{}\n{}\n```", attr("language"), code).replace('\n', &format!("\n{}", indent))
        }
        "blockquote" | "panel" => blocks(n, "").lines().map(|l| format!("> {}", l))
            .collect::<Vec<_>>().join(&format!("\n{}", indent)),
        "rule" => "---".to_string(),
        "table" => table(n),
        "mediaSingle" | "mediaGroup" => children(n).iter()
            .map(|m| format!("*[{}]*", m["attrs"]["alt"].as_str().filter(|s| !s.is_empty()).unwrap_or("attachment")))
            .collect::<Vec<_>>().join(" "),
        "expand" | "nestedExpand" => {
            let title = attr("title");
            let body = blocks(n, indent);
            if title.is_empty() { body } else { format!("**{}**\n\n{}{}", title, indent, body) }
        }
        // Unknown container: keep what's inside.
        _ if children(n).iter().any(|c| c["type"] == "text") => inlines(n),
        _ if !children(n).is_empty() => blocks(n, indent),
        _ => inline(n),
    }
}

fn table(n: &serde_json::Value) -> String {
    let rows: Vec<Vec<String>> = children(n).iter().map(|row| {
        children(row).iter().map(|cell| blocks(cell, "").replace('\n', " ").replace('|', "\\|")).collect()
    }).collect();
    let Some(width) = rows.iter().map(|r| r.len()).max().filter(|w| *w > 0) else { return String::new() };
    let line = |r: &Vec<String>| format!("| {} |", (0..width).map(|i| r.get(i).map(|s| s.as_str()).unwrap_or("")).collect::<Vec<_>>().join(" | "));
    let mut out = vec![line(&rows[0]), format!("|{}", " --- |".repeat(width))];
    out.extend(rows[1..].iter().map(line));
    out.join("\n")
}

fn inlines(n: &serde_json::Value) -> String {
    children(n).iter().map(inline).collect()
}

fn inline(n: &serde_json::Value) -> String {
    let attr = |k: &str| n["attrs"][k].as_str().unwrap_or("").to_string();
    match n["type"].as_str().unwrap_or("") {
        "text" => {
            let mut text = n["text"].as_str().unwrap_or("").to_string();
            let marks = n["marks"].as_array().cloned().unwrap_or_default();
            let has = |m: &str| marks.iter().any(|x| x["type"] == m);
            if has("code") {
                text = format!("`{}`", text);
            } else {
                if has("strong") { text = format!("**{}**", text) }
                if has("em") { text = format!("_{}_", text) }
                if has("strike") { text = format!("~~{}~~", text) }
            }
            if let Some(href) = marks.iter().find(|x| x["type"] == "link").and_then(|l| l["attrs"]["href"].as_str()) {
                text = format!("[{}]({})", text, href);
            }
            text
        }
        "hardBreak" => "  \n".to_string(),
        "mention" | "emoji" | "status" => {
            let text = attr("text");
            if text.is_empty() { attr("shortName") } else { text }
        }
        "inlineCard" | "blockCard" | "embedCard" => format!("<{}>", attr("url")),
        "date" => attr("timestamp"),
        _ => inlines(n),
    }
}

fn parse_link(n: &serde_json::Value) -> Option<JiraIssueLink> {
    let f = if n["fields"].is_object() { &n["fields"] } else { n };
    Some(JiraIssueLink {
        key: n["key"].as_str()?.to_string(),
        summary: f["summary"].as_str().unwrap_or("").to_string(),
        status: f["status"]["name"].as_str().map(|s| s.to_string()),
    })
}

/// Everything shown when a Jira card is opened: description, comments, parent and subtasks.
#[tauri::command]
pub async fn get_jira_issue_detail(key: String) -> Result<JiraIssueDetail, String> {
    if !valid_key(&key) {
        return Err(format!("Invalid issue key: {}", key));
    }
    let fields = "summary,description,status,issuetype,reporter,created,labels,parent,subtasks,comment";
    let json = if let Some(out) = run(Cli::Twg, &[
        "jira", "workitem", "get", &key, "--comments", "--fields", fields,
        "--output", "json", "--output-summary", "none",
    ]).await? {
        parse_json(&out)?
    } else if let Some(out) = run(Cli::Acli, &[
        "jira", "workitem", "view", &key, "--fields", fields, "--json",
    ]).await? {
        parse_json(&out)?
    } else {
        return Err("JIRA_NOT_INSTALLED".to_string());
    };

    // twg: `data: [issue]` with flat fields and `comments`; acli: the REST issue with `fields.comment.comments`.
    let n = if json["data"].is_array() { &json["data"][0] } else if json["data"].is_object() { &json["data"] } else { &json };
    let f = if n["fields"].is_object() { &n["fields"] } else { n };
    let str_at = |v: &serde_json::Value| v.as_str().map(|s| s.to_string());
    let comments = n["comments"].as_array().or_else(|| f["comment"]["comments"].as_array()).cloned().unwrap_or_default();
    Ok(JiraIssueDetail {
        key: n["key"].as_str().unwrap_or(&key).to_string(),
        description: adf_to_markdown(&f["description"]),
        issue_type: str_at(&f["issuetype"]["name"]).or_else(|| str_at(&f["issueType"]["name"])),
        reporter: str_at(&f["reporter"]["displayName"]),
        created: str_at(&f["created"]),
        labels: f["labels"].as_array().map(|l| l.iter().filter_map(str_at).collect()).unwrap_or_default(),
        parent: if f["parent"].is_object() { parse_link(&f["parent"]) } else { None },
        subtasks: f["subtasks"].as_array().map(|s| s.iter().filter_map(parse_link).collect()).unwrap_or_default(),
        comments: comments.iter().map(|c| JiraComment {
            author: str_at(&c["author"]["displayName"]),
            created: str_at(&c["created"]),
            body: adf_to_markdown(&c["body"]),
        }).collect(),
    })
}
