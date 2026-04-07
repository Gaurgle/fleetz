use chrono::{DateTime, Utc};
use serde::Deserialize;

/// A repo as returned by the GitHub GraphQL API.
#[derive(Debug, Clone)]
pub struct GithubRepo {
    pub name: String,
    pub pushed_at: DateTime<Utc>,
    pub pr_count: u32,
}

// --- Raw deserialization types (match GraphQL response shape) ---

#[derive(Deserialize)]
struct GraphQlResponse {
    data: Option<GraphQlData>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlData {
    viewer: Viewer,
}

#[derive(Deserialize)]
struct Viewer {
    repositories: RepositoryConnection,
}

#[derive(Deserialize)]
struct RepositoryConnection {
    nodes: Vec<RepoNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoNode {
    name: String,
    pushed_at: Option<String>,
    pull_requests: Option<PrConnection>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrConnection {
    total_count: u32,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}

/// Build the GraphQL query string.
fn build_query(repo_limit: u32, include_prs: bool) -> String {
    let pr_field = if include_prs {
        "pullRequests(states: OPEN) { totalCount }"
    } else {
        ""
    };
    format!(
        r#"{{ viewer {{
  repositories(first: {repo_limit}, orderBy: {{field: PUSHED_AT, direction: DESC}},
    affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]) {{
    nodes {{ name, pushedAt {pr_field} }}
  }}
}}}}"#
    )
}

/// Fetch repos from GitHub GraphQL API using the `gh` CLI's auth token.
pub async fn fetch_repos(
    repo_limit: u32,
    include_prs: bool,
) -> Result<Vec<GithubRepo>, String> {
    // Get auth token from gh CLI
    let token = get_gh_token().await?;
    let query = build_query(repo_limit, include_prs);

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.github.com/graphql")
        .header("Authorization", format!("bearer {token}"))
        .header("User-Agent", "epoz")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API returned {}", resp.status()));
    }

    let body: GraphQlResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse GitHub response: {e}"))?;

    if let Some(errors) = body.errors {
        let msgs: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        return Err(format!("GitHub API errors: {}", msgs.join(", ")));
    }

    let data = body.data.ok_or("GitHub response missing data field")?;
    Ok(parse_repos(data.viewer.repositories.nodes))
}

/// Parse raw GraphQL nodes into GithubRepo structs.
fn parse_repos(nodes: Vec<RepoNode>) -> Vec<GithubRepo> {
    nodes
        .into_iter()
        .filter_map(|node| {
            let pushed_at = node.pushed_at.as_ref()?;
            let dt = DateTime::parse_from_rfc3339(pushed_at).ok()?;
            Some(GithubRepo {
                name: node.name,
                pushed_at: dt.with_timezone(&Utc),
                pr_count: node.pull_requests.map_or(0, |pr| pr.total_count),
            })
        })
        .collect()
}

/// Filter repos by a cutoff time (only repos pushed after cutoff).
pub fn filter_since(repos: &[GithubRepo], cutoff: DateTime<Utc>) -> Vec<GithubRepo> {
    repos
        .iter()
        .filter(|r| r.pushed_at > cutoff)
        .cloned()
        .collect()
}

/// Filter repos by a before time (only repos pushed before that time).
pub fn filter_before(repos: &[GithubRepo], before: DateTime<Utc>) -> Vec<GithubRepo> {
    repos
        .iter()
        .filter(|r| r.pushed_at < before)
        .cloned()
        .collect()
}

/// Get the GitHub auth token from the `gh` CLI.
async fn get_gh_token() -> Result<String, String> {
    let output = tokio::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
        .map_err(|e| format!("failed to run `gh auth token`: {e}"))?;

    if !output.status.success() {
        return Err("not authenticated with GitHub. Run: gh auth login".to_string());
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err("gh auth token returned empty. Run: gh auth login".to_string());
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_repos_filters_null_pushed_at() {
        let nodes = vec![
            RepoNode {
                name: "good".to_string(),
                pushed_at: Some("2026-04-01T10:00:00Z".to_string()),
                pull_requests: None,
            },
            RepoNode {
                name: "no_date".to_string(),
                pushed_at: None,
                pull_requests: None,
            },
        ];
        let repos = parse_repos(nodes);
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "good");
    }

    #[test]
    fn parse_repos_extracts_pr_count() {
        let nodes = vec![RepoNode {
            name: "with_prs".to_string(),
            pushed_at: Some("2026-04-01T10:00:00Z".to_string()),
            pull_requests: Some(PrConnection { total_count: 3 }),
        }];
        let repos = parse_repos(nodes);
        assert_eq!(repos[0].pr_count, 3);
    }

    #[test]
    fn filter_since_works() {
        let cutoff = DateTime::parse_from_rfc3339("2026-04-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let repos = vec![
            GithubRepo {
                name: "old".to_string(),
                pushed_at: DateTime::parse_from_rfc3339("2026-03-15T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                pr_count: 0,
            },
            GithubRepo {
                name: "new".to_string(),
                pushed_at: DateTime::parse_from_rfc3339("2026-04-05T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                pr_count: 0,
            },
        ];
        let filtered = filter_since(&repos, cutoff);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "new");
    }

    #[test]
    fn filter_before_works() {
        let before = DateTime::parse_from_rfc3339("2026-04-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let repos = vec![
            GithubRepo {
                name: "old".to_string(),
                pushed_at: DateTime::parse_from_rfc3339("2026-03-15T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                pr_count: 0,
            },
            GithubRepo {
                name: "new".to_string(),
                pushed_at: DateTime::parse_from_rfc3339("2026-04-05T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                pr_count: 0,
            },
        ];
        let filtered = filter_before(&repos, before);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "old");
    }

    #[test]
    fn build_query_without_prs() {
        let q = build_query(50, false);
        assert!(q.contains("first: 50"));
        assert!(!q.contains("pullRequests"));
    }

    #[test]
    fn build_query_with_prs() {
        let q = build_query(25, true);
        assert!(q.contains("first: 25"));
        assert!(q.contains("pullRequests"));
    }
}
