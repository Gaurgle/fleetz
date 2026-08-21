use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::contributions::{Calendar, Source};
use crate::model::{CiStatus, PullRequest, RepoKind, ReviewStatus};

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
    pub repo_kind: RepoKind,
    /// The owner's login (`Gaurgle`, or an org name). `repo_kind` says which
    /// *kind* of owner it is; this says who.
    pub owner: Option<String>,
}

/// One GitHub poll's results. Boxed calendar because it is ~1.5KB and this
/// travels through an `AppEvent` enum whose other variants are pointer-sized.
pub struct GithubFetch {
    pub repos: Vec<GithubRepo>,
    pub calendar: Option<Box<Calendar>>,
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
    /// Repos reached through org membership rather than through an
    /// affiliation on `repositories`. See `build_query` for why both are
    /// needed; absent in cached payloads written before this was added.
    #[serde(default)]
    organizations: Option<OrganizationConnection>,
    /// The account-wide contribution calendars, one per year of the grid,
    /// keyed by the alias `build_query` gave each one.
    ///
    /// Flattened rather than named, because the number of windows follows
    /// `contributions::YEARS` and Rust has no variadic struct fields. Safe
    /// here in a way it would not be against an arbitrary API: a GraphQL
    /// response carries exactly the fields the query asked for, and the only
    /// ones not already declared above are these aliases. A payload cached
    /// before the calendar was multi-year lands its lone
    /// `contributionsCollection` key in the same map and still parses.
    #[serde(flatten, default)]
    contribution_years: std::collections::BTreeMap<String, ContributionsCollection>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContributionsCollection {
    contribution_calendar: ContributionCalendar,
}

#[derive(Deserialize)]
struct ContributionCalendar {
    weeks: Vec<CalendarWeek>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarWeek {
    contribution_days: Vec<CalendarDay>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarDay {
    date: String,
    contribution_count: u32,
}

/// Fold every year window's week/day nesting into one grid. The API's own
/// week boundaries are ignored on purpose: `Calendar` derives the slot from
/// the date, so the two sources land on the same cells even if GitHub ever
/// changes where it starts a week.
///
/// `set` rather than `add`, defensively. Probed against the live API the
/// windows come back trimmed to exactly the range asked for, so today they do
/// not overlap -- but `contributionCalendar` is a week-shaped structure and
/// nothing in the contract stops it rounding a window out to week boundaries.
/// If it ever does, summing the shared day would silently double it and the
/// year total with it. Setting costs nothing and cannot.
fn parse_calendar(
    years: std::collections::BTreeMap<String, ContributionsCollection>,
    today: chrono::NaiveDate,
) -> Option<Calendar> {
    if years.is_empty() {
        return None;
    }
    let mut calendar = Calendar::new(today, Source::Github);
    for collection in years.into_values() {
        for week in collection.contribution_calendar.weeks {
            for day in week.contribution_days {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") {
                    calendar.set(date, day.contribution_count);
                }
            }
        }
    }
    Some(calendar)
}

#[derive(Deserialize)]
struct OrganizationConnection {
    nodes: Vec<OrganizationNode>,
}

#[derive(Deserialize)]
struct OrganizationNode {
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
    is_private: Option<bool>,
    owner: Option<OwnerNode>,
    pull_requests: Option<PrConnectionNode>,
    default_branch_ref: Option<DefaultBranchRef>,
}

#[derive(Deserialize)]
struct OwnerNode {
    #[serde(rename = "__typename")]
    typename: String,
    login: Option<String>,
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
fn build_query(repo_limit: u32, include_prs: bool, today: chrono::NaiveDate) -> String {
    // The COUNT is free and the LIST is not. Measured against the live API on
    // the same fleet: no PR field at all and `totalCount` alone both price at
    // cost 13 / nodeCount 2625, while adding `first: 5` nodes takes it to
    // cost 26 / nodeCount 9125 -- double the cost, triple the nodes, because
    // a paged connection is charged per node while a bare count is not.
    //
    // So knowing a repo has open PRs rides along for nothing and is always
    // on, which is all "am I forgetting a PR" needs. Titles, authors and
    // review state stay behind `-p`, where the price is asked for. This is
    // the "never widen the GitHub fetch without checking the cache"
    // invariant applied rather than waived: the widening that was free
    // shipped, the widening that was not stayed opt-in.
    let pr_field = if include_prs {
        r#"pullRequests(states: OPEN, first: 5, orderBy: {field: CREATED_AT, direction: DESC}) {
        totalCount
        nodes { number, title, author { login }, createdAt, url, reviewDecision }
      }"#
    } else {
        "pullRequests(states: OPEN) { totalCount }"
    };
    // The repo fields are identical for both sources, so they are built once
    // and interpolated twice.
    let repo_fields = format!(
        r#"nodes {{
      name, pushedAt, isPrivate
      owner {{ __typename, login }}
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
    }}"#
    );
    // Two sources, unioned, because a repo created directly inside an org and
    // reached only through an owner/admin role does NOT surface under
    // viewer.repositories at any affiliation. Without the second source those
    // repos are invisible to fleetz even though you can push to them. repoz
    // hits the same wall and solves it the same way.
    // The calendar fields are free. Measured against the live API: this whole
    // query prices at cost 13 / nodeCount 2625 both with the four aliases and
    // with them stripped out, because none has a `first:` connection to pay
    // for. Fields, not requests, which is what the "never widen the GitHub
    // fetch without checking the cache" invariant permits. One alias per year
    // because contributionsCollection refuses a range longer than a year.
    let calendar_fields: String = crate::contributions::year_windows(today)
        .into_iter()
        .enumerate()
        .map(|(i, (from, to))| {
            format!(
                r#"  y{i}: contributionsCollection(from: "{}T00:00:00Z", to: "{}T23:59:59Z") {{
    contributionCalendar {{
      weeks {{ contributionDays {{ date, contributionCount }} }}
    }}
  }}
"#,
                from.format("%Y-%m-%d"),
                to.format("%Y-%m-%d")
            )
        })
        .collect();
    format!(
        r#"{{ viewer {{
{calendar_fields}  repositories(first: {repo_limit}, orderBy: {{field: PUSHED_AT, direction: DESC}},
    affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]) {{
    {repo_fields}
  }}
  organizations(first: 25) {{
    nodes {{
      repositories(first: {repo_limit}, orderBy: {{field: PUSHED_AT, direction: DESC}}) {{
        {repo_fields}
      }}
    }}
  }}
}}}}"#
    )
}

/// Flatten both repo sources into one list, newest push first, keeping the
/// first entry per name so a repo returned by both sources appears once.
fn merge_sources(viewer: Viewer) -> Vec<RepoNode> {
    let mut nodes = viewer.repositories.nodes;
    if let Some(orgs) = viewer.organizations {
        for org in orgs.nodes {
            nodes.extend(org.repositories.nodes);
        }
    }
    // Sort before dedup so the survivor of a duplicate pair is the freshest.
    nodes.sort_by(|a, b| b.pushed_at.cmp(&a.pushed_at));
    let mut seen = std::collections::HashSet::new();
    nodes.retain(|n| seen.insert(n.name.to_lowercase()));
    nodes
}

/// Parse a cached raw response body into repos plus the calendar, if it
/// parses at all. The calendar is `None` for a payload written before the
/// field existed, which is exactly what an upgraded install reads first.
fn parse_cached(json: Option<String>, today: chrono::NaiveDate) -> Option<GithubFetch> {
    let response: GraphQlResponse = serde_json::from_str(&json?).ok()?;
    Some(split_viewer(response.data?.viewer, today))
}

/// Pull the calendar out of a viewer and flatten the rest into repos. Both
/// halves consume the viewer, so it is destructured rather than borrowed.
fn split_viewer(viewer: Viewer, today: chrono::NaiveDate) -> GithubFetch {
    let Viewer {
        repositories,
        organizations,
        contribution_years,
    } = viewer;
    GithubFetch {
        repos: parse_repos(merge_sources(Viewer {
            repositories,
            organizations,
            contribution_years: Default::default(),
        })),
        calendar: parse_calendar(contribution_years, today).map(Box::new),
    }
}

/// Fetch repos from GitHub GraphQL API with disk caching.
/// `bypass_cache`: if true, skips reading cache but still writes (used by the `R` reload).
pub async fn fetch_repos(
    repo_limit: u32,
    include_prs: bool,
    cache: &crate::cache::ApiCache,
    bypass_cache: bool,
) -> Result<GithubFetch, String> {
    // One "today" for the whole fetch, so every path builds the same grid.
    let today = chrono::Local::now().date_naive();

    // Try cache first. A payload written before the calendar field existed
    // parses fine but carries no calendar, and serving it would show an empty
    // year for as long as the TTL lasts. Treat that as a miss: the whole
    // point of the poll is that the calendar rides along for free.
    if !bypass_cache
        && let Some(fetch) = parse_cached(cache.read(), today)
        && fetch.calendar.is_some()
    {
        return Ok(fetch);
    }

    // Fetch fresh
    let token = get_gh_token().await?;
    let query = build_query(repo_limit, include_prs, today);

    let client = reqwest::Client::new();
    let resp = match client
        .post("https://api.github.com/graphql")
        .header("Authorization", format!("bearer {token}"))
        .header("User-Agent", "fleetz")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
    {
        Ok(r) => r,
        // Transport failure (offline, DNS, TLS): serve stale cache if any,
        // matching the non-2xx path below, so launch works offline.
        Err(e) => {
            if let Some(fetch) = parse_cached(cache.read_stale(), today) {
                return Ok(fetch);
            }
            return Err(format!("GitHub API request failed: {e}"));
        }
    };

    if !resp.status().is_success() {
        // Fallback to stale cache
        if let Some(fetch) = parse_cached(cache.read_stale(), today) {
            return Ok(fetch);
        }
        return Err(format!("GitHub API returned {}", resp.status()));
    }

    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("failed to read response: {e}"))?;

    // Cache the raw response
    cache.write(&body_text);

    let body: GraphQlResponse =
        serde_json::from_str(&body_text).map_err(|e| format!("failed to parse response: {e}"))?;

    if let Some(errors) = body.errors {
        let msgs: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        return Err(format!("GitHub API errors: {}", msgs.join(", ")));
    }

    let data = body.data.ok_or("GitHub response missing data field")?;
    Ok(split_viewer(data.viewer, today))
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
            let repo_kind = match (&node.owner, node.is_private) {
                (Some(o), _) if o.typename == "Organization" => RepoKind::Org,
                (_, Some(true)) => RepoKind::Private,
                (_, Some(false)) => RepoKind::Public,
                _ => RepoKind::Unknown,
            };

            let owner = node.owner.and_then(|o| o.login);

            Some(GithubRepo {
                name: node.name,
                pushed_at: dt.with_timezone(&Utc),
                pr_count,
                prs,
                ci_status,
                ci_workflow,
                ci_duration,
                repo_kind,
                owner,
            })
        })
        .collect()
}

fn parse_prs(pr_conn: &Option<PrConnectionNode>) -> Vec<PullRequest> {
    let Some(conn) = pr_conn else {
        return Vec::new();
    };
    let Some(nodes) = &conn.nodes else {
        return Vec::new();
    };

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
                author: pr
                    .author
                    .as_ref()
                    .map_or("unknown".to_string(), |a| a.login.clone()),
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

/// Whether a cutoff reaches further back than the fetched dataset can
/// vouch for. The GraphQL fetch returns only the top `repo_limit` repos by
/// push date; when the dataset is at that limit AND the cutoff predates its
/// oldest entry, repos may exist on GitHub that the view silently omits.
pub fn window_exceeds_dataset(
    repos: &[GithubRepo],
    cutoff: DateTime<Utc>,
    repo_limit: u32,
) -> bool {
    if (repos.len() as u32) < repo_limit {
        return false;
    }
    repos
        .iter()
        .map(|r| r.pushed_at)
        .min()
        .is_some_and(|oldest| cutoff < oldest)
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
            is_private: None,
            owner: None,
            pull_requests: None,
            default_branch_ref: None,
        }
    }

    fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
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
            repo_kind: RepoKind::Unknown,
            owner: None,
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
            is_private: None,
            owner: None,
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
    fn repo_kind_org_takes_priority_over_private() {
        let nodes = vec![RepoNode {
            name: "org_repo".to_string(),
            pushed_at: Some("2026-04-01T10:00:00Z".to_string()),
            is_private: Some(true),
            owner: Some(OwnerNode {
                typename: "Organization".to_string(),
                login: Some("acme".to_string()),
            }),
            pull_requests: None,
            default_branch_ref: None,
        }];
        let repos = parse_repos(nodes);
        assert_eq!(repos[0].repo_kind, RepoKind::Org);
    }

    #[test]
    fn repo_kind_private_when_not_org() {
        let nodes = vec![RepoNode {
            name: "priv_repo".to_string(),
            pushed_at: Some("2026-04-01T10:00:00Z".to_string()),
            is_private: Some(true),
            owner: Some(OwnerNode {
                typename: "User".to_string(),
                login: Some("Gaurgle".to_string()),
            }),
            pull_requests: None,
            default_branch_ref: None,
        }];
        let repos = parse_repos(nodes);
        assert_eq!(repos[0].repo_kind, RepoKind::Private);
    }

    #[test]
    fn repo_kind_public_when_not_private_or_org() {
        let nodes = vec![RepoNode {
            name: "pub_repo".to_string(),
            pushed_at: Some("2026-04-01T10:00:00Z".to_string()),
            is_private: Some(false),
            owner: Some(OwnerNode {
                typename: "User".to_string(),
                login: Some("Gaurgle".to_string()),
            }),
            pull_requests: None,
            default_branch_ref: None,
        }];
        let repos = parse_repos(nodes);
        assert_eq!(repos[0].repo_kind, RepoKind::Public);
    }

    #[test]
    fn parse_repos_extracts_owner_login() {
        let nodes = vec![RepoNode {
            name: "org_repo".to_string(),
            pushed_at: Some("2026-04-01T10:00:00Z".to_string()),
            is_private: Some(true),
            owner: Some(OwnerNode {
                typename: "Organization".to_string(),
                login: Some("acme".to_string()),
            }),
            pull_requests: None,
            default_branch_ref: None,
        }];
        let repos = parse_repos(nodes);
        assert_eq!(repos[0].owner.as_deref(), Some("acme"));
    }

    #[test]
    fn parse_repos_tolerates_missing_owner() {
        // The login is nullable in the schema, and a repo can come back with
        // no owner node at all; neither must drop the repo from the list.
        let repos = parse_repos(vec![make_node("bare", Some("2026-04-01T10:00:00Z"))]);
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].owner, None);
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
        let q = build_query(50, false, day("2026-08-18"));
        assert!(q.contains("first: 50"));
        // The count is always asked for; only the node list is optional.
        assert!(q.contains("pullRequests(states: OPEN) { totalCount }"));
        // Matched against the PR field itself: a bare "first: 5" is also a
        // substring of the "first: 50" repo limit, so that spelling would
        // fail for a reason that has nothing to do with PRs.
        assert!(
            !q.contains("pullRequests(states: OPEN, first:"),
            "no paged node list without -p"
        );
        assert!(!q.contains("reviewDecision"));
        assert!(q.contains("statusCheckRollup"));
    }

    #[test]
    fn build_query_with_prs() {
        let q = build_query(25, true, day("2026-08-18"));
        assert!(q.contains("first: 25"));
        assert!(q.contains("pullRequests"));
        assert!(q.contains("reviewDecision"));
    }

    #[test]
    fn window_exceeds_dataset_when_full_and_cutoff_older() {
        let repos = vec![
            make_gh_repo("a", "2026-07-01T00:00:00Z"),
            make_gh_repo("b", "2026-07-10T00:00:00Z"),
        ];
        let old_cutoff = DateTime::parse_from_rfc3339("2026-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // Dataset is at the fetch limit (2) and the cutoff predates the
        // oldest entry: repos may be missing.
        assert!(window_exceeds_dataset(&repos, old_cutoff, 2));
        // Cutoff inside the dataset range: nothing can be missing.
        let new_cutoff = DateTime::parse_from_rfc3339("2026-07-05T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!window_exceeds_dataset(&repos, new_cutoff, 2));
        // Dataset below the limit means the fetch got everything.
        assert!(!window_exceeds_dataset(&repos, old_cutoff, 50));
        // Empty dataset: nothing to warn about.
        assert!(!window_exceeds_dataset(&[], old_cutoff, 50));
    }

    /// The calendar rides the existing payload, so an old cache file (written
    /// before the field existed) must still parse. `organizations` already
    /// carries this treatment; this is the same trap.
    #[test]
    fn payload_without_a_calendar_parses_with_none() {
        let json = r#"{"data":{"viewer":{"repositories":{"nodes":[]}}}}"#;
        let response: GraphQlResponse = serde_json::from_str(json).unwrap();
        let viewer = response.data.unwrap().viewer;
        assert!(parse_calendar(viewer.contribution_years, day("2026-08-18")).is_none());
    }

    /// A payload carrying the single un-aliased `contributionsCollection` key
    /// is what an install upgraded from the one-year grid reads out of its
    /// cache. It has to keep parsing: the flattened map takes it like any
    /// other year window.
    #[test]
    fn payload_with_a_calendar_lands_on_the_right_days() {
        let json = r#"{"data":{"viewer":{
            "repositories":{"nodes":[]},
            "contributionsCollection":{"contributionCalendar":{"weeks":[
              {"contributionDays":[
                {"date":"2025-08-17","contributionCount":2},
                {"date":"2025-08-18","contributionCount":0}
              ]},
              {"contributionDays":[{"date":"2026-08-18","contributionCount":7}]}
            ]}}
        }}}"#;
        let response: GraphQlResponse = serde_json::from_str(json).unwrap();
        let viewer = response.data.unwrap().viewer;
        let cal = parse_calendar(viewer.contribution_years, day("2026-08-18")).unwrap();
        assert_eq!(cal.total(), 9);
        // The three-year grid opens on 2023-08-20, so 2025-08-17 is no longer
        // slot 0: it is the Sunday of week 104.
        assert_eq!(cal.day_at(104, 0).unwrap().1, 2);
        assert_eq!(cal.max(), 7);
    }

    /// GitHub pads each window out to whole weeks, so two adjacent year
    /// windows can both report the same day. `parse_calendar` must `set` it,
    /// not `add` it: summing the overlap silently doubles that day and the
    /// year total with it.
    #[test]
    fn a_day_reported_by_two_year_windows_is_not_counted_twice() {
        let json = r#"{"data":{"viewer":{
            "repositories":{"nodes":[]},
            "y0":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2025-08-19","contributionCount":4}]}
            ]}},
            "y1":{"contributionCalendar":{"weeks":[
              {"contributionDays":[
                {"date":"2025-08-19","contributionCount":4},
                {"date":"2024-08-20","contributionCount":3}
              ]}
            ]}}
        }}}"#;
        let response: GraphQlResponse = serde_json::from_str(json).unwrap();
        let cal = parse_calendar(
            response.data.unwrap().viewer.contribution_years,
            day("2026-08-18"),
        )
        .unwrap();
        assert_eq!(cal.total(), 7, "the shared day was counted twice");
    }

    /// Every year of the grid must be asked for, or the oldest columns render
    /// as an empty year that looks like a bug rather than like no data.
    #[test]
    fn the_query_covers_every_year_of_the_grid() {
        let today = day("2026-08-18");
        let windows = crate::contributions::year_windows(today);
        assert_eq!(windows.len(), 3, "three years, three windows");
        let q = build_query(50, false, today);
        for (i, _) in windows.iter().enumerate() {
            assert!(
                q.contains(&format!("y{i}: contributionsCollection")),
                "missing y{i}"
            );
        }
        // The oldest window must reach the grid's own first day, not stop a
        // few days short and leave unqueried columns pinned at zero.
        assert_eq!(
            windows.last().unwrap().0,
            crate::contributions::grid_start(today)
        );
    }

    /// A malformed date must drop that one day, not the whole calendar.
    #[test]
    fn an_unparseable_date_is_skipped_not_fatal() {
        let json = r#"{"data":{"viewer":{
            "repositories":{"nodes":[]},
            "contributionsCollection":{"contributionCalendar":{"weeks":[
              {"contributionDays":[
                {"date":"not-a-date","contributionCount":5},
                {"date":"2026-08-18","contributionCount":3}
              ]}
            ]}}
        }}}"#;
        let response: GraphQlResponse = serde_json::from_str(json).unwrap();
        let viewer = response.data.unwrap().viewer;
        let cal = parse_calendar(viewer.contribution_years, day("2026-08-18")).unwrap();
        assert_eq!(cal.total(), 3);
    }

    /// Knowing a repo has open PRs is free; listing them is not. Measured on
    /// the live API: no PR field and `totalCount` alone both price at cost 13
    /// / nodeCount 2625, while `first: 5` nodes take it to 26 / 9125. So the
    /// count ships unconditionally and the list stays behind `-p`. If the
    /// bare count ever grows a `first:` connection it stops being free, and
    /// the "never widen the fetch" invariant bites.
    #[test]
    fn the_pr_count_rides_every_query_but_the_list_does_not() {
        let today = day("2026-08-18");
        let plain = build_query(50, false, today);
        assert!(plain.contains("pullRequests(states: OPEN) { totalCount }"));
        assert!(
            !plain.contains("pullRequests(states: OPEN, first:"),
            "the paged list must stay opt-in"
        );

        let detailed = build_query(50, true, today);
        assert!(detailed.contains("pullRequests(states: OPEN, first: 5,"));
        assert!(detailed.contains("reviewDecision"));
        // Still one request either way.
        assert_eq!(plain.matches("viewer").count(), 1);
        assert_eq!(detailed.matches("viewer").count(), 1);
    }

    /// Measured before it was written: `contributionCalendar` has no `first:`
    /// connection, so it costs nothing on top of the existing query. If it
    /// ever grows one, this query stops being free and the invariant bites.
    #[test]
    fn the_calendar_field_rides_the_existing_query() {
        let q = build_query(50, false, day("2026-08-18"));
        assert!(q.contains("contributionsCollection"));
        assert!(q.contains("contributionCalendar"));
        assert!(!q.contains("contributionCalendar(first"));
        // Still one query, not two.
        assert_eq!(q.matches("viewer").count(), 1);
    }

    /// A payload cached before the calendar field existed parses fine and
    /// carries no calendar. Serving it would show an empty year for as long
    /// as the TTL lasts, which is exactly what a fresh upgrade hits.
    #[test]
    fn a_cached_payload_without_a_calendar_is_not_good_enough_to_serve() {
        let old = r#"{"data":{"viewer":{"repositories":{"nodes":[]}}}}"#;
        let fetch = parse_cached(Some(old.to_string()), day("2026-08-18")).unwrap();
        assert!(
            fetch.calendar.is_none(),
            "this is the payload the guard has to reject"
        );

        let new = r#"{"data":{"viewer":{
            "repositories":{"nodes":[]},
            "contributionsCollection":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":1}]}
            ]}}
        }}}"#;
        let fetch = parse_cached(Some(new.to_string()), day("2026-08-18")).unwrap();
        assert!(fetch.calendar.is_some(), "a current payload is servable");
    }
}
