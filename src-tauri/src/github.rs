use std::collections::HashSet;
use serde::Serialize;

// Pull requests come from the GitHub CLI (`gh`), which is already signed in
// on the user's machine — Locus never stores a GitHub token itself.

const PR_QUERY: &str = r#"
query {
  viewer { login }
  reviewRequested: search(query: "is:pr is:open archived:false review-requested:@me sort:updated-desc", type: ISSUE, first: 30) { nodes { ...pr } }
  authored: search(query: "is:pr is:open archived:false author:@me sort:updated-desc", type: ISSUE, first: 30) { nodes { ...pr } }
  involved: search(query: "is:pr is:open archived:false involves:@me sort:updated-desc", type: ISSUE, first: 50) { nodes { ...pr } }
}
fragment pr on PullRequest {
  id number title url isDraft updatedAt
  repository { nameWithOwner }
  author { login avatarUrl }
  reviewDecision
  comments { totalCount }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
}
"#;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    id: String,
    number: u64,
    title: String,
    url: String,
    repo: String,
    author: String,
    author_avatar: Option<String>,
    is_draft: bool,
    updated_at: String,
    /// APPROVED | CHANGES_REQUESTED | REVIEW_REQUIRED, or None when the repo doesn't require reviews.
    review_decision: Option<String>,
    /// SUCCESS | FAILURE | ERROR | PENDING | EXPECTED, or None when there are no checks.
    checks: Option<String>,
    comments: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestLists {
    login: String,
    review_requested: Vec<PullRequest>,
    authored: Vec<PullRequest>,
    /// Involved in some other way (commented, mentioned, assigned), minus the lists above.
    involved: Vec<PullRequest>,
}

fn parse_prs(nodes: &serde_json::Value) -> Vec<PullRequest> {
    nodes.as_array().map(|arr| arr.iter()
        // Search can return empty objects for items the token can't see.
        .filter(|n| n["id"].is_string())
        .map(|n| PullRequest {
            id: n["id"].as_str().unwrap_or("").to_string(),
            number: n["number"].as_u64().unwrap_or(0),
            title: n["title"].as_str().unwrap_or("").to_string(),
            url: n["url"].as_str().unwrap_or("").to_string(),
            repo: n["repository"]["nameWithOwner"].as_str().unwrap_or("").to_string(),
            author: n["author"]["login"].as_str().unwrap_or("ghost").to_string(),
            author_avatar: n["author"]["avatarUrl"].as_str().map(|s| s.to_string()),
            is_draft: n["isDraft"].as_bool().unwrap_or(false),
            updated_at: n["updatedAt"].as_str().unwrap_or("").to_string(),
            review_decision: n["reviewDecision"].as_str().map(|s| s.to_string()),
            checks: n["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"].as_str().map(|s| s.to_string()),
            comments: n["comments"]["totalCount"].as_u64().unwrap_or(0),
        })
        .collect()
    ).unwrap_or_default()
}

fn gh_command() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("gh");
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[tauri::command]
pub async fn get_pull_requests() -> Result<PullRequestLists, String> {
    let output = gh_command()
        .args(["api", "graphql", "-f", &format!("query={}", PR_QUERY)])
        .output()
        .await
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
            "GH_NOT_INSTALLED".to_string()
        } else {
            format!("Failed to run the GitHub CLI: {}", e)
        })?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let lower = err.to_lowercase();
        if lower.contains("gh auth login") || lower.contains("not logged") || lower.contains("authentication") {
            return Err("GH_NOT_AUTHENTICATED".to_string());
        }
        return Err(if err.is_empty() { "The GitHub CLI reported an error.".to_string() } else { err });
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Unexpected response from GitHub: {}", e))?;
    if let Some(msg) = json["errors"][0]["message"].as_str() {
        return Err(format!("GitHub error: {}", msg));
    }
    let data = &json["data"];

    let review_requested = parse_prs(&data["reviewRequested"]["nodes"]);
    let authored = parse_prs(&data["authored"]["nodes"]);
    let seen: HashSet<String> = review_requested.iter().chain(&authored).map(|p| p.id.clone()).collect();
    let involved = parse_prs(&data["involved"]["nodes"]).into_iter()
        .filter(|p| !seen.contains(&p.id))
        .collect();

    Ok(PullRequestLists {
        login: data["viewer"]["login"].as_str().unwrap_or("").to_string(),
        review_requested,
        authored,
        involved,
    })
}
