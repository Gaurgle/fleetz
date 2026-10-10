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

impl GithubRepo {
    /// The key two dataset entries are the same remote by: lowercased owner
    /// and name, as `main.rs::remote_key` and discovery's `repo_key` build it.
    /// No owner is the legacy key of a payload without `login`, so such an
    /// entry is the same as another only by name, and never the same as an
    /// owned one. Logins and repo names are ASCII, so this agrees with the
    /// `eq_ignore_ascii_case` in `main.rs::names_remote` too.
    ///
    /// Takes the parts rather than `&self` so `merge_sources` can key the raw
    /// nodes before they are parsed into entries.
    pub fn identity_key(owner: Option<&str>, name: &str) -> (Option<String>, String) {
        (owner.map(str::to_lowercase), name.to_lowercase())
    }
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
    usable(parse_cached(cache.read(), today).map(|f| stamp_written(f, cache.written())))
        .or_else(|| usable(read_cached(cache, today)))
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
async fn fetch_keyed<S, F>(
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

const GRAPHQL_URL: &str = "https://api.github.com/graphql";

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
fn client() -> &'static reqwest::Client {
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
fn transport_failure(error: &reqwest::Error, context: &str) -> Failure {
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
fn classify_refusal(
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
fn read_budget(headers: &reqwest::header::HeaderMap, token_id: &str) -> Option<BudgetReading> {
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
async fn get_gh_token() -> Result<String, String> {
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

    fn days_ago(days: i64) -> DateTime<Utc> {
        Utc::now() - chrono::Duration::days(days)
    }

    /// `count` repo nodes of `owner`, pushed from one day ago back to exactly
    /// `oldest_days` ago, newest first.
    fn nodes_back_to(owner: &str, count: usize, oldest_days: i64) -> Vec<String> {
        (0..count)
            .map(|i| {
                let days = match count {
                    1 => oldest_days,
                    _ => 1 + (oldest_days - 1) * i as i64 / (count as i64 - 1),
                };
                let pushed = days_ago(days).format("%Y-%m-%dT%H:%M:%SZ").to_string();
                node_json(Some(owner), &format!("{owner}-{i}"), &pushed, 0)
            })
            .collect()
    }

    /// A connection as the payload spells it, with the coverage fields when
    /// `facts` gives them (`totalCount`, `hasNextPage`).
    fn page_json(nodes: &[String], facts: Option<(u32, bool)>) -> String {
        let facts = facts.map_or(String::new(), |(total, more)| {
            format!(r#","totalCount":{total},"pageInfo":{{"hasNextPage":{more}}}"#)
        });
        format!(r#"{{"nodes":[{}]{facts}}}"#, nodes.join(","))
    }

    /// A payload with the viewer page, the org list's own facts and its org
    /// pages, parsed the way a cached body is.
    fn covered_fetch(
        viewer: String,
        org_list: Option<(u32, bool)>,
        orgs: &[String],
    ) -> GithubFetch {
        let pages: Vec<String> = orgs
            .iter()
            .map(|page| format!(r#"{{"repositories":{page}}}"#))
            .collect();
        let org_facts = org_list.map_or(String::new(), |(total, more)| {
            format!(r#","totalCount":{total},"pageInfo":{{"hasNextPage":{more}}}"#)
        });
        let body = format!(
            r#"{{"data":{{"viewer":{{"repositories":{viewer},
                "organizations":{{"nodes":[{}]{org_facts}}}}}}}}}"#,
            pages.join(",")
        );
        parse_cached(Some(body), day("2026-10-09")).expect("payload parses")
    }

    /// The owner's measured org shape (design 1.2): 17 repos in four complete
    /// pages of 6, 4, 4 and 3, the oldest 2060 days back, five older than
    /// the viewer boundary of 237 days.
    fn owners_org_pages(facts: bool) -> Vec<String> {
        let page = |owner: &str, count: u32, oldest: i64| {
            let complete = facts.then_some((count, false));
            page_json(&nodes_back_to(owner, count as usize, oldest), complete)
        };
        vec![
            page("orga", 6, 2060),
            page("orgb", 4, 900),
            page("orgc", 4, 500),
            page("orgd", 3, 300),
        ]
    }

    fn note_of(fetch: &GithubFetch, cutoff: Option<DateTime<Utc>>) -> Option<String> {
        let coverage = fetch
            .meta
            .coverage
            .as_ref()
            .expect("a payload has coverage");
        coverage_note(coverage, cutoff, 50)
    }

    /// The owner's viewer page (design 1.2, R3): 50 of 95, a next page, the
    /// oldest push 237 days ago.
    fn owners_viewer_page() -> String {
        page_json(&nodes_back_to("me", 50, 237), Some((95, true)))
    }

    /// CV1: the viewer page is cut off and the window reaches past it.
    #[test]
    fn cv1_a_window_past_a_cut_off_viewer_page_shows_the_note() {
        let fetch = covered_fetch(owners_viewer_page(), None, &[]);
        let coverage = fetch.meta.coverage.as_ref().unwrap();
        assert_eq!(coverage.viewer.returned, 50);
        assert_eq!(coverage.viewer.total, Some(95));
        assert_eq!(coverage.viewer.has_next_page, Some(true));
        assert_eq!(
            note_of(&fetch, Some(days_ago(300))).as_deref(),
            Some("fetched 50 of 95 repos")
        );
        // Inside the page, nothing can be missing.
        assert_eq!(note_of(&fetch, Some(days_ago(200))), None);
    }

    /// CV2: org repos older than the viewer boundary used to hide the cut:
    /// the merged list's oldest entry is theirs.
    #[test]
    fn cv2_org_repos_older_than_the_boundary_do_not_hide_the_note() {
        let fetch = covered_fetch(
            owners_viewer_page(),
            Some((4, false)),
            &owners_org_pages(true),
        );
        let cutoff = days_ago(365);
        assert_eq!(fetch.repos.len(), 67, "50 viewer and 17 org repos");
        assert!(
            !window_exceeds_dataset(&fetch.repos, cutoff, 50),
            "the old rule on the merged list: no note"
        );
        assert_eq!(
            note_of(&fetch, Some(cutoff)).as_deref(),
            Some("fetched 50 of 95 repos")
        );
    }

    /// CV3: `all` has no cutoff, and the old rule never warned under it.
    #[test]
    fn cv3_no_cutoff_shows_the_note_for_a_cut_off_viewer_page() {
        let fetch = covered_fetch(owners_viewer_page(), None, &[]);
        assert_eq!(
            note_of(&fetch, None).as_deref(),
            Some("fetched 50 of 95 repos")
        );
    }

    /// CV4: a complete viewer page plus org repos reaches `repo_limit`
    /// merged, which the old rule read as a cut.
    #[test]
    fn cv4_a_complete_fetch_shows_no_note() {
        let viewer = page_json(&nodes_back_to("me", 40, 600), Some((40, false)));
        let fetch = covered_fetch(viewer, Some((4, false)), &owners_org_pages(true));
        let cutoff = days_ago(3000);
        assert_eq!(fetch.repos.len(), 57);
        assert!(
            window_exceeds_dataset(&fetch.repos, cutoff, 50),
            "the old rule on the merged list: a false note"
        );
        assert_eq!(note_of(&fetch, Some(cutoff)), None);
    }

    /// CV5: an org page cut off at its own boundary, the viewer complete.
    #[test]
    fn cv5_a_cut_off_org_page_shows_the_note_past_its_own_boundary() {
        let viewer = page_json(&nodes_back_to("me", 30, 400), Some((30, false)));
        let org = page_json(&nodes_back_to("orga", 12, 90), Some((15, true)));
        let fetch = covered_fetch(viewer, Some((1, false)), &[org]);
        assert_eq!(note_of(&fetch, Some(days_ago(30))), None);
        assert_eq!(
            note_of(&fetch, Some(days_ago(120))).as_deref(),
            Some("fetched 12 of 15 org repos")
        );
        assert_eq!(
            note_of(&fetch, None).as_deref(),
            Some("fetched 12 of 15 org repos")
        );
    }

    /// CV6: a cut-off org list shows whatever the cutoff: nothing says how
    /// recent the missing orgs' repos are.
    #[test]
    fn cv6_a_cut_off_org_list_shows_the_note_whatever_the_cutoff() {
        let viewer = page_json(&nodes_back_to("me", 30, 400), Some((30, false)));
        let orgs: Vec<String> = (0..8)
            .map(|i| page_json(&nodes_back_to(&format!("org{i}"), 1, 20), Some((1, false))))
            .collect();
        let fetch = covered_fetch(viewer, Some((10, true)), &orgs);
        assert_eq!(
            note_of(&fetch, Some(days_ago(1))).as_deref(),
            Some("fetched 8 of 10 orgs")
        );
        assert_eq!(
            note_of(&fetch, None).as_deref(),
            Some("fetched 8 of 10 orgs")
        );
    }

    /// CV7: a payload cached before the coverage fields. Its coverage stays
    /// unknown, and the note follows `window_exceeds_dataset`'s rule on the
    /// viewer page alone, not the merged list reaching 2060 days back.
    #[test]
    fn cv7_a_payload_without_coverage_fields_follows_the_old_rule_on_the_viewer_page() {
        let viewer = page_json(&nodes_back_to("me", 50, 237), None);
        let fetch = covered_fetch(viewer, None, &owners_org_pages(false));
        let coverage = fetch.meta.coverage.as_ref().unwrap();
        assert_eq!(coverage.viewer.total, None);
        assert_eq!(coverage.viewer.has_next_page, None);
        assert_eq!(coverage.org_list.as_ref().unwrap().has_next_page, None);
        assert!(
            coverage
                .org_pages
                .iter()
                .all(|page| page.total.is_none() && page.has_next_page.is_none())
        );
        assert_eq!(
            note_of(&fetch, Some(days_ago(365))).as_deref(),
            Some("fetched 50 of ? repos")
        );
        assert_eq!(note_of(&fetch, None), None, "the old rule needs a cutoff");
    }

    /// The names directly under `viewer`, aliases and the field each names
    /// alike: what a GraphQL server would read as viewer-level fields.
    fn viewer_level_names(query: &str) -> Vec<String> {
        let (mut braces, mut parens) = (0, 0);
        let mut word = String::new();
        let mut names = Vec::new();
        for c in query.chars() {
            if c.is_alphanumeric() || c == '_' {
                word.push(c);
                continue;
            }
            let word = std::mem::take(&mut word);
            if !word.is_empty() && braces == 2 && parens == 0 {
                names.push(word);
            }
            match c {
                '{' => braces += 1,
                '}' => braces -= 1,
                '(' => parens += 1,
                ')' => parens -= 1,
                _ => {}
            }
        }
        names
    }

    /// CV8: the coverage fields ride the three connections already asked
    /// for, and nothing new lands at the viewer level, where it would fall
    /// into `Viewer`'s flattened calendar map.
    #[test]
    fn cv8_the_query_asks_coverage_on_the_three_connections_only() {
        let q = build_query(50, day("2026-10-09"), 0);
        assert_eq!(
            q.matches("totalCount\n    pageInfo { hasNextPage }")
                .count(),
            3
        );
        assert_eq!(q.matches("pageInfo").count(), 3);
        for connection in [
            "affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]) {\n    totalCount",
            &format!("organizations(first: {MAX_ORGS}) {{\n    totalCount"),
            &format!(
                "repositories(first: {ORG_REPO_LIMIT}, orderBy: {{field: PUSHED_AT, direction: DESC}}, isArchived: false) {{\n        totalCount"
            ),
        ] {
            assert!(q.contains(connection), "no coverage on: {connection}");
        }
        let names = viewer_level_names(&q);
        assert!(names.contains(&"repositories".to_string()), "{names:?}");
        assert!(names.contains(&"organizations".to_string()), "{names:?}");
        for name in names {
            let calendar = name == "recent"
                || name == "contributionsCollection"
                || name
                    .strip_prefix('y')
                    .is_some_and(|n| n.parse::<u32>().is_ok());
            assert!(
                calendar || name == "repositories" || name == "organizations",
                "a new viewer-level field: {name}"
            );
        }
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

    /// #69: an archived repo is not live. Archiving counts as a push, so with
    /// the filter missing on either connection an archived repo sorted near
    /// the top and took a slot a live repo needed.
    #[test]
    fn both_repo_connections_skip_archived_repos() {
        let q = build_query(50, day("2026-10-10"), 0);
        for connection in [
            "repositories(first: 50, orderBy: {field: PUSHED_AT, direction: DESC},\n    isArchived: false, affiliations:",
            &format!(
                "repositories(first: {ORG_REPO_LIMIT}, orderBy: {{field: PUSHED_AT, direction: DESC}}, isArchived: false)"
            ),
        ] {
            assert!(
                q.contains(connection),
                "archived repos not filtered on: {connection}"
            );
        }
        assert_eq!(q.matches("isArchived: false").count(), 2);
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

    /// One repo node as the payload spells it. `owner` None leaves the key
    /// out, as a payload written before `login` was asked for does.
    fn node_json(owner: Option<&str>, name: &str, pushed: &str, prs: u32) -> String {
        let owner = owner.map_or(String::new(), |login| {
            format!(r#""owner":{{"__typename":"User","login":"{login}"}},"#)
        });
        format!(
            r#"{{"name":"{name}",{owner}"pushedAt":"{pushed}","pullRequests":{{"totalCount":{prs}}}}}"#
        )
    }

    /// A payload with the viewer's repos and one org page.
    fn two_source_payload(viewer: &[String], org: &[String]) -> String {
        format!(
            r#"{{"data":{{"viewer":{{
                "repositories":{{"nodes":[{}]}},
                "organizations":{{"nodes":[{{"repositories":{{"nodes":[{}]}}}}]}}
            }}}}}}"#,
            viewer.join(","),
            org.join(",")
        )
    }

    /// The full dataset a payload parses to, as `(owner, name, pr_count)`.
    fn dataset_of(payload: &str) -> Vec<(Option<String>, String, u32)> {
        parse_cached(Some(payload.to_string()), day("2026-08-18"))
            .unwrap()
            .repos
            .into_iter()
            .map(|r| (r.owner, r.name, r.pr_count))
            .collect()
    }

    fn entry(owner: Option<&str>, name: &str, prs: u32) -> (Option<String>, String, u32) {
        (owner.map(str::to_string), name.to_string(), prs)
    }

    /// S1 and S3. `alice/api` comes from the viewer, `company/api` from an org
    /// page. Deduped by bare name, the older push was dropped from the
    /// dataset: company's clone had no row and company's CI was nowhere.
    #[test]
    fn same_named_repos_of_two_owners_both_stay_in_the_dataset() {
        let payload = two_source_payload(
            &[node_json(Some("alice"), "api", "2026-08-06T10:00:00Z", 1)],
            &[node_json(Some("company"), "api", "2026-08-06T09:00:00Z", 2)],
        );
        assert_eq!(
            dataset_of(&payload),
            vec![
                entry(Some("alice"), "api", 1),
                entry(Some("company"), "api", 2)
            ],
            "newest push first, and neither owner's repo dropped"
        );
    }

    /// S3b. The window shows only `alice/api`, but the full dataset must
    /// still hold `company/api`, pushed 60 days ago: ambiguity is judged on
    /// the full dataset (M1), and with the name dedupe it saw one owner.
    #[test]
    fn the_full_dataset_keeps_a_same_named_repo_the_window_leaves_out() {
        let payload = two_source_payload(
            &[node_json(Some("alice"), "api", "2026-08-18T10:00:00Z", 0)],
            &[node_json(Some("company"), "api", "2026-06-19T10:00:00Z", 0)],
        );
        let full = parse_cached(Some(payload), day("2026-08-18"))
            .unwrap()
            .repos;
        let owners: Vec<_> = full.iter().map(|r| r.owner.as_deref()).collect();
        assert_eq!(owners, vec![Some("alice"), Some("company")]);

        let cutoff = DateTime::parse_from_rfc3339("2026-08-17T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let window: Vec<_> = filter_since(&full, cutoff)
            .into_iter()
            .map(|r| r.owner)
            .collect();
        assert_eq!(window, vec![Some("alice".to_string())]);
    }

    /// S12. A fork `Gaurgle/tool` and its upstream `acme/tool`, pushed later.
    /// By name the fork was dropped, so a clone whose origin is the fork had
    /// nothing to match and was shown as acme's.
    #[test]
    fn a_fork_and_its_upstream_both_survive() {
        let payload = two_source_payload(
            &[node_json(
                Some("Gaurgle"),
                "tool",
                "2026-08-06T08:00:00Z",
                0,
            )],
            &[node_json(Some("acme"), "tool", "2026-08-06T10:00:00Z", 3)],
        );
        assert_eq!(
            dataset_of(&payload),
            vec![
                entry(Some("acme"), "tool", 3),
                entry(Some("Gaurgle"), "tool", 0)
            ]
        );
    }

    /// S16. Two same-named repos, both reached through the viewer, stay two
    /// entries and each keeps its own GitHub fields, so each NotCloned row
    /// the load builds from them gets its own CI.
    #[test]
    fn two_same_named_repos_from_one_source_stay_two_entries() {
        let payload = two_source_payload(
            &[
                node_json(Some("alice"), "api", "2026-08-06T10:00:00Z", 4),
                node_json(Some("company"), "api", "2026-08-06T09:00:00Z", 5),
            ],
            &[],
        );
        assert_eq!(
            dataset_of(&payload),
            vec![
                entry(Some("alice"), "api", 4),
                entry(Some("company"), "api", 5)
            ]
        );
    }

    /// A repo the viewer lists and an org page lists again is one repo. The
    /// owner and name compare case-insensitively, and on an equal push the
    /// viewer's copy is the one kept, as it was under the name dedupe.
    #[test]
    fn a_repo_in_both_the_viewer_and_an_org_page_stays_one_entry() {
        let payload = two_source_payload(
            &[node_json(Some("acme"), "tool", "2026-08-06T10:00:00Z", 1)],
            &[node_json(Some("Acme"), "Tool", "2026-08-06T10:00:00Z", 9)],
        );
        assert_eq!(dataset_of(&payload), vec![entry(Some("acme"), "tool", 1)]);
    }

    /// A payload without `login` (written before it was asked for) has no
    /// owner anywhere. Those entries dedupe by name, case-insensitively, the
    /// newest push winning.
    #[test]
    fn entries_without_an_owner_fall_back_to_the_name() {
        let payload = two_source_payload(
            &[node_json(None, "api", "2026-08-06T09:00:00Z", 1)],
            &[
                node_json(None, "API", "2026-08-06T10:00:00Z", 2),
                node_json(None, "web", "2026-08-06T08:00:00Z", 3),
            ],
        );
        assert_eq!(
            dataset_of(&payload),
            vec![entry(None, "API", 2), entry(None, "web", 3)]
        );
    }

    /// An entry with an owner and one without are different keys, as they are
    /// in `main.rs::remote_key` and discovery's `repo_key`: the owner-less one
    /// dedupes by name only against other owner-less entries. The query asks
    /// for `owner { login }` on both connections and `login` is non-null, so
    /// one payload never mixes the two; this pins what happens if one does,
    /// rather than letting an owner-less entry delete an owned repo.
    #[test]
    fn an_owned_entry_and_an_owner_less_one_are_kept_apart() {
        let payload = two_source_payload(
            &[node_json(Some("alice"), "api", "2026-08-06T10:00:00Z", 1)],
            &[node_json(None, "api", "2026-08-06T09:00:00Z", 2)],
        );
        assert_eq!(
            dataset_of(&payload),
            vec![entry(Some("alice"), "api", 1), entry(None, "api", 2)]
        );
    }

    // --- FZ-10a: the API budget and failure classification. Scenario numbers
    // are `docs/api-budget-design.md` section 5's.

    /// A header map from name and value pairs, as a response would carry.
    fn headers(pairs: &[(&str, &str)]) -> reqwest::header::HeaderMap {
        let mut map = reqwest::header::HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                reqwest::header::HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn status(code: u16) -> reqwest::StatusCode {
        reqwest::StatusCode::from_u16(code).unwrap()
    }

    /// 2026-10-01 17:23:06Z, M1's reset, in the header's epoch seconds.
    const M1_RESET: &str = "1790875386";

    /// M1's headers: the four numbers and the server's `Date`.
    fn m1_headers() -> Vec<(&'static str, &'static str)> {
        vec![
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "3564"),
            ("x-ratelimit-used", "1436"),
            ("x-ratelimit-reset", M1_RESET),
            ("date", "Thu, 01 Oct 2026 17:00:43 GMT"),
        ]
    }

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    /// S1. A 200's headers are a reading: the numbers as stated, the reset raw
    /// on the server's clock, its `Date`, and whose token it was.
    #[test]
    fn s1_a_success_carries_the_budget_its_headers_state() {
        assert_eq!(
            M1_RESET.parse::<i64>().unwrap(),
            utc("2026-10-01T17:23:06Z").timestamp(),
            "fixture"
        );
        let reading = read_budget(&headers(&m1_headers()), "abc").expect("a reading");
        assert_eq!(
            reading,
            BudgetReading {
                limit: 5000,
                remaining: 3564,
                used: 1436,
                reset: utc("2026-10-01T17:23:06Z").timestamp(),
                date: Some(utc("2026-10-01T17:00:43Z")),
                token_id: "abc".to_string(),
            }
        );
    }

    /// Half a window is nothing to pace on: any of the four numbers missing or
    /// unreadable is no reading. A missing or unreadable `Date` is only a
    /// reading without one.
    #[test]
    fn a_reading_needs_all_four_numbers_but_not_the_date() {
        let without = |name: &str| {
            let pairs: Vec<_> = m1_headers()
                .into_iter()
                .filter(|(n, _)| *n != name)
                .collect();
            read_budget(&headers(&pairs), "t")
        };
        for name in [
            "x-ratelimit-limit",
            "x-ratelimit-remaining",
            "x-ratelimit-used",
            "x-ratelimit-reset",
        ] {
            assert_eq!(without(name), None, "a reading without {name}");
        }
        let garbled: Vec<_> = m1_headers()
            .into_iter()
            .map(|(n, v)| match n {
                "x-ratelimit-remaining" => (n, "lots"),
                _ => (n, v),
            })
            .collect();
        assert_eq!(read_budget(&headers(&garbled), "t"), None);

        let undated = without("date").expect("the date is optional");
        assert_eq!(undated.date, None);
        assert_eq!(undated.remaining, 3564);
        let mut bad_date = m1_headers();
        bad_date.retain(|(n, _)| *n != "date");
        bad_date.push(("date", "yesterday"));
        assert_eq!(read_budget(&headers(&bad_date), "t").unwrap().date, None);
    }

    /// S2. A gateway's 502: an nginx page and no rate headers. No reading, a
    /// `Server` failure whose message keeps the status and leaves the HTML out.
    #[test]
    fn s2_a_gateway_502_is_a_server_failure_with_no_reading() {
        let page = "<html><head><title>502 Bad Gateway</title></head><body>nginx</body></html>";
        let failure = classify_refusal(status(502), &headers(&[]), page);
        assert_eq!(failure.kind, FailureKind::Server);
        assert!(failure.message.contains("502"), "{}", failure.message);
        assert!(!failure.message.contains('<'), "{}", failure.message);
        assert!(!failure.rate_limited);
        assert_eq!(failure.limit, None);
        assert_eq!(read_budget(&headers(&[]), "t"), None);
    }

    /// S2a. Every 5xx is `Server`, not `Http`; the 4xx below the range is not.
    #[test]
    fn s2a_a_504_and_the_rest_of_5xx_are_server_failures() {
        for code in [500, 502, 504, 599] {
            let failure = classify_refusal(status(code), &headers(&[]), "");
            assert_eq!(failure.kind, FailureKind::Server, "{code}");
            assert!(!failure.rate_limited, "{code}");
        }
        assert_eq!(
            classify_refusal(status(499), &headers(&[]), "").kind,
            FailureKind::Http
        );
    }

    /// S4a. The measured step function (M2, M1, M6), and a limit between two
    /// measured points takes the higher price, so pacing errs slow.
    #[test]
    fn s4a_poll_cost_steps_at_the_measured_limits() {
        assert_eq!(poll_cost(20), 2, "M2");
        assert_eq!(poll_cost(50), 3, "M1");
        assert_eq!(poll_cost(100), 4, "M6");
        assert_eq!(poll_cost(30), 3);
        assert_eq!(poll_cost(1), 2);
        assert_eq!(poll_cost(21), 3);
        assert_eq!(poll_cost(51), 4);
    }

    /// S6, classification. A `RATE_LIMITED` body is an `Api` failure and the
    /// primary limit: the hourly budget, lifting at the reset.
    #[test]
    fn s6_a_rate_limited_body_is_the_primary_limit() {
        let dir = std::env::temp_dir().join(format!("fleetz_s6_{}", std::process::id()));
        let cache = crate::cache::ApiCache::at(dir.join("github-50.json"), 86_400);
        let body = r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#;
        let failure = accept_response(body, &cache, day("2026-08-18"))
            .err()
            .unwrap();
        assert_eq!(failure.kind, FailureKind::Api);
        assert!(failure.rate_limited);
        assert_eq!(failure.limit, Some(RateLimit::Primary));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S6b. A primary limit with no reset header is still the primary limit;
    /// it just carries no reading, so nothing claims to know when it lifts.
    #[test]
    fn s6b_a_primary_limit_without_a_reset_is_primary_with_no_reading() {
        let stated = headers(&[
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-used", "5000"),
        ]);
        let failure = classify_refusal(status(403), &stated, "");
        assert_eq!(failure.kind, FailureKind::Http);
        assert_eq!(failure.limit, Some(RateLimit::Primary));
        assert!(failure.rate_limited);
        assert_eq!(read_budget(&stated, "t"), None);
    }

    /// S7. A 403 with nothing remaining is the primary limit, read as rate
    /// limited. It used to be `http error`.
    #[test]
    fn s7_a_403_with_nothing_remaining_is_the_primary_limit() {
        let body = r#"{"message":"API rate limit exceeded for user ID 1."}"#;
        let failure = classify_refusal(
            status(403),
            &headers(&[("x-ratelimit-remaining", "0")]),
            body,
        );
        assert_eq!(failure.kind, FailureKind::Http);
        assert!(failure.rate_limited);
        assert_eq!(failure.limit, Some(RateLimit::Primary));
        assert_eq!(crate::ui::github_failure_label(&failure), "rate limited");

        // A 429 says the same with the same header.
        let failure =
            classify_refusal(status(429), &headers(&[("x-ratelimit-remaining", "0")]), "");
        assert_eq!(failure.limit, Some(RateLimit::Primary));
    }

    /// S8. A `retry-after` names a secondary limit and its delay, and wins over
    /// `x-ratelimit-remaining: 0` beside it: the table's first matching row.
    #[test]
    fn s8_a_retry_after_is_a_secondary_limit_with_its_delay() {
        let delay = Some(std::time::Duration::from_secs(120));
        for code in [403, 429] {
            let failure = classify_refusal(status(code), &headers(&[("retry-after", "120")]), "");
            assert_eq!(failure.kind, FailureKind::Http, "{code}");
            assert!(failure.rate_limited, "{code}");
            assert_eq!(
                failure.limit,
                Some(RateLimit::Secondary { retry_after: delay }),
                "{code}"
            );
        }
        let both = headers(&[("retry-after", "120"), ("x-ratelimit-remaining", "0")]);
        assert_eq!(
            classify_refusal(status(403), &both, "").limit,
            Some(RateLimit::Secondary { retry_after: delay })
        );
        // A `retry-after` that is not a number of seconds still names the
        // limit, without a delay to trust.
        let odd = headers(&[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")]);
        assert_eq!(
            classify_refusal(status(403), &odd, "").limit,
            Some(RateLimit::Secondary { retry_after: None })
        );
    }

    /// S9. A 429 with no `retry-after` is still a secondary limit, with no
    /// delay; so is a 403 whose body says so.
    #[test]
    fn s9_a_429_or_a_403_saying_so_is_a_secondary_limit_without_a_delay() {
        let secondary = Some(RateLimit::Secondary { retry_after: None });
        let failure = classify_refusal(status(429), &headers(&[]), "");
        assert_eq!(failure.kind, FailureKind::Http);
        assert!(failure.rate_limited);
        assert_eq!(failure.limit, secondary);

        let body = r#"{"message":"You have exceeded a Secondary Rate Limit. Please wait."}"#;
        let failure = classify_refusal(status(403), &headers(&[]), body);
        assert_eq!(failure.kind, FailureKind::Http);
        assert_eq!(failure.limit, secondary);
        assert!(failure.rate_limited);
    }

    /// S9a. A 403 with no limit header and no secondary wording is a refusal
    /// of the credentials or their scope, not a rate limit; so is one that
    /// still has budget left.
    #[test]
    fn s9a_a_plain_403_is_refused_not_rate_limited() {
        let body = r#"{"message":"Resource protected by organization SAML enforcement."}"#;
        for stated in [headers(&[]), headers(&[("x-ratelimit-remaining", "4000")])] {
            let failure = classify_refusal(status(403), &stated, body);
            assert_eq!(failure.kind, FailureKind::Refused);
            assert!(!failure.rate_limited);
            assert_eq!(failure.limit, None);
        }
    }

    /// S14, classification. A 401 is refused, never a rate limit, whatever its
    /// headers say: only a 403 or 429 can be one.
    #[test]
    fn s14_a_401_is_refused() {
        for stated in [
            headers(&[]),
            headers(&[("x-ratelimit-remaining", "0"), ("retry-after", "60")]),
        ] {
            let failure =
                classify_refusal(status(401), &stated, r#"{"message":"Bad credentials"}"#);
            assert_eq!(failure.kind, FailureKind::Refused);
            assert!(!failure.rate_limited);
            assert_eq!(failure.limit, None);
        }
    }

    /// Any other 4xx stays `Http`, even with limit-looking headers.
    #[test]
    fn other_4xx_answers_stay_http() {
        for code in [400, 404, 422] {
            let failure = classify_refusal(
                status(code),
                &headers(&[("x-ratelimit-remaining", "0"), ("retry-after", "5")]),
                "secondary rate limit",
            );
            assert_eq!(failure.kind, FailureKind::Http, "{code}");
            assert!(!failure.rate_limited, "{code}");
        }
    }

    /// The message keeps the status and what GitHub said, on one line and
    /// trimmed: the JSON `message` rather than the whole body, plain text as
    /// it is, an HTML page not at all.
    #[test]
    fn a_refusal_message_keeps_the_status_and_a_trimmed_reason() {
        let json = r#"{"message":"Bad credentials","documentation_url":"https://docs"}"#;
        assert_eq!(
            refusal_message(status(401), json),
            "GitHub API returned 401 Unauthorized: Bad credentials"
        );
        assert_eq!(
            refusal_message(status(404), "  not\n  here  "),
            "GitHub API returned 404 Not Found: not here"
        );
        assert_eq!(
            refusal_message(status(502), "  <html>gateway</html>"),
            "GitHub API returned 502 Bad Gateway"
        );
        assert_eq!(
            refusal_message(status(500), "   "),
            "GitHub API returned 500 Internal Server Error"
        );
        let long = "x".repeat(REFUSAL_DETAIL_CHARS + 50);
        let message = refusal_message(status(500), &long);
        assert!(message.ends_with(&"x".repeat(REFUSAL_DETAIL_CHARS)));
        assert!(!message.contains(&"x".repeat(REFUSAL_DETAIL_CHARS + 1)));
    }

    /// The header's words: any rate limit reads `rate limited`, an `Http` one
    /// included; the two new kinds are not `http error`.
    #[test]
    fn every_rate_limit_is_labelled_rate_limited() {
        use crate::ui::github_failure_label as label;
        let limited = |kind| Failure::limited(kind, RateLimit::Primary, "m");
        assert_eq!(label(&limited(FailureKind::Http)), "rate limited");
        assert_eq!(label(&limited(FailureKind::Api)), "rate limited");
        assert_eq!(label(&Failure::new(FailureKind::Http, "m")), "http error");
        assert_eq!(label(&Failure::new(FailureKind::Api, "m")), "api error");
        for kind in [FailureKind::Server, FailureKind::Refused] {
            let words = label(&Failure::new(kind, "m"));
            assert!(
                !matches!(words, "http error" | "rate limited"),
                "{kind:?}: {words}"
            );
        }
    }

    /// S19a and A5. reqwest marks a connect timeout as both a connect and a
    /// timeout error; the connect test goes first, so it is `Offline`. Only a
    /// timeout after connecting is `TimedOut`.
    #[test]
    fn s19a_a_connect_timeout_is_offline_and_a_later_timeout_is_timed_out() {
        assert_eq!(transport_kind(true, true), FailureKind::Offline);
        assert_eq!(transport_kind(true, false), FailureKind::Offline);
        assert_eq!(transport_kind(false, true), FailureKind::TimedOut);
        assert_eq!(transport_kind(false, false), FailureKind::Offline);
    }

    /// S19 and amendment 4. The client gives up after 30 s in total and 10 s
    /// to connect, well before the 60 s give-up, which stays the backstop.
    #[test]
    fn s19_the_client_deadline_comes_before_the_give_up() {
        assert_eq!(CONNECT_TIMEOUT, std::time::Duration::from_secs(10));
        assert_eq!(TOTAL_TIMEOUT, std::time::Duration::from_secs(30));
        assert!(TOTAL_TIMEOUT < crate::GITHUB_POLL_STUCK_AFTER);
        assert!(std::ptr::eq(client(), client()), "one client, reused");
    }

    /// A local listener that accepts one connection, reads the request, writes
    /// `response` and then holds the socket open without a word more. No
    /// network: everything stays on the loopback.
    async fn serve_once(response: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 64 * 1024];
            let _ = socket.read(&mut request).await;
            let _ = socket.write_all(response.as_bytes()).await;
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            drop(socket);
        });
        format!("http://{addr}/graphql")
    }

    /// `exchange` against `url` with short deadlines and a cache in a scratch
    /// directory.
    async fn exchange_with(
        url: &str,
        tag: &str,
    ) -> (Result<GithubFetch, Failure>, Option<BudgetReading>) {
        let dir = std::env::temp_dir().join(format!("fleetz_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = crate::cache::ApiCache::at(dir.join("github-50.json"), 86_400);
        let client = build_client(
            std::time::Duration::from_secs(2),
            std::time::Duration::from_millis(300),
        );
        let result = exchange(&client, url, "tok", "{}", &cache, day("2026-08-18")).await;
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    /// S19. Connected, then silence: the total deadline ends it as `TimedOut`.
    #[tokio::test]
    async fn s19_silence_after_connecting_is_timed_out() {
        let url = serve_once("").await;
        let (result, budget) = exchange_with(&url, "s19_silent").await;
        let failure = result.err().expect("no answer, yet Ok");
        assert_eq!(failure.kind, FailureKind::TimedOut, "{}", failure.message);
        assert_eq!(budget, None);
    }

    /// S19, the body read. The status and headers arrive, the body stalls: the
    /// same deadline, the same `TimedOut`, and the headers' reading is kept.
    #[tokio::test]
    async fn s19_a_body_that_stalls_is_timed_out_and_keeps_the_reading() {
        let url = serve_once(concat!(
            "HTTP/1.1 200 OK\r\n",
            "content-length: 4096\r\n",
            "x-ratelimit-limit: 5000\r\n",
            "x-ratelimit-remaining: 3564\r\n",
            "x-ratelimit-used: 1436\r\n",
            "x-ratelimit-reset: 1790875386\r\n",
            "\r\n",
            "{\"data\":",
        ))
        .await;
        let (result, budget) = exchange_with(&url, "s19_body").await;
        let failure = result.err().expect("a cut-off body, yet Ok");
        assert_eq!(failure.kind, FailureKind::TimedOut, "{}", failure.message);
        assert_eq!(budget.expect("the headers arrived").remaining, 3564);
    }

    /// A5. Nothing listening: the connection cannot be opened, so `Offline`.
    #[tokio::test]
    async fn a_refused_connection_is_offline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let (result, budget) = exchange_with(&format!("http://{addr}/graphql"), "refused").await;
        let failure = result.err().expect("nothing listening, yet Ok");
        assert_eq!(failure.kind, FailureKind::Offline, "{}", failure.message);
        assert_eq!(budget, None);
    }

    /// A refused answer states the budget as truly as an accepted one, so the
    /// reading travels with the failure: a 403 limit and a 200 `RATE_LIMITED`
    /// body both carry it, keyed by the token's fingerprint.
    #[tokio::test]
    async fn a_refused_answer_still_carries_its_reading() {
        let url = serve_once(concat!(
            "HTTP/1.1 403 Forbidden\r\n",
            "content-length: 2\r\n",
            "x-ratelimit-limit: 5000\r\n",
            "x-ratelimit-remaining: 0\r\n",
            "x-ratelimit-used: 5000\r\n",
            "x-ratelimit-reset: 1790875386\r\n",
            "\r\n",
            "{}",
        ))
        .await;
        let (result, budget) = exchange_with(&url, "refused_403").await;
        let failure = result.err().unwrap();
        assert_eq!(failure.limit, Some(RateLimit::Primary));
        let budget = budget.expect("a reading");
        assert_eq!((budget.remaining, budget.used), (0, 5000));
        assert_eq!(budget.token_id, token_fingerprint("tok"));

        let url = serve_once(concat!(
            "HTTP/1.1 200 OK\r\n",
            "content-length: 72\r\n",
            "x-ratelimit-limit: 5000\r\n",
            "x-ratelimit-remaining: 0\r\n",
            "x-ratelimit-used: 5000\r\n",
            "x-ratelimit-reset: 1790875386\r\n",
            "\r\n",
            r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#,
        ))
        .await;
        let (result, budget) = exchange_with(&url, "refused_body").await;
        let failure = result.err().unwrap();
        assert_eq!(failure.kind, FailureKind::Api);
        assert_eq!(failure.limit, Some(RateLimit::Primary));
        assert_eq!(budget.expect("a reading").remaining, 0);
    }

    /// The reading leaves `fetch_repos` on whichever side the attempt landed.
    #[test]
    fn the_reading_travels_with_a_success_and_with_a_failure() {
        let budget = read_budget(&headers(&m1_headers()), "t");
        assert!(budget.is_some(), "fixture");
        let dir = std::env::temp_dir().join(format!("fleetz_meta_{}", std::process::id()));
        let cache = crate::cache::ApiCache::at(dir.join("github-50.json"), 86_400);
        let ok = accept_response(SERVABLE, &cache, day("2026-08-18"))
            .ok()
            .unwrap();
        let fetch = with_budget(Ok(ok), budget.clone()).ok().unwrap();
        assert_eq!(fetch.meta.budget, budget);

        let failure = Failure::new(FailureKind::Server, "502");
        let settled = settle_attempt(Err(failure), &cache, day("2026-08-18"));
        let error = with_budget(settled, budget.clone()).err().unwrap();
        assert_eq!(error.meta.budget, budget);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A network answer's coverage is the payload's, read in `split_viewer`.
    /// The attempt's meta carries none, and attaching it must not drop the
    /// payload's record while it adds the account.
    #[test]
    fn a_network_answer_keeps_its_payload_coverage_beside_the_attempts_meta() {
        let dir = std::env::temp_dir().join(format!("fleetz_cov_meta_{}", std::process::id()));
        let cache = crate::cache::ApiCache::at(dir.join("github-50.json"), 86_400);
        let body = format!(
            r#"{{"data":{{"viewer":{{"repositories":{}}}}}}}"#,
            owners_viewer_page()
        );
        let answer =
            accept_response(&body, &cache, day("2026-10-09")).expect("the payload is accepted");
        let payload = answer
            .meta
            .coverage
            .clone()
            .expect("the payload has coverage");
        assert_eq!(payload.viewer.total, Some(95), "fixture");

        let attempt = FetchMeta {
            account: Some("acct".to_string()),
            ..FetchMeta::default()
        };
        let fetch = with_meta(Ok(answer), attempt).ok().unwrap();
        assert_eq!(fetch.meta.coverage, Some(payload));
        assert_eq!(fetch.meta.account.as_deref(), Some("acct"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refusal is decided by its status and headers. A body that stalls
    /// after them does not turn GitHub's 403 limit into a timeout.
    #[tokio::test]
    async fn a_refusal_whose_body_stalls_is_still_classified_by_its_status() {
        let url = serve_once(concat!(
            "HTTP/1.1 403 Forbidden\r\n",
            "content-length: 4096\r\n",
            "x-ratelimit-remaining: 0\r\n",
            "\r\n",
            "{\"mess",
        ))
        .await;
        let (result, _) = exchange_with(&url, "refusal_stall").await;
        let failure = result.err().unwrap();
        assert_eq!(failure.kind, FailureKind::Http, "{}", failure.message);
        assert_eq!(failure.limit, Some(RateLimit::Primary));
    }

    /// A cache-served load asked GitHub nothing, so it carries no reading.
    #[tokio::test]
    async fn a_cache_served_load_carries_no_reading() {
        let (path, _) = planted_cache("meta_cached", std::time::Duration::from_secs(60));
        let cache = crate::cache::ApiCache::at(path.clone(), 86_400);
        let fetch = fetch_repos(50, &cache, CacheMode::AnyCached).await.unwrap();
        assert_eq!(fetch.meta.budget, None);
        assert_eq!(fetch.meta.account, None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// K1, the vectors of `docs/coverage-cache-design.md` (CK6), pinned so the
    /// fingerprint cannot drift between releases. Surrounding whitespace is
    /// not part of the token.
    #[test]
    fn the_token_fingerprint_is_pinned() {
        let token = "gho_EXAMPLEexampleEXAMPLEexample0000";
        assert_eq!(token_fingerprint(token), "2a8d4ab865f3ca20");
        assert_eq!(
            token_fingerprint(&format!("  {token}\n")),
            "2a8d4ab865f3ca20"
        );
        assert_eq!(token_fingerprint(""), "5fccb9a3d0a45785");
    }

    /// The published FNV-1a 64 values, for the hash under the fingerprint.
    #[test]
    fn fnv1a_64_matches_the_published_vectors() {
        assert_eq!(fnv1a_64(*b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(*b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    // --- FZ-09c: one lookup per attempt, the keyed file, migration ---
    //
    // Every test injects its token source and points the request at the
    // loopback: no `gh`, no network, no real cache directory.

    /// Dummy tokens. A is the design's example token, B the same shape.
    const TOKEN_A: &str = "gho_EXAMPLEexampleEXAMPLEexample0000";
    const TOKEN_B: &str = "gho_EXAMPLEexampleEXAMPLEexample1111";
    const HOUR: std::time::Duration = std::time::Duration::from_secs(3600);
    const DAY: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
    /// Valid `data` with no calendar: it parses, so a fallback read returns
    /// it, but launch's `usable` refuses to paint it.
    const NO_CALENDAR: &str = r#"{"data":{"viewer":{"repositories":{"nodes":[]}}}}"#;

    /// A fresh scratch directory, removed when dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("fleetz_09c_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn cache(&self) -> crate::cache::KeyedCache {
            crate::cache::KeyedCache::at(self.0.clone(), 86_400, 50, HOST)
        }

        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }

        fn own_path(&self, token: &str) -> std::path::PathBuf {
            self.cache()
                .for_account(&token_fingerprint(token))
                .path()
                .to_path_buf()
        }

        fn legacy_path(&self) -> std::path::PathBuf {
            self.0.join("github-50.json")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Writes `body` at `path`, dated `age` ago, and returns that time.
    fn plant(
        path: &std::path::Path,
        body: &str,
        age: std::time::Duration,
    ) -> std::time::SystemTime {
        std::fs::write(path, body).unwrap();
        let written = std::time::SystemTime::now() - age;
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(written)
            .unwrap();
        written
    }

    /// A servable payload holding one repo, so a test can tell which file
    /// was painted.
    fn servable_named(name: &str) -> String {
        let node = node_json(Some("alice"), name, "2026-08-01T00:00:00Z", 1);
        SERVABLE.replace(r#""nodes":[]"#, &format!(r#""nodes":[{node}]"#))
    }

    fn repo_names(fetch: &GithubFetch) -> Vec<String> {
        fetch.repos.iter().map(|repo| repo.name.clone()).collect()
    }

    fn signed_in(
        token: &'static str,
    ) -> impl FnOnce() -> std::future::Ready<Result<String, String>> {
        move || std::future::ready(Ok(token.to_string()))
    }

    fn signed_out() -> impl FnOnce() -> std::future::Ready<Result<String, String>> {
        || {
            std::future::ready(Err(
                "not authenticated with GitHub. Run: gh auth login".into()
            ))
        }
    }

    /// A loopback URL nothing listens on: the request fails as `Offline`.
    fn nowhere() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}/graphql")
    }

    /// A loopback listener that answers one request with `body` as a 200 and
    /// hands back the request it read, headers and body.
    async fn serve_capturing(body: String) -> (String, tokio::sync::oneshot::Receiver<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (sent, received) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            // Read the headers, then as much body as they announce, so the
            // answer never races the client's write.
            loop {
                let text = String::from_utf8_lossy(&seen).to_lowercase();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if seen.len() >= end + 4 + length {
                        break;
                    }
                }
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => seen.extend_from_slice(&chunk[..n]),
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
            let _ = sent.send(String::from_utf8_lossy(&seen).into_owned());
        });
        (format!("http://{addr}/graphql"), received)
    }

    fn test_client() -> reqwest::Client {
        build_client(
            std::time::Duration::from_secs(2),
            std::time::Duration::from_secs(5),
        )
    }

    /// `fetch_keyed` with the test client and a 2 s launch bound.
    async fn fetch_with<S, F>(
        cache: &crate::cache::KeyedCache,
        mode: CacheMode,
        source: S,
        url: &str,
    ) -> Result<GithubFetch, FetchError>
    where
        S: FnOnce() -> F,
        F: std::future::Future<Output = Result<String, String>>,
    {
        let client = test_client();
        fetch_keyed(50, cache, mode, source, &client, url, LAUNCH_LOOKUP_BOUND).await
    }

    /// C9: the token asked of `gh` is github.com's, whatever `GH_HOST` says.
    #[test]
    fn c9_the_token_is_asked_for_github_com() {
        assert_eq!(GH_TOKEN_ARGS, ["auth", "token", "--hostname", "github.com"]);
        assert_eq!(HOST, "github.com");
    }

    /// O11: the launch lookup is bounded at 2 s.
    #[test]
    fn the_launch_lookup_is_bounded_at_two_seconds() {
        assert_eq!(LAUNCH_LOOKUP_BOUND, std::time::Duration::from_secs(2));
    }

    /// CK2: another account's saved copy is never painted; the launch goes
    /// to the network as if there were no cache, and with the request failing
    /// there is nothing to fall back on.
    #[tokio::test]
    async fn ck2_another_accounts_file_is_not_painted() {
        let scratch = Scratch::new("ck2");
        let file_a = scratch.own_path(TOKEN_A);
        plant(&file_a, &servable_named("a-repo"), HOUR);

        let error = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_in(TOKEN_B),
            &nowhere(),
        )
        .await
        .err()
        .expect("A's file was painted for B");

        assert_eq!(error.failure.kind, FailureKind::Offline);
        assert!(error.fallback.is_none(), "fell back on A's file");
        assert_eq!(error.meta.account, Some(token_fingerprint(TOKEN_B)));
        assert_eq!(
            std::fs::read_to_string(&file_a).unwrap(),
            servable_named("a-repo")
        );
    }

    /// CK4: signed out, the newest current keyed file is painted as a cache
    /// load with no account and no reading; nothing is cleaned up; the poll
    /// that follows fails as `Auth` with no fallback.
    #[tokio::test]
    async fn ck4_signed_out_serves_the_newest_keyed_file_and_the_poll_is_auth() {
        let scratch = Scratch::new("ck4");
        plant(
            &scratch.own_path(TOKEN_A),
            &servable_named("a-repo"),
            2 * HOUR,
        );
        let written_b = plant(&scratch.own_path(TOKEN_B), &servable_named("b-repo"), HOUR);
        plant(
            &scratch.legacy_path(),
            &servable_named("legacy-repo"),
            40 * DAY,
        );
        let before = scratch.names();

        let fetch = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_out(),
            &nowhere(),
        )
        .await
        .expect("the newest keyed file is served");

        assert_eq!(repo_names(&fetch), ["b-repo"]);
        assert_written_near(fetch.origin, written_b);
        assert_eq!(fetch.meta.budget, None, "account must be unknown");
        assert_eq!(fetch.meta.account, None, "account must be unknown");
        assert_eq!(scratch.names(), before, "cleanup ran without an account");

        let poll = fetch_with(
            &scratch.cache(),
            CacheMode::Bypass,
            signed_out(),
            &nowhere(),
        )
        .await
        .err()
        .expect("a signed-out poll succeeded");
        assert_eq!(poll.failure.kind, FailureKind::Auth);
        assert!(poll.fallback.is_none());
        assert_eq!(poll.meta.account, None);
    }

    /// CK11: a lookup that does not answer within the bound is treated as
    /// signed out (as CK4). The bound is a parameter, so this waits 200 ms;
    /// production passes `LAUNCH_LOOKUP_BOUND`, pinned above at 2 s.
    #[tokio::test]
    async fn ck11_a_stalled_lookup_is_treated_as_signed_out_after_the_bound() {
        let scratch = Scratch::new("ck11");
        plant(&scratch.own_path(TOKEN_B), &servable_named("b-repo"), HOUR);
        let bound = std::time::Duration::from_millis(200);
        let client = test_client();
        let started = std::time::Instant::now();

        let fetch = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            fetch_keyed(
                50,
                &scratch.cache(),
                CacheMode::AnyCached,
                std::future::pending::<Result<String, String>>,
                &client,
                &nowhere(),
                bound,
            ),
        )
        .await
        .expect("the launch lookup was not bounded")
        .expect("B's file is served under the signed-out rule");

        assert!(started.elapsed() >= bound);
        assert_eq!(repo_names(&fetch), ["b-repo"]);
        assert!(matches!(fetch.origin, Origin::Cache { .. }));
        assert_eq!(fetch.meta.account, None);
    }

    /// A stalled lookup with nothing to paint fails as `Auth` and says why,
    /// without a request.
    #[tokio::test]
    async fn a_stalled_lookup_with_no_cache_fails_as_auth() {
        let scratch = Scratch::new("stalled_empty");
        let client = test_client();
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            fetch_keyed(
                50,
                &scratch.cache(),
                CacheMode::AnyCached,
                std::future::pending::<Result<String, String>>,
                &client,
                &nowhere(),
                std::time::Duration::from_millis(50),
            ),
        )
        .await
        .expect("the launch lookup was not bounded")
        .err()
        .expect("nothing to serve, yet Ok");
        assert_eq!(error.failure.kind, FailureKind::Auth);
        assert!(error.failure.message.contains("did not answer"));
        assert!(error.fallback.is_none());
    }

    /// CK10: one lookup per attempt. The source would say B on a second call;
    /// it is called once, the request carries A, and the answer lands in A's
    /// file only.
    #[tokio::test]
    async fn ck10_one_lookup_picks_the_file_and_authorizes_the_request() {
        let scratch = Scratch::new("ck10");
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = calls.clone();
        let source = move || {
            let call = counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let token = if call == 0 { TOKEN_A } else { TOKEN_B };
            std::future::ready(Ok::<_, String>(token.to_string()))
        };
        let (url, request) = serve_capturing(SERVABLE.to_string()).await;

        let fetch = fetch_with(&scratch.cache(), CacheMode::AnyCached, source, &url)
            .await
            .expect("the loopback answered");

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let request = request.await.unwrap().to_lowercase();
        assert!(
            request.contains(&format!("authorization: bearer {}", TOKEN_A.to_lowercase())),
            "the request did not carry token A"
        );
        assert_eq!(fetch.origin, Origin::Network);
        assert_eq!(fetch.meta.account, Some(token_fingerprint(TOKEN_A)));
        assert_eq!(
            std::fs::read_to_string(scratch.own_path(TOKEN_A)).unwrap(),
            SERVABLE
        );
        assert!(!scratch.own_path(TOKEN_B).exists());
        assert_eq!(
            scratch.names(),
            [scratch
                .own_path(TOKEN_A)
                .file_name()
                .unwrap()
                .to_string_lossy()]
        );
    }

    /// CK7: the token reaches no file name, no file body, no status line and
    /// no debug output, on a success, a failure or a signed-out launch; the
    /// fingerprint stands in for it.
    #[tokio::test]
    async fn ck7_the_token_appears_nowhere_but_the_request() {
        let scratch = Scratch::new("ck7");
        let (url, _request) = serve_capturing(SERVABLE.to_string()).await;
        let ok = fetch_with(
            &scratch.cache(),
            CacheMode::Bypass,
            signed_in(TOKEN_A),
            &url,
        )
        .await
        .unwrap();
        let failed = fetch_with(
            &scratch.cache(),
            CacheMode::Bypass,
            signed_in(TOKEN_A),
            &nowhere(),
        )
        .await
        .err()
        .unwrap();
        let auth = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_out(),
            &nowhere(),
        )
        .await;

        let mut seen = vec![
            format!("{:?}", ok.meta),
            failed.failure.message.clone(),
            format!("{failed:?} {:?}", failed.meta),
        ];
        match auth {
            Ok(fetch) => seen.push(format!("{:?}", fetch.meta)),
            Err(error) => seen.push(format!("{} {error:?}", error.failure.message)),
        }
        for entry in std::fs::read_dir(&scratch.0).unwrap() {
            let entry = entry.unwrap();
            seen.push(entry.file_name().to_string_lossy().into_owned());
            seen.push(std::fs::read_to_string(entry.path()).unwrap());
        }
        for text in &seen {
            assert!(!text.contains(TOKEN_A), "the token leaked: {text}");
        }
        assert!(scratch.names()[0].contains(&token_fingerprint(TOKEN_A)));
    }

    /// CK8: a pinned `ApiCache` served through `fetch_repos` returns from the
    /// cache branch, before `request`, the only place it asks `gh`: a cache
    /// origin and no account say no attempt was made.
    #[tokio::test]
    async fn ck8_a_pinned_cache_is_served_without_an_attempt() {
        let (path, _) = planted_cache("ck8", std::time::Duration::from_secs(60));
        let cache = crate::cache::ApiCache::at(path.clone(), 0);
        let fetch = fetch_repos(50, &cache, CacheMode::AnyCached).await.unwrap();
        assert!(matches!(fetch.origin, Origin::Cache { .. }));
        assert_eq!(fetch.meta.budget, None);
        assert_eq!(fetch.meta.account, None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// CM1: only the legacy file: painted as a cache load dated by its mtime,
    /// account unknown. The first successful poll writes the keyed file and
    /// leaves the legacy one alone.
    #[tokio::test]
    async fn cm1_only_the_legacy_file_is_painted_until_a_poll_writes_the_keyed_one() {
        let scratch = Scratch::new("cm1");
        let written = plant(
            &scratch.legacy_path(),
            &servable_named("legacy-repo"),
            3 * HOUR,
        );

        let fetch = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_in(TOKEN_A),
            &nowhere(),
        )
        .await
        .expect("the legacy file is painted");
        assert_eq!(repo_names(&fetch), ["legacy-repo"]);
        assert_written_near(fetch.origin, written);
        assert_eq!(fetch.meta.account, None, "the legacy file has no account");

        let (url, _request) = serve_capturing(servable_named("fresh-repo")).await;
        let poll = fetch_with(
            &scratch.cache(),
            CacheMode::Bypass,
            signed_in(TOKEN_A),
            &url,
        )
        .await
        .expect("the poll succeeded");
        assert_eq!(repo_names(&poll), ["fresh-repo"]);
        assert_eq!(
            std::fs::read_to_string(scratch.own_path(TOKEN_A)).unwrap(),
            servable_named("fresh-repo")
        );
        assert_eq!(
            std::fs::read_to_string(scratch.legacy_path()).unwrap(),
            servable_named("legacy-repo")
        );
    }

    /// CM2: legacy and this credential's keyed file: the keyed one, with its
    /// account.
    #[tokio::test]
    async fn cm2_the_own_keyed_file_wins_over_the_legacy_one() {
        let scratch = Scratch::new("cm2");
        plant(&scratch.legacy_path(), &servable_named("legacy-repo"), HOUR);
        plant(
            &scratch.own_path(TOKEN_A),
            &servable_named("own-repo"),
            2 * HOUR,
        );

        let fetch = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_in(TOKEN_A),
            &nowhere(),
        )
        .await
        .unwrap();

        assert_eq!(repo_names(&fetch), ["own-repo"]);
        assert!(matches!(fetch.origin, Origin::Cache { .. }));
        assert_eq!(fetch.meta.account, Some(token_fingerprint(TOKEN_A)));
    }

    /// CM3 (a): `R` fails with only a legacy file on disk: no fallback, the
    /// legacy file untouched.
    #[tokio::test]
    async fn cm3a_a_failed_reload_never_falls_back_on_the_legacy_file() {
        let scratch = Scratch::new("cm3a");
        plant(&scratch.legacy_path(), &servable_named("legacy-repo"), HOUR);

        let error = fetch_with(
            &scratch.cache(),
            CacheMode::Bypass,
            signed_in(TOKEN_A),
            &nowhere(),
        )
        .await
        .err()
        .expect("offline, yet Ok");

        assert_eq!(error.failure.kind, FailureKind::Offline);
        assert!(error.fallback.is_none(), "fell back on the legacy file");
        assert_eq!(
            std::fs::read_to_string(scratch.legacy_path()).unwrap(),
            servable_named("legacy-repo")
        );
    }

    /// CM3 (b): a launch whose legacy file is unusable and whose request
    /// fails: no fallback, the load fails. The legacy body parses, so a
    /// fallback read of it would succeed; only launch's `usable` refuses it.
    #[tokio::test]
    async fn cm3b_an_unusable_legacy_file_and_a_failed_launch_have_no_fallback() {
        let scratch = Scratch::new("cm3b");
        plant(&scratch.legacy_path(), NO_CALENDAR, HOUR);
        assert!(
            read_cached(
                &scratch.cache().launch_reads(None)[0].cache,
                day("2026-08-18")
            )
            .is_some(),
            "fixture: the legacy body must parse"
        );

        let error = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_in(TOKEN_A),
            &nowhere(),
        )
        .await
        .err()
        .expect("nothing usable, yet Ok");

        assert_eq!(error.failure.kind, FailureKind::Offline);
        assert!(error.fallback.is_none());
        assert_eq!(
            std::fs::read_to_string(scratch.legacy_path()).unwrap(),
            NO_CALENDAR
        );
    }

    /// The other side of CM3: a failed `R` falls back on this credential's
    /// own keyed file, as a failure carrying it (rule 2).
    #[tokio::test]
    async fn a_failed_reload_falls_back_on_the_own_keyed_file() {
        let scratch = Scratch::new("own_fallback");
        let written = plant(
            &scratch.own_path(TOKEN_A),
            &servable_named("own-repo"),
            HOUR,
        );

        let error = fetch_with(
            &scratch.cache(),
            CacheMode::Bypass,
            signed_in(TOKEN_A),
            &nowhere(),
        )
        .await
        .err()
        .expect("offline, yet Ok");

        let fallback = error.fallback.expect("the own file is the fallback");
        assert_eq!(repo_names(&fallback), ["own-repo"]);
        assert_written_near(fallback.origin, written);
    }

    /// CM4: with this credential's keyed file, launch deletes a legacy file
    /// 31 days unwritten and keeps one 2 days unwritten.
    #[tokio::test]
    async fn cm4_launch_deletes_a_legacy_file_unwritten_for_31_days_only() {
        for (age, kept) in [(31 * DAY, false), (2 * DAY, true)] {
            let scratch = Scratch::new(&format!("cm4_{}", age.as_secs()));
            plant(
                &scratch.own_path(TOKEN_A),
                &servable_named("own-repo"),
                HOUR,
            );
            plant(&scratch.legacy_path(), &servable_named("legacy-repo"), age);

            fetch_with(
                &scratch.cache(),
                CacheMode::AnyCached,
                signed_in(TOKEN_A),
                &nowhere(),
            )
            .await
            .expect("the own file is served");

            assert_eq!(scratch.legacy_path().exists(), kept, "age {age:?}");
        }
    }

    /// A launch whose request succeeds cleans up too; one whose request
    /// fails, and a poll, never do.
    #[tokio::test]
    async fn cleanup_runs_after_a_successful_launch_only() {
        let scratch = Scratch::new("cleanup_paths");
        plant(&scratch.legacy_path(), "not json", 40 * DAY);
        plant(
            &scratch.own_path(TOKEN_B),
            &servable_named("b-repo"),
            40 * DAY,
        );
        // A's current file exists, so every deletion rule but "not on a
        // failure path" holds for the two 40 day old files. It is unusable,
        // so A's launch goes to the network; failing, it must delete nothing.
        plant(
            &scratch.own_path(TOKEN_A),
            NO_CALENDAR,
            std::time::Duration::ZERO,
        );
        let before = scratch.names();
        let failed = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_in(TOKEN_A),
            &nowhere(),
        )
        .await;
        assert!(failed.is_err());
        assert_eq!(scratch.names(), before, "a failed launch cleaned up");

        // A poll rewrites A's file but is not launch.
        let (url, _request) = serve_capturing(SERVABLE.to_string()).await;
        fetch_with(
            &scratch.cache(),
            CacheMode::Bypass,
            signed_in(TOKEN_A),
            &url,
        )
        .await
        .unwrap();
        assert_eq!(scratch.names(), before, "a poll cleaned up");

        // A launch that reaches the network and succeeds cleans up.
        std::fs::remove_file(scratch.own_path(TOKEN_A)).unwrap();
        let (url, _request) = serve_capturing(SERVABLE.to_string()).await;
        fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_in(TOKEN_A),
            &url,
        )
        .await
        .unwrap();
        assert_eq!(
            scratch.names(),
            [scratch
                .own_path(TOKEN_A)
                .file_name()
                .unwrap()
                .to_string_lossy()]
        );
    }

    /// CM5: only an older version's keyed file for this credential: not
    /// served, a cold launch.
    #[tokio::test]
    async fn cm5_an_older_versions_keyed_file_is_not_served() {
        let scratch = Scratch::new("cm5");
        let account = token_fingerprint(TOKEN_A);
        plant(
            &scratch
                .0
                .join(format!("github-v1-github.com-{account}-50.json")),
            &servable_named("v1-repo"),
            HOUR,
        );

        let error = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_in(TOKEN_A),
            &nowhere(),
        )
        .await
        .err()
        .expect("an older version was served");

        assert_eq!(error.failure.kind, FailureKind::Offline);
        assert!(error.fallback.is_none());
    }

    /// CM6: legacy and account A's keyed file, launch on B: neither is
    /// painted, B behaves as no cache.
    #[tokio::test]
    async fn cm6_another_accounts_keyed_file_retires_the_legacy_one() {
        let scratch = Scratch::new("cm6");
        plant(&scratch.legacy_path(), &servable_named("legacy-repo"), HOUR);
        plant(&scratch.own_path(TOKEN_A), &servable_named("a-repo"), HOUR);

        let error = fetch_with(
            &scratch.cache(),
            CacheMode::AnyCached,
            signed_in(TOKEN_B),
            &nowhere(),
        )
        .await
        .err()
        .expect("the legacy or A's file was painted for B");

        assert!(error.fallback.is_none());
        assert_eq!(error.meta.account, Some(token_fingerprint(TOKEN_B)));
    }
}
