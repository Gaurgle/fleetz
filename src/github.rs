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
    /// Whether the payload carried the `pullRequests` field at all.
    ///
    /// Distinct from "every repo has zero open PRs", which is a perfectly
    /// ordinary answer. A payload written before the count was requested has
    /// the field absent on every node, so the two are told apart by the
    /// presence of the key rather than by its value. Without that, an
    /// upgraded install serves its old cache and reports no PRs anywhere for
    /// a full TTL, silently.
    pub has_pr_counts: bool,
    /// Whether this came off the network or the disk cache, and how old.
    ///
    /// Startup serves any cached payload however old (`CacheMode::AnyCached`),
    /// so "the load returned a dataset" does not mean "GitHub was just asked".
    /// The caller needs the difference: starting the re-poll clock from a
    /// cached payload leaves the first real poll a whole interval away, and
    /// the dashboard spends it showing whatever the last session left behind.
    pub origin: Origin,
}

/// Where a payload came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A request made just now, answered with data.
    Network,
    /// The disk cache. `written` is the cache file's write time, which is when
    /// the payload left GitHub; `None` when it could not be read, and then the
    /// age is unknown, never "now".
    Cache {
        written: Option<DateTime<chrono::Local>>,
    },
}

/// How a request to GitHub failed. See `docs/freshness-contract.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// Transport: DNS, TLS, no route, a body cut off.
    Offline,
    /// A non-2xx status.
    Http,
    /// A GraphQL `errors` body; the rate limit is one of these.
    Api,
    /// No token from `gh`.
    Auth,
    /// A body that does not parse, or parses without `data`.
    Malformed,
    /// Given up on after `GITHUB_POLL_STUCK_AFTER` with no answer.
    TimedOut,
}

/// One failed attempt: its kind, and the message the status line shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    /// An `Api` failure that is GitHub's rate limit: the common way the API
    /// refuses, and one that clears on its own, so the header names it.
    pub rate_limited: bool,
}

impl Failure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            rate_limited: false,
        }
    }

    /// An `Api` failure that is a rate limit.
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self {
            rate_limited: true,
            ..Self::new(FailureKind::Api, message)
        }
    }
}

/// A request that failed, with the cached payload if there was one to fall
/// back on.
///
/// A fallback used to come back as `Ok`, which made an offline poll look
/// exactly like a successful one: the caller cleared the failure marker and
/// restarted the clock. Carrying the payload inside the error keeps "the
/// request failed" impossible to miss, while the callers with nothing in
/// memory to keep (launch, `R`) can still use the data.
pub struct FetchError {
    pub failure: Failure,
    /// Always `Origin::Cache` when present.
    pub fallback: Option<Box<GithubFetch>>,
}

impl std::fmt::Debug for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FetchError")
            .field("failure", &self.failure)
            .field("has_fallback", &self.fallback.is_some())
            .finish()
    }
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
    /// The last few days, re-asked for in a window of their own because
    /// GitHub's year-range calendar lags. Declared rather than left to the
    /// flattened map above so the merge order is stated, not inherited from
    /// how `BTreeMap` happens to sort the aliases: "recent" sorts *before*
    /// "y0", so a map-ordered merge would let the stale year window overwrite
    /// the fresh values instead of the other way round. Absent in payloads
    /// cached before this was added, hence `Option`.
    #[serde(default)]
    recent: Option<ContributionsCollection>,
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
    /// GitHub's own shade step, absent from a payload cached before it was
    /// asked for, and not asked for in the `recent` window at all.
    #[serde(default)]
    contribution_level: Option<String>,
}

/// GitHub's `ContributionLevel` enum as a shade step, or None for anything
/// unrecognised.
fn level_from_api(level: &str) -> Option<u8> {
    match level {
        "NONE" => Some(0),
        "FIRST_QUARTILE" => Some(1),
        "SECOND_QUARTILE" => Some(2),
        "THIRD_QUARTILE" => Some(3),
        "FOURTH_QUARTILE" => Some(4),
        _ => None,
    }
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
    recent: Option<ContributionsCollection>,
    today: chrono::NaiveDate,
) -> Option<Calendar> {
    if years.is_empty() && recent.is_none() {
        return None;
    }
    let mut calendar = Calendar::new(today, Source::Github);
    for collection in years.into_values() {
        fill(&mut calendar, collection, None);
    }
    // Last, so the fresh numbers win over the lagging year windows for the
    // days both cover. Clamped to its own `from`: the year windows are the
    // only source for everything older, and a padded week reaching back past
    // the window would `set` those days to the zero it pads with.
    if let Some(collection) = recent {
        let floor = crate::contributions::recent_merge_floor(today);
        fill(&mut calendar, collection, Some(floor));
        // The days just overwritten lost their levels with their counts. Put
        // them back on the current year window's scale, which is the only
        // scale those days' neighbours are on; see `derive_missing_levels`.
        if let Some((from, _)) = crate::contributions::year_windows(today).first() {
            calendar.derive_missing_levels(*from);
        }
    }
    Some(calendar)
}

/// Fold one window's week/day nesting into the grid, skipping days before
/// `floor` when the caller has one.
fn fill(
    calendar: &mut Calendar,
    collection: ContributionsCollection,
    floor: Option<chrono::NaiveDate>,
) {
    for week in collection.contribution_calendar.weeks {
        for day in week.contribution_days {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") {
                if floor.is_some_and(|f| date < f) {
                    continue;
                }
                calendar.set(date, day.contribution_count);
                if let Some(level) = day.contribution_level.as_deref().and_then(level_from_api) {
                    calendar.set_level(date, level);
                }
            }
        }
    }
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
    /// GitHub's machine-readable error class, `RATE_LIMITED` among them.
    /// Absent on some errors, so optional.
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

/// Whether a GraphQL `errors` body is a rate limit. Decided on GitHub's
/// `type` when any error carries one; the message text is only a fallback for
/// a body with no `type` at all, so the UI never has to match words.
fn is_rate_limited(errors: &[GraphQlError]) -> bool {
    if errors.iter().any(|e| e.kind.is_some()) {
        return errors
            .iter()
            .any(|e| e.kind.as_deref() == Some("RATE_LIMITED"));
    }
    errors
        .iter()
        .any(|e| e.message.to_lowercase().contains("rate limit"))
}

/// Build the GraphQL query string.
/// How many organizations the fallback branch asks for, and how many repos
/// inside each.
///
/// Node count is billed on the `first:` you ask for, not on what comes back,
/// and this branch multiplies: orgs x repos x (PR page + check suite). That
/// makes it the whole cost of the query. Measured against the live fleet on
/// 2026-08-24, every row returning the identical repo set of 17 org repos
/// across 4 orgs:
///
///   25 x 50 (was `repo_limit`)   cost 26 / nodeCount 9125
///   10 x 25                      cost  6 / nodeCount 2110
///    8 x 12                      cost  3 / nodeCount 1030
///    5 x 10                      cost  2 / nodeCount  705
///
/// At 25 x 50 the branch was 83% of the node count, and 26 points every 30
/// seconds is 3120/hour against a 5000/hour budget: one fleetz idled at 62%
/// of the account's GraphQL allowance and two could not both run. That is how
/// the limit came to be exhausted.
///
/// 8 x 12 is roughly double the fleet it has to cover, which is the headroom
/// worth paying one point for: overrunning either page drops repos silently.
/// Both connections order by PUSHED_AT DESC, so what a too-small page loses
/// is always the least recently touched.
///
/// Trimming the *viewer* branch instead does nothing. At `first: 20`, which
/// still covers a three-week window twice over, the same query measured cost
/// 6 / 1900 -- node count down a tenth, cost unmoved, because the connection
/// floor dominates. `repo_limit` is also what bounds how far the time window
/// can be widened without re-fetching (`window_exceeds_dataset`), so lowering
/// it spends real flexibility for no saving.
const MAX_ORGS: u32 = 8;
const ORG_REPO_LIMIT: u32 = 12;
/// Raising either back past what it replaced puts the 26-point query back.
const _: () = assert!(MAX_ORGS < 25 && ORG_REPO_LIMIT < 50);

fn build_query(repo_limit: u32, today: chrono::NaiveDate, rotation: i64) -> String {
    // PR numbers cannot be had cheaply, and half-measures buy nothing.
    // Measured against the live API on the same fleet:
    //
    //   totalCount only        cost 13 / nodeCount 2625
    //   first: 1 { number }    cost 26 / nodeCount 3925
    //   first: 5 { number }    cost 26 / nodeCount 9125
    //   first: 5 every field   cost 26 / nodeCount 9125
    //
    // Cost doubles the moment a paged connection appears, whatever its page
    // size or field set, because the floor is per-call rather than per-node.
    // A one-PR page costs exactly what a five-PR page carrying every field
    // costs, so asking for less than the full set is pure loss.
    //
    // Affordable rather than free -- which is what "never widen the GitHub
    // fetch without checking the cache" asks: not that a field be free, but
    // that its price be known before it ships. Note the price is paid per
    // *poll*, on `github_poll_interval` (30s) and not on `refresh_interval`:
    // at cost 26 that was 3120 points an hour against a 5000/hour limit, and
    // 6240 while a CI run held the poll at its 15s floor. See `MAX_ORGS` for
    // where that cost actually came from and what it is now.
    // `-p` now controls how much of this gets *drawn*, not what is fetched.
    let pr_field = r#"pullRequests(states: OPEN, first: 5, orderBy: {field: CREATED_AT, direction: DESC}) {
        totalCount
        nodes { number, title, author { login }, createdAt, url, reviewDecision }
      }"#;
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
    // The calendar fields are free: none has a `first:` connection to pay for,
    // so adding one moves nodeCount not at all. Fields, not requests, which is
    // what the "never widen the GitHub fetch without checking the cache"
    // invariant permits. One alias per year because contributionsCollection
    // refuses a range longer than a year.
    //
    // This block used to claim the whole query priced at cost 13 / nodeCount
    // 2625. That was measured before the PR page landed and was never
    // updated; re-measured on 2026-08-24 the same query is cost 26 / 9125,
    // which is what the `pr_field` comment above has said all along.
    let calendar_fields: String = crate::contributions::year_windows(today)
        .into_iter()
        .enumerate()
        .map(|(i, (from, to))| {
            format!(
                r#"  y{i}: contributionsCollection(from: "{}T00:00:00Z", to: "{}T23:59:59Z") {{
    contributionCalendar {{
      weeks {{ contributionDays {{ date, contributionCount, contributionLevel }} }}
    }}
  }}
"#,
                from.format("%Y-%m-%d"),
                to.format("%Y-%m-%d")
            )
        })
        .collect();
    // The newest days again, in a window of their own. GitHub answers a
    // year-long range from an index that lags the live one, so the day you
    // just worked can come back as 0 from `y0` while a short window on the
    // same date reports it correctly. Another alias, so still no request and
    // still no `first:` connection to pay for.
    //
    // No `contributionLevel` here: GitHub scales a level to the window asked
    // for, and this window's `from` rotates, so its levels would re-shade the
    // newest week every few minutes. Those days get the year window's scale
    // instead (`Calendar::derive_missing_levels`).
    let (recent_from, recent_to) = crate::contributions::recent_window(today, rotation);
    let recent_field = format!(
        r#"  recent: contributionsCollection(from: "{}T00:00:00Z", to: "{}T23:59:59Z") {{
    contributionCalendar {{
      weeks {{ contributionDays {{ date, contributionCount }} }}
    }}
  }}
"#,
        recent_from.format("%Y-%m-%d"),
        recent_to.format("%Y-%m-%d")
    );
    format!(
        r#"{{ viewer {{
{calendar_fields}{recent_field}  repositories(first: {repo_limit}, orderBy: {{field: PUSHED_AT, direction: DESC}},
    affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]) {{
    {repo_fields}
  }}
  organizations(first: {MAX_ORGS}) {{
    nodes {{
      repositories(first: {ORG_REPO_LIMIT}, orderBy: {{field: PUSHED_AT, direction: DESC}}) {{
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
///
/// The origin's write time is unknown here; `read_cached` fills it in.
fn parse_cached(json: Option<String>, today: chrono::NaiveDate) -> Option<GithubFetch> {
    let response: GraphQlResponse = serde_json::from_str(&json?).ok()?;
    Some(split_viewer(
        response.data?.viewer,
        today,
        Origin::Cache { written: None },
    ))
}

/// The cache file's payload however old, stamped with the file's write time
/// as its data time.
fn read_cached(cache: &crate::cache::ApiCache, today: chrono::NaiveDate) -> Option<GithubFetch> {
    let cached = cache.read_stale()?;
    parse_cached(Some(cached.body), today).map(|fetch| stamp_written(fetch, cached.written))
}

fn stamp_written(mut fetch: GithubFetch, written: Option<std::time::SystemTime>) -> GithubFetch {
    fetch.origin = Origin::Cache {
        written: written.map(DateTime::<chrono::Local>::from),
    };
    fetch
}

/// Pull the calendar out of a viewer and flatten the rest into repos. Both
/// halves consume the viewer, so it is destructured rather than borrowed.
fn split_viewer(viewer: Viewer, today: chrono::NaiveDate, origin: Origin) -> GithubFetch {
    let Viewer {
        repositories,
        organizations,
        contribution_years,
        recent,
    } = viewer;
    let nodes = merge_sources(Viewer {
        repositories,
        organizations,
        contribution_years: Default::default(),
        recent: None,
    });
    // An empty fleet carries no evidence either way, so it must not be read
    // as a stale payload -- that would make every poll a cache miss forever.
    let has_pr_counts = nodes.is_empty() || nodes.iter().any(|n| n.pull_requests.is_some());
    GithubFetch {
        repos: parse_repos(nodes),
        calendar: parse_calendar(contribution_years, recent, today).map(Box::new),
        has_pr_counts,
        origin,
    }
}

/// Fetch repos from GitHub GraphQL API with disk caching.
/// `bypass_cache`: if true, skips reading cache but still writes (used by the `R` reload).
/// How much the caller is willing to trust the disk cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    /// Serve any cached payload, however old, rather than waiting on the
    /// network. Startup uses this: the whole repo list is derived from this
    /// dataset, so blocking on it meant six to nine seconds of nothing before
    /// the first paint. A poll fires on the first tick of the event loop and
    /// replaces it a moment later, so the staleness is measured in seconds and
    /// the wait is gone.
    AnyCached,
    /// Ignore the cache and go to the network. The background poll and `R`,
    /// which exist precisely to be current.
    Bypass,
}

pub async fn fetch_repos(
    repo_limit: u32,
    cache: &crate::cache::ApiCache,
    mode: CacheMode,
) -> Result<GithubFetch, FetchError> {
    // One "today" for the whole fetch, so every path builds the same grid.
    let today = chrono::Local::now().date_naive();

    // A payload written before the calendar field existed parses fine but
    // carries no calendar, and serving it would show an empty year. Same for
    // one written before PR counts rode along. Treat either as a miss: the
    // whole point of the poll is that both come for free.
    let usable =
        |fetch: Option<GithubFetch>| fetch.filter(|f| f.calendar.is_some() && f.has_pr_counts);
    if mode == CacheMode::AnyCached
        && let Some(fetch) =
            usable(parse_cached(cache.read(), today).map(|f| stamp_written(f, cache.written())))
                .or_else(|| usable(read_cached(cache, today)))
    {
        return Ok(fetch);
    }

    let attempt = request(repo_limit, cache, today).await;
    settle_attempt(attempt, cache, today)
}

/// Turn a finished attempt into the fetch result: a success as it is, a
/// failure with whatever the cache holds attached.
///
/// The fallback used to be returned as `Ok`, so a background poll with the
/// network down cleared the failure marker and restarted the clock, and the
/// header said nothing while the data aged. Split out so the rule is
/// reachable without a network.
fn settle_attempt(
    attempt: Result<GithubFetch, Failure>,
    cache: &crate::cache::ApiCache,
    today: chrono::NaiveDate,
) -> Result<GithubFetch, FetchError> {
    attempt.map_err(|failure| FetchError {
        failure,
        fallback: read_cached(cache, today).map(Box::new),
    })
}

/// One network attempt: token, request, status, body.
async fn request(
    repo_limit: u32,
    cache: &crate::cache::ApiCache,
    today: chrono::NaiveDate,
) -> Result<GithubFetch, Failure> {
    let token = get_gh_token()
        .await
        .map_err(|e| Failure::new(FailureKind::Auth, e))?;
    let query = build_query(
        repo_limit,
        today,
        crate::contributions::rotation_tick(chrono::Utc::now()),
    );

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
        // Transport failure (offline, DNS, TLS). `settle_attempt` attaches
        // the cache, so launch and `R` still work offline.
        Err(e) => {
            return Err(Failure::new(
                FailureKind::Offline,
                format!("GitHub API request failed: {e}"),
            ));
        }
    };

    if !resp.status().is_success() {
        return Err(Failure::new(
            FailureKind::Http,
            format!("GitHub API returned {}", resp.status()),
        ));
    }

    let body_text = resp.text().await.map_err(|e| {
        Failure::new(
            FailureKind::Offline,
            format!("failed to read response: {e}"),
        )
    })?;

    accept_response(&body_text, cache, today)
}

/// Parse a fresh response body, and cache it only if it carried data.
///
/// The write used to happen the moment the body arrived, before anything had
/// looked at it. A GraphQL error is served with HTTP 200 and an `errors` key,
/// so a rate limit -- 95 bytes of `{"errors":[{"type":"RATE_LIMITED"...}]}` --
/// overwrote the last good payload and took the stale fallback down with it:
/// the offline and non-2xx paths above both read `read_stale`, and after one
/// rate-limited poll there was nothing left for them to read. Every poll after
/// that rewrote the same 95 bytes, so the cache could not heal until a poll
/// succeeded, which is exactly when it was least likely to.
///
/// Split out of `fetch_repos` so the rule is reachable without a network:
/// everything above it is transport, everything here is the payload.
fn accept_response(
    body_text: &str,
    cache: &crate::cache::ApiCache,
    today: chrono::NaiveDate,
) -> Result<GithubFetch, Failure> {
    let body: GraphQlResponse = serde_json::from_str(body_text).map_err(|e| {
        Failure::new(
            FailureKind::Malformed,
            format!("failed to parse response: {e}"),
        )
    })?;

    if let Some(errors) = body.errors {
        let msgs: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        let message = format!("GitHub API errors: {}", msgs.join(", "));
        return Err(match is_rate_limited(&errors) {
            true => Failure::rate_limit(message),
            false => Failure::new(FailureKind::Api, message),
        });
    }

    let data = body.data.ok_or_else(|| {
        Failure::new(FailureKind::Malformed, "GitHub response missing data field")
    })?;
    cache.write(body_text);
    Ok(split_viewer(data.viewer, today, Origin::Network))
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

    /// PR data is no longer optional: a one-PR page costs exactly what a
    /// five-PR page with every field costs, so there is no cheaper shape to
    /// fall back to and `-p` gates drawing rather than fetching.
    #[test]
    fn build_query_always_asks_for_full_pr_data() {
        let q = build_query(50, day("2026-08-18"), 0);
        assert!(q.contains("first: 50"));
        assert!(q.contains("pullRequests(states: OPEN, first: 5,"));
        assert!(q.contains("totalCount"));
        assert!(q.contains("reviewDecision"));
        assert!(q.contains("statusCheckRollup"));
        // Still one request.
        assert_eq!(q.matches("viewer").count(), 1);
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

    /// Regression: an install upgrading past the always-on PR count reads its
    /// own old cache first. That payload parses fine and has the calendar, so
    /// it used to be served -- reporting no open PRs anywhere for a full TTL,
    /// with no way to tell that from a genuinely quiet fleet. The field being
    /// absent on every node is the signal, not the counts being zero.
    #[test]
    fn a_cached_payload_without_pr_counts_is_treated_as_stale() {
        let today = day("2026-08-18");
        let without = r#"{"data":{"viewer":{"repositories":{"nodes":[
            {"name":"a","pushedAt":"2026-08-01T00:00:00Z"}
        ]}}}}"#;
        let fetch = parse_cached(Some(without.to_string()), today).unwrap();
        assert!(!fetch.has_pr_counts, "no pullRequests key anywhere");

        // A fleet that genuinely has no open PRs still carries the field, and
        // must NOT be mistaken for a stale payload.
        let quiet = r#"{"data":{"viewer":{"repositories":{"nodes":[
            {"name":"a","pushedAt":"2026-08-01T00:00:00Z","pullRequests":{"totalCount":0}}
        ]}}}}"#;
        let fetch = parse_cached(Some(quiet.to_string()), today).unwrap();
        assert!(fetch.has_pr_counts, "zero PRs is an answer, not an absence");
        assert_eq!(fetch.repos[0].pr_count, 0);
    }

    /// An empty fleet carries no evidence either way. Reading that as stale
    /// would make every poll a guaranteed cache miss.
    #[test]
    fn an_empty_repo_list_is_not_mistaken_for_a_stale_payload() {
        let empty = r#"{"data":{"viewer":{"repositories":{"nodes":[]}}}}"#;
        let fetch = parse_cached(Some(empty.to_string()), day("2026-08-18")).unwrap();
        assert!(fetch.has_pr_counts);
    }

    /// The calendar rides the existing payload, so an old cache file (written
    /// before the field existed) must still parse. `organizations` already
    /// carries this treatment; this is the same trap.
    #[test]
    fn payload_without_a_calendar_parses_with_none() {
        let json = r#"{"data":{"viewer":{"repositories":{"nodes":[]}}}}"#;
        let response: GraphQlResponse = serde_json::from_str(json).unwrap();
        let viewer = response.data.unwrap().viewer;
        assert!(
            parse_calendar(viewer.contribution_years, viewer.recent, day("2026-08-18")).is_none()
        );
    }

    /// The year windows' `contributionLevel` is the shade drawn, not a bucket
    /// of the count: github.com trims outliers before scaling and the formula
    /// does not, so the two disagree on ordinary days. A day the `recent`
    /// window overwrote has no level of its own and is put on the year's
    /// scale by example. Measured cost of the field: none (`rateLimit` cost
    /// 1, nodeCount 0 with and without it).
    #[test]
    fn github_levels_are_drawn_as_given_and_recent_days_join_their_scale() {
        let json = r#"{"data":{"viewer":{
            "repositories":{"nodes":[]},
            "y0":{"contributionCalendar":{"weeks":[
              {"contributionDays":[
                {"date":"2026-08-10","contributionCount":12,"contributionLevel":"THIRD_QUARTILE"},
                {"date":"2026-08-11","contributionCount":38,"contributionLevel":"FOURTH_QUARTILE"},
                {"date":"2026-08-18","contributionCount":0,"contributionLevel":"NONE"}
              ]}
            ]}},
            "recent":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":20}]}
            ]}}
        }}}"#;
        let response: GraphQlResponse = serde_json::from_str(json).unwrap();
        let viewer = response.data.unwrap().viewer;
        let cal =
            parse_calendar(viewer.contribution_years, viewer.recent, day("2026-08-18")).unwrap();
        let at = |date: &str| {
            let slot = cal.slot(day(date)).unwrap();
            cal.level_at(slot / 7, slot % 7)
        };
        assert_eq!(cal.intensity(12), 2, "the formula would have said 2");
        assert_eq!(at("2026-08-10"), 3, "github said 3, and github is drawn");
        assert_eq!(at("2026-08-11"), 4);
        assert_eq!(
            cal.day_at(cal.slot(day("2026-08-18")).unwrap() / 7, 2)
                .unwrap()
                .1,
            20
        );
        assert_eq!(
            at("2026-08-18"),
            3,
            "20 reaches the level-3 example (12), not the 4 (38)"
        );
    }

    /// A payload cached before `contributionLevel` was requested has no
    /// levels anywhere. It must still parse, and draw by the formula.
    #[test]
    fn a_payload_without_levels_still_parses_and_draws_by_formula() {
        let json = r#"{"data":{"viewer":{
            "repositories":{"nodes":[]},
            "y0":{"contributionCalendar":{"weeks":[
              {"contributionDays":[
                {"date":"2026-08-10","contributionCount":10},
                {"date":"2026-08-11","contributionCount":40}
              ]}
            ]}}
        }}}"#;
        let response: GraphQlResponse = serde_json::from_str(json).unwrap();
        let viewer = response.data.unwrap().viewer;
        let cal =
            parse_calendar(viewer.contribution_years, viewer.recent, day("2026-08-18")).unwrap();
        let slot = cal.slot(day("2026-08-10")).unwrap();
        assert_eq!(cal.level_at(slot / 7, slot % 7), 1, "10 of 40");
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
        let cal =
            parse_calendar(viewer.contribution_years, viewer.recent, day("2026-08-18")).unwrap();
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
        let viewer = response.data.unwrap().viewer;
        let cal =
            parse_calendar(viewer.contribution_years, viewer.recent, day("2026-08-18")).unwrap();
        assert_eq!(cal.total(), 7, "the shared day was counted twice");
    }

    /// Every year of the grid must be asked for, or the oldest columns render
    /// as an empty year that looks like a bug rather than like no data.
    #[test]
    fn the_query_covers_every_year_of_the_grid() {
        let today = day("2026-08-18");
        let windows = crate::contributions::year_windows(today);
        assert_eq!(windows.len(), 3, "three years, three windows");
        let q = build_query(50, today, 0);
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
        let cal =
            parse_calendar(viewer.contribution_years, viewer.recent, day("2026-08-18")).unwrap();
        assert_eq!(cal.total(), 3);
    }

    /// Measured before it was written: `contributionCalendar` has no `first:`
    /// connection, so it costs nothing on top of the existing query. If it
    /// ever grows one, this query stops being free and the invariant bites.
    #[test]
    fn the_calendar_field_rides_the_existing_query() {
        let q = build_query(50, day("2026-08-18"), 0);
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
    /// Startup used to block on the network for the whole GitHub fetch --
    /// measured at 5.7s to 8.7s against a 57-repo account -- because the repo
    /// list is derived from that dataset and a `cache_ttl` of two minutes
    /// meant any launch after a short break was a miss. It serves whatever is
    /// on disk now, however old; a poll fires on the first tick of the event
    /// loop and replaces it a moment later.
    #[tokio::test]
    async fn startup_serves_an_expired_payload_rather_than_waiting_on_the_network() {
        let dir = std::env::temp_dir().join(format!("fleetz_stale_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("github-50.json");
        std::fs::write(
            &path,
            r#"{"data":{"viewer":{
                "repositories":{"nodes":[]},
                "contributionsCollection":{"contributionCalendar":{"weeks":[
                  {"contributionDays":[{"date":"2026-08-18","contributionCount":7}]}
                ]}}
            }}}"#,
        )
        .unwrap();

        // TTL zero, so the payload is expired the instant it is written.
        let cache = crate::cache::ApiCache::at(path, 0);
        assert!(cache.read().is_none(), "fixture is not actually expired");

        let fetch = fetch_repos(50, &cache, CacheMode::AnyCached)
            .await
            .expect("an expired payload should still be served");
        assert!(
            fetch.calendar.is_some(),
            "served a payload the caller will reject as calendar-less"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The ordinary case, alongside the expired one above: a payload inside
    /// its TTL is served without a network round trip at all.
    ///
    /// `CacheMode::Bypass` is deliberately not covered here. Observing it
    /// means observing a network call, and a unit test that reaches GitHub is
    /// worse than no test -- it fails on a plane and passes for the wrong
    /// reason behind a proxy.
    #[tokio::test]
    async fn a_fresh_payload_is_served_without_touching_the_network() {
        let dir = std::env::temp_dir().join(format!("fleetz_bypass_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("github-50.json");
        std::fs::write(
            &path,
            r#"{"data":{"viewer":{"repositories":{"nodes":[]},
            "contributionsCollection":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":7}]}]}}}}}"#,
        )
        .unwrap();
        let cache = crate::cache::ApiCache::at(path, 86_400);
        assert!(cache.read().is_some(), "fixture should be fresh");
        assert!(
            fetch_repos(50, &cache, CacheMode::AnyCached).await.is_ok(),
            "AnyCached should have taken the fixture"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A GraphQL error arrives with HTTP 200 and an `errors` key, so nothing
    /// about the transport marks it as junk. It used to be written to disk
    /// the moment it arrived: one rate-limited poll replaced a 73KB payload
    /// with 95 bytes, and because the offline and non-2xx paths both fall back
    /// to `read_stale`, that took the last good data with it.
    #[test]
    fn a_rate_limited_body_does_not_overwrite_the_cache() {
        let dir = std::env::temp_dir().join(format!("fleetz_errbody_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("github-50.json");
        let good = r#"{"data":{"viewer":{"repositories":{"nodes":[]},
            "contributionsCollection":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":7}]}]}}}}}"#;
        std::fs::write(&path, good).unwrap();
        let cache = crate::cache::ApiCache::at(path.clone(), 86_400);

        let rate_limited =
            r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#;
        let result = accept_response(rate_limited, &cache, day("2026-08-18"));

        assert!(result.is_err(), "an errors-only body is not a fetch");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            good,
            "the error body overwrote the last good payload"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other half of the same rule: a body that did carry data still has
    /// to land, or the cache never fills and every launch blocks on
    /// the network.
    #[test]
    fn a_payload_with_data_is_cached() {
        let dir = std::env::temp_dir().join(format!("fleetz_okbody_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("github-50.json");
        let cache = crate::cache::ApiCache::at(path.clone(), 86_400);

        let good = r#"{"data":{"viewer":{"repositories":{"nodes":[]},
            "contributionsCollection":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":7}]}]}}}}}"#;
        assert!(accept_response(good, &cache, day("2026-08-18")).is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), good);
        let _ = std::fs::remove_dir_all(&dir);
    }

    const SERVABLE: &str = r#"{"data":{"viewer":{"repositories":{"nodes":[]},
            "contributionsCollection":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":7}]}]}}}}}"#;

    /// A cache file in its own directory, written `age` ago.
    fn planted_cache(
        tag: &str,
        age: std::time::Duration,
    ) -> (std::path::PathBuf, std::time::SystemTime) {
        let dir = std::env::temp_dir().join(format!("fleetz_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("github-50.json");
        std::fs::write(&path, SERVABLE).unwrap();
        let planted = std::time::SystemTime::now() - age;
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(planted)
            .unwrap();
        (path, planted)
    }

    fn assert_written_near(origin: Origin, planted: std::time::SystemTime) {
        let Origin::Cache {
            written: Some(written),
        } = origin
        else {
            panic!("expected a dated cache origin, got {origin:?}");
        };
        let drift = (written - DateTime::<chrono::Local>::from(planted))
            .num_seconds()
            .abs();
        assert!(drift < 1, "data time {written} is not the file's");
    }

    /// Rule 2: a request that failed with a cache on disk is a failure that
    /// happens to have data, never `Ok`. It used to come back as `Ok`, and an
    /// offline poll looked like a successful one. The cache file is left
    /// exactly as it was (rule 5).
    #[test]
    fn a_failed_attempt_with_a_cache_is_a_failure_carrying_the_cache() {
        let (path, planted) = planted_cache("fallback", std::time::Duration::from_secs(2 * 3600));
        let cache = crate::cache::ApiCache::at(path.clone(), 86_400);
        let failure = Failure::new(FailureKind::Offline, "GitHub API request failed: dns");

        let Err(error) = settle_attempt(Err(failure.clone()), &cache, day("2026-08-18")) else {
            panic!("a fallback came back as Ok");
        };
        assert_eq!(error.failure, failure);
        let fallback = error.fallback.expect("the cache was there to fall back on");
        assert_written_near(fallback.origin, planted);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SERVABLE);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// No cache to fall back on is the same failure with nothing attached:
    /// the launch error panel's case.
    #[test]
    fn a_failed_attempt_without_a_cache_has_nothing_to_fall_back_on() {
        let dir = std::env::temp_dir().join(format!("fleetz_nocache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = crate::cache::ApiCache::at(dir.join("github-50.json"), 86_400);
        let failure = Failure::new(FailureKind::Http, "GitHub API returned 502");
        let Err(error) = settle_attempt(Err(failure.clone()), &cache, day("2026-08-18")) else {
            panic!("no data, yet Ok");
        };
        assert_eq!(error.failure, failure);
        assert!(error.fallback.is_none());
    }

    /// A cache-served launch carries the cache file's write time as its data
    /// time, so the header can say how old it is.
    #[tokio::test]
    async fn a_cache_served_launch_reports_the_files_age() {
        let (path, planted) = planted_cache("aged", std::time::Duration::from_secs(3 * 3600));
        let cache = crate::cache::ApiCache::at(path.clone(), 0);
        let fetch = fetch_repos(50, &cache, CacheMode::AnyCached)
            .await
            .expect("an expired payload should still be served");
        assert_written_near(fetch.origin, planted);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// The fresh-within-TTL read is dated the same way as the stale one.
    #[tokio::test]
    async fn a_fresh_cache_read_reports_the_files_age_too() {
        let (path, planted) = planted_cache("aged_fresh", std::time::Duration::from_secs(60));
        let cache = crate::cache::ApiCache::at(path.clone(), 86_400);
        assert!(cache.read().is_some(), "fixture should be fresh");
        let fetch = fetch_repos(50, &cache, CacheMode::AnyCached).await.unwrap();
        assert_written_near(fetch.origin, planted);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// An accepted answer is network data, and the kinds of a refused one are
    /// told apart: a rate limit is an API failure, never "offline".
    #[test]
    fn accepted_answers_are_network_and_refusals_have_a_kind() {
        let dir = std::env::temp_dir().join(format!("fleetz_kinds_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = crate::cache::ApiCache::at(dir.join("github-50.json"), 86_400);
        let today = day("2026-08-18");

        let ok = accept_response(SERVABLE, &cache, today).ok().unwrap();
        assert_eq!(ok.origin, Origin::Network);

        let rate_limited =
            r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#;
        let kind = |body| accept_response(body, &cache, today).err().unwrap().kind;
        assert_eq!(kind(rate_limited), FailureKind::Api);
        assert_eq!(kind("<html>"), FailureKind::Malformed);
        assert_eq!(kind(r#"{"data":null}"#), FailureKind::Malformed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rate limit is read off GitHub's `type`, not the wording of its
    /// message, so the header can name it whatever the text says.
    #[test]
    fn a_typed_rate_limit_body_is_a_rate_limit() {
        let dir = std::env::temp_dir().join(format!("fleetz_typed_rl_{}", std::process::id()));
        let cache = crate::cache::ApiCache::at(dir.join("github-50.json"), 86_400);
        let today = day("2026-08-18");
        let fail = |body| accept_response(body, &cache, today).err().unwrap();

        let typed = fail(r#"{"errors":[{"type":"RATE_LIMITED","message":"Slow down"}]}"#);
        assert_eq!(typed.kind, FailureKind::Api);
        assert!(typed.rate_limited, "typed RATE_LIMITED missed");

        // A typed error of another class is not a rate limit, whatever the
        // message happens to say.
        let other = fail(r#"{"errors":[{"type":"NOT_FOUND","message":"no rate limit here"}]}"#);
        assert_eq!(other.kind, FailureKind::Api);
        assert!(!other.rate_limited);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With no `type` on any error, the message is all there is to go on.
    #[test]
    fn an_untyped_rate_limit_body_falls_back_to_the_message() {
        let dir = std::env::temp_dir().join(format!("fleetz_untyped_rl_{}", std::process::id()));
        let cache = crate::cache::ApiCache::at(dir.join("github-50.json"), 86_400);
        let today = day("2026-08-18");
        let fail = |body| accept_response(body, &cache, today).err().unwrap();

        assert!(fail(r#"{"errors":[{"message":"API rate limit exceeded"}]}"#).rate_limited);
        assert!(!fail(r#"{"errors":[{"message":"Something went wrong"}]}"#).rate_limited);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bug this window exists for. GitHub's year-range calendar is served
    /// from an index that lags: on 2026-08-24 the year window reported that
    /// day as 0 while a one-day window on the same date reported 3, so a
    /// morning's work was missing from the green strip. The short window has
    /// to win for the days both cover.
    #[test]
    fn the_recent_window_overrides_a_stale_year_window() {
        let json = r#"{"data":{"viewer":{
            "repositories":{"nodes":[]},
            "y0":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":0}]}
            ]}},
            "recent":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":3}]}
            ]}}
        }}}"#;
        let viewer = serde_json::from_str::<GraphQlResponse>(json)
            .unwrap()
            .data
            .unwrap()
            .viewer;
        assert!(
            !viewer.contribution_years.contains_key("recent"),
            "the recent window must not also land in the flattened map, or its \
             merge position is decided by BTreeMap ordering again"
        );
        let cal =
            parse_calendar(viewer.contribution_years, viewer.recent, day("2026-08-18")).unwrap();
        assert_eq!(
            cal.total(),
            3,
            "the lagging year window overwrote the fresh one"
        );
    }

    /// `recent` is merged last, so anything it reports for a day wins. A week
    /// padded back past its own `from` would therefore `set` days that only
    /// the year windows can know about to the zero it pads with, blanking the
    /// left edge of the strip one day per launch.
    #[test]
    fn a_recent_week_padded_past_its_start_cannot_blank_older_days() {
        let today = day("2026-08-18");
        let from = crate::contributions::recent_merge_floor(today);
        let before = (from - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let json = format!(
            r#"{{"data":{{"viewer":{{
            "repositories":{{"nodes":[]}},
            "y0":{{"contributionCalendar":{{"weeks":[
              {{"contributionDays":[{{"date":"{before}","contributionCount":5}}]}}
            ]}}}},
            "recent":{{"contributionCalendar":{{"weeks":[
              {{"contributionDays":[{{"date":"{before}","contributionCount":0}}]}}
            ]}}}}
        }}}}}}"#
        );
        let viewer = serde_json::from_str::<GraphQlResponse>(&json)
            .unwrap()
            .data
            .unwrap()
            .viewer;
        let cal = parse_calendar(viewer.contribution_years, viewer.recent, today).unwrap();
        assert_eq!(
            cal.total(),
            5,
            "a padded week zeroed a day outside its window"
        );
    }

    /// Re-checks, against the live API, the GitHub behaviour the rotation is
    /// built on. Not part of the suite: it needs the network and an
    /// authenticated `gh`, and a unit test that reaches GitHub is worse than
    /// no test -- it fails on a plane and passes for the wrong reason behind a
    /// proxy. Run it by hand when the strip looks wrong again:
    ///
    ///   cargo test rotation_premise -- --ignored --nocapture
    ///
    /// It asks the same day through two `from` dates and prints both. They may
    /// legitimately agree when nothing has changed recently; what would be
    /// news is them agreeing *right after a push that only one reports*, which
    /// would mean the cache is gone and the rotation is now dead weight.
    #[tokio::test]
    #[ignore]
    async fn rotation_premise_still_holds() {
        let today = chrono::Local::now().date_naive();
        let day_str = today.format("%Y-%m-%d").to_string();
        let ask = |from: chrono::NaiveDate| {
            let q = format!(
                r#"{{ viewer {{ c: contributionsCollection(from: "{}T00:00:00Z", to: "{}T23:59:59Z") {{ contributionCalendar {{ weeks {{ contributionDays {{ date contributionCount }} }} }} }} }} }}"#,
                from.format("%Y-%m-%d"),
                day_str
            );
            async move {
                let out = tokio::process::Command::new("gh")
                    .args(["api", "graphql", "-f"])
                    .arg(format!("query={q}"))
                    .output()
                    .await
                    .expect("gh must be on PATH");
                String::from_utf8_lossy(&out.stdout).to_string()
            }
        };
        let count = |body: &str| {
            body.split(&format!(r#""date":"{day_str}","contributionCount":"#))
                .nth(1)
                .and_then(|t| t.split('}').next())
                .map(str::to_string)
        };
        for back in [7_i64, 8, 9] {
            let from = today - chrono::Duration::days(back);
            let body = ask(from).await;
            println!(
                "from {} (today - {back}) -> today = {:?}",
                from,
                count(&body)
            );
        }
        println!(
            "If these disagree, the per-`from` cache is still there and the \
             rotation is earning its keep. If they always agree even right \
             after a push, re-read contributions.rs::ROTATION_SPAN."
        );
    }

    /// The rotation has to reach the query, not just exist. GitHub caches a
    /// `contributionsCollection` against its `from` date, so two polls that
    /// build the same `from` get the same cached answer however much has
    /// happened in between.
    #[test]
    fn the_rotation_changes_the_from_the_query_asks_for() {
        let today = day("2026-08-18");
        let extract = |q: &str| {
            let at = q.find("recent: contributionsCollection").unwrap();
            let from = q[at..].find("from: \"").unwrap() + at + 7;
            q[from..from + 10].to_string()
        };
        let a = extract(&build_query(50, today, 7));
        let b = extract(&build_query(50, today, 8));
        assert_ne!(a, b, "consecutive holds asked GitHub the same question");
        // `to` is not in GitHub's cache key, so it must not move: today is
        // what the window is for.
        for tick in [7, 8, 300] {
            assert!(
                build_query(50, today, tick).contains(r#"to: "2026-08-18T23:59:59Z""#),
                "tick {tick} stopped reaching today"
            );
        }
    }

    /// A payload off the disk cache was written under whatever rotation was in
    /// force then, which the parser cannot recover. The merged range is
    /// therefore fixed rather than derived from the window, and any rotation's
    /// payload has to merge identically.
    #[test]
    fn a_payload_from_any_rotation_merges_the_same_days() {
        let today = day("2026-08-18");
        let json = r#"{"data":{"viewer":{
            "repositories":{"nodes":[]},
            "y0":{"contributionCalendar":{"weeks":[
              {"contributionDays":[{"date":"2026-08-18","contributionCount":0}]}
            ]}},
            "recent":{"contributionCalendar":{"weeks":[
              {"contributionDays":[
                {"date":"2026-08-18","contributionCount":11},
                {"date":"2026-01-01","contributionCount":99}
              ]}
            ]}}
        }}}"#;
        let viewer = serde_json::from_str::<GraphQlResponse>(json)
            .unwrap()
            .data
            .unwrap()
            .viewer;
        let cal = parse_calendar(viewer.contribution_years, viewer.recent, today).unwrap();
        // Today comes from the fresh window, beating the stale year one.
        assert_eq!(
            cal.total(),
            11,
            "the fresh value did not win, or the far day leaked in"
        );
    }

    /// The alias has to be in the query at all, and has to reach today: a
    /// window that stops short of it re-asks for nothing that was lagging.
    #[test]
    fn the_query_asks_for_the_recent_window_too() {
        let today = day("2026-08-18");
        let q = build_query(50, today, 0);
        assert!(
            q.contains("recent: contributionsCollection"),
            "no recent alias"
        );
        assert!(
            q.contains(r#"to: "2026-08-18T23:59:59Z""#),
            "the recent window must reach today"
        );
        // A `first:` on this alias would make it a paid connection rather than
        // a free field, which is the whole reason it is affordable.
        let recent = &q[q.find("recent: contributionsCollection").unwrap()..];
        let recent = &recent[..recent.find("repositories(").unwrap_or(recent.len())];
        assert!(
            !recent.contains("first:"),
            "the recent alias must stay free"
        );
    }

    /// Node count is billed on the `first:` asked for, not on what comes back,
    /// and the org fallback multiplies it by the page size inside each org.
    /// Reusing `repo_limit` there measured at cost 26 / 9125 nodes on
    /// 2026-08-24, to retrieve 17 repos from 4 orgs; at its own page size the
    /// identical result set measured cost 3 / 1030. One fleetz polling every
    /// 30s at cost 26 spends 3120 of a 5000/hour budget, which is how the
    /// limit ran out; at cost 3 it spends 360.
    #[test]
    fn the_org_fallback_is_billed_at_its_own_page_size() {
        let q = build_query(50, day("2026-08-18"), 0);
        assert!(
            q.contains(&format!("organizations(first: {MAX_ORGS})")),
            "org page not wired to MAX_ORGS"
        );
        assert!(
            q.contains(&format!(
                "repositories(first: {ORG_REPO_LIMIT}, orderBy: {{field: PUSHED_AT"
            )),
            "org repo page not wired to ORG_REPO_LIMIT"
        );
        // The viewer branch keeps `repo_limit`; the fallback must not, or the
        // node count multiplies by it again.
        assert_eq!(
            q.matches("repositories(first: 50").count(),
            1,
            "the org fallback is reusing repo_limit"
        );
    }

    /// The PR page is deliberately *not* shrunk to save cost. Measured on the
    /// same fleet, dropping it from 5 to 3 moved nodeCount 1198 -> 858 and
    /// left cost at 4: the connection's per-call floor dominates, so a
    /// smaller page buys nothing and loses PRs.
    #[test]
    fn the_pr_page_is_not_shrunk_for_cost() {
        let q = build_query(50, day("2026-08-18"), 0);
        assert!(q.contains("pullRequests(states: OPEN, first: 5,"));
    }
}
