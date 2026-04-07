use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::model::{CiStatus, PullRequest, ReviewStatus};

/// A repo as returned by the GitHub GraphQL API.
#[derive(Debug, Clone)]
pub struct GithubRepo {
    pub name: String,
    pub pushed_at: DateTime<Utc>,
    pub pr_count: u32,
    pub prs: Vec<PullRequest>,
    pub ci_status: CiStatus,
    pub ci_workflow: Option<String>,
    pub ci_duration: Option<String>,
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
    pull_requests: Option<PrConnectionNode>,
    default_branch_ref: Option<DefaultBranchRef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrConnectionNode {
    total_count: u32,
    nodes: Option<Vec<PrNode>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrNode {
    number: u32,
    title: String,
    author: Option<PrAuthor>,
    created_at: String,
    url: String,
    review_decision: Option<String>,
}

#[derive(Deserialize)]
struct PrAuthor {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultBranchRef {
    target: Option<CommitTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitTarget {
    status_check_rollup: Option<StatusCheckRollup>,
    check_suites: Option<CheckSuiteConnection>,
}

#[derive(Deserialize)]
struct StatusCheckRollup {
    state: String,
}

#[derive(Deserialize)]
struct CheckSuiteConnection {
    nodes: Option<Vec<CheckSuiteNode>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckSuiteNode {
    workflow_run: Option<WorkflowRun>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Deserialize)]
struct WorkflowRun {
    workflow: Option<WorkflowRef>,
}

#[derive(Deserialize)]
struct WorkflowRef {
    name: String,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}

/// Build the GraphQL query string.
fn build_query(repo_limit: u32, include_prs: bool) -> String {
    let pr_field = if include_prs {
        r#"pullRequests(states: OPEN, first: 5, orderBy: {field: CREATED_AT, direction: DESC}) {
        totalCount
        nodes { number, title, author { login }, createdAt, url, reviewDecision }
      }"#
    } else {
        ""
    };
    format!(
        r#"{{ viewer {{
  repositories(first: {repo_limit}, orderBy: {{field: PUSHED_AT, direction: DESC}},
    affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]) {{
    nodes {{
      name, pushedAt
      {pr_field}
      defaultBranchRef {{
        target {{
          ... on Commit {{
            statusCheckRollup {{ state }}
            checkSuites(first: 1) {{
              nodes {{
                workflowRun {{ workflow {{ name }} }}
                createdAt, updatedAt
              }}
            }}
          }}
        }}
      }}
    }}
  }}
}}}}"#
    )
}

/// Fetch repos from GitHub GraphQL API with disk caching.
/// `bypass_cache`: if true, skips reading cache but still writes (used by `r` refresh).
pub async fn fetch_repos(
    repo_limit: u32,
    include_prs: bool,
    cache: &crate::cache::ApiCache,
    bypass_cache: bool,
) -> Result<Vec<GithubRepo>, String> {
    // Try cache first
    if !bypass_cache {
        if let Some(cached_json) = cache.read() {
            if let Ok(response) = serde_json::from_str::<GraphQlResponse>(&cached_json) {
                if let Some(data) = response.data {
                    return Ok(parse_repos(data.viewer.repositories.nodes));
                }
            }
        }
    }

    // Fetch fresh
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
        // Fallback to stale cache
        if let Some(stale) = cache.read_stale() {
            if let Ok(response) = serde_json::from_str::<GraphQlResponse>(&stale) {
                if let Some(data) = response.data {
                    return Ok(parse_repos(data.viewer.repositories.nodes));
                }
            }
        }
        return Err(format!("GitHub API returned {}", resp.status()));
    }

    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("failed to read response: {e}"))?;

    // Cache the raw response
    cache.write(&body_text);

    let body: GraphQlResponse = serde_json::from_str(&body_text)
        .map_err(|e| format!("failed to parse response: {e}"))?;

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

            let pr_count = node.pull_requests.as_ref().map_or(0, |pr| pr.total_count);
            let prs = parse_prs(&node.pull_requests);
            let (ci_status, ci_workflow, ci_duration) = parse_ci(&node.default_branch_ref);

            Some(GithubRepo {
                name: node.name,
                pushed_at: dt.with_timezone(&Utc),
                pr_count,
                prs,
                ci_status,
                ci_workflow,
                ci_duration,
            })
        })
        .collect()
}

fn parse_prs(pr_conn: &Option<PrConnectionNode>) -> Vec<PullRequest> {
    let Some(conn) = pr_conn else { return Vec::new() };
    let Some(nodes) = &conn.nodes else { return Vec::new() };

    nodes
        .iter()
        .filter_map(|pr| {
            let created_at = DateTime::parse_from_rfc3339(&pr.created_at)
                .ok()?
                .with_timezone(&Utc);
            let review_status = match pr.review_decision.as_deref() {
                Some("APPROVED") => ReviewStatus::Approved,
                Some("CHANGES_REQUESTED") => ReviewStatus::ChangesRequested,
                Some("REVIEW_REQUIRED") => ReviewStatus::ReviewRequired,
                _ => ReviewStatus::Unknown,
            };
            Some(PullRequest {
                number: pr.number,
                title: pr.title.clone(),
                author: pr.author.as_ref().map_or("unknown".to_string(), |a| a.login.clone()),
                created_at,
                review_status,
                url: pr.url.clone(),
            })
        })
        .collect()
}

fn parse_ci(branch_ref: &Option<DefaultBranchRef>) -> (CiStatus, Option<String>, Option<String>) {
    let Some(br) = branch_ref else {
        return (CiStatus::Unknown, None, None);
    };
    let Some(target) = &br.target else {
        return (CiStatus::Unknown, None, None);
    };

    let status = match &target.status_check_rollup {
        Some(rollup) => match rollup.state.as_str() {
            "SUCCESS" => CiStatus::Passed,
            "FAILURE" | "ERROR" => CiStatus::Failed,
            "PENDING" | "EXPECTED" => CiStatus::Pending,
            _ => CiStatus::Unknown,
        },
        None => CiStatus::Unknown,
    };

    let (workflow_name, duration) = match &target.check_suites {
        Some(cs) => {
            let suite = cs.nodes.as_ref().and_then(|n| n.first());
            let name = suite
                .and_then(|s| s.workflow_run.as_ref())
                .and_then(|wr| wr.workflow.as_ref())
                .map(|w| w.name.clone());
            let dur = suite.and_then(|s| {
                let created = DateTime::parse_from_rfc3339(s.created_at.as_ref()?).ok()?;
                let updated = DateTime::parse_from_rfc3339(s.updated_at.as_ref()?).ok()?;
                let secs = (updated - created).num_seconds();
                if secs < 60 {
                    Some(format!("{secs}s"))
                } else {
                    Some(format!("{}m {}s", secs / 60, secs % 60))
                }
            });
            (name, dur)
        }
        None => (None, None),
    };

    (status, workflow_name, duration)
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

    fn make_node(name: &str, pushed_at: Option<&str>) -> RepoNode {
        RepoNode {
            name: name.to_string(),
            pushed_at: pushed_at.map(|s| s.to_string()),
            pull_requests: None,
            default_branch_ref: None,
        }
    }

    fn make_gh_repo(name: &str, pushed_at: &str) -> GithubRepo {
        GithubRepo {
            name: name.to_string(),
            pushed_at: DateTime::parse_from_rfc3339(pushed_at)
                .unwrap()
                .with_timezone(&Utc),
            pr_count: 0,
            prs: vec![],
            ci_status: CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
        }
    }

    #[test]
    fn parse_repos_filters_null_pushed_at() {
        let nodes = vec![
            make_node("good", Some("2026-04-01T10:00:00Z")),
            make_node("no_date", None),
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
            pull_requests: Some(PrConnectionNode {
                total_count: 3,
                nodes: None,
            }),
            default_branch_ref: None,
        }];
        let repos = parse_repos(nodes);
        assert_eq!(repos[0].pr_count, 3);
    }

    #[test]
    fn parse_ci_success() {
        let br = Some(DefaultBranchRef {
            target: Some(CommitTarget {
                status_check_rollup: Some(StatusCheckRollup {
                    state: "SUCCESS".to_string(),
                }),
                check_suites: None,
            }),
        });
        let (status, _, _) = parse_ci(&br);
        assert_eq!(status, CiStatus::Passed);
    }

    #[test]
    fn parse_ci_failure() {
        let br = Some(DefaultBranchRef {
            target: Some(CommitTarget {
                status_check_rollup: Some(StatusCheckRollup {
                    state: "FAILURE".to_string(),
                }),
                check_suites: None,
            }),
        });
        let (status, _, _) = parse_ci(&br);
        assert_eq!(status, CiStatus::Failed);
    }

    #[test]
    fn parse_ci_unknown_when_missing() {
        let (status, _, _) = parse_ci(&None);
        assert_eq!(status, CiStatus::Unknown);
    }

    #[test]
    fn filter_since_works() {
        let cutoff = DateTime::parse_from_rfc3339("2026-04-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let repos = vec![
            make_gh_repo("old", "2026-03-15T00:00:00Z"),
            make_gh_repo("new", "2026-04-05T00:00:00Z"),
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
            make_gh_repo("old", "2026-03-15T00:00:00Z"),
            make_gh_repo("new", "2026-04-05T00:00:00Z"),
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
        assert!(q.contains("statusCheckRollup"));
    }

    #[test]
    fn build_query_with_prs() {
        let q = build_query(25, true);
        assert!(q.contains("first: 25"));
        assert!(q.contains("pullRequests"));
        assert!(q.contains("reviewDecision"));
    }
}
