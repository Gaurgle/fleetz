# Contributions Pane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A GitHub-style contribution heatmap docked as a horizontal strip at the bottom of fleetz's main area, fed either by GitHub's own calendar or by `git log` across the repos currently on screen.

**Architecture:** One pure module (`src/contributions.rs`) owns the 53x7 grid arithmetic and is the only place that knows how a year maps to slots. Two producers fill it: a new field on the existing GraphQL query (measured free), and a per-repo daily histogram computed during the sweep so filter changes never shell out. One renderer (`src/ui/contributions.rs`) draws it and owns the layout split, which both `ui::render` and `ui::panel_rects` call so hit-testing cannot drift from what is drawn.

**Tech Stack:** Rust 2024, ratatui 0.30, crossterm 0.29, tokio, chrono 0.4, serde, toml_edit.

**Spec:** `docs/superpowers/specs/2026-08-18-contributions-pane-design.md`

## Global Constraints

- **Never use em-dashes or en-dashes** in any output: chat, code, comments, docs. Use a hyphen, a colon, parentheses, or rewrite.
- **Conventional Commits**, first line under 72 characters. No `Co-Authored-By` lines, ever.
- **Do not run the commit.** Every "Commit" step below means: present the full `git commit -m "..."` command and let the human run it. The human commits and pushes.
- **Commit straight to `main`.** No feature branches, no PRs.
- Tests live beside the code in `#[cfg(test)] mod tests` in the same file, never in `tests/`.
- Every behavior change lands with a test that would have caught the old behavior.
- `cargo test`, `cargo clippy --all-targets`, and `cargo fmt --check` must all be clean before a commit is presented.
- **Do not add dependencies.** Everything here is covered by the existing `Cargo.toml`.
- Ask before deleting files or changing anything under `.github/`.

## Deviations from the spec, decided while planning

Three refinements, all narrowing rather than widening scope. Flag them to the human at the first checkpoint:

1. **The layout helper lives in `src/ui/contributions.rs`, not `src/ui/detail.rs`.** The spec put it beside `split_main`, but the strip applies to Compact view too, and `detail.rs` is the Detail view's module. It still satisfies the parity invariant: one function, called by both `render` and `panel_rects`.
2. **`month_labels()` returns column indices, not pixel columns.** The spec signature took a `cell_width`. Keeping the pure module ignorant of cell widths is a cleaner boundary; the renderer multiplies.
3. **`totalContributions` is not requested.** `Calendar::total()` sums the days. Requesting a field only a test reads would be a dead field in every non-test build.

## File Structure

| File | Responsibility |
|---|---|
| `src/contributions.rs` (new) | Pure grid model: `Calendar`, `Source`, `DailyCounts`, slot arithmetic, intensity buckets, month labels. No I/O. |
| `src/ui/contributions.rs` (new) | The strip: layout split, width tiers, cell/label/legend rendering, click hit-testing. |
| `src/github.rs` | One added GraphQL field, its deserialization, and `GithubFetch` as the fetch return type. |
| `src/git.rs` | `daily_commits` and `user_email` probes, plus `parse_daily`. |
| `src/model.rs` | `RepoStatus.daily` field. |
| `src/app.rs` | `ContributionsMode`, `FocusedPanel::Contributions`, cursor state, `local_calendar`, the settings field. |
| `src/ui/mod.rs` | `PanelRects.contributions`, wiring the split into `render` and `panel_rects`. |
| `src/ui/colors.rs` | `CONTRIB_LADDER`. |
| `src/main.rs` | Key routing, sweep plumbing, event plumbing. |
| `src/config.rs` | `contributions` and `contributions_author` keys. |

---

### Task 1: The pure grid model

**Files:**
- Create: `src/contributions.rs`
- Modify: `src/main.rs` (add `mod contributions;` beside the other module declarations)
- Test: `src/contributions.rs` (`#[cfg(test)] mod tests` at the bottom)

**Interfaces:**
- Consumes: nothing.
- Produces: `contributions::WEEKS: usize`, `contributions::SLOTS: usize`, `contributions::Source` (`Github`, `Local`, with `label() -> &'static str`), `contributions::grid_start(NaiveDate) -> NaiveDate`, `contributions::DailyCounts` (a newtype over `[u16; SLOTS]`, `Default + Clone + Copy + Debug`), and `contributions::Calendar` with `new(NaiveDate, Source)`, `slot(NaiveDate) -> Option<usize>`, `add(NaiveDate, u32)`, `add_daily(&DailyCounts)`, `source() -> Source`, `today_slot() -> usize`, `total() -> u32`, `max() -> u32`, `weeks() -> impl Iterator<Item = [Option<u32>; 7]>`, `day_at(usize, usize) -> Option<(NaiveDate, u32)>`, `intensity(u32) -> u8`, `month_labels() -> Vec<(usize, &'static str)>`.

- [ ] **Step 1: Write the failing tests**

Create `src/contributions.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// The exact start the live API returned when this was designed: probing
    /// `contributionCalendar` on 2026-08-18 opened the grid on 2025-08-17.
    /// If this drifts, the two sources stop sharing a grid.
    #[test]
    fn grid_start_matches_the_probed_github_calendar() {
        assert_eq!(grid_start(d("2026-08-18")), d("2025-08-17"));
    }

    #[test]
    fn grid_start_is_always_a_sunday_and_always_53_columns() {
        // Walk a full week of "today"s so every weekday is covered.
        for offset in 0..7 {
            let today = d("2026-08-16") + chrono::Duration::days(offset);
            let start = grid_start(today);
            assert_eq!(start.weekday(), chrono::Weekday::Sun, "today {today}");
            let span = (today - start).num_days();
            assert_eq!(span / 7, 52, "today {today} spans {span} days");
            assert!((span as usize) < SLOTS);
        }
    }

    #[test]
    fn slots_outside_the_year_are_rejected() {
        let cal = Calendar::new(d("2026-08-18"), Source::Local);
        assert_eq!(cal.slot(d("2025-08-17")), Some(0));
        assert_eq!(cal.slot(d("2025-08-16")), None, "day before the grid opens");
        assert_eq!(cal.slot(d("2026-08-18")), Some(366));
    }

    #[test]
    fn add_ignores_dates_outside_the_year() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2026-08-18"), 5);
        cal.add(d("2020-01-01"), 99);
        assert_eq!(cal.total(), 5);
    }

    /// The grid opens on a Sunday by construction, so the FIRST column is
    /// always complete. Only the last one is short.
    #[test]
    fn first_week_is_complete_and_last_week_stops_at_today() {
        // 2026-08-18 is a Tuesday: Sun, Mon, Tue present, Wed..Sat absent.
        let cal = Calendar::new(d("2026-08-18"), Source::Github);
        let weeks: Vec<_> = cal.weeks().collect();
        assert_eq!(weeks.len(), WEEKS);
        assert!(weeks[0].iter().all(|c| c.is_some()), "no leading gap");
        let last = weeks[WEEKS - 1];
        assert!(last[0].is_some() && last[1].is_some() && last[2].is_some());
        assert!(last[3].is_none() && last[6].is_none());
    }

    #[test]
    fn day_at_maps_back_to_a_real_date() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2025-08-19"), 3); // Tuesday of the first column
        assert_eq!(cal.day_at(0, 2), Some((d("2025-08-19"), 3)));
        assert_eq!(cal.day_at(WEEKS - 1, 6), None, "past today");
    }

    #[test]
    fn intensity_buckets_by_quartile_of_the_busiest_day() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2026-01-05"), 100);
        assert_eq!(cal.intensity(0), 0, "an empty day is never shaded");
        assert_eq!(cal.intensity(1), 1, "one contribution is visible");
        assert_eq!(cal.intensity(25), 1, "top of the first quartile");
        assert_eq!(cal.intensity(26), 2);
        assert_eq!(cal.intensity(75), 3);
        assert_eq!(cal.intensity(100), 4, "the busiest day is the brightest");
    }

    #[test]
    fn intensity_of_an_empty_year_never_divides_by_zero() {
        let cal = Calendar::new(d("2026-08-18"), Source::Github);
        assert_eq!(cal.max(), 0);
        assert_eq!(cal.intensity(0), 0);
    }

    #[test]
    fn a_single_contribution_year_shows_that_day_at_full_brightness() {
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add(d("2026-03-12"), 1);
        assert_eq!(cal.intensity(1), 4);
    }

    #[test]
    fn month_labels_land_on_the_column_the_month_opens_in() {
        let cal = Calendar::new(d("2026-08-18"), Source::Github);
        let labels = cal.month_labels();
        assert_eq!(labels[0], (0, "Aug"), "grid opens 2025-08-17");
        assert_eq!(labels[1].1, "Sep");
        // Thirteen months are touched: Aug 2025 through Aug 2026.
        assert_eq!(labels.len(), 13);
        assert_eq!(labels[12].1, "Aug");
        // Strictly increasing columns, one label per column at most.
        assert!(labels.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn add_daily_folds_a_per_repo_histogram_into_the_grid() {
        let mut counts = DailyCounts::default();
        counts.0[0] = 2;
        counts.0[366] = 4;
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add_daily(&counts);
        cal.add_daily(&counts);
        assert_eq!(cal.total(), 12);
        assert_eq!(cal.day_at(0, 0).unwrap().1, 4);
    }

    /// Slots after today hold zero but must never reach the total, or a
    /// second repo's stale tail would inflate the count.
    #[test]
    fn slots_past_today_are_excluded_from_total_and_max() {
        let mut counts = DailyCounts::default();
        counts.0[SLOTS - 1] = 9;
        let mut cal = Calendar::new(d("2026-08-18"), Source::Local);
        cal.add_daily(&counts);
        assert_eq!(cal.total(), 0);
        assert_eq!(cal.max(), 0);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test contributions`
Expected: compile failure, `cannot find function grid_start` and friends. A compile error is the correct failure here: nothing is implemented yet.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `src/contributions.rs`:

```rust
//! The contribution grid: 53 columns of 7 days, the shape GitHub draws.
//!
//! Pure by design. Both producers (the GraphQL calendar and the local
//! `git log` histogram) fill the same structure through the same slot
//! arithmetic, so the two can never disagree about which cell a date is.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// Columns in the grid. GitHub returns exactly 53 and so do we.
pub const WEEKS: usize = 53;

/// Cells in the grid: 53 columns of 7 days. Slots past today exist so the
/// array is a fixed size, and are excluded from every aggregate.
pub const SLOTS: usize = WEEKS * 7;

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Where a calendar's numbers came from. They will not agree: GitHub counts
/// PRs, issues, reviews and private repos outside the fleet, the local source
/// counts commits in cloned repos. The pane names the active one for exactly
/// that reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Github,
    Local,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Local => "local",
        }
    }
}

/// The Sunday the grid opens on: the Sunday on or before `today - 364d`.
///
/// Reproduces GitHub's own window exactly. Probed on 2026-08-18, the API
/// opened its calendar on 2025-08-17, which is what this returns. Because
/// 364 is a whole number of weeks, the result is always 52 weeks plus a
/// partial one back from today, so the grid is always 53 columns wide.
pub fn grid_start(today: NaiveDate) -> NaiveDate {
    let back = today - Duration::days(364);
    back - Duration::days(back.weekday().num_days_from_sunday() as i64)
}

/// One repo's commits per grid slot. `u16` because no repo gets 65k commits
/// from one author in one day, and 742 bytes per repo is small enough to
/// keep on every `RepoStatus`.
#[derive(Debug, Clone, Copy)]
pub struct DailyCounts(pub [u16; SLOTS]);

impl Default for DailyCounts {
    fn default() -> Self {
        Self([0; SLOTS])
    }
}

/// A filled year of contribution counts.
#[derive(Debug, Clone)]
pub struct Calendar {
    start: NaiveDate,
    today: NaiveDate,
    counts: [u32; SLOTS],
    source: Source,
}

impl Calendar {
    pub fn new(today: NaiveDate, source: Source) -> Self {
        Self {
            start: grid_start(today),
            today,
            counts: [0; SLOTS],
            source,
        }
    }

    pub fn source(&self) -> Source {
        self.source
    }

    pub fn start(&self) -> NaiveDate {
        self.start
    }

    /// The grid slot a date falls in, or None when it is outside the year.
    pub fn slot(&self, date: NaiveDate) -> Option<usize> {
        let offset = (date - self.start).num_days();
        (offset >= 0 && (offset as usize) < SLOTS).then_some(offset as usize)
    }

    /// The last slot carrying real data. Everything after it is future.
    pub fn today_slot(&self) -> usize {
        self.slot(self.today).unwrap_or(SLOTS - 1)
    }

    fn live(&self) -> usize {
        self.today_slot() + 1
    }

    pub fn add(&mut self, date: NaiveDate, count: u32) {
        if let Some(slot) = self.slot(date) {
            self.counts[slot] += count;
        }
    }

    /// Fold one repo's histogram in. Used once per visible repo to build the
    /// local calendar, so it must stay allocation-free.
    pub fn add_daily(&mut self, daily: &DailyCounts) {
        for (slot, n) in daily.0.iter().enumerate() {
            self.counts[slot] += *n as u32;
        }
    }

    pub fn total(&self) -> u32 {
        self.counts[..self.live()].iter().sum()
    }

    pub fn max(&self) -> u32 {
        self.counts[..self.live()].iter().copied().max().unwrap_or(0)
    }

    /// One column per week, Sunday first. `None` marks a slot past today, so
    /// the partial trailing week is expressed in exactly one place.
    pub fn weeks(&self) -> impl Iterator<Item = [Option<u32>; 7]> + '_ {
        let live = self.live();
        (0..WEEKS).map(move |w| {
            let mut column = [None; 7];
            for (weekday, cell) in column.iter_mut().enumerate() {
                let slot = w * 7 + weekday;
                if slot < live {
                    *cell = Some(self.counts[slot]);
                }
            }
            column
        })
    }

    /// The date and count at a grid position, or None past today.
    pub fn day_at(&self, week: usize, weekday: usize) -> Option<(NaiveDate, u32)> {
        let slot = week * 7 + weekday;
        (slot < self.live()).then(|| {
            (
                self.start + Duration::days(slot as i64),
                self.counts[slot],
            )
        })
    }

    /// Shade step 0..=4 for a count. Quartiles of the calendar's own busiest
    /// day, matching GitHub's relative scaling: a quiet fleet still shows
    /// shape instead of one flat tone.
    pub fn intensity(&self, count: u32) -> u8 {
        if count == 0 {
            return 0;
        }
        let max = self.max().max(1);
        ((count as u64 * 4).div_ceil(max as u64)).clamp(1, 4) as u8
    }

    /// Which column each month opens in, for the label row.
    pub fn month_labels(&self) -> Vec<(usize, &'static str)> {
        let mut out = Vec::new();
        let mut previous = None;
        for week in 0..WEEKS {
            let date = self.start + Duration::days((week * 7) as i64);
            let month = date.month0() as usize;
            if previous != Some(month) {
                out.push((week, MONTH_NAMES[month]));
                previous = Some(month);
            }
        }
        out
    }
}
```

Add `mod contributions;` to `src/main.rs` in alphabetical position among the existing `mod` declarations.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test contributions`
Expected: all 12 tests PASS.

Note on `Weekday`: it is imported for the `assert_eq!(start.weekday(), Weekday::Sun)` test. If clippy reports the import as unused in the non-test build, move it into the test module's `use super::*` scope instead of the module header.

- [ ] **Step 5: Verify the whole suite and the lints**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: all pass, no warnings.

- [ ] **Step 6: Commit (present the command, do not run it)**

```bash
git add src/contributions.rs src/main.rs && git commit -m "feat: add the contribution grid model"
```

---

### Task 2: The GitHub source

**Files:**
- Modify: `src/github.rs` (add the query field, the deserialization types, `GithubFetch`, and change `fetch_repos`)
- Modify: `src/main.rs` (`fetch_dataset`, `spawn_github_poll`, `AppEvent::Github`, `LoadResult`, and the two match arms that consume them)
- Test: `src/github.rs`

**Interfaces:**
- Consumes: `contributions::{Calendar, Source}` from Task 1.
- Produces: `github::GithubFetch { pub repos: Vec<GithubRepo>, pub calendar: Option<Box<Calendar>> }`; `github::fetch_repos(u32, bool, &ApiCache, bool) -> Result<GithubFetch, String>`.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/github.rs`:

```rust
    /// The calendar rides the existing payload, so an old cache file (written
    /// before the field existed) must still parse. `organizations` already
    /// carries this treatment; this is the same trap.
    #[test]
    fn payload_without_a_calendar_parses_with_none() {
        let json = r#"{"data":{"viewer":{"repositories":{"nodes":[]}}}}"#;
        let response: GraphQlResponse = serde_json::from_str(json).unwrap();
        let viewer = response.data.unwrap().viewer;
        assert!(parse_calendar(viewer.contributions_collection, day("2026-08-18")).is_none());
    }

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
        let cal = parse_calendar(viewer.contributions_collection, day("2026-08-18")).unwrap();
        assert_eq!(cal.total(), 9);
        assert_eq!(cal.day_at(0, 0).unwrap().1, 2, "grid opens on 2025-08-17");
        assert_eq!(cal.max(), 7);
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
        let cal = parse_calendar(viewer.contributions_collection, day("2026-08-18")).unwrap();
        assert_eq!(cal.total(), 3);
    }

    /// Measured before it was written: `contributionCalendar` has no `first:`
    /// connection, so it costs nothing on top of the existing query. If it
    /// ever grows one, this query stops being free and the invariant bites.
    #[test]
    fn the_calendar_field_rides_the_existing_query() {
        let q = build_query(50, false);
        assert!(q.contains("contributionsCollection"));
        assert!(q.contains("contributionCalendar"));
        assert!(!q.contains("contributionCalendar(first"));
        // Still one query, not two.
        assert_eq!(q.matches("viewer").count(), 1);
    }
```

Add this helper next to the existing `make_node` helper in the same test module:

```rust
    fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib github::tests`
Expected: compile failure on `contributions_collection` and `parse_calendar`.

- [ ] **Step 3: Add the deserialization types and the parser**

In `src/github.rs`, extend `Viewer` and add the new types below it:

```rust
#[derive(Deserialize)]
struct Viewer {
    repositories: RepositoryConnection,
    /// Repos reached through org membership rather than through an
    /// affiliation on `repositories`. See `build_query` for why both are
    /// needed; absent in cached payloads written before this was added.
    #[serde(default)]
    organizations: Option<OrganizationConnection>,
    /// The account-wide contribution calendar. Absent in cached payloads
    /// written before this was added, exactly like `organizations`.
    #[serde(rename = "contributionsCollection", default)]
    contributions_collection: Option<ContributionsCollection>,
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

/// Fold the API's week/day nesting into our own grid. The API's own week
/// boundaries are ignored on purpose: `Calendar` derives the slot from the
/// date, so the two sources land on the same cells even if GitHub ever
/// changes where it starts a week.
fn parse_calendar(
    collection: Option<ContributionsCollection>,
    today: chrono::NaiveDate,
) -> Option<Calendar> {
    let collection = collection?;
    let mut calendar = Calendar::new(today, Source::Github);
    for week in collection.contribution_calendar.weeks {
        for day in week.contribution_days {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") {
                calendar.add(date, day.contribution_count);
            }
        }
    }
    Some(calendar)
}
```

Add to the imports at the top of `src/github.rs`:

```rust
use crate::contributions::{Calendar, Source};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib github::tests`
Expected: the four new tests PASS. The existing github tests still pass.

- [ ] **Step 5: Add the field to the query**

In `build_query`, extend the `viewer` block. The field goes beside `repositories` and `organizations`:

```rust
    format!(
        r#"{{ viewer {{
  contributionsCollection {{
    contributionCalendar {{
      weeks {{ contributionDays {{ date, contributionCount }} }}
    }}
  }}
  repositories(first: {repo_limit}, orderBy: {{field: PUSHED_AT, direction: DESC}},
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
```

Add this comment directly above the `contributionsCollection` line:

```rust
  // Free: measured at cost 1 / nodeCount 0 on its own, and it has no `first:`
  // connection, so unioning it here leaves the poll at its existing price.
  // A field, not a request: see the "never widen the GitHub fetch" invariant.
```

Note: the comment must go inside the Rust source above the `format!`, not inside the GraphQL string, since `#` starts a comment in GraphQL but the string is also used by the query-shape test.

- [ ] **Step 6: Change the fetch return type**

Replace the `Vec<GithubRepo>` return with a struct. Add near `GithubRepo`:

```rust
/// One GitHub poll's results. Boxed calendar because it is ~1.5KB and this
/// travels through an `AppEvent` enum whose other variants are pointer-sized.
pub struct GithubFetch {
    pub repos: Vec<GithubRepo>,
    pub calendar: Option<Box<Calendar>>,
}
```

Update `parse_cached` and `fetch_repos`:

```rust
fn parse_cached(json: Option<String>, today: chrono::NaiveDate) -> Option<GithubFetch> {
    let response: GraphQlResponse = serde_json::from_str(&json?).ok()?;
    let viewer = response.data?.viewer;
    let calendar = parse_calendar(viewer.contributions_collection, today).map(Box::new);
    Some(GithubFetch {
        repos: parse_repos(merge_sources(viewer)),
        calendar,
    })
}
```

`merge_sources` takes `Viewer` by value and `parse_calendar` needs one of its fields, so move the calendar out first as shown. Adjust `merge_sources` to take the two connections it actually uses if the borrow checker objects:

```rust
fn merge_sources(viewer: Viewer) -> Vec<RepoNode> { /* unchanged body */ }
```

Since `parse_calendar` consumes `viewer.contributions_collection` by move and `merge_sources` consumes the rest, destructure instead:

```rust
    let Viewer {
        repositories,
        organizations,
        contributions_collection,
    } = response.data?.viewer;
    let calendar = parse_calendar(contributions_collection, today).map(Box::new);
    let repos = parse_repos(merge_sources(Viewer {
        repositories,
        organizations,
        contributions_collection: None,
    }));
```

In `fetch_repos`, thread `today` through and return the struct. The signature becomes:

```rust
pub async fn fetch_repos(
    repo_limit: u32,
    include_prs: bool,
    cache: &crate::cache::ApiCache,
    bypass_cache: bool,
) -> Result<GithubFetch, String> {
```

Inside it, compute `let today = chrono::Local::now().date_naive();` once at the top and pass it to every `parse_cached` call. The final success path becomes:

```rust
    let Viewer {
        repositories,
        organizations,
        contributions_collection,
    } = data.viewer;
    let calendar = parse_calendar(contributions_collection, today).map(Box::new);
    Ok(GithubFetch {
        repos: parse_repos(merge_sources(Viewer {
            repositories,
            organizations,
            contributions_collection: None,
        })),
        calendar,
    })
```

- [ ] **Step 7: Update the three call sites**

In `src/main.rs`:

1. `AppEvent::Github(Result<Vec<github::GithubRepo>, String>)` becomes `AppEvent::Github(Result<Box<github::GithubFetch>, String>)`.
2. `spawn_github_poll` sends `result.map(Box::new)`.
3. `fetch_dataset` returns `Result<github::GithubFetch, String>`.
4. `LoadResult.dataset: Option<Vec<github::GithubRepo>>` gains a sibling: `pub github_calendar: Option<Box<contributions::Calendar>>`. Set it from the fetch on the load paths, and `None` on `LoadKind::WindowApply` (a pure re-filter changes no data).
5. Wherever `load_repos` returns its tuple, widen it to carry the calendar alongside the dataset.
6. At the `AppEvent::Github` match arm, take `fetch.repos` where the code reads the old `Vec`, and store `fetch.calendar` on the app in Task 4. For now, bind it with `let _ = fetch.calendar;` and leave a `// wired up in the app-state task` comment so nothing is silently dropped.

- [ ] **Step 8: Verify the whole suite and the lints**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: all pass. Expect to chase compile errors through `main.rs` in step 7; that is the work.

- [ ] **Step 9: Sanity-check against the live API**

Run: `cargo run -- --help` first to confirm it builds, then launch `cargo run` once with a valid `gh auth token` present and confirm no "GitHub API errors" status appears. The calendar is not drawn yet; this only proves the widened query is still accepted.

- [ ] **Step 10: Commit (present the command, do not run it)**

```bash
git add src/github.rs src/main.rs && git commit -m "feat: fetch the contribution calendar with the repo poll"
```

---

### Task 3: The local source

**Files:**
- Modify: `src/git.rs` (add `daily_commits`, `parse_daily`, `user_email`)
- Modify: `src/model.rs` (add `RepoStatus.daily`)
- Modify: `src/main.rs` (every `RepoStatus { .. }` literal needs the new field)
- Test: `src/git.rs`

**Interfaces:**
- Consumes: `contributions::{DailyCounts, SLOTS}` from Task 1.
- Produces: `git::parse_daily(&str, NaiveDate) -> DailyCounts`; `git::daily_commits(&Path, NaiveDate, &str) -> Option<DailyCounts>`; `git::user_email(&Path) -> Option<String>`; `RepoStatus.daily: Option<Box<DailyCounts>>`.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/git.rs`:

```rust
    fn start_day() -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str("2025-08-17", "%Y-%m-%d").unwrap()
    }

    #[test]
    fn parse_daily_accumulates_repeated_days() {
        let out = "2025-08-17\n2025-08-17\n2025-08-18\n";
        let counts = parse_daily(out, start_day());
        assert_eq!(counts.0[0], 2);
        assert_eq!(counts.0[1], 1);
        assert_eq!(counts.0[2], 0, "a day with no commits stays zero");
    }

    #[test]
    fn parse_daily_ignores_days_outside_the_grid() {
        let out = "2020-01-01\n2025-08-16\n2025-08-17\n";
        let counts = parse_daily(out, start_day());
        assert_eq!(counts.0.iter().map(|n| *n as u32).sum::<u32>(), 1);
    }

    #[test]
    fn parse_daily_survives_junk_lines() {
        // A git that printed a warning to stdout must not poison the year.
        let out = "warning: something\n\n2025-08-17\n";
        let counts = parse_daily(out, start_day());
        assert_eq!(counts.0[0], 1);
    }

    #[test]
    fn parse_daily_fills_the_last_slot() {
        let last = start_day() + chrono::Duration::days((crate::contributions::SLOTS - 1) as i64);
        let out = format!("{}\n", last.format("%Y-%m-%d"));
        let counts = parse_daily(&out, start_day());
        assert_eq!(counts.0[crate::contributions::SLOTS - 1], 1);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib git::tests::parse_daily`
Expected: compile failure, `cannot find function parse_daily`.

- [ ] **Step 3: Write the implementation**

Add to `src/git.rs`, next to the other log probes:

```rust
/// Per-day commit counts for one author over the contribution grid.
///
/// `--all` rather than the default branch: work on a feature branch is still
/// work, and git de-duplicates a commit reachable from several refs. `%ad`
/// with `--date=short` is the author date in local time, which is the day you
/// would say you did the work on.
pub async fn daily_commits(
    repo_path: &Path,
    start: chrono::NaiveDate,
    author: &str,
) -> Option<crate::contributions::DailyCounts> {
    let since = format!("--since={}", start.format("%Y-%m-%d"));
    let author_arg = format!("--author={author}");
    let output = run_git(
        repo_path,
        &[
            "log",
            "--all",
            "--date=short",
            "--pretty=%ad",
            &since,
            &author_arg,
        ],
    )
    .await?;
    if !output.status.success() {
        return None;
    }
    Some(parse_daily(
        &String::from_utf8_lossy(&output.stdout),
        start,
    ))
}

/// Bucket `git log --date=short --pretty=%ad` output into grid slots. Lines
/// that are not a date are skipped rather than treated as errors: a warning
/// on stdout must not cost a year of history.
pub fn parse_daily(
    stdout: &str,
    start: chrono::NaiveDate,
) -> crate::contributions::DailyCounts {
    let mut counts = crate::contributions::DailyCounts::default();
    for line in stdout.lines() {
        let Ok(date) = chrono::NaiveDate::parse_from_str(line.trim(), "%Y-%m-%d") else {
            continue;
        };
        let offset = (date - start).num_days();
        if offset >= 0 && (offset as usize) < crate::contributions::SLOTS {
            let slot = offset as usize;
            counts.0[slot] = counts.0[slot].saturating_add(1);
        }
    }
    counts
}

/// The identity git would stamp on a commit made here, honouring a
/// repo-local override. A clone configured with a work address must count
/// against that address, not against the global one.
pub async fn user_email(repo_path: &Path) -> Option<String> {
    let output = run_git(repo_path, &["config", "user.email"]).await?;
    if !output.status.success() {
        return None;
    }
    let email = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!email.is_empty()).then_some(email)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib git::tests`
Expected: the four new tests PASS.

- [ ] **Step 5: Add the field to RepoStatus**

In `src/model.rs`, add to `RepoStatus` after `work_key`:

```rust
    /// Per-day commit counts for this repo over the contribution grid, or
    /// None when the local calendar has never been asked for. Boxed so a
    /// repo that will never need it costs one pointer rather than 742 bytes.
    pub daily: Option<Box<crate::contributions::DailyCounts>>,
```

- [ ] **Step 6: Fix every construction site**

Run: `cargo build 2>&1 | grep "missing field"` to list them. Add `daily: None` to each `RepoStatus { .. }` literal in `src/main.rs` (there are several, including the test helpers around lines 1935, 2009, 2124). `rebuild_repo_status` ends in `..(*old.clone())`, so it carries the old value forward automatically; leave it alone for now, Task 7 gives it a real value.

- [ ] **Step 7: Verify the whole suite and the lints**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: all pass.

- [ ] **Step 8: Commit (present the command, do not run it)**

```bash
git add src/git.rs src/model.rs src/main.rs && git commit -m "feat: probe per-day commit counts for the local calendar"
```

---

### Task 4: App state, focus, and the day cursor

**Files:**
- Modify: `src/app.rs`
- Test: `src/app.rs`

**Interfaces:**
- Consumes: `contributions::{Calendar, DailyCounts, Source, SLOTS}` from Task 1, `RepoStatus.daily` from Task 3.
- Produces: `app::ContributionsMode` (`Off`, `Github`, `Local`; `from_config(&str) -> Self`, `config_value(self) -> &'static str`, `label(self) -> &'static str`, `is_open(self) -> bool`, `source(self) -> Option<Source>`); `FocusedPanel::Contributions`; `App` fields `contributions: ContributionsMode`, `github_calendar: Option<Box<Calendar>>`, `contrib_slot: usize`; `App` methods `toggle_contributions()`, `cycle_contributions_source()`, `move_contrib_cursor(i32, NaiveDate)`, `contrib_cursor_ends(NaiveDate)`, `local_calendar(NaiveDate) -> Calendar`.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src/app.rs`:

```rust
    fn today() -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str("2026-08-18", "%Y-%m-%d").unwrap()
    }

    #[test]
    fn c_toggles_the_pane_and_remembers_the_source() {
        let mut app = test_app();
        assert_eq!(app.contributions, ContributionsMode::Off);
        app.toggle_contributions();
        assert_eq!(app.contributions, ContributionsMode::Github, "defaults to github");
        app.cycle_contributions_source();
        assert_eq!(app.contributions, ContributionsMode::Local);
        app.toggle_contributions();
        assert_eq!(app.contributions, ContributionsMode::Off);
        app.toggle_contributions();
        assert_eq!(app.contributions, ContributionsMode::Local, "reopens where it closed");
    }

    #[test]
    fn cycling_the_source_on_a_closed_pane_opens_it() {
        let mut app = test_app();
        app.cycle_contributions_source();
        assert!(app.contributions.is_open());
    }

    #[test]
    fn closing_the_pane_moves_focus_off_it() {
        let mut app = test_app();
        app.toggle_contributions();
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.toggle_contributions();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList, "focus cannot strand");
    }

    #[test]
    fn the_pane_is_only_focusable_while_open() {
        let mut app = test_app();
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::RepoList, "closed: refused");
        app.toggle_contributions();
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
    }

    /// Unlike the other three panels, this one exists in Compact view, so the
    /// Detail-only guard on `focus_panel` must not apply to it.
    #[test]
    fn the_pane_is_focusable_in_compact_view() {
        let mut app = test_app();
        app.view = View::Compact;
        app.toggle_contributions();
        app.focus_panel(FocusedPanel::Contributions);
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.focus_panel(FocusedPanel::Detail);
        assert_eq!(
            app.focused_panel,
            FocusedPanel::Contributions,
            "detail is still unreachable in compact"
        );
    }

    #[test]
    fn tab_includes_the_pane_only_while_open() {
        let mut app = test_app();
        app.view = View::Detail;
        app.toggle_panel_focus();
        app.toggle_panel_focus();
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList, "closed: three panels");

        app.toggle_contributions();
        app.focused_panel = FocusedPanel::Inspector;
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList);
    }

    #[test]
    fn tab_in_compact_toggles_between_the_list_and_the_pane() {
        let mut app = test_app();
        app.view = View::Compact;
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList, "closed: no-op");
        app.toggle_contributions();
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::Contributions);
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, FocusedPanel::RepoList);
    }

    #[test]
    fn the_cursor_starts_on_today_and_clamps_at_both_ends() {
        let mut app = test_app();
        let (first, last) = app.contrib_cursor_ends(today());
        assert_eq!(first, 0);
        assert_eq!(app.contrib_slot, last, "opens on today");

        app.move_contrib_cursor(5, today());
        assert_eq!(app.contrib_slot, last, "cannot walk into the future");

        app.move_contrib_cursor(-(crate::contributions::SLOTS as i32), today());
        assert_eq!(app.contrib_slot, 0, "cannot walk before the grid opens");

        app.move_contrib_cursor(-1, today());
        assert_eq!(app.contrib_slot, 0);
    }

    #[test]
    fn the_cursor_moves_by_a_week_at_a_time() {
        let mut app = test_app();
        let (_, last) = app.contrib_cursor_ends(today());
        app.move_contrib_cursor(-7, today());
        assert_eq!(app.contrib_slot, last - 7);
    }

    #[test]
    fn the_local_calendar_sums_only_the_visible_repos() {
        let mut app = test_app();
        let mut a = crate::contributions::DailyCounts::default();
        a.0[0] = 3;
        let mut b = crate::contributions::DailyCounts::default();
        b.0[0] = 4;
        app.repos = vec![
            {
                let mut r = test_repo();
                r.name = "alpha".into();
                r.daily = Some(Box::new(a));
                r
            },
            {
                let mut r = test_repo();
                r.name = "beta".into();
                r.daily = Some(Box::new(b));
                r
            },
        ];
        assert_eq!(app.local_calendar(today()).total(), 7, "both visible");

        app.search_query = "alpha".to_string();
        assert_eq!(
            app.local_calendar(today()).total(),
            3,
            "a search narrows the heatmap the same way it narrows the list"
        );
    }

    #[test]
    fn a_repo_with_no_histogram_contributes_nothing() {
        let mut app = test_app();
        app.repos = vec![test_repo()];
        assert_eq!(app.local_calendar(today()).total(), 0);
        assert_eq!(app.local_calendar(today()).source(), crate::contributions::Source::Local);
    }
```

If `test_app()` and `test_repo()` helpers do not already exist in `src/app.rs`'s test module, look for the equivalents already used by the neighbouring tests and reuse those names rather than inventing new ones.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib app::tests`
Expected: compile failure on `ContributionsMode`, `toggle_contributions`, and `FocusedPanel::Contributions`.

- [ ] **Step 3: Add the mode enum and the focus variant**

In `src/app.rs`:

```rust
/// Whether the contribution strip is showing, and from which source.
/// Persisted to `config.toml` as one key, so the source survives a restart
/// along with the open state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContributionsMode {
    #[default]
    Off,
    Github,
    Local,
}

impl ContributionsMode {
    pub fn from_config(value: &str) -> Self {
        match value {
            "github" => Self::Github,
            "local" => Self::Local,
            _ => Self::Off,
        }
    }

    pub fn config_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Github => "github",
            Self::Local => "local",
        }
    }

    /// Label for the settings row and the pane title.
    pub fn label(self) -> &'static str {
        self.config_value()
    }

    pub fn is_open(self) -> bool {
        self != Self::Off
    }

    pub fn source(self) -> Option<crate::contributions::Source> {
        match self {
            Self::Off => None,
            Self::Github => Some(crate::contributions::Source::Github),
            Self::Local => Some(crate::contributions::Source::Local),
        }
    }
}
```

Extend `FocusedPanel`:

```rust
    /// The contribution strip. Unlike the other three it exists in Compact
    /// view too, and only while the pane is open, so both `focus_panel` and
    /// `toggle_panel_focus` special-case it.
    Contributions,
```

- [ ] **Step 4: Add the App fields**

On `struct App`, after `inspector_pct`:

```rust
    /// Whether the contribution strip is open, and from which source.
    pub contributions: ContributionsMode,
    /// The last source the strip was on, so closing and reopening returns to
    /// it instead of always snapping back to github.
    last_contributions_source: ContributionsMode,
    /// The account-wide calendar from the last GitHub poll. None until the
    /// first poll lands, or when the payload predates the field.
    pub github_calendar: Option<Box<crate::contributions::Calendar>>,
    /// Grid slot the day cursor sits on. Meaningful only while the pane has
    /// focus; clamped against today on every move, since "today" advances
    /// under a long-running session.
    pub contrib_slot: usize,
```

Initialize in `App::new` (or wherever the struct is built): `contributions: ContributionsMode::Off`, `last_contributions_source: ContributionsMode::Github`, `github_calendar: None`, `contrib_slot: 0`. The cursor is clamped to today on first use, so 0 is a safe seed.

- [ ] **Step 5: Add the behaviour**

```rust
    /// `c`: open or close the strip. Reopening returns to whichever source it
    /// was last on.
    pub fn toggle_contributions(&mut self) {
        if self.contributions.is_open() {
            self.last_contributions_source = self.contributions;
            self.contributions = ContributionsMode::Off;
            // Focus cannot be left pointing at a panel that is no longer on
            // screen: every key routed to it would silently do nothing.
            if self.focused_panel == FocusedPanel::Contributions {
                self.focused_panel = FocusedPanel::RepoList;
            }
        } else {
            self.open_contributions(self.last_contributions_source);
        }
        self.needs_redraw = true;
    }

    /// Open the strip on `mode` and put the day cursor back on today. Shared
    /// by `c` and `C` so both entry points land in the same state.
    fn open_contributions(&mut self, mode: ContributionsMode) {
        self.contributions = mode;
        self.last_contributions_source = mode;
        let today = chrono::Local::now().date_naive();
        self.contrib_slot = self.contrib_cursor_ends(today).1;
    }

    /// `C`: switch which source fills the strip, opening it if it was closed.
    /// Opening from `Off` lands on the remembered source rather than always
    /// snapping to github, so `c` and `C` agree about where "open" means.
    pub fn cycle_contributions_source(&mut self) {
        let next = match self.contributions {
            ContributionsMode::Off => self.last_contributions_source,
            ContributionsMode::Github => ContributionsMode::Local,
            ContributionsMode::Local => ContributionsMode::Github,
        };
        self.open_contributions(next);
        self.needs_redraw = true;
    }

    /// First and last selectable grid slots for `today`.
    pub fn contrib_cursor_ends(&self, today: chrono::NaiveDate) -> (usize, usize) {
        let calendar = crate::contributions::Calendar::new(today, crate::contributions::Source::Local);
        (0, calendar.today_slot())
    }

    /// Move the day cursor by `delta` days (7 for a week), clamped to the
    /// grid. Takes `today` rather than reading the clock so the clamp is
    /// testable and so a session open past midnight follows the date.
    pub fn move_contrib_cursor(&mut self, delta: i32, today: chrono::NaiveDate) {
        let (first, last) = self.contrib_cursor_ends(today);
        let current = self.contrib_slot.clamp(first, last) as i32;
        self.contrib_slot = (current + delta).clamp(first as i32, last as i32) as usize;
        self.needs_redraw = true;
    }

    /// The local calendar: every visible repo's histogram summed. Pure
    /// arithmetic over data the sweep already gathered, so this is safe to
    /// call on every frame and every keystroke.
    pub fn local_calendar(&self, today: chrono::NaiveDate) -> crate::contributions::Calendar {
        let mut calendar =
            crate::contributions::Calendar::new(today, crate::contributions::Source::Local);
        for (_, repo) in self.visible_repos() {
            if let Some(daily) = &repo.daily {
                calendar.add_daily(daily);
            }
        }
        calendar
    }
```

`App::new` seeds `contrib_slot` the same way `open_contributions` does, so the cursor is on today from the very first frame rather than on the oldest day of the year:

```rust
    contrib_slot: crate::contributions::Calendar::new(
        chrono::Local::now().date_naive(),
        crate::contributions::Source::Local,
    )
    .today_slot(),
```

This replaces the `contrib_slot: 0` seed named in Step 4.

- [ ] **Step 6: Update the focus functions**

```rust
    pub fn toggle_panel_focus(&mut self) {
        let strip = self.contributions.is_open();
        if self.view != View::Detail {
            // Compact has one grid, but the strip is still a real target.
            if strip {
                self.focused_panel = match self.focused_panel {
                    FocusedPanel::Contributions => FocusedPanel::RepoList,
                    _ => FocusedPanel::Contributions,
                };
            }
            return;
        }
        self.focused_panel = match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Detail,
            FocusedPanel::Detail => FocusedPanel::Inspector,
            FocusedPanel::Inspector if strip => FocusedPanel::Contributions,
            FocusedPanel::Inspector | FocusedPanel::Contributions => FocusedPanel::RepoList,
        };
    }

    pub fn focus_panel(&mut self, panel: FocusedPanel) {
        // The strip is the one panel that exists in both views, and it is
        // only a target while it is open.
        if panel == FocusedPanel::Contributions {
            if self.contributions.is_open() {
                self.focused_panel = panel;
            }
            return;
        }
        if self.view != View::Detail {
            return;
        }
        self.focused_panel = panel;
    }
```

Also add a `FocusedPanel::Contributions` arm anywhere the compiler reports a non-exhaustive match (`scroll_focused`, the j/k routing in `main.rs`, and any badge or title helper). Where scrolling has no meaning for the strip, the arm is `{}`.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --lib app::tests`
Expected: all eleven new tests PASS.

- [ ] **Step 8: Verify the whole suite and the lints**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: all pass.

- [ ] **Step 9: Commit (present the command, do not run it)**

```bash
git add src/app.rs && git commit -m "feat: add contributions pane state and day cursor"
```

---

### Task 5: The layout split and PanelRects

**Files:**
- Create: `src/ui/contributions.rs` (layout half only; rendering lands in Task 6)
- Modify: `src/ui/mod.rs` (`PanelRects`, `panel_rects`, `render`), `src/ui/colors.rs`, `CLAUDE.md`
- Test: `src/ui/contributions.rs`, `src/ui/mod.rs`

**Interfaces:**
- Consumes: `App.contributions` from Task 4.
- Produces: `ui::contributions::CONTRIB_HEIGHT: u16`; `ui::contributions::split_main_area(Rect, bool) -> (Rect, Option<Rect>)`; `PanelRects.contributions: Option<Rect>`; `colors::CONTRIB_LADDER: [Color; 5]`.

- [ ] **Step 1: Write the failing tests**

Create `src/ui/contributions.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn area(height: u16) -> Rect {
        Rect::new(0, 1, 120, height)
    }

    #[test]
    fn a_closed_pane_leaves_the_main_area_alone() {
        let (main, strip) = split_main_area(area(40), false);
        assert_eq!(main, area(40));
        assert!(strip.is_none());
    }

    #[test]
    fn an_open_pane_takes_a_fixed_strip_off_the_bottom() {
        let (main, strip) = split_main_area(area(40), true);
        let strip = strip.expect("room for the strip");
        assert_eq!(strip.height, CONTRIB_HEIGHT);
        assert_eq!(
            strip.y + strip.height,
            area(40).y + area(40).height,
            "docked to the bottom edge"
        );
        assert_eq!(main.y, area(40).y, "the panes above keep the top edge");
        assert!(main.height < 40, "the strip costs the panes rows");
        assert_eq!(
            main.y + main.height + 1,
            strip.y,
            "one blank row between them, like the other splits"
        );
    }

    /// The dashboard outranks the heatmap: on a short terminal the strip is
    /// dropped rather than squeezing the panes into slivers.
    #[test]
    fn a_short_area_suppresses_the_strip_entirely() {
        assert!(split_main_area(area(22), true).1.is_none());
        assert!(split_main_area(area(23), true).1.is_some());
    }
}
```

Add `use ratatui::layout::{Constraint, Layout, Rect};` at the top of the file so the tests compile.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib ui::contributions`
Expected: compile failure, `cannot find function split_main_area`. Remember to add `pub mod contributions;` to `src/ui/mod.rs` first, or the file is not compiled at all and the test simply does not exist.

- [ ] **Step 3: Write the implementation**

At the top of `src/ui/contributions.rs`:

```rust
//! The contribution strip: a year of activity docked under the main panes.

use ratatui::layout::{Constraint, Layout, Rect};

/// Rows the strip occupies: title, month labels, seven day rows, legend.
/// Fixed, because the content height is fixed: a resize handle here could
/// only ever add empty space.
pub const CONTRIB_HEIGHT: u16 = 10;

/// Rows the panes above must keep for the strip to be worth docking. Below
/// this the strip is dropped: a dashboard squeezed into slivers is worse
/// than no heatmap.
const MAIN_MIN_HEIGHT: u16 = 12;

/// Carve the strip off the bottom of the main area.
///
/// Single source of truth for that geometry, in the same sense as
/// `detail::split_main` and `detail::split_right_column`: `ui::render` draws
/// from it and `ui::panel_rects` hit-tests against it. It lives here rather
/// than in `detail.rs` because the strip exists in Compact view too.
pub fn split_main_area(main: Rect, open: bool) -> (Rect, Option<Rect>) {
    if !open || main.height < MAIN_MIN_HEIGHT + CONTRIB_HEIGHT + 1 {
        return (main, None);
    }
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(CONTRIB_HEIGHT)])
        .spacing(1)
        .split(main);
    (v[0], Some(v[1]))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib ui::contributions`
Expected: three tests PASS.

- [ ] **Step 5: Add the color ladder**

In `src/ui/colors.rs`:

```rust
/// Contribution intensity, empty through busiest. Step 0 is `SURFACE`, the
/// same near-invisible tone the dividers use, so an empty day reads as
/// structure rather than as data. Step 4 is `GREEN`, the palette's existing
/// "ahead / synced" green, so the busiest day matches the brightest green
/// already on screen. The three between are interpolated in RGB.
///
/// One ladder per concept: the cells and the Less/More legend both read this,
/// so they cannot drift apart.
pub const CONTRIB_LADDER: [Color; 5] = [
    Color::Rgb(69, 71, 90),
    Color::Rgb(93, 110, 108),
    Color::Rgb(117, 149, 125),
    Color::Rgb(141, 188, 143),
    Color::Rgb(166, 227, 161),
];
```

- [ ] **Step 6: Write the PanelRects parity test**

Add to the `mod tests` block in `src/ui/mod.rs`:

```rust
    #[test]
    fn panel_rects_reports_the_strip_only_when_it_is_drawn() {
        let area = Rect::new(0, 0, 120, 44);
        let mut app = /* the existing test-app constructor used by neighbouring tests */;

        let closed = panel_rects(area, &app);
        assert!(closed.contributions.is_none());

        app.contributions = crate::app::ContributionsMode::Github;
        let open = panel_rects(area, &app);
        let strip = open.contributions.expect("strip present");
        assert_eq!(strip.height, contributions::CONTRIB_HEIGHT);
        assert!(
            open.repo_list.height < closed.repo_list.height,
            "the panes above give up the rows the strip takes"
        );
        assert_eq!(
            open.at(strip.x + 2, strip.y + 2),
            Some(crate::app::FocusedPanel::Contributions),
            "a click in the strip lands on the strip"
        );
    }

    /// Compact has no detail or inspector, but it does have the strip.
    #[test]
    fn the_strip_is_laid_out_in_compact_view_too() {
        let area = Rect::new(0, 0, 120, 44);
        let mut app = /* same constructor */;
        app.view = crate::app::View::Compact;
        app.contributions = crate::app::ContributionsMode::Local;
        let rects = panel_rects(area, &app);
        assert!(rects.contributions.is_some());
        assert!(rects.detail.is_none());
        assert!(rects.repo_list.height < area.height - contributions::CONTRIB_HEIGHT);
    }
```

Replace the two `/* ... */` placeholders with whatever constructor the existing tests in that module already use for an `App`. If `src/ui/mod.rs` has no test module yet, create one and build the `App` the same way `src/app.rs`'s tests do.

- [ ] **Step 7: Wire the split into panel_rects and render**

In `src/ui/mod.rs`, add `contributions: Option<Rect>` to `PanelRects`, and extend `PanelRects::at` before the `None` fallback:

```rust
        if self.contributions.is_some_and(|r| r.contains(p)) {
            return Some(FocusedPanel::Contributions);
        }
```

Rewrite `panel_rects` so both views carve the strip first:

```rust
pub fn panel_rects(area: Rect, app: &App) -> PanelRects {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);
    // The strip comes off the bottom before anything else is laid out, so
    // both views shrink by the same amount and neither has its own copy of
    // the arithmetic.
    let (main, contributions) =
        contributions::split_main_area(chunks[1], app.contributions.is_open());

    match app.view {
        View::Compact => PanelRects {
            main,
            repo_list: main,
            detail: None,
            inspector: None,
            contributions,
        },
        View::Detail => {
            let (repo_list, right) = detail::split_main(main, app.repo_list_pct);
            let (detail, inspector) = detail::split_right_column(right, app.inspector_pct);
            PanelRects {
                main,
                repo_list,
                detail: Some(detail),
                inspector,
                contributions,
            }
        }
    }
}
```

In `render`, apply the identical split before dispatching to the view:

```rust
    let (main, strip) = contributions::split_main_area(chunks[1], app.contributions.is_open());

    match app.view {
        View::Detail => detail::render(frame, main, app),
        View::Compact => compact::render(frame, main, app),
    }
    if let Some(strip) = strip {
        contributions::render(frame, strip, app);
    }
```

`contributions::render` does not exist yet. For this task, add a placeholder that draws the outline only, so the geometry is verifiable by eye:

```rust
/// Draw the strip. Content lands in the next task; this establishes the
/// frame so the split can be checked against a running terminal.
pub fn render(frame: &mut ratatui::Frame, area: Rect, _app: &crate::app::App) {
    use ratatui::widgets::{Block, Borders};
    frame.render_widget(Block::default().borders(Borders::TOP), area);
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --lib ui`
Expected: the new parity tests PASS, existing `ui` tests still pass.

- [ ] **Step 9: Update CLAUDE.md**

Under the `panel_rects` invariant, extend the existing paragraph. Add this sentence after the sentence naming `detail::split_right_column`:

```
The contribution strip is carved off the bottom by
`ui::contributions::split_main_area`, called by both for the same reason; it
lives in its own module rather than beside the other two because it applies
to Compact view as well.
```

- [ ] **Step 10: Verify the whole suite and the lints**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: all pass.

- [ ] **Step 11: Commit (present the command, do not run it)**

```bash
git add src/ui/ CLAUDE.md && git commit -m "feat: dock a contributions strip under the main panes"
```

---

### Task 6: Rendering the grid

**Files:**
- Modify: `src/ui/contributions.rs`
- Test: `src/ui/contributions.rs`

**Interfaces:**
- Consumes: `Calendar` from Task 1, `App` state from Task 4, `split_main_area` and `CONTRIB_LADDER` from Task 5.
- Produces: `ui::contributions::Tier` (`Wide`, `Dense`, `Trimmed(usize)`, with `cell_width() -> u16` and `weeks_shown() -> usize`); `ui::contributions::tier(u16) -> Tier`; `ui::contributions::title(&Calendar, Tier, Option<usize>) -> String`; `ui::contributions::thousands(u32) -> String`; `ui::contributions::slot_at(Rect, Tier, u16, u16) -> Option<usize>`; a real `render`.

- [ ] **Step 1: Write the failing tests**

Add to the test module in `src/ui/contributions.rs`:

```rust
    use crate::contributions::{Calendar, Source, WEEKS};

    fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// A year is 53 columns. Wide spends two cells on each and drops the gap
    /// after the last one; dense spends one. Both sit behind a 4-column day
    /// gutter. Getting these off by one silently crops the oldest week.
    #[test]
    fn tiers_switch_at_the_widths_they_actually_need() {
        assert!(matches!(tier(109), Tier::Wide));
        assert!(matches!(tier(108), Tier::Dense), "one column short of wide");
        assert!(matches!(tier(57), Tier::Dense));
        assert!(matches!(tier(56), Tier::Trimmed(52)), "one short of dense");
        assert!(matches!(tier(30), Tier::Trimmed(26)));
        assert!(matches!(tier(4), Tier::Trimmed(0)));
        assert!(matches!(tier(0), Tier::Trimmed(0)), "never underflows");
    }

    #[test]
    fn tier_cell_widths_and_week_counts_agree() {
        assert_eq!(tier(120).cell_width(), 2);
        assert_eq!(tier(120).weeks_shown(), WEEKS);
        assert_eq!(tier(60).cell_width(), 1);
        assert_eq!(tier(60).weeks_shown(), WEEKS);
        assert_eq!(tier(30).weeks_shown(), 26);
    }

    #[test]
    fn thousands_groups_the_total_the_way_github_does() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(2_516), "2,516");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn the_title_names_the_source_and_the_span() {
        let mut cal = Calendar::new(day("2026-08-18"), Source::Github);
        cal.add(day("2026-08-18"), 3);
        assert_eq!(
            title(&cal, tier(120), None),
            "3 contributions in the last year (github)"
        );
    }

    /// A trimmed grid must not be presented as a full year: the title says
    /// how much is actually on screen.
    #[test]
    fn a_trimmed_title_names_the_weeks_it_can_draw() {
        let cal = Calendar::new(day("2026-08-18"), Source::Local);
        assert_eq!(
            title(&cal, tier(30), None),
            "0 contributions in the last 26 weeks (local)"
        );
    }

    #[test]
    fn a_focused_title_reads_out_the_selected_day() {
        let mut cal = Calendar::new(day("2026-08-18"), Source::Local);
        cal.add(day("2025-08-19"), 7);
        assert_eq!(title(&cal, tier(120), Some(2)), "7 contributions on Tue 19 Aug");
    }

    #[test]
    fn a_focused_title_says_none_rather_than_zero_contributions() {
        let cal = Calendar::new(day("2026-08-18"), Source::Local);
        assert_eq!(title(&cal, tier(120), Some(2)), "no contributions on Tue 19 Aug");
    }

    #[test]
    fn a_click_maps_back_to_the_slot_under_it() {
        let area = Rect::new(0, 5, 120, CONTRIB_HEIGHT);
        let t = tier(area.width);
        // Row 0 is the title, row 1 the month labels, rows 2..9 the days.
        // Column 4 is the first week, and each week is two cells wide.
        assert_eq!(slot_at(area, t, 4, 7), Some(0), "first Sunday");
        assert_eq!(slot_at(area, t, 4, 8), Some(1), "the Monday under it");
        assert_eq!(slot_at(area, t, 6, 7), Some(7), "next week's Sunday");
        assert_eq!(slot_at(area, t, 5, 7), None, "the gap between columns");
        assert_eq!(slot_at(area, t, 2, 7), None, "the day gutter");
        assert_eq!(slot_at(area, t, 4, 5), None, "the title row");
        assert_eq!(slot_at(area, t, 119, 7), None, "past the last week");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib ui::contributions`
Expected: compile failure on `tier`, `title`, `thousands`, `slot_at`.

- [ ] **Step 3: Write the tier and title helpers**

```rust
/// Columns reserved for the Mon/Wed/Fri gutter.
const GUTTER: u16 = 4;

/// Row offsets inside the strip.
const TITLE_ROW: u16 = 0;
const MONTH_ROW: u16 = 1;
const FIRST_DAY_ROW: u16 = 2;
const LEGEND_ROW: u16 = 9;

/// How much room each week column gets, chosen from the strip's own width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Cell plus a gap, the closest to GitHub's separated squares.
    Wide,
    /// One cell per week: a solid ribbon, but the whole year fits.
    Dense,
    /// Dense, and only the newest N weeks fit at all.
    Trimmed(usize),
}

impl Tier {
    pub fn cell_width(self) -> u16 {
        match self {
            Self::Wide => 2,
            Self::Dense | Self::Trimmed(_) => 1,
        }
    }

    pub fn weeks_shown(self) -> usize {
        match self {
            Self::Wide | Self::Dense => WEEKS,
            Self::Trimmed(n) => n,
        }
    }
}

/// Pick a tier for a strip of this width. The wide tier drops the gap after
/// the final column, so it needs 109 rather than 110.
pub fn tier(width: u16) -> Tier {
    let year = WEEKS as u16;
    if width >= GUTTER + year * 2 - 1 {
        Tier::Wide
    } else if width >= GUTTER + year {
        Tier::Dense
    } else {
        Tier::Trimmed(width.saturating_sub(GUTTER) as usize)
    }
}

/// Group a count with commas, the way the GitHub header does.
pub fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The strip's first line. Focused, it reads out the selected day; otherwise
/// it reports the span actually drawn and which source filled it, because the
/// two sources legitimately disagree.
pub fn title(calendar: &Calendar, tier: Tier, cursor: Option<usize>) -> String {
    if let Some(slot) = cursor
        && let Some((date, count)) = calendar.day_at(slot / 7, slot % 7)
    {
        let when = date.format("%a %-d %b");
        return if count == 0 {
            format!("no contributions on {when}")
        } else {
            format!("{} contributions on {when}", thousands(count))
        };
    }
    let span = match tier {
        Tier::Wide | Tier::Dense => "in the last year".to_string(),
        Tier::Trimmed(n) => format!("in the last {n} weeks"),
    };
    format!(
        "{} contributions {span} ({})",
        thousands(calendar.total()),
        calendar.source().label()
    )
}
```

Add `use crate::contributions::{Calendar, WEEKS};` to the module header.

Note on `%-d`: the dash-flag is glibc/BSD strftime and chrono supports it. If a test fails on the day format, switch to building the string from `date.day()` and a month-name lookup rather than fighting the formatter.

- [ ] **Step 4: Write the click hit-test**

```rust
/// The grid slot under a screen cell, or None for the gutter, the gaps, the
/// label rows, and anything past the last drawn week.
pub fn slot_at(area: Rect, tier: Tier, col: u16, row: u16) -> Option<usize> {
    let weekday = row.checked_sub(area.y + FIRST_DAY_ROW)?;
    if weekday >= 7 {
        return None;
    }
    let offset = col.checked_sub(area.x + GUTTER)?;
    let cell = tier.cell_width();
    if cell == 2 && offset % 2 == 1 {
        return None; // the gap between two columns is not a day
    }
    let drawn = offset / cell;
    let shown = tier.weeks_shown();
    if drawn as usize >= shown {
        return None;
    }
    // A trimmed grid shows the NEWEST weeks, so column 0 is not week 0.
    let week = WEEKS - shown + drawn as usize;
    Some(week * 7 + weekday as usize)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib ui::contributions`
Expected: all tests PASS.

- [ ] **Step 6: Write the renderer**

Replace the placeholder `render`:

```rust
use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, FocusedPanel};
use crate::ui::colors;

/// The cell glyph. Not a Nerd Font codepoint: this one is plain Unicode, so
/// the strip is the one part of the UI that reads correctly without one.
const CELL: &str = "■";

/// Draw the strip: title, month labels, seven day rows, legend.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let today = chrono::Local::now().date_naive();
    let Some(calendar) = calendar_for(app, today) else {
        return;
    };
    let tier = tier(area.width);
    let focused = app.focused_panel == FocusedPanel::Contributions;
    let cursor = focused.then(|| app.contrib_slot.min(calendar.today_slot()));

    let mut lines = Vec::with_capacity(CONTRIB_HEIGHT as usize);
    lines.push(Line::from(Span::styled(
        title(&calendar, tier, cursor),
        Style::default()
            .fg(if focused { colors::TEXT } else { colors::SUBTEXT })
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(month_line(&calendar, tier));
    for weekday in 0..7 {
        lines.push(day_line(&calendar, tier, weekday, cursor));
    }
    lines.push(legend_line());

    frame.render_widget(Paragraph::new(lines), area);
}

/// Which calendar the strip is showing. Borrowed for the GitHub source so a
/// 1.5KB clone does not happen every frame; owned for the local one, which
/// is summed from the visible repos on demand.
fn calendar_for(app: &App, today: chrono::NaiveDate) -> Option<std::borrow::Cow<'_, Calendar>> {
    use crate::app::ContributionsMode;
    use std::borrow::Cow;
    match app.contributions {
        ContributionsMode::Off => None,
        ContributionsMode::Github => app.github_calendar.as_deref().map(Cow::Borrowed),
        ContributionsMode::Local => Some(Cow::Owned(app.local_calendar(today))),
    }
}

/// The first drawn week, given that a trimmed grid shows the newest ones.
fn first_week(tier: Tier) -> usize {
    WEEKS - tier.weeks_shown()
}

fn month_line(calendar: &Calendar, tier: Tier) -> Line<'static> {
    let cell = tier.cell_width() as usize;
    let start = first_week(tier);
    let mut row = " ".repeat(GUTTER as usize);
    for (week, name) in calendar.month_labels() {
        if week < start {
            continue;
        }
        let at = GUTTER as usize + (week - start) * cell;
        // Skip a label that would overwrite the previous one or run off the
        // end: a half-drawn month name is noise, not information.
        if at < row.chars().count() || at + name.len() > GUTTER as usize + tier.weeks_shown() * cell
        {
            continue;
        }
        row.push_str(&" ".repeat(at - row.chars().count()));
        row.push_str(name);
    }
    Line::from(Span::styled(row, Style::default().fg(colors::OVERLAY)))
}

fn day_line(
    calendar: &Calendar,
    tier: Tier,
    weekday: usize,
    cursor: Option<usize>,
) -> Line<'static> {
    // Only three of the seven rows are labelled, as GitHub does: seven
    // labels is a wall of text beside a picture.
    let label = match weekday {
        1 => "Mon ",
        3 => "Wed ",
        5 => "Fri ",
        _ => "    ",
    };
    let mut spans = vec![Span::styled(
        label.to_string(),
        Style::default().fg(colors::OVERLAY),
    )];
    let start = first_week(tier);
    for week in start..start + tier.weeks_shown() {
        let slot = week * 7 + weekday;
        let Some((_, count)) = calendar.day_at(week, weekday) else {
            // Past today: hold the column open so the grid keeps its shape.
            spans.push(Span::raw(" ".repeat(tier.cell_width() as usize)));
            continue;
        };
        let mut style =
            Style::default().fg(colors::CONTRIB_LADDER[calendar.intensity(count) as usize]);
        if cursor == Some(slot) {
            style = style.add_modifier(Modifier::REVERSED);
        }
        spans.push(Span::styled(CELL, style));
        if tier.cell_width() == 2 {
            spans.push(Span::raw(" "));
        }
    }
    Line::from(spans)
}

fn legend_line() -> Line<'static> {
    let mut spans = vec![Span::styled(
        "    Less ".to_string(),
        Style::default().fg(colors::OVERLAY),
    )];
    for shade in colors::CONTRIB_LADDER {
        spans.push(Span::styled(CELL, Style::default().fg(shade)));
    }
    spans.push(Span::styled(
        " More".to_string(),
        Style::default().fg(colors::OVERLAY),
    ));
    Line::from(spans)
}
```

Note: `LEGEND_ROW` is declared for `slot_at`'s benefit and for anyone reading the row map. If clippy flags it as unused, keep it and add a `#[allow(dead_code)]` with a comment saying it documents the row layout, or delete it and fold the comment into `FIRST_DAY_ROW`. Prefer deleting.

- [ ] **Step 7: Run the tests and look at it**

Run: `cargo test --lib ui::contributions && cargo run`
Expected: tests pass. In the app, press `c` and confirm: a strip appears at the bottom, the year is drawn, the total matches what github.com says, and the legend reads Less to More. Resize the terminal narrow and confirm the tier changes and the title says "in the last N weeks".

- [ ] **Step 8: Verify the whole suite and the lints**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: all pass.

- [ ] **Step 9: Commit (present the command, do not run it)**

```bash
git add src/ui/contributions.rs && git commit -m "feat: render the contribution grid"
```

---

### Task 7: Key routing, mouse, and sweep plumbing

**Files:**
- Modify: `src/main.rs`
- Modify: `README.md`, `src/ui/help.rs`
- Test: `src/main.rs`

**Interfaces:**
- Consumes: everything from Tasks 1 through 6.
- Produces: no new public API. The keys `c`, `C`, `4`, and the cursor movement keys are live; the sweep fills `RepoStatus.daily`.

- [ ] **Step 1: Add the key arms**

In `handle_normal_input` in `src/main.rs`. The guarded arms must come before the unguarded `j`/`k` arms, and `h`/`l` are currently unbound in normal mode (verify with a search before adding, in case that changed):

```rust
        KeyCode::Char('c') => app.toggle_contributions(),
        KeyCode::Char('C') => {
            app.cycle_contributions_source();
            // Switching to the local source is useless without a histogram,
            // and the sweep is the only thing that fills one. Ask for one
            // rather than showing an empty year and letting it look broken.
            if app.contributions == app::ContributionsMode::Local
                && app.repos.iter().all(|r| r.daily.is_none())
            {
                app.set_status("Sweeping for local history…".to_string(), false);
                start_sweep(app, tx, false, args.author.clone(), true);
            }
        }
        KeyCode::Char('4') => app.focus_panel(FocusedPanel::Contributions),
        KeyCode::Char('h') | KeyCode::Left
            if app.focused_panel == FocusedPanel::Contributions =>
        {
            app.move_contrib_cursor(-1, chrono::Local::now().date_naive())
        }
        KeyCode::Char('l') | KeyCode::Right
            if app.focused_panel == FocusedPanel::Contributions =>
        {
            app.move_contrib_cursor(1, chrono::Local::now().date_naive())
        }
        KeyCode::Home if app.focused_panel == FocusedPanel::Contributions => {
            app.move_contrib_cursor(-(contributions::SLOTS as i32), chrono::Local::now().date_naive())
        }
        KeyCode::End if app.focused_panel == FocusedPanel::Contributions => {
            app.move_contrib_cursor(contributions::SLOTS as i32, chrono::Local::now().date_naive())
        }
```

Extend the existing `j`/`k` match arms with the new focus target:

```rust
        KeyCode::Char('j') | KeyCode::Down => match app.focused_panel {
            FocusedPanel::RepoList => app.move_down(),
            FocusedPanel::Detail => app.detail_nav_down(),
            FocusedPanel::Inspector => app.scroll_inspector(1),
            FocusedPanel::Contributions => {
                app.move_contrib_cursor(7, chrono::Local::now().date_naive())
            }
        },
```

and the mirror for `k` with `-7`.

The existing `KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL)` arm (ctrl-c quits) must stay ABOVE the new plain `c` arm, or ctrl-c will toggle the pane instead of quitting. Check this explicitly.

- [ ] **Step 2: Write the ctrl-c regression test**

Add to the `mod tests` block in `src/main.rs`:

```rust
    /// `c` toggles the strip and ctrl-c quits. Match arms are ordered, so an
    /// arm added above the modifier check silently breaks the quit key.
    #[test]
    fn ctrl_c_still_quits_after_c_was_bound() {
        let source = include_str!("main.rs");
        let ctrl_c = source
            .find("KeyCode::Char('c') if key.modifiers")
            .expect("ctrl-c arm present");
        let plain_c = source
            .find("KeyCode::Char('c') => app.toggle_contributions()")
            .expect("toggle arm present");
        assert!(
            ctrl_c < plain_c,
            "the ctrl-c arm must be matched before the plain c arm"
        );
    }
```

- [ ] **Step 3: Run it to verify it fails, then passes**

Run: `cargo test --lib ctrl_c_still_quits`
Expected: FAIL before step 1's arms are ordered correctly, PASS after. If it passes immediately, deliberately move the plain `c` arm above the ctrl-c arm once, confirm the test fails, then move it back.

- [ ] **Step 4: Wire the mouse**

At `src/main.rs:680` where `ui::panel_rects` is already called for click routing, add day selection after the existing focus assignment:

```rust
    // A click inside the strip both focuses it and picks the day under the
    // cursor, so the readout answers what you actually pointed at.
    if let Some(strip) = rects.contributions
        && strip.contains(ratatui::layout::Position { x: col, y: row })
        && let Some(slot) = ui::contributions::slot_at(
            strip,
            ui::contributions::tier(strip.width),
            col,
            row,
        )
    {
        app.contrib_slot = slot;
        app.needs_redraw = true;
    }
```

Adjust the variable names to whatever the surrounding code calls the column and row.

- [ ] **Step 5: Store the GitHub calendar when a poll lands**

At the `AppEvent::Github` arm, replace the Task 2 placeholder:

```rust
            // The calendar rides the same payload as the repos, so a poll
            // that updated CI also refreshed the year at no extra cost.
            app.github_calendar = fetch.calendar;
```

Do the same on the `AppEvent::Loaded` path, from `LoadResult.github_calendar`, guarding against `None` on a `WindowApply` (a pure re-filter must not blank a calendar it never fetched):

```rust
            if let Some(calendar) = result.github_calendar {
                app.github_calendar = Some(calendar);
            }
```

- [ ] **Step 6: Fill the histogram during the sweep**

Thread two values into the sweep. `start_sweep` and `spawn_sweep` each take two new parameters:

```rust
    want_daily: bool,
    contrib_author: Option<String>,
```

Pass `app.contributions == ContributionsMode::Local` and `config.contributions_author.clone()` (the config key lands in Task 8; until then pass `None` and leave a comment). In `rebuild_repo_status`, extend the expensive tier:

```rust
    // The local calendar is a year of log per repo, so it is gated twice:
    // only when the strip is actually showing it, and only when the WorkKey
    // says this repo moved. An idle fleet costs nothing.
    let daily = if !want_daily {
        old.daily.clone()
    } else if unchanged && old.daily.is_some() {
        old.daily.clone()
    } else {
        let author = match contrib_author {
            Some(a) => Some(a.to_string()),
            None => git::user_email(path).await,
        };
        match author {
            Some(author) => git::daily_commits(path, contributions::grid_start(today), &author)
                .await
                .map(Box::new),
            // No identity configured: better to show nothing than to show
            // everyone's commits as if they were yours.
            None => None,
        }
    };
```

`today` here is `chrono::Local::now().date_naive()`, computed once in `spawn_sweep` and passed down, so every repo in one sweep shares a grid. Add `daily` to the `RepoStatus { .. }` literal that `rebuild_repo_status` returns, above the `..(*old.clone())` line.

- [ ] **Step 7: Write the gating test**

Add to `src/main.rs`'s test module:

```rust
    /// The local histogram is the most expensive probe in the sweep: a year
    /// of log per repo. It must not run for a fleet whose strip is closed.
    #[test]
    fn the_daily_probe_is_skipped_when_the_strip_is_closed() {
        let mut app = /* the existing test-app constructor */;
        assert_eq!(app.contributions, app::ContributionsMode::Off);
        assert!(!want_daily(&app), "closed strip: no year-long log");
        app.contributions = app::ContributionsMode::Github;
        assert!(!want_daily(&app), "github source needs no local log");
        app.contributions = app::ContributionsMode::Local;
        assert!(want_daily(&app));
    }
```

and extract the predicate so it is testable:

```rust
/// Whether this sweep should pay for the per-repo commit histogram.
fn want_daily(app: &App) -> bool {
    app.contributions == app::ContributionsMode::Local
}
```

Use `want_daily(app)` at both `start_sweep` call sites.

- [ ] **Step 8: Update the docs**

In `README.md`'s keybindings table, after the `v` row:

```
| `c` | Toggle the contributions strip (a year of activity, GitHub-style) |
| `C` | Switch its source: GitHub's own calendar, or your commits in the listed repos |
| `4` | Focus the contributions strip (only while it is open) |
```

and inside the `j`/`k` row's description add `· Contributions: move the day cursor (h/l by day, j/k by week)`.

In `src/ui/help.rs`'s `keybinds` array, after the `v` entry:

```rust
        ("c", "Toggle the contributions strip (a year of activity)"),
        ("C", "Switch its source: github calendar ↔ your commits here"),
```

and change the `1 / 2 / 3` entry's text to `"Focus repo list / detail / inspector / contributions (Tab cycles)"` with the key column reading `"1 / 2 / 3 / 4"`.

- [ ] **Step 9: Verify by hand**

Run: `cargo run`
Check, in order: `c` opens the strip; `C` switches to local and triggers a sweep; typing `/` and a repo name reshapes the local heatmap with no lag; `4` focuses it and `h`/`l`/`j`/`k` walk the cursor with the title reading out each day; clicking a cell selects it; ctrl-c still quits; `v` into Compact still shows the strip.

- [ ] **Step 10: Verify the whole suite and the lints**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: all pass.

- [ ] **Step 11: Commit (present the command, do not run it)**

```bash
git add src/main.rs src/ui/help.rs README.md && git commit -m "feat: wire the contributions strip to keys, mouse, and sweeps"
```

---

### Task 8: Config key and settings row

**Files:**
- Modify: `src/config.rs`, `src/app.rs`, `src/main.rs`, `README.md`
- Test: `src/config.rs`, `src/app.rs`

**Interfaces:**
- Consumes: `ContributionsMode` from Task 4.
- Produces: `Config.contributions: Option<String>`, `Config.contributions_author: Option<String>`, `SettingField::Contributions`.

- [ ] **Step 1: Write the failing tests**

In `src/config.rs`'s test module:

```rust
    #[test]
    fn contributions_defaults_to_off_and_round_trips() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.contributions, None);
        assert_eq!(
            crate::app::ContributionsMode::from_config(cfg.contributions.as_deref().unwrap_or("off")),
            crate::app::ContributionsMode::Off
        );

        let cfg: Config = toml::from_str(r#"contributions = "local""#).unwrap();
        assert_eq!(
            crate::app::ContributionsMode::from_config(cfg.contributions.as_deref().unwrap()),
            crate::app::ContributionsMode::Local
        );
    }

    /// A typo must not be a crash, and must not silently mean "github".
    #[test]
    fn an_unknown_contributions_value_falls_back_to_off() {
        assert_eq!(
            crate::app::ContributionsMode::from_config("gitub"),
            crate::app::ContributionsMode::Off
        );
    }

    #[test]
    fn contributions_author_is_optional() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.contributions_author, None);
        let cfg: Config = toml::from_str(r#"contributions_author = "a@b\\|c@d""#).unwrap();
        assert_eq!(cfg.contributions_author.as_deref(), Some(r"a@b\|c@d"));
    }
```

In `src/app.rs`'s test module:

```rust
    #[test]
    fn the_settings_row_cycles_the_three_modes() {
        let mut overlay = /* the existing SettingsOverlay constructor, with
                             ContributionsMode::Off as the new argument */;
        overlay.cursor = SETTING_FIELDS
            .iter()
            .position(|f| *f == SettingField::Contributions)
            .unwrap();
        assert_eq!(overlay.value_label(SettingField::Contributions), "off");
        overlay.step(1);
        assert_eq!(overlay.value_label(SettingField::Contributions), "github");
        overlay.step(1);
        assert_eq!(overlay.value_label(SettingField::Contributions), "local");
        overlay.step(1);
        assert_eq!(overlay.value_label(SettingField::Contributions), "off", "wraps");
        overlay.step(-1);
        assert_eq!(overlay.value_label(SettingField::Contributions), "local", "wraps back");
    }

    #[test]
    fn the_settings_row_writes_the_contributions_key() {
        assert_eq!(SettingField::Contributions.config_key(), "contributions");
        assert_eq!(SettingField::Contributions.applies(), Applies::Live);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib config::tests && cargo test --lib app::tests::the_settings_row`
Expected: compile failures on the missing fields and variant.

- [ ] **Step 3: Add the config keys**

In `src/config.rs`, on `struct Config`:

```rust
    /// Whether the contribution strip starts open, and from which source:
    /// "off", "github", or "local". `c` and `C` toggle it live; this is what
    /// a fresh launch starts from.
    #[serde(default)]
    pub contributions: Option<String>,
    /// Git author regex the local contribution source filters by, overriding
    /// each repo's own `user.email`. `--author` takes a regex, so several
    /// identities can be joined: `me@work.com\|me@home.com`.
    #[serde(default)]
    pub contributions_author: Option<String>,
```

Both are `Option<String>` and `#[serde(default)]`, so no `Default` impl change is needed beyond whatever `#[derive(Default)]` already covers. If `Config` implements `Default` by hand, add both as `None`.

- [ ] **Step 4: Add the settings field**

In `src/app.rs`: add `Contributions` to `SettingField`, extend `SETTING_FIELDS` from `[SettingField; 7]` to `[SettingField; 8]` with `SettingField::Contributions` appended, and add arms to `label` (`"contributions"`), `config_key` (`"contributions"`), and `applies` (`Applies::Live`). It is not `numeric`.

On `SettingsOverlay`: add `pub contributions: ContributionsMode`, take it as a new argument in `new` (the `#[allow(clippy::too_many_arguments)]` is already there), return `self.contributions.label().to_string()` from `value_label` for this field, and in `step` cycle it:

```rust
            SettingField::Contributions => {
                self.contributions = match (self.contributions, delta >= 0) {
                    (ContributionsMode::Off, true) => ContributionsMode::Github,
                    (ContributionsMode::Github, true) => ContributionsMode::Local,
                    (ContributionsMode::Local, true) => ContributionsMode::Off,
                    (ContributionsMode::Off, false) => ContributionsMode::Local,
                    (ContributionsMode::Github, false) => ContributionsMode::Off,
                    (ContributionsMode::Local, false) => ContributionsMode::Github,
                };
            }
```

In the save path, emit `SettingValue::Str(self.contributions.config_value())`.

- [ ] **Step 5: Apply the config at startup and on save**

Where `App` is built in `src/main.rs`, set `contributions: app::ContributionsMode::from_config(config.contributions.as_deref().unwrap_or("off"))`. Where the settings overlay is opened, pass `app.contributions`. Where Enter applies a saved overlay (the `Applies::Live` path), assign `app.contributions = overlay.contributions`.

Replace the Task 7 placeholder: `start_sweep` now receives `config.contributions_author.clone()` instead of `None`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test`
Expected: all pass, including the two new settings tests and the three config tests.

- [ ] **Step 7: Document the config keys**

In `README.md`'s config block, after the `persist_pane_sizes` entry:

```toml
# Contribution strip: "off" (default), "github" (your GitHub calendar), or
# "local" (your commits in the repos currently listed). `c` toggles it and
# `C` switches source; this is what a fresh launch starts from.
contributions = "off"

# Git author regex the "local" source filters by, overriding each repo's own
# user.email. --author takes a regex, so several identities can be joined.
contributions_author = "me@work.com\\|me@home.com"
```

Also add a `,` settings-row mention: the existing README line for `,` lists the rows, so append `contributions` to it.

- [ ] **Step 8: Verify by hand**

Run: `cargo run`, press `,`, move to the contributions row, cycle it to `local`, press Enter, quit, relaunch. Expected: the strip comes back open on the local source, and `~/.config/fleetz/config.toml` gained `contributions = "local"` with every comment in the file intact.

- [ ] **Step 9: Verify the whole suite and the lints**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: all pass.

- [ ] **Step 10: Commit (present the command, do not run it)**

```bash
git add src/config.rs src/app.rs src/main.rs README.md && git commit -m "feat: persist the contributions strip in config and settings"
```

---

### Task 9: Final verification

**Files:** none changed unless something below fails.

- [ ] **Step 1: Full suite, lints, formatting**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: clean.

- [ ] **Step 2: Confirm the GitHub query is still one call at one point**

Run:

```bash
gh api graphql -f query='{ rateLimit { cost remaining nodeCount } }'
```

Note `remaining`, launch `cargo run` with a cache-bypassing `R`, then run the same command again. Expected: `remaining` dropped by 1, not 2. If it dropped by more, the calendar became its own request and the invariant is violated.

- [ ] **Step 3: Confirm the cache still serves an old payload**

Run: `ls ~/.cache/fleetz/` and confirm the filename is unchanged from before this feature (`github-<limit>-<prs>.json`). Then launch offline (turn off wifi) and confirm fleetz still starts from cache with the strip showing "0 contributions" rather than crashing.

- [ ] **Step 4: Walk the invariants in CLAUDE.md**

Read each "Domain invariants" entry and confirm this feature respects it. Specifically: `panel_rects` matches `render` (one shared helper), `visible_repos` order is untouched, discovery pass order is untouched, one color ladder per concept (`CONTRIB_LADDER` is the only source for intensity), status vocabulary is untouched, batch on GraphQL (a field, not a request), and the cache was checked before widening the fetch.

- [ ] **Step 5: Check the docs match the code**

Confirm `README.md`'s keybinding table lists `c`, `C`, `4`; the config block lists `contributions` and `contributions_author`; `src/ui/help.rs` lists `c` and `C`; and `CLAUDE.md` names `ui::contributions::split_main_area` under the parity invariant.

- [ ] **Step 6: Report**

Summarize for the human: what shipped, the three spec deviations listed at the top of this plan, the measured rate-limit result from step 2, and anything that turned out differently from the plan.

## Self-Review

**Spec coverage:** Every spec section maps to a task. Module boundary and grid model: Task 1. GitHub source including the serde-default trap and the unchanged cache key: Task 2. Local source including `--all`, the `WorkKey` gate, and per-repo identity: Tasks 3 and 7. Layout including the shared helper and the suppression floor: Task 5. Rendering including the three tiers, the ladder, month labels and the legend: Task 6. Interaction including all seven keys, the two title forms, and click selection: Tasks 4 and 7. Config and settings: Task 8. The spec's full testing list is distributed across the tasks that own each behavior, and every item in it appears in exactly one task.

**Placeholders:** Four places deliberately say "the existing test-app constructor" rather than naming it, because the constructor's name has to be read out of the file at implementation time. Every one of them is marked in a comment at the point of use. No step says TBD, "handle errors appropriately", or "similar to Task N".

**Type consistency:** `DailyCounts` is a tuple newtype accessed as `.0` in Tasks 1, 3, 4 and 7. `Calendar::day_at(week, weekday)` takes two arguments everywhere, including in `title` where the slot is split as `slot / 7, slot % 7`. `ContributionsMode` is the app-level enum and `contributions::Source` the model-level one, converted only through `ContributionsMode::source()`. `fetch_repos` returns `GithubFetch` from Task 2 onward, and `AppEvent::Github` carries `Box<GithubFetch>` in every later reference. `split_main_area(Rect, bool)` and `tier(u16)` keep the same signatures in Tasks 5, 6 and 7.

**One known ordering hazard**, called out where it bites: `RepoStatus.daily` (Task 3) must exist before Task 4's `local_calendar` compiles, and Task 5's `pub mod contributions;` must be added before its own tests can run at all.
