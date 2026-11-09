use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::contributions::{Calendar, Source};
pub use crate::model::GithubRepo;
use crate::model::{CiStatus, PullRequest, RepoKind, ReviewStatus};

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
    /// upgraded install serves its old cache and reports no PRs anywhere
    /// until a poll replaces it, silently.
    pub has_pr_counts: bool,
    /// Whether this came off the network or the disk cache, and how old.
    ///
    /// Startup serves any cached payload however old (`CacheMode::AnyCached`),
    /// so "the load returned a dataset" does not mean "GitHub was just asked".
    /// The caller needs the difference: starting the re-poll clock from a
    /// cached payload leaves the first real poll a whole interval away, and
    /// the dashboard spends it showing whatever the last session left behind.
    pub origin: Origin,
    /// What the attempt learned besides the payload. Default for a
    /// cache-served load, which asked GitHub nothing.
    pub meta: FetchMeta,
}

/// What one attempt learned about the account and the answer, beside the
/// payload or the failure. One struct with a `Default`, so a later field
/// (coverage, the account key: FZ-09) lands inside it and no test that builds
/// a `GithubFetch` or `FetchError` has to change again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchMeta {
    /// The API budget the response's headers reported, if they did.
    pub budget: Option<BudgetReading>,
    /// The account key: `token_fingerprint` of the token the attempt carried,
    /// or whose keyed cache file the load was served from. `None` when
    /// unknown: a launch served from the legacy file or under the signed-out
    /// rule, or a lookup that failed. Never the token itself.
    pub account: Option<String>,
    /// How much of GitHub's answer the payload holds, read from the payload
    /// itself, so a cached payload and a fresh one say the same. `None` when
    /// there was no payload (a failed attempt).
    pub coverage: Option<Coverage>,
}

/// What one payload covers, per page, built from the pages before
/// `merge_sources` so its dedupe does not touch the counts.
/// See `docs/coverage-cache-design.md` 4.1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    /// `viewer.repositories`.
    pub viewer: PageCoverage,
    /// `viewer.organizations`, the org list itself (its `boundary` is always
    /// `None`: orgs are not ordered by push). `None` when the payload has no
    /// org list at all.
    pub org_list: Option<PageCoverage>,
    /// Each org's `repositories`, in the order the org list returned them.
    pub org_pages: Vec<PageCoverage>,
}

/// One page of a connection as the payload describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageCoverage {
    /// Nodes on the page.
    pub returned: u32,
    /// GitHub's `totalCount`. `None` for a payload written before it was
    /// asked for: unknown, never "complete".
    pub total: Option<u32>,
    /// `pageInfo { hasNextPage }`, unknown the same way.
    pub has_next_page: Option<bool>,
    /// The page's oldest push. The connections order by push time, so
    /// nothing older than this was fetched from the rest of the connection.
    pub boundary: Option<DateTime<Utc>>,
}

/// GitHub's API budget as one response's `x-ratelimit-*` headers stated it.
/// See `docs/api-budget-design.md` 4.1.
///
/// Read from every response that carries the headers, success or failure,
/// since a refused answer states the budget as truly as an accepted one. A
/// response without them (a gateway 502) carries no reading, and the last
/// known one stands. Nothing here is anchored to the local clock: `reset` and
/// `date` are the server's, raw, so the caller can turn the reset into a delay
/// in the server's own time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetReading {
    /// `x-ratelimit-limit`: points per window.
    pub limit: u32,
    /// `x-ratelimit-remaining`.
    pub remaining: u32,
    /// `x-ratelimit-used`.
    pub used: u32,
    /// `x-ratelimit-reset`: when the window ends, in epoch seconds on the
    /// server's clock.
    pub reset: i64,
    /// The response's `Date`, the server's clock at the same moment. `None`
    /// when absent or unparseable.
    pub date: Option<DateTime<Utc>>,
    /// `token_fingerprint` of the token the request carried. Readings from two
    /// tokens are two accounts, possibly, and never compared. Memory only.
    pub token_id: String,
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
    /// Transport: DNS, TLS, no route, a connection that could not be opened
    /// (a connect timeout among them), a body cut off.
    Offline,
    /// A non-2xx status no other kind covers, a 403 or 429 rate limit among
    /// them.
    Http,
    /// A GraphQL `errors` body; the primary rate limit is one of these.
    Api,
    /// No token from `gh`.
    Auth,
    /// A body that does not parse, or parses without `data`.
    Malformed,
    /// No answer in time after the connection was made: the client's total
    /// deadline (`TOTAL_TIMEOUT`), or the give-up after
    /// `GITHUB_POLL_STUCK_AFTER`.
    TimedOut,
    /// A 5xx answer. Not "query too slow": a 502 or 504 can be GitHub's
    /// deadline, a deploy or an outage, and one answer does not say which.
    Server,
    /// Credentials or permissions refused: a 401, or a 403 that is not a
    /// rate limit (scopes, SSO).
    Refused,
}

/// Which of GitHub's rate limits refused an attempt.
/// See `docs/api-budget-design.md` 4.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimit {
    /// The hourly points budget is spent; it lifts at `x-ratelimit-reset`.
    Primary,
    /// A secondary limit (concurrency, points a minute, server time). It lifts
    /// after `retry_after` when GitHub said, otherwise after an unstated wait.
    Secondary {
        retry_after: Option<std::time::Duration>,
    },
}

/// One failed attempt: its kind, and the message the status line shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    /// Any failure that is one of GitHub's rate limits, an `Api` body or a
    /// 403 or 429: the common way the API refuses, and one that clears on its
    /// own, so the header names it. True exactly when `limit` is set.
    pub rate_limited: bool,
    /// Which limit, with its `retry-after` when it is a secondary one.
    pub limit: Option<RateLimit>,
}

impl Failure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            rate_limited: false,
            limit: None,
        }
    }

    /// An `Api` failure that is a rate limit.
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self::limited(FailureKind::Api, RateLimit::Primary, message)
    }

    /// A failure that is a rate limit of the given kind.
    fn limited(kind: FailureKind, limit: RateLimit, message: impl Into<String>) -> Self {
        Self {
            rate_limited: true,
            limit: Some(limit),
            ..Self::new(kind, message)
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
    /// What the failed attempt learned, a refused answer's budget among it.
    /// Boxed so the error stays small enough to return by value as it grows
    /// (clippy's `result_large_err`).
    pub meta: Box<FetchMeta>,
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
        // Levels are kept, not drawn: the strip shades every day by count
        // (`Calendar::shade_scale`, FZ-91).
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

/// `totalCount` and `pageInfo` on the connections are absent from a payload
/// cached before they were asked for, hence the defaults.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrganizationConnection {
    nodes: Vec<OrganizationNode>,
    #[serde(default)]
    total_count: Option<u32>,
    #[serde(default)]
    page_info: Option<PageInfo>,
}

#[derive(Deserialize)]
struct OrganizationNode {
    repositories: RepositoryConnection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryConnection {
    nodes: Vec<RepoNode>,
    #[serde(default)]
    total_count: Option<u32>,
    #[serde(default)]
    page_info: Option<PageInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
}

impl RepositoryConnection {
    fn coverage(&self) -> PageCoverage {
        PageCoverage {
            returned: self.nodes.len() as u32,
            total: self.total_count,
            has_next_page: self.page_info.as_ref().map(|p| p.has_next_page),
            boundary: self
                .nodes
                .iter()
                .filter_map(|n| DateTime::parse_from_rfc3339(n.pushed_at.as_deref()?).ok())
                .map(|t| t.with_timezone(&Utc))
                .min(),
        }
    }
}

impl Coverage {
    /// The coverage of the viewer's pages, read before they are merged.
    fn of(
        repositories: &RepositoryConnection,
        organizations: Option<&OrganizationConnection>,
    ) -> Self {
        Self {
            viewer: repositories.coverage(),
            org_list: organizations.map(|orgs| PageCoverage {
                returned: orgs.nodes.len() as u32,
                total: orgs.total_count,
                has_next_page: orgs.page_info.as_ref().map(|p| p.has_next_page),
                boundary: None,
            }),
            org_pages: organizations
                .map(|orgs| {
                    orgs.nodes
                        .iter()
                        .map(|org| org.repositories.coverage())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
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
    /// Absent from a payload cached before FZ-35b asked for it, which
    /// parses unchanged: no cache format change.
    #[serde(default)]
    disk_usage: Option<u64>,
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

/// What one fleet poll costs in GraphQL points at `repo_limit`, as measured
/// (`docs/api-budget-design.md` section 2): 2 at 20 (M2), 3 at 50 (M1), 4 at
/// 100 (M6, a dry run). A limit between two measured points takes the higher
/// price, so pacing on it errs slow. The headers carry no per-query cost, so
/// this is the only source: a change to `build_query` re-measures and updates
/// it under "Pricing a query change".
pub fn poll_cost(repo_limit: u32) -> u32 {
    match repo_limit {
        0..=20 => 2,
        21..=50 => 3,
        _ => 4,
    }
}

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
    //
    // `diskUsage` feeds the fetch cap's pre-check (FZ-35b, #85). Free, as a
    // scalar with no `first:` should be: measured on 2026-10-20 at
    // `repo_limit` 50 the poll priced cost 3 / nodeCount 1030 with and
    // without it.
    let repo_fields = format!(
        r#"nodes {{
      name, pushedAt, isPrivate, diskUsage
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
    // for, and this window's `from` rotates, so its levels would sit on a
    // scale that changes every few minutes. Those days get the year window's
    // level instead (`Calendar::derive_missing_levels`). No level is drawn:
    // the strip shades by count (`Calendar::shade_scale`, FZ-91).
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
    // `totalCount` and `pageInfo` on each of the three connections say how
    // much of it the page holds (`Coverage`). Free: a field on a connection
    // already asked for adds no `first:`, and measured on 2026-10-09 the query
    // priced cost 3 / nodeCount 1030 before and after (FZ-09b, #58).
    //
    // `isArchived: false` on both repo connections keeps archived repos out
    // (#69): archiving counts as a push, so one sorted near the top and took
    // a slot in `repo_limit`. Measured on 2026-10-10 at `repo_limit` 50 it is
    // free too, cost 3 / nodeCount 1030 before and after. `totalCount` is the
    // filtered count, so coverage counts live repos only: the viewer's went
    // 98 -> 97 and the four orgs' (6, 5, 4, 5) held, having no archived repos.
    let page_fields = "totalCount\n    pageInfo { hasNextPage }";
    format!(
        r#"{{ viewer {{
{calendar_fields}{recent_field}  repositories(first: {repo_limit}, orderBy: {{field: PUSHED_AT, direction: DESC}},
    isArchived: false, affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]) {{
    {page_fields}
    {repo_fields}
  }}
  organizations(first: {MAX_ORGS}) {{
    {page_fields}
    nodes {{
      repositories(first: {ORG_REPO_LIMIT}, orderBy: {{field: PUSHED_AT, direction: DESC}}, isArchived: false) {{
        {page_fields}
        {repo_fields}
      }}
    }}
  }}
}}}}"#
    )
}

/// Flatten both repo sources into one list, newest push first, keeping the
/// first entry per owner and name (`GithubRepo::identity_key`) so a repo
/// returned by both sources appears once.
///
/// Keyed by the bare name, `alice/api` and `company/api` were one repo and
/// the older push was dropped from the dataset, not even shown as not cloned.
/// repoz dedupes by `nameWithOwner` for the same reason. An entry without an
/// owner (a payload cached before `login` was asked for) dedupes by name.
fn merge_sources(viewer: Viewer) -> Vec<RepoNode> {
    let mut nodes = viewer.repositories.nodes;
    if let Some(orgs) = viewer.organizations {
        for org in orgs.nodes {
            nodes.extend(org.repositories.nodes);
        }
    }
    // Sort before dedup so the survivor of a duplicate pair is the freshest.
    // The sort is stable, so on an equal push the viewer's copy is kept.
    nodes.sort_by(|a, b| b.pushed_at.cmp(&a.pushed_at));
    let mut seen = std::collections::HashSet::new();
    nodes.retain(|n| {
        let owner = n.owner.as_ref().and_then(|o| o.login.as_deref());
        seen.insert(GithubRepo::identity_key(owner, &n.name))
    });
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
    // Before the merge: its dedupe would change the counts.
    let coverage = Coverage::of(&repositories, organizations.as_ref());
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
        meta: FetchMeta {
            coverage: Some(coverage),
            ..FetchMeta::default()
        },
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

/// The single-file fetch from before FZ-09c, kept for the tests that pin a
/// payload with `ApiCache::at`. Production goes through `fetch_github`.
#[cfg(test)]
pub async fn fetch_repos(
    repo_limit: u32,
    cache: &crate::cache::ApiCache,
    mode: CacheMode,
) -> Result<GithubFetch, FetchError> {
    // One "today" for the whole fetch, so every path builds the same grid.
    let today = chrono::Local::now().date_naive();

    if mode == CacheMode::AnyCached
        && let Some(fetch) = serve_cached(cache, today)
    {
        return Ok(fetch);
    }

    let (attempt, budget) = request(repo_limit, cache, today).await;
    with_budget(settle_attempt(attempt, cache, today), budget)
}

/// The cache file's payload if it is good enough to paint, stamped with the
/// file's write time.
///
/// A payload written before the calendar field existed parses fine but
/// carries no calendar, and serving it would show an empty year. Same for one
/// written before PR counts rode along. Treat either as a miss: the whole
/// point of the poll is that both come for free.
fn serve_cached(cache: &crate::cache::ApiCache, today: chrono::NaiveDate) -> Option<GithubFetch> {
    let usable =
        |fetch: Option<GithubFetch>| fetch.filter(|f| f.calendar.is_some() && f.has_pr_counts);
    usable(read_cached(cache, today))
}

/// The host every request goes to, as the cache file name and a `RemoteRef`
/// key spell it. The endpoint is `api.github.com`; Enterprise is not
/// supported.
pub const HOST: &str = "github.com";

/// How long launch waits on the token lookup before it treats the user as
/// signed out (O11). `gh` answers in about 0.05 s; this is a margin for a
/// keychain that stalls, not a measured worst case. Only the launch lookup is
/// bounded: a poll's is covered by the client deadline and the give-up.
pub const LAUNCH_LOOKUP_BOUND: std::time::Duration = std::time::Duration::from_secs(2);

/// Fetch the dataset through the keyed cache, looking the token up with `gh`.
///
/// `CacheMode::AnyCached` is launch: one token lookup bounded at
/// `LAUNCH_LOOKUP_BOUND` picks the file to paint from (`KeyedCache::launch_reads`),
/// and superseded files are cleaned up once the launch has data.
/// `CacheMode::Bypass` is a poll or `R`: the request, with only this
/// credential's own keyed file to fall back on.
pub async fn fetch_github(
    repo_limit: u32,
    cache: &crate::cache::KeyedCache,
    mode: CacheMode,
) -> Result<GithubFetch, FetchError> {
    fetch_keyed(
        repo_limit,
        cache,
        mode,
        get_gh_token,
        client(),
        GRAPHQL_URL,
        LAUNCH_LOOKUP_BOUND,
    )
    .await
}

/// `fetch_github` with the token source, the endpoint and the launch bound
/// passed in, so a test drives every path without `gh` or the network.
///
/// The token is looked up **once** per attempt (`token_source` is `FnOnce`):
/// the lookup that picks the file is the lookup whose token goes into the
/// request's `Authorization` header, and that request's answer is written to
/// that token's file. Two lookups could straddle an account switch and write
/// one account's answer into the other's file.
pub(crate) async fn fetch_keyed<S, F>(
    repo_limit: u32,
    cache: &crate::cache::KeyedCache,
    mode: CacheMode,
    token_source: S,
    client: &reqwest::Client,
    url: &str,
    launch_bound: std::time::Duration,
) -> Result<GithubFetch, FetchError>
where
    S: FnOnce() -> F,
    F: std::future::Future<Output = Result<String, String>>,
{
    // One "today" for the whole fetch, so every path builds the same grid.
    let today = chrono::Local::now().date_naive();
    let lookup = match mode {
        CacheMode::AnyCached => tokio::time::timeout(launch_bound, token_source())
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "`gh auth token` did not answer within {} s",
                    launch_bound.as_secs_f32()
                ))
            }),
        CacheMode::Bypass => token_source().await,
    };
    let account = lookup.as_deref().ok().map(token_fingerprint);

    if mode == CacheMode::AnyCached
        && let Some((fetch, served)) = read_at_launch(cache, account.as_deref(), today)
    {
        cache.remove_superseded(account.as_deref(), Some(&served));
        return Ok(fetch);
    }

    // Signed out, or a launch lookup that timed out with nothing to paint:
    // no token, no request, and no account whose file could be a fallback.
    let token = lookup.map_err(|message| FetchError {
        failure: Failure::new(FailureKind::Auth, message),
        fallback: None,
        meta: Default::default(),
    })?;
    let account = token_fingerprint(&token);
    let own = cache.for_account(&account);
    let query = build_query(
        repo_limit,
        today,
        crate::contributions::rotation_tick(chrono::Utc::now()),
    );
    let (attempt, budget) = exchange(client, url, &token, &query, &own, today).await;
    let meta = FetchMeta {
        budget,
        account: Some(account.clone()),
        coverage: None,
    };
    let result = with_meta(settle_attempt(attempt, &own, today), meta);
    if mode == CacheMode::AnyCached && result.is_ok() {
        cache.remove_superseded(Some(&account), None);
    }
    result
}

/// The first file `launch_reads` offers that is good enough to paint, with
/// its path. Only the credential's own file makes the load's account known.
fn read_at_launch(
    cache: &crate::cache::KeyedCache,
    account: Option<&str>,
    today: chrono::NaiveDate,
) -> Option<(GithubFetch, std::path::PathBuf)> {
    cache.launch_reads(account).into_iter().find_map(|read| {
        let mut fetch = serve_cached(&read.cache, today)?;
        if read.own {
            fetch.meta.account = account.map(str::to_string);
        }
        Some((fetch, read.cache.path().to_path_buf()))
    })
}

/// Attach the attempt's budget reading to its result, whichever way it went.
#[cfg(test)]
fn with_budget(
    result: Result<GithubFetch, FetchError>,
    budget: Option<BudgetReading>,
) -> Result<GithubFetch, FetchError> {
    with_meta(
        result,
        FetchMeta {
            budget,
            ..FetchMeta::default()
        },
    )
}

/// Attach what the attempt learned to its result, whichever way it went. A
/// payload's coverage is the payload's own and is kept.
fn with_meta(
    result: Result<GithubFetch, FetchError>,
    meta: FetchMeta,
) -> Result<GithubFetch, FetchError> {
    match result {
        Ok(fetch) => Ok(GithubFetch {
            meta: FetchMeta {
                coverage: fetch.meta.coverage,
                ..meta
            },
            ..fetch
        }),
        Err(error) => Err(FetchError {
            meta: Box::new(meta),
            ..error
        }),
    }
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
        meta: Default::default(),
    })
}

pub(crate) const GRAPHQL_URL: &str = "https://api.github.com/graphql";

/// How long opening the connection may take. A connect that runs out is
/// `Offline`: the path to GitHub is not up (A5).
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// How long the whole request may take, the body read included. Clears the
/// measured 6 to 7.5 s answer and the 11 s gateway 502 with margin, and fires
/// well before the 60 s give-up, which stays the backstop for a hung token
/// subprocess.
const TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The one client every request goes through. It used to be built per
/// request with no timeout at all, so a request that never answered was
/// only ever ended by the give-up, and every poll paid a fresh TLS handshake.
/// The CI pane's REST calls (`ci.rs`) share it.
pub(crate) fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| build_client(CONNECT_TIMEOUT, TOTAL_TIMEOUT))
}

/// A client with the two deadlines. `reqwest::Client::new()` panics on the
/// same TLS backend failure `build` reports, so the `expect` loses nothing.
fn build_client(connect: std::time::Duration, total: std::time::Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(connect)
        .timeout(total)
        .build()
        .expect("the HTTP client could not be built")
}

/// One network attempt: token, request, status, body. The budget reading
/// travels beside the result, since a refused answer carries one too.
#[cfg(test)]
async fn request(
    repo_limit: u32,
    cache: &crate::cache::ApiCache,
    today: chrono::NaiveDate,
) -> (Result<GithubFetch, Failure>, Option<BudgetReading>) {
    let token = match get_gh_token().await {
        Ok(token) => token,
        Err(e) => return (Err(Failure::new(FailureKind::Auth, e)), None),
    };
    let query = build_query(
        repo_limit,
        today,
        crate::contributions::rotation_tick(chrono::Utc::now()),
    );
    exchange(client(), GRAPHQL_URL, &token, &query, cache, today).await
}

/// Send the query and turn whatever comes back into a result. Split from
/// `request` so a test can point it at a local listener with short deadlines.
async fn exchange(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    query: &str,
    cache: &crate::cache::ApiCache,
    today: chrono::NaiveDate,
) -> (Result<GithubFetch, Failure>, Option<BudgetReading>) {
    let resp = match client
        .post(url)
        .header("Authorization", format!("bearer {token}"))
        .header("User-Agent", "fleetz")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
    {
        Ok(r) => r,
        // No answer. `settle_attempt` attaches the cache, so launch and `R`
        // still work offline.
        Err(e) => {
            return (
                Err(transport_failure(&e, "GitHub API request failed")),
                None,
            );
        }
    };

    let status = resp.status();
    let headers = resp.headers().clone();
    let budget = read_budget(&headers, &token_fingerprint(token));
    let body = match resp.text().await {
        Ok(body) => body,
        Err(e) if status.is_success() => {
            return (
                Err(transport_failure(&e, "failed to read response")),
                budget,
            );
        }
        // A refusal is decided by its status and headers; the body only adds
        // the secondary-limit wording and the message, so a lost one is no
        // reason to call GitHub's answer something else.
        Err(_) => String::new(),
    };
    if !status.is_success() {
        return (Err(classify_refusal(status, &headers, &body)), budget);
    }
    (accept_response(&body, cache, today), budget)
}

/// A request that got no complete answer. A connect failure, a connect
/// timeout included, is `Offline`; a deadline reached after connecting,
/// during the body read included, is `TimedOut` (A5).
pub(crate) fn transport_failure(error: &reqwest::Error, context: &str) -> Failure {
    Failure::new(
        transport_kind(error.is_connect(), error.is_timeout()),
        format!("{context}: {error}"),
    )
}

/// reqwest marks a connect timeout as both a connect and a timeout error, so
/// the connect test goes first: a dropped Wi-Fi is not a slow server, and as
/// `TimedOut` it would be put on the server's backoff ladder.
fn transport_kind(is_connect: bool, is_timeout: bool) -> FailureKind {
    if is_connect {
        FailureKind::Offline
    } else if is_timeout {
        FailureKind::TimedOut
    } else {
        FailureKind::Offline
    }
}

/// Classify a non-2xx answer by the table in `docs/api-budget-design.md` 4.3.
/// The rows are checked in order: a `retry-after` names a secondary limit
/// even beside `x-ratelimit-remaining: 0`.
pub(crate) fn classify_refusal(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> Failure {
    let message = refusal_message(status, body);
    let code = status.as_u16();
    if code == 403 || code == 429 {
        if let Some(value) = headers.get(reqwest::header::RETRY_AFTER) {
            let retry_after = value
                .to_str()
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(std::time::Duration::from_secs);
            return Failure::limited(
                FailureKind::Http,
                RateLimit::Secondary { retry_after },
                message,
            );
        }
        if header_number::<u32>(headers, "x-ratelimit-remaining") == Some(0) {
            return Failure::limited(FailureKind::Http, RateLimit::Primary, message);
        }
        if code == 429 || body.to_lowercase().contains("secondary rate limit") {
            let limit = RateLimit::Secondary { retry_after: None };
            return Failure::limited(FailureKind::Http, limit, message);
        }
    }
    let kind = match code {
        401 | 403 => FailureKind::Refused,
        500..=599 => FailureKind::Server,
        _ => FailureKind::Http,
    };
    Failure::new(kind, message)
}

/// The longest stretch of a refusal's body the message keeps.
const REFUSAL_DETAIL_CHARS: usize = 200;

/// The status, and what the body says about it when it says something
/// readable: GitHub's JSON `message`, or plain text, on one line and
/// trimmed. An HTML page (a gateway's) is left out.
fn refusal_message(status: reqwest::StatusCode, body: &str) -> String {
    let head = format!("GitHub API returned {status}");
    let text = body.trim();
    if text.starts_with('<') {
        return head;
    }
    let said = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|json| json.get("message")?.as_str().map(str::to_string))
        .unwrap_or_else(|| text.to_string());
    let line: String = said
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(REFUSAL_DETAIL_CHARS)
        .collect();
    match line.is_empty() {
        true => head,
        false => format!("{head}: {line}"),
    }
}

/// The budget the response's headers state, or `None` unless all four of
/// limit, remaining, used and reset are there and numeric: half a window is
/// nothing to pace on. `Date` is optional; without it the caller falls back
/// to the local clock.
pub(crate) fn read_budget(
    headers: &reqwest::header::HeaderMap,
    token_id: &str,
) -> Option<BudgetReading> {
    Some(BudgetReading {
        limit: header_number(headers, "x-ratelimit-limit")?,
        remaining: header_number(headers, "x-ratelimit-remaining")?,
        used: header_number(headers, "x-ratelimit-used")?,
        reset: header_number(headers, "x-ratelimit-reset")?,
        date: headers
            .get(reqwest::header::DATE)
            .and_then(|value| value.to_str().ok())
            .and_then(|text| DateTime::parse_from_rfc2822(text).ok())
            .map(|date| date.with_timezone(&Utc)),
        token_id: token_id.to_string(),
    })
}

fn header_number<T: std::str::FromStr>(
    headers: &reqwest::header::HeaderMap,
    name: &str,
) -> Option<T> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

/// A stable fingerprint of a token: which credential a budget reading (and,
/// from FZ-09c, a cache file) belongs to, without keeping the token. Specified
/// byte for byte in `docs/coverage-cache-design.md` 4.3 (K1): FNV-1a 64 over
/// `fleetz-account-v1:` and the trimmed token, as 16 lowercase hex digits.
///
/// Not `std`'s `DefaultHasher`, whose algorithm may change between Rust
/// releases, and not through `Hash`, whose `str` impl appends a terminator
/// byte: the value is pinned by test vectors and may end up in a file name.
/// Not cryptographic, and not meant to be: it identifies, it does not
/// protect. Never log the token, only this.
pub fn token_fingerprint(token: &str) -> String {
    const DOMAIN: &str = "fleetz-account-v1:";
    let bytes = DOMAIN.bytes().chain(token.trim().bytes());
    format!("{:016x}", fnv1a_64(bytes))
}

/// FNV-1a, 64 bits: offset basis `0xcbf29ce484222325`, prime
/// `0x100000001b3`, wrapping multiplication.
pub(crate) fn fnv1a_64(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
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
pub(crate) fn accept_response(
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
                disk_usage: node.disk_usage,
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
///
/// The header no longer calls this on the merged list, whose count and
/// oldest entry include org repos; `coverage_note` applies its rule
/// (`page_outrun`) to the viewer page of a payload without coverage fields.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "its rule lives on in `page_outrun`")
)]
pub fn window_exceeds_dataset(
    repos: &[GithubRepo],
    cutoff: DateTime<Utc>,
    repo_limit: u32,
) -> bool {
    page_outrun(
        repos.len(),
        repos.iter().map(|r| r.pushed_at).min(),
        cutoff,
        repo_limit,
    )
}

/// `window_exceeds_dataset`'s rule on a page's count and oldest push, so the
/// coverage note can apply it to a page it holds only the facts of.
fn page_outrun(
    returned: usize,
    oldest: Option<DateTime<Utc>>,
    cutoff: DateTime<Utc>,
    repo_limit: u32,
) -> bool {
    if (returned as u32) < repo_limit {
        return false;
    }
    oldest.is_some_and(|oldest| cutoff < oldest)
}

/// The header's coverage note for a view with `cutoff` (`None` for `all`),
/// or `None` when the view can be missing nothing GitHub has.
///
/// Shown when any of (`docs/coverage-cache-design.md` 4.1):
/// - the viewer page has a next page, and the cutoff is absent or older than
///   the page's oldest push;
/// - an org page has a next page, and the cutoff is absent or older than that
///   page's own oldest push;
/// - the org list has a next page, whatever the cutoff: nothing says how
///   recent the missing orgs' repos are.
///
/// A payload written before the coverage fields were asked for has the viewer
/// page's `hasNextPage` unknown; for it the note follows
/// `window_exceeds_dataset`'s rule on the viewer page alone, never the merged
/// list, whose oldest entry is an org repo's and whose count includes them.
/// Coverage is a counter, so the words are counts: "fetched 50 of 95 repos".
pub fn coverage_note(
    coverage: &Coverage,
    cutoff: Option<DateTime<Utc>>,
    repo_limit: u32,
) -> Option<String> {
    let reaches_past = |page: &PageCoverage| match (cutoff, page.boundary) {
        (Some(cutoff), Some(oldest)) => cutoff < oldest,
        _ => true,
    };
    let viewer = &coverage.viewer;
    let viewer_short = match viewer.has_next_page {
        Some(more) => more && reaches_past(viewer),
        None => cutoff.is_some_and(|cutoff| {
            page_outrun(
                viewer.returned as usize,
                viewer.boundary,
                cutoff,
                repo_limit,
            )
        }),
    };
    let mut parts = Vec::new();
    if viewer_short {
        parts.push(counted(viewer.returned, viewer.total, "repos"));
    }
    let short_orgs: Vec<&PageCoverage> = coverage
        .org_pages
        .iter()
        .filter(|page| page.has_next_page == Some(true) && reaches_past(page))
        .collect();
    if !short_orgs.is_empty() {
        let returned = short_orgs.iter().map(|page| page.returned).sum();
        let total = short_orgs.iter().map(|page| page.total).sum();
        parts.push(counted(returned, total, "org repos"));
    }
    if let Some(list) = &coverage.org_list
        && list.has_next_page == Some(true)
    {
        parts.push(counted(list.returned, list.total, "orgs"));
    }
    (!parts.is_empty()).then(|| format!("fetched {}", parts.join(", ")))
}

/// "12 of 15 org repos", or "12 of ? org repos" when the total is unknown.
fn counted(returned: u32, total: Option<u32>, what: &str) -> String {
    match total {
        Some(total) => format!("{returned} of {total} {what}"),
        None => format!("{returned} of ? {what}"),
    }
}

/// What `get_gh_token` asks `gh` (C9). `--hostname` names the host the
/// request goes to, so with `GH_HOST` set to another server fleetz still gets
/// its github.com token instead of failing as `Auth`; `GH_TOKEN` is still
/// honoured.
const GH_TOKEN_ARGS: [&str; 4] = ["auth", "token", "--hostname", HOST];

/// Get the GitHub auth token from the `gh` CLI. The child is killed if the
/// lookup is dropped, so one cut off by the launch bound leaves no process.
pub(crate) async fn get_gh_token() -> Result<String, String> {
    let output = tokio::process::Command::new("gh")
        .args(GH_TOKEN_ARGS)
        .kill_on_drop(true)
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
mod tests;
