# Time Window Selector Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An in-TUI modal overlay (key `t`) that changes fleetz's activity time window live (slot/hours/days/weeks/months/years), applies instantly by re-filtering the cached GitHub dataset, and remembers the last choice across launches.

**Architecture:** Split `load_repos` into `fetch_dataset` (GitHub call + cache) and `assemble_view` (filter + discovery + stat, with optional git fetch). `App` holds the unfiltered dataset and the active `TimeWindow`. The selector is a modal overlay like the existing branches overlay; applying runs `assemble_view` over the in-memory dataset with no git fetch. The window persists to `window.toml` next to the API cache.

**Tech Stack:** Rust, ratatui, crossterm, chrono, serde + toml, tokio. All already in Cargo.toml; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-07-13-time-window-selector-design.md`

## Global Constraints

- Repo: `/Users/at-a/Repos/fleetz`. All paths below are relative to it.
- No new dependencies. serde, toml, chrono, dirs are already direct deps.
- Conventional Commits, first line under 72 chars, NO `Co-Authored-By` lines.
- Git: stage with `git add`, then PRESENT the commit command for the user to run or approve. Do not run `git commit` without the user's approval for this execution.
- Never use em-dashes or en-dashes anywhere (code, comments, docs, commit messages). Use hyphens, colons, or parentheses.
- Comments explain why, not what. Match the existing comment style (fleetz comments are prose-heavy and intent-focused).
- Every task must leave the crate compiling and `cargo test` green.
- Run commands from the repo root: `cd /Users/at-a/Repos/fleetz`.

---

### Task 1: TimeWindow model in time.rs

**Files:**
- Modify: `src/time.rs` (add types after the existing `TimeSlot` impl, around line 30)

**Interfaces:**
- Consumes: existing `auto_cutoff(work_start, work_end) -> (DateTime<Utc>, String)` in the same file.
- Produces (later tasks rely on these exact names):
  - `pub enum WindowUnit { Slot, Hours, Days, Weeks, Months, Years }` (Copy, Eq, serde lowercase)
  - `pub struct TimeWindow { pub n: u8, pub unit: WindowUnit }` (Copy, Eq, serde, Default = `{ n: 1, unit: Slot }`)
  - `pub const WINDOW_N_MIN: u8 = 1;` / `pub const WINDOW_N_MAX: u8 = 31;`
  - `TimeWindow::clamped(n: u8, unit: WindowUnit) -> TimeWindow`
  - `TimeWindow::cutoff(&self, work_start: u8, work_end: u8) -> DateTime<Utc>`
  - `TimeWindow::cutoff_from(&self, now: DateTime<Utc>, work_start: u8, work_end: u8) -> DateTime<Utc>` (deterministic core, `cutoff` calls it with `Utc::now()`)
  - `TimeWindow::label(&self, work_start: u8, work_end: u8) -> String`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `src/time.rs`:

```rust
    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn window_cutoff_hours_days_weeks() {
        let now = utc("2026-07-13T12:00:00Z");
        let w = TimeWindow { n: 6, unit: WindowUnit::Hours };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2026-07-13T06:00:00Z"));
        let w = TimeWindow { n: 3, unit: WindowUnit::Days };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2026-07-10T12:00:00Z"));
        let w = TimeWindow { n: 2, unit: WindowUnit::Weeks };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2026-06-29T12:00:00Z"));
    }

    #[test]
    fn window_cutoff_months_is_calendar_aware() {
        // March 31 minus one month clamps to February's last day.
        let now = utc("2026-03-31T10:00:00Z");
        let w = TimeWindow { n: 1, unit: WindowUnit::Months };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2026-02-28T10:00:00Z"));
    }

    #[test]
    fn window_cutoff_years_via_months() {
        let now = utc("2026-07-13T12:00:00Z");
        let w = TimeWindow { n: 2, unit: WindowUnit::Years };
        assert_eq!(w.cutoff_from(now, 9, 18), utc("2024-07-13T12:00:00Z"));
    }

    #[test]
    fn window_cutoff_slot_matches_auto_cutoff() {
        let w = TimeWindow { n: 1, unit: WindowUnit::Slot };
        let (expected, _) = auto_cutoff(9, 18);
        let got = w.cutoff(9, 18);
        // Two separate now() reads: allow a small skew.
        assert!((got - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn window_clamped_bounds_n() {
        assert_eq!(TimeWindow::clamped(0, WindowUnit::Days).n, 1);
        assert_eq!(TimeWindow::clamped(99, WindowUnit::Days).n, 31);
        assert_eq!(TimeWindow::clamped(7, WindowUnit::Days).n, 7);
    }

    #[test]
    fn window_label_pluralizes() {
        assert_eq!(TimeWindow { n: 1, unit: WindowUnit::Days }.label(9, 18), "since 1 day");
        assert_eq!(TimeWindow { n: 2, unit: WindowUnit::Weeks }.label(9, 18), "since 2 weeks");
        assert_eq!(TimeWindow { n: 3, unit: WindowUnit::Months }.label(9, 18), "since 3 months");
    }

    #[test]
    fn window_label_slot_uses_auto_label() {
        let w = TimeWindow { n: 1, unit: WindowUnit::Slot };
        let (_, expected) = auto_cutoff(9, 18);
        assert_eq!(w.label(9, 18), expected);
    }

    #[test]
    fn window_default_is_one_slot() {
        let w = TimeWindow::default();
        assert_eq!(w.n, 1);
        assert_eq!(w.unit, WindowUnit::Slot);
    }

    #[test]
    fn window_unit_serde_roundtrip() {
        let w = TimeWindow { n: 2, unit: WindowUnit::Weeks };
        let s = toml::to_string(&w).unwrap();
        assert!(s.contains("n = 2"));
        assert!(s.contains("unit = \"weeks\""));
        let back: TimeWindow = toml::from_str(&s).unwrap();
        assert_eq!(back, w);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test window_ 2>&1 | tail -20`
Expected: compile errors ("cannot find type `TimeWindow`" etc).

- [ ] **Step 3: Implement the model**

In `src/time.rs`, change the first line's imports to include `Duration` and `Months`:

```rust
use chrono::{DateTime, Duration, Local, Months, NaiveDate, NaiveTime, TimeZone, Utc};
```

Add `use serde::{Deserialize, Serialize};` below it, and insert after the `TimeSlot` impl block:

```rust
/// Bounds for the user-adjustable window count. 0 would mean an empty
/// window, so the floor is 1; 31 covers "a month of days" and keeps the
/// spinner wrap cycle short.
pub const WINDOW_N_MIN: u8 = 1;
pub const WINDOW_N_MAX: u8 = 31;

/// Unit for the interactive time window. `Slot` reproduces the classic
/// auto mode (previous work/evening/night boundary); the rest are rolling
/// "now minus n units" windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowUnit {
    Slot,
    Hours,
    Days,
    Weeks,
    Months,
    Years,
}

/// The activity window selected in the TUI: show repos pushed within the
/// last `n` `unit`s. `n` is ignored when `unit` is `Slot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub n: u8,
    pub unit: WindowUnit,
}

impl Default for TimeWindow {
    fn default() -> Self {
        Self { n: 1, unit: WindowUnit::Slot }
    }
}

impl TimeWindow {
    /// Construct with `n` clamped into the valid range, so persisted or
    /// hand-edited values can never produce an empty or absurd window.
    pub fn clamped(n: u8, unit: WindowUnit) -> Self {
        Self { n: n.clamp(WINDOW_N_MIN, WINDOW_N_MAX), unit }
    }

    /// The cutoff instant for this window, evaluated against the real clock.
    pub fn cutoff(&self, work_start: u8, work_end: u8) -> DateTime<Utc> {
        self.cutoff_from(Utc::now(), work_start, work_end)
    }

    /// Deterministic core of `cutoff` (testable with a fixed `now`).
    /// Months/years use calendar-aware arithmetic and saturate rather than
    /// panic on impossible dates. `Slot` ignores `now`: it delegates to
    /// `auto_cutoff`, which reads the local clock itself.
    pub fn cutoff_from(&self, now: DateTime<Utc>, work_start: u8, work_end: u8) -> DateTime<Utc> {
        let n = self.n.clamp(WINDOW_N_MIN, WINDOW_N_MAX) as i64;
        match self.unit {
            WindowUnit::Slot => auto_cutoff(work_start, work_end).0,
            WindowUnit::Hours => now - Duration::hours(n),
            WindowUnit::Days => now - Duration::days(n),
            WindowUnit::Weeks => now - Duration::weeks(n),
            WindowUnit::Months => now
                .checked_sub_months(Months::new(n as u32))
                .unwrap_or(now),
            WindowUnit::Years => now
                .checked_sub_months(Months::new(n as u32 * 12))
                .unwrap_or(now),
        }
    }

    /// Header label: the slot label for `Slot`, otherwise "since 2 weeks".
    pub fn label(&self, work_start: u8, work_end: u8) -> String {
        let unit = match self.unit {
            WindowUnit::Slot => return auto_cutoff(work_start, work_end).1,
            WindowUnit::Hours => "hour",
            WindowUnit::Days => "day",
            WindowUnit::Weeks => "week",
            WindowUnit::Months => "month",
            WindowUnit::Years => "year",
        };
        let n = self.n.clamp(WINDOW_N_MIN, WINDOW_N_MAX);
        let s = if n == 1 { "" } else { "s" };
        format!("since {n} {unit}{s}")
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass, zero failures.

- [ ] **Step 5: Stage and present commit**

```bash
git add src/time.rs
```

Present: `git commit -m "feat: add TimeWindow model with slot/hours/days/weeks/months/years cutoffs"`

---

### Task 2: Window persistence (window_state.rs)

**Files:**
- Create: `src/window_state.rs`
- Modify: `src/main.rs:1-10` (add `mod window_state;` to the module list, alphabetical order: after `mod ui;`)

**Interfaces:**
- Consumes: `time::TimeWindow` (serde-capable, from Task 1).
- Produces:
  - `WindowState::new() -> WindowState` (default path: `<cache_dir>/fleetz/window.toml`, same dir as `ApiCache`)
  - `WindowState::at(path: PathBuf) -> WindowState` (test constructor)
  - `WindowState::load(&self) -> Option<TimeWindow>` (None on missing/corrupt; `n` clamped)
  - `WindowState::save(&self, window: &TimeWindow)` (creates parent dirs, ignores IO errors)

- [ ] **Step 1: Write the failing tests**

Create `src/window_state.rs`:

```rust
use std::path::PathBuf;

use crate::time::{TimeWindow, WindowUnit};

/// Persists the last applied time window so the next plain `fleetz` launch
/// reuses it ("remember last choice"). Lives next to the API cache rather
/// than the config file: it's machine-local state, not user configuration.
pub struct WindowState {
    path: PathBuf,
}

impl WindowState {
    pub fn new() -> Self {
        let dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("fleetz");
        Self { path: dir.join("window.toml") }
    }

    /// Test constructor with an explicit path.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Read the saved window. Missing or corrupt files fall back to None
    /// (caller uses the default); `n` is re-clamped in case the file was
    /// hand-edited.
    pub fn load(&self) -> Option<TimeWindow> {
        let content = std::fs::read_to_string(&self.path).ok()?;
        let w: TimeWindow = toml::from_str(&content).ok()?;
        Some(TimeWindow::clamped(w.n, w.unit))
    }

    /// Write the window. Errors are ignored on purpose: losing persistence
    /// must never break the running TUI.
    pub fn save(&self, window: &TimeWindow) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = toml::to_string(window) {
            let _ = std::fs::write(&self.path, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(name: &str) -> WindowState {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        WindowState::at(dir.join("window.toml"))
    }

    #[test]
    fn save_and_load_roundtrip() {
        let state = temp_state("fleetz_window_roundtrip");
        let w = TimeWindow { n: 2, unit: WindowUnit::Weeks };
        state.save(&w);
        assert_eq!(state.load(), Some(w));
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("fleetz_window_roundtrip"));
    }

    #[test]
    fn load_missing_file_is_none() {
        let state = temp_state("fleetz_window_missing");
        assert_eq!(state.load(), None);
    }

    #[test]
    fn load_corrupt_file_is_none() {
        let state = temp_state("fleetz_window_corrupt");
        state.save(&TimeWindow::default());
        std::fs::write(
            std::env::temp_dir().join("fleetz_window_corrupt").join("window.toml"),
            "not = [valid {{",
        )
        .unwrap();
        assert_eq!(state.load(), None);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("fleetz_window_corrupt"));
    }

    #[test]
    fn load_clamps_hand_edited_n() {
        let state = temp_state("fleetz_window_clamp");
        std::fs::create_dir_all(std::env::temp_dir().join("fleetz_window_clamp")).unwrap();
        std::fs::write(
            std::env::temp_dir().join("fleetz_window_clamp").join("window.toml"),
            "n = 99\nunit = \"days\"\n",
        )
        .unwrap();
        assert_eq!(
            state.load(),
            Some(TimeWindow { n: 31, unit: WindowUnit::Days })
        );
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("fleetz_window_clamp"));
    }
}
```

Note: implementation and tests land together here because the file is new and tiny; the test run below is the gate.

- [ ] **Step 2: Register the module**

In `src/main.rs`, the module list at the top becomes:

```rust
mod app;
mod cache;
mod cli;
mod config;
mod discovery;
mod git;
mod github;
mod model;
mod time;
mod ui;
mod window_state;
```

- [ ] **Step 3: Run tests**

Run: `cargo test window_state 2>&1 | tail -8`
Expected: 4 tests pass. Then `cargo test 2>&1 | tail -3` for the full suite: all pass. A `dead_code` warning for `WindowState` is expected until Task 6 wires it in; that is acceptable, but if the crate denies warnings, add `#[allow(dead_code)]` on the struct and REMOVE it in Task 6.

- [ ] **Step 4: Stage and present commit**

```bash
git add src/window_state.rs src/main.rs
```

Present: `git commit -m "feat: persist last time window to window.toml beside the API cache"`

---

### Task 3: Split load_repos into fetch_dataset + assemble_view

This task is a behavior-preserving refactor plus the dataset/window plumbing. No UI yet.

**Files:**
- Modify: `src/main.rs` (the `load_repos` function, `main`, `reload_all`)
- Modify: `src/github.rs` (add `window_exceeds_dataset` helper + tests)
- Modify: `src/app.rs` (App struct: add `dataset`, `window`, `dirty_mode`, `window_apply_requested` fields)

**Interfaces:**
- Consumes: `time::TimeWindow` (Task 1), `window_state::WindowState` (Task 2).
- Produces:
  - `github::window_exceeds_dataset(dataset: &[GithubRepo], cutoff: DateTime<Utc>, repo_limit: u32) -> bool`
  - `fn resolve_cutoff(window: Option<time::TimeWindow>, mode: &Mode, config: &config::Config) -> Result<(Option<chrono::DateTime<chrono::Utc>>, String), String>` in main.rs
  - `async fn fetch_dataset(config: &config::Config, args: &Args, cache: &cache::ApiCache, bypass_cache: bool) -> Result<Vec<github::GithubRepo>, String>` in main.rs
  - `async fn assemble_view(dataset: &[github::GithubRepo], cutoff: Option<chrono::DateTime<chrono::Utc>>, header: String, args: &Args, config: &config::Config, repo_dir: &Path, git_fetch: bool) -> Result<(Vec<RepoStatus>, String), Box<dyn std::error::Error>>` in main.rs
  - `load_repos` now takes an extra `window: Option<time::TimeWindow>` parameter and returns `(Vec<RepoStatus>, String, Vec<github::GithubRepo>)` (dataset is empty in Dirty mode)
  - App fields: `pub dataset: Vec<crate::github::GithubRepo>`, `pub window: Option<crate::time::TimeWindow>`, `pub dirty_mode: bool`, `pub window_apply_requested: bool` (all defaulted in `App::new`: empty/None/false/false)

- [ ] **Step 1: Write the failing test for the repo_limit helper**

Append to the `tests` module in `src/github.rs` (helpers `make_gh_repo` already exist there):

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test window_exceeds 2>&1 | tail -5`
Expected: compile error, `window_exceeds_dataset` not found.

- [ ] **Step 3: Implement the helper**

In `src/github.rs`, after `filter_before`:

```rust
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test window_exceeds 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: Add the App fields**

In `src/app.rs`, add to the end of the `App` struct (after `reload_requested`):

```rust
    /// The last unfiltered GitHub fetch (top repo_limit repos by push date).
    /// The time-window overlay re-filters this in memory, so applying a new
    /// window needs no network round trip.
    pub dataset: Vec<crate::github::GithubRepo>,
    /// The active time window. None means "follow the CLI flags", which is
    /// the state until the user opens the selector (or a saved window was
    /// loaded at startup).
    pub window: Option<crate::time::TimeWindow>,
    /// True when launched with -d: there is no GitHub dataset, so the
    /// time-window selector is unavailable.
    pub dirty_mode: bool,
    /// Set by Enter in the time-window overlay; the event loop consumes it
    /// to re-assemble the view (it needs args/config/repo_dir context the
    /// key handler doesn't have). Mirrors `reload_requested`.
    pub window_apply_requested: bool,
```

And in `App::new`, add to the struct literal:

```rust
            dataset: Vec::new(),
            window: None,
            dirty_mode: false,
            window_apply_requested: false,
```

- [ ] **Step 6: Refactor load_repos**

In `src/main.rs`, replace the body of `load_repos` (keep the Dirty-mode block as-is except its return) with the split. The new code, replacing everything from the `// Fetch from GitHub (with caching)` comment to the end of the function:

Signature changes first:

```rust
/// Load all repo data: GitHub API + local discovery + git status.
/// Returns the assembled view plus the unfiltered dataset so the TUI can
/// re-filter it live (time-window overlay) without re-fetching.
async fn load_repos(
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    mode: &Mode,
    window: Option<time::TimeWindow>,
    bypass_cache: bool,
) -> Result<(Vec<RepoStatus>, String, Vec<github::GithubRepo>), Box<dyn std::error::Error>> {
```

The Dirty-mode early return becomes (only the return line changes):

```rust
        return Ok((repos, header, Vec::new()));
```

Then the rest of the function body:

```rust
    // Fetch from GitHub (with caching)
    let api_cache = cache::ApiCache::new(config.cache_ttl);
    let dataset = fetch_dataset(config, args, &api_cache, bypass_cache).await?;

    let (cutoff, header) = resolve_cutoff(window, mode, config)?;
    let (repos, full_header) =
        assemble_view(&dataset, cutoff, header, args, config, repo_dir, true).await?;

    Ok((repos, full_header, dataset))
}

/// The GitHub GraphQL fetch (with disk cache), unfiltered: the top
/// repo_limit repos by push date.
async fn fetch_dataset(
    config: &config::Config,
    args: &Args,
    cache: &cache::ApiCache,
    bypass_cache: bool,
) -> Result<Vec<github::GithubRepo>, String> {
    github::fetch_repos(config.repo_limit, args.prs, cache, bypass_cache).await
}

/// Cutoff + header for the current view. An active TimeWindow (set by the
/// in-TUI selector or loaded from state) takes precedence over the launch
/// mode; Dirty mode never reaches here.
fn resolve_cutoff(
    window: Option<time::TimeWindow>,
    mode: &Mode,
    config: &config::Config,
) -> Result<(Option<chrono::DateTime<chrono::Utc>>, String), String> {
    if let Some(w) = window {
        return Ok((
            Some(w.cutoff(config.work_start, config.work_end)),
            w.label(config.work_start, config.work_end),
        ));
    }
    match mode {
        Mode::Auto => {
            let (c, h) = time::auto_cutoff(config.work_start, config.work_end);
            Ok((Some(c), h))
        }
        Mode::Since(s) => {
            let date = time::parse_relative_date(s)?;
            Ok((Some(time::date_to_utc_midnight(date)), format!("since {s}")))
        }
        Mode::Today => {
            let today = chrono::Local::now().date_naive();
            Ok((Some(time::date_to_utc_midnight(today)), "today".to_string()))
        }
        Mode::All => Ok((None, "all divergence".to_string())),
        Mode::Dirty => Err("dirty mode has no time cutoff".to_string()),
    }
}

/// Everything downstream of the GitHub fetch: time filtering, local
/// discovery, optional per-repo git fetch, statting, and header assembly.
/// `git_fetch: false` makes this purely local (used by the time-window
/// overlay so applying a window is instant).
async fn assemble_view(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    git_fetch: bool,
) -> Result<(Vec<RepoStatus>, String), Box<dyn std::error::Error>> {
```

Inside `assemble_view`, move the existing code verbatim from the old `load_repos` starting at the `kind_by_name` map down to the final header assembly, with these exact adaptations:

1. `kind_by_name` builds from `dataset` instead of `gh_repos`:

```rust
    let kind_by_name: HashMap<String, model::RepoKind> = dataset
        .iter()
        .map(|r| (r.name.to_lowercase(), r.repo_kind))
        .collect();
```

2. Delete the old `let (cutoff, header) = match mode { ... }` block (moved to `resolve_cutoff`). Keep the `--before` block as-is.

3. The active-filter block becomes:

```rust
    // Filter active repos
    let mut active: Vec<github::GithubRepo> = dataset.to_vec();
    if let Some(c) = cutoff {
        active = github::filter_since(&active, c);
    }
    if let Some(b) = before_cutoff {
        active = github::filter_before(&active, b);
    }
```

4. The parallel git fetch block gets gated:

```rust
    if git_fetch {
        let fetch_handles: Vec<_> = all_found
            .iter()
            .map(|f| {
                let path = f.path.clone();
                tokio::spawn(async move { git::fetch(&path).await })
            })
            .collect();

        for handle in fetch_handles {
            let _ = handle.await;
        }
    }
```

5. The header assembly at the bottom gains the repo_limit warning and returns:

```rust
    // Build header with --before and --author if present
    let mut full_header = header;
    if let Some(ref b) = args.before {
        full_header = format!("{full_header} -> before {b}");
    }
    if let Some(ref a) = args.author {
        full_header = format!("{full_header} (author: {a})");
    }
    // The dataset only reaches back repo_limit repos; flag a window that
    // outruns it so an incomplete view never masquerades as complete.
    if let Some(c) = cutoff {
        if github::window_exceeds_dataset(dataset, c, config.repo_limit) {
            full_header = format!(
                "{full_header} (window exceeds fetched repos, repo_limit={})",
                config.repo_limit
            );
        }
    }

    Ok((repos, full_header))
}
```

Note the existing `->` arrow in the before-suffix: the current code uses a Unicode arrow character. Keep whatever character is already there; do not introduce a new one.

`build_repo_status` is unchanged and is called with `&active` exactly as before.

- [ ] **Step 7: Wire main() and reload_all to the new signature**

In `main()`:

```rust
    let mode = args.mode();
    // "Remember last choice": only a plain launch (no explicit time-mode
    // flags) adopts the saved window; flags are one-off overrides that
    // neither read nor write it.
    let initial_window = if mode == Mode::Auto && args.before.is_none() {
        window_state::WindowState::new().load()
    } else {
        None
    };
    let (repos, header, dataset) =
        load_repos(&args, &config, &repo_dir, &mode, initial_window, false).await?;

    let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut app = App::new(repos, header, args.prs, args.stat, term_width, term_height);
    app.watch_mode = args.watch;
    app.refresh_interval = config.refresh_interval;
    app.dataset = dataset;
    app.window = initial_window;
    app.dirty_mode = mode == Mode::Dirty;
```

In `reload_all`, the load call becomes (note: `r` must respect the active window, not silently reset to the launch flags):

```rust
    let mode = args.mode();
    match load_repos(args, config, repo_dir, &mode, app.window, true).await {
        Ok((repos, header, dataset)) => {
            app.repos = repos;
            app.dataset = dataset;
            apply_groups(&mut app.repos, config);
            app.header = header;
            app.clamp_after_reload();
            app.last_refreshed = Some(chrono::Local::now());
            app.set_status("Refreshed".to_string(), false);
        }
        Err(e) => app.set_status(format!("Refresh failed: {e}"), true),
    }
```

`Mode` needs `PartialEq` for `mode == Mode::Auto`; it already derives it (`src/cli.rs:47`).

- [ ] **Step 8: Add a resolve_cutoff test**

At the bottom of `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_cutoff_window_overrides_mode() {
        let config = config::Config::default();
        let w = time::TimeWindow { n: 2, unit: time::WindowUnit::Weeks };
        let (cutoff, header) = resolve_cutoff(Some(w), &Mode::All, &config).unwrap();
        assert!(cutoff.is_some());
        assert_eq!(header, "since 2 weeks");
    }

    #[test]
    fn resolve_cutoff_all_mode_has_no_cutoff() {
        let config = config::Config::default();
        let (cutoff, header) = resolve_cutoff(None, &Mode::All, &config).unwrap();
        assert!(cutoff.is_none());
        assert_eq!(header, "all divergence");
    }

    #[test]
    fn resolve_cutoff_dirty_mode_is_error() {
        let config = config::Config::default();
        assert!(resolve_cutoff(None, &Mode::Dirty, &config).is_err());
    }
}
```

- [ ] **Step 9: Full verification**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass (including every pre-existing test: this refactor is behavior-preserving).
Run: `cargo clippy 2>&1 | tail -5`
Expected: no new warnings beyond any pre-existing ones. `window_apply_requested` and `dirty_mode` may warn as unused until Task 6; if so, that is acceptable for this task.

- [ ] **Step 10: Stage and present commit**

```bash
git add src/main.rs src/github.rs src/app.rs
```

Present: `git commit -m "refactor: split load_repos into fetch_dataset and assemble_view"`

---

### Task 4: TimeWindowOverlay state machine

**Files:**
- Modify: `src/app.rs` (new overlay variant + struct + methods + tests)
- Modify: `src/main.rs` (stub match arms so the crate keeps compiling)
- Modify: `src/ui/mod.rs` (stub match arm)

**Interfaces:**
- Consumes: `time::{TimeWindow, WindowUnit, WINDOW_N_MIN, WINDOW_N_MAX}`.
- Produces:
  - `pub enum WindowField { Number, Unit }` (Copy, Eq)
  - `pub struct TimeWindowOverlay { pub n: u8, pub unit: WindowUnit, pub field: WindowField, digits: String }`
  - Methods: `new(current: TimeWindow)`, `toggle_field()`, `step_up()`, `step_down()`, `push_digit(d: u8)`, `window() -> TimeWindow`
  - `Overlay::TimeWindow(TimeWindowOverlay)` variant
  - `App::open_time_window(&mut self)`
- Semantics (Task 5 renders these, Task 6 binds keys to them):
  - k/Up = `step_up` (number +1 wrapping 31 to 1; unit cycles backward), j/Down = `step_down` (number -1 wrapping 1 to 31; unit cycles forward)
  - Unit order: Slot, Hours, Days, Weeks, Months, Years
  - When unit is Slot, focus is forced to the Unit field and digits are ignored

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `src/app.rs`:

```rust
    use crate::time::{TimeWindow, WindowUnit};

    #[test]
    fn window_overlay_opens_on_number_field() {
        let o = TimeWindowOverlay::new(TimeWindow { n: 2, unit: WindowUnit::Weeks });
        assert_eq!(o.field, WindowField::Number);
        assert_eq!(o.n, 2);
    }

    #[test]
    fn window_overlay_slot_forces_unit_field() {
        let o = TimeWindowOverlay::new(TimeWindow::default());
        assert_eq!(o.field, WindowField::Unit);
        let mut o = o;
        o.toggle_field();
        assert_eq!(o.field, WindowField::Unit); // can't focus the dimmed number
    }

    #[test]
    fn window_overlay_toggle_field_flips() {
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 1, unit: WindowUnit::Days });
        assert_eq!(o.field, WindowField::Number);
        o.toggle_field();
        assert_eq!(o.field, WindowField::Unit);
        o.toggle_field();
        assert_eq!(o.field, WindowField::Number);
    }

    #[test]
    fn window_overlay_number_wraps_both_ways() {
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 31, unit: WindowUnit::Days });
        o.step_up();
        assert_eq!(o.n, 1);
        o.step_down();
        assert_eq!(o.n, 31);
    }

    #[test]
    fn window_overlay_unit_cycles_forward_and_back() {
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 1, unit: WindowUnit::Days });
        o.toggle_field(); // focus unit
        o.step_down(); // Days -> Weeks
        assert_eq!(o.unit, WindowUnit::Weeks);
        o.step_up(); // Weeks -> Days
        assert_eq!(o.unit, WindowUnit::Days);
    }

    #[test]
    fn window_overlay_unit_wraps_and_slot_grabs_focus() {
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 1, unit: WindowUnit::Years });
        o.toggle_field(); // focus unit
        o.step_down(); // Years wraps to Slot
        assert_eq!(o.unit, WindowUnit::Slot);
        assert_eq!(o.field, WindowField::Unit);
    }

    #[test]
    fn window_overlay_digit_entry_two_digit_and_clamp() {
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 1, unit: WindowUnit::Days });
        o.push_digit(2);
        assert_eq!(o.n, 2);
        o.push_digit(9);
        assert_eq!(o.n, 29); // "2" then "9" = 29
        o.push_digit(5);
        assert_eq!(o.n, 5); // third digit starts a fresh entry
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 1, unit: WindowUnit::Days });
        o.push_digit(3);
        o.push_digit(5);
        assert_eq!(o.n, 31); // 35 clamps to max
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 5, unit: WindowUnit::Days });
        o.push_digit(0);
        assert_eq!(o.n, 1); // 0 clamps to min
    }

    #[test]
    fn window_overlay_digits_ignored_on_unit_field() {
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 4, unit: WindowUnit::Days });
        o.toggle_field();
        o.push_digit(9);
        assert_eq!(o.n, 4);
    }

    #[test]
    fn window_overlay_window_returns_clamped() {
        let o = TimeWindowOverlay::new(TimeWindow { n: 3, unit: WindowUnit::Months });
        assert_eq!(o.window(), TimeWindow { n: 3, unit: WindowUnit::Months });
    }

    #[test]
    fn open_time_window_prefills_from_app_window() {
        let mut app = make_app(1);
        app.window = Some(TimeWindow { n: 2, unit: WindowUnit::Weeks });
        app.open_time_window();
        match &app.overlay {
            Overlay::TimeWindow(o) => {
                assert_eq!(o.n, 2);
                assert_eq!(o.unit, WindowUnit::Weeks);
            }
            _ => panic!("expected time window overlay"),
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test window_overlay 2>&1 | tail -5`
Expected: compile errors (`TimeWindowOverlay` not found).

- [ ] **Step 3: Implement the overlay state machine**

In `src/app.rs`, add the import at the top:

```rust
use crate::time::{TimeWindow, WindowUnit, WINDOW_N_MAX, WINDOW_N_MIN};
```

(The tests module then drops its own `use crate::time::...` line if it duplicates; keep exactly one import.)

Extend the `Overlay` enum:

```rust
/// Overlay shown on top of the main view.
#[derive(Debug, Clone)]
pub enum Overlay {
    None,
    Help,
    Branches(BranchOverlay),
    TimeWindow(TimeWindowOverlay),
}
```

Add after the `BranchEntry` impl:

```rust
/// Which field of the time-window spinner has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowField {
    Number,
    Unit,
}

/// Cycle order for the unit field.
const WINDOW_UNITS: [WindowUnit; 6] = [
    WindowUnit::Slot,
    WindowUnit::Hours,
    WindowUnit::Days,
    WindowUnit::Weeks,
    WindowUnit::Months,
    WindowUnit::Years,
];

/// State for the time-window overlay: a two-field spinner (count + unit).
/// Fully modal -- all input routes here while open, so its hjkl bindings
/// can't collide with list navigation.
#[derive(Debug, Clone)]
pub struct TimeWindowOverlay {
    pub n: u8,
    pub unit: WindowUnit,
    pub field: WindowField,
    /// Rolling buffer of the last typed digits, so "2" then "9" reads as
    /// 29 rather than two separate entries.
    digits: String,
}

impl TimeWindowOverlay {
    pub fn new(current: TimeWindow) -> Self {
        // Slot has no meaningful count, so focus lands on the unit field.
        let field = if current.unit == WindowUnit::Slot {
            WindowField::Unit
        } else {
            WindowField::Number
        };
        Self {
            n: current.n.clamp(WINDOW_N_MIN, WINDOW_N_MAX),
            unit: current.unit,
            field,
            digits: String::new(),
        }
    }

    pub fn toggle_field(&mut self) {
        self.digits.clear();
        // The number field is dimmed while Slot is selected; focus stays put.
        if self.unit == WindowUnit::Slot {
            self.field = WindowField::Unit;
            return;
        }
        self.field = match self.field {
            WindowField::Number => WindowField::Unit,
            WindowField::Unit => WindowField::Number,
        };
    }

    fn unit_index(&self) -> usize {
        WINDOW_UNITS.iter().position(|u| *u == self.unit).unwrap_or(0)
    }

    /// k / Up: number +1 (wrapping), or previous unit.
    pub fn step_up(&mut self) {
        self.digits.clear();
        match self.field {
            WindowField::Number => {
                self.n = if self.n >= WINDOW_N_MAX { WINDOW_N_MIN } else { self.n + 1 };
            }
            WindowField::Unit => {
                let len = WINDOW_UNITS.len();
                self.unit = WINDOW_UNITS[(self.unit_index() + len - 1) % len];
                self.after_unit_change();
            }
        }
    }

    /// j / Down: number -1 (wrapping), or next unit.
    pub fn step_down(&mut self) {
        self.digits.clear();
        match self.field {
            WindowField::Number => {
                self.n = if self.n <= WINDOW_N_MIN { WINDOW_N_MAX } else { self.n - 1 };
            }
            WindowField::Unit => {
                self.unit = WINDOW_UNITS[(self.unit_index() + 1) % WINDOW_UNITS.len()];
                self.after_unit_change();
            }
        }
    }

    fn after_unit_change(&mut self) {
        if self.unit == WindowUnit::Slot {
            self.field = WindowField::Unit;
        }
    }

    /// Typed digits set the count directly. Two digits combine ("2" "9" ->
    /// 29); a third starts over. Clamped so typed values can't escape the
    /// valid range.
    pub fn push_digit(&mut self, d: u8) {
        if self.field != WindowField::Number || self.unit == WindowUnit::Slot {
            return;
        }
        if self.digits.len() >= 2 {
            self.digits.clear();
        }
        self.digits.push((b'0' + d) as char);
        let parsed: u8 = self.digits.parse().unwrap_or(WINDOW_N_MIN);
        self.n = parsed.clamp(WINDOW_N_MIN, WINDOW_N_MAX);
    }

    /// The window this overlay currently describes.
    pub fn window(&self) -> TimeWindow {
        TimeWindow::clamped(self.n, self.unit)
    }
}
```

Add to `impl App` (near `toggle_help`):

```rust
    /// Open the time-window overlay, prefilled with the active window (or
    /// the default auto slot when none has been set).
    pub fn open_time_window(&mut self) {
        let current = self.window.unwrap_or_default();
        self.overlay = Overlay::TimeWindow(TimeWindowOverlay::new(current));
    }
```

- [ ] **Step 4: Add stub match arms so the crate compiles**

`src/main.rs`, in `run_event_loop`'s overlay match (real handling arrives in Task 6):

```rust
                        Overlay::TimeWindow(_) => {
                            if let KeyCode::Esc = key.code {
                                app.overlay = Overlay::None;
                            }
                            continue;
                        }
```

`src/ui/mod.rs`, in the overlay render match (real rendering arrives in Task 5):

```rust
        Overlay::TimeWindow(_) => {}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test 2>&1 | tail -5`
Expected: all pass, including the 10 new overlay tests.

- [ ] **Step 6: Stage and present commit**

```bash
git add src/app.rs src/main.rs src/ui/mod.rs
```

Present: `git commit -m "feat: time-window overlay state machine (spinner fields, wrap, digits)"`

---

### Task 5: Overlay rendering

**Files:**
- Create: `src/ui/timewindow.rs`
- Modify: `src/ui/mod.rs` (module declaration + replace the stub render arm)
- Modify: `src/ui/help.rs` (add the `t` keybinding line)

**Interfaces:**
- Consumes: `app::{TimeWindowOverlay, WindowField}`, `time::WindowUnit`, `ui::colors` (TEXT, OVERLAY, SURFACE, LAVENDER: same palette help.rs uses).
- Produces: `pub fn render(frame: &mut Frame, area: Rect, overlay: &TimeWindowOverlay)` in `ui::timewindow`.

Rendering is not unit-tested (matches the existing codebase: no ui render tests). The gate is compile + manual smoke test in Task 7.

- [ ] **Step 1: Create the renderer**

Create `src/ui/timewindow.rs`:

```rust
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{TimeWindowOverlay, WindowField};
use crate::time::WindowUnit;
use crate::ui::colors;

/// Render the time-window spinner overlay centered on screen.
pub fn render(frame: &mut Frame, area: Rect, overlay: &TimeWindowOverlay) {
    let popup_area = centered_fixed(48, 7, area);
    frame.render_widget(Clear, popup_area);

    let slot = overlay.unit == WindowUnit::Slot;

    let focused = Style::default()
        .fg(colors::LAVENDER)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    let idle = Style::default().fg(colors::TEXT);
    // The count is meaningless for Slot, so it renders dimmed and
    // unfocusable rather than disappearing (layout stays stable).
    let number_style = if slot {
        Style::default().fg(colors::SURFACE)
    } else if overlay.field == WindowField::Number {
        focused
    } else {
        idle
    };
    let unit_style = if overlay.field == WindowField::Unit { focused } else { idle };

    let unit_label = match overlay.unit {
        WindowUnit::Slot => "last slot",
        WindowUnit::Hours => "hours",
        WindowUnit::Days => "days",
        WindowUnit::Weeks => "weeks",
        WindowUnit::Months => "months",
        WindowUnit::Years => "years",
    };
    let number_label = if slot { " -".to_string() } else { format!("{:2}", overlay.n) };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("   since:  ", Style::default().fg(colors::OVERLAY)),
            Span::styled(format!("[ {number_label} ]"), number_style),
            Span::raw("  "),
            Span::styled(format!("[ {unit_label} ]"), unit_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "   h/l switch field · j/k adjust · 0-9 type",
            Style::default().fg(colors::SURFACE),
        )),
        Line::from(Span::styled(
            "   Enter apply · Esc cancel",
            Style::default().fg(colors::SURFACE),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::LAVENDER))
        .title(Span::styled(
            " Time window ",
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Paragraph::new(lines).block(block), popup_area);
}

/// Return a centered Rect of fixed `width` x `height` within `area`.
fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
```

- [ ] **Step 2: Wire it into ui/mod.rs**

Add `mod timewindow;` next to the other module declarations in `src/ui/mod.rs`, and replace the stub arm from Task 4:

```rust
        Overlay::TimeWindow(two) => timewindow::render(frame, area, two),
```

- [ ] **Step 3: Add the help line**

In `src/ui/help.rs`, insert into the `keybinds` array after the `("b / s", ...)` row:

```rust
        ("t", "Time window: pick how far back to show activity"),
```

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `cargo test 2>&1 | tail -5`
Expected: all pass, no new warnings from `cargo clippy 2>&1 | tail -5`.

- [ ] **Step 5: Stage and present commit**

```bash
git add src/ui/timewindow.rs src/ui/mod.rs src/ui/help.rs
```

Present: `git commit -m "feat: render time-window spinner overlay, document t key in help"`

---

### Task 6: Input handling, apply, and persistence wiring

**Files:**
- Modify: `src/main.rs` (`t` binding, overlay input handler, apply function, event-loop service)

**Interfaces:**
- Consumes: everything above: `TimeWindowOverlay` methods (Task 4), `assemble_view` + `apply_groups` (Task 3), `WindowState::save` (Task 2), `TimeWindow::{cutoff, label}` (Task 1).
- Produces: `fn handle_time_window_input(app: &mut App, key: KeyEvent)` and `async fn apply_time_window(app: &mut App, args: &Args, config: &config::Config, repo_dir: &Path)` in main.rs.

- [ ] **Step 1: Bind `t` in normal mode**

In `handle_normal_input` in `src/main.rs`, add near the `'b' | 's'` binding (`t` is currently unbound; verified against every existing match arm):

```rust
        // Time window overlay: pick how far back "activity" reaches. Not
        // available in -d mode, which never fetched a GitHub dataset.
        KeyCode::Char('t') => {
            if app.dirty_mode {
                app.set_status("Time window needs GitHub data (not in -d mode)".to_string(), true);
            } else {
                app.open_time_window();
            }
        }
```

- [ ] **Step 2: Replace the Task 4 stub with the real modal handler**

In `run_event_loop`, replace the `Overlay::TimeWindow(_) => { ... }` stub arm with:

```rust
                        Overlay::TimeWindow(_) => {
                            handle_time_window_input(app, key);
                            continue;
                        }
```

Add the handler function (near `handle_branch_input`):

```rust
/// Modal input for the time-window overlay. All keys are consumed here
/// while it's open, so hjkl/arrows can't leak into list navigation.
fn handle_time_window_input(app: &mut App, key: KeyEvent) {
    let Overlay::TimeWindow(ref mut two) = app.overlay else { return };
    match key.code {
        KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('q') => {
            app.overlay = Overlay::None;
        }
        KeyCode::Char('h') | KeyCode::Char('l') | KeyCode::Left | KeyCode::Right
        | KeyCode::Tab => two.toggle_field(),
        KeyCode::Char('k') | KeyCode::Up => two.step_up(),
        KeyCode::Char('j') | KeyCode::Down => two.step_down(),
        KeyCode::Char(c) if c.is_ascii_digit() => two.push_digit(c as u8 - b'0'),
        KeyCode::Enter => {
            let w = two.window();
            app.window = Some(w);
            app.window_apply_requested = true;
            app.overlay = Overlay::None;
        }
        _ => {}
    }
}
```

- [ ] **Step 3: Service the apply request in the event loop**

In `run_event_loop`, next to the existing `reload_requested` service block at the bottom of the loop:

```rust
        // Applying a window is a pure re-filter of the in-memory dataset
        // (no GitHub call, no git fetch), serviced here for the same reason
        // as reload_requested: it needs args/config/repo_dir.
        if app.window_apply_requested {
            app.window_apply_requested = false;
            apply_time_window(app, args, config, repo_dir).await;
        }
```

- [ ] **Step 4: Implement apply_time_window**

Add near `reload_all`:

```rust
/// Re-assemble the view for the window the overlay just applied: re-filter
/// the cached dataset, rediscover and re-stat local clones (no git fetch,
/// so this is instant), persist the choice, and refresh the header.
async fn apply_time_window(
    app: &mut App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
) {
    let Some(w) = app.window else { return };
    let cutoff = w.cutoff(config.work_start, config.work_end);
    let label = w.label(config.work_start, config.work_end);
    match assemble_view(&app.dataset, Some(cutoff), label, args, config, repo_dir, false).await {
        Ok((repos, header)) => {
            app.repos = repos;
            apply_groups(&mut app.repos, config);
            app.header = header;
            app.clamp_after_reload();
            window_state::WindowState::new().save(&w);
            app.set_status(
                format!("Window: {}", w.label(config.work_start, config.work_end)),
                false,
            );
        }
        Err(e) => app.set_status(format!("Window apply failed: {e}"), true),
    }
}
```

If Task 2 added `#[allow(dead_code)]` on `WindowState`, remove it now.

- [ ] **Step 5: Verify**

Run: `cargo test 2>&1 | tail -5` and `cargo clippy 2>&1 | tail -5`
Expected: all tests pass; no dead-code warnings remain for window_state, dirty_mode, or window_apply_requested.

- [ ] **Step 6: Stage and present commit**

```bash
git add src/main.rs src/window_state.rs
```

Present: `git commit -m "feat: t opens time-window overlay; apply re-filters live and persists"`

---

### Task 7: README, manual smoke test, final verification

**Files:**
- Modify: `README.md` (keybindings table, config section note, modes note)

**Interfaces:** none produced; this is documentation plus end-to-end verification.

- [ ] **Step 1: Update README**

In the Keybindings table, add after the `b` row:

```markdown
| `t` | Time window: pick how far back to show activity (slot / hours / days / weeks / months / years) |
```

In the Modes section, add below the table:

```markdown
The window can also be changed live inside the TUI with `t`. A window applied
there is remembered across launches; explicit mode flags override it for that
session without overwriting it.
```

- [ ] **Step 2: Manual smoke test**

Run `cargo run` in a real terminal and verify, in order:

1. Launch with no flags: header shows either the saved window or the slot label.
2. Press `t`: overlay opens. If the unit shows `last slot`, the number shows a dimmed `-` and h/l does not focus it.
3. `j`/`k` on the unit field cycles slot -> hours -> days -> weeks -> months -> years -> slot.
4. Pick `weeks`, h to the number, type `2`, Enter: header changes to `since 2 weeks`, repo list updates instantly, footer shows `Window: since 2 weeks`.
5. While the overlay is open, confirm `j`/`k` does NOT move the repo list selection behind it.
6. Press `r`: reload completes and the header still says `since 2 weeks` (window survives reload).
7. Quit, relaunch with no flags: header shows `since 2 weeks` (persistence).
8. Relaunch with `--since 1d`: header shows `since 1d` (flag override), quit, relaunch plain: back to `since 2 weeks` (flag did not overwrite state).
9. Relaunch with `-d`, press `t`: footer shows the "not in -d mode" message, no overlay.
10. Set a window of 12 months (or wide enough to outrun your 50-repo fetch) and confirm the header gains `(window exceeds fetched repos, repo_limit=50)` if applicable.
11. `cat ~/.cache/fleetz/window.toml` shows `n = 2` and `unit = "weeks"`.

- [ ] **Step 3: Final verification**

Run: `cargo test 2>&1 | tail -5` (all pass), `cargo clippy -- -D warnings 2>&1 | tail -5` (clean, or only pre-existing warnings), `cargo build --release 2>&1 | tail -3` (builds).

- [ ] **Step 4: Stage and present commit**

```bash
git add README.md
```

Present: `git commit -m "docs: document t time-window key, live window, and persistence"`
