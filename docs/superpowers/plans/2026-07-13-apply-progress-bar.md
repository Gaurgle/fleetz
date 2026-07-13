# Apply Progress Bar + Unit-Aware Caps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A determinate progress bar that fills in real time while a time window is applied, plus unit-aware count caps in the selector.

**Architecture:** The apply work moves to a spawned tokio task that reports per-repo progress over an unbounded channel; the event loop awaits the task's handle with an 80ms timeout, draining progress and redrawing a footer `Gauge` each tick. Separately, the overlay's count is clamped per unit instead of a flat 1-31.

**Tech Stack:** Rust, ratatui (Gauge widget), tokio (spawn, mpsc, time::timeout), crossterm. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-07-13-apply-progress-bar-design.md`

## Global Constraints

- Repo: `/Users/at-a/Repos/fleetz`, branch `feat/time-window` (already checked out). All paths relative to the repo root.
- No new dependencies (tokio, ratatui already direct deps; Gauge and mpsc are in the versions already used).
- Never use em-dashes or en-dashes anywhere (code, comments, docs, commit messages). Use hyphens, colons, or parentheses. A middle-dot `·` and the ellipsis `…` are allowed (already used in the codebase).
- Conventional Commits, first line under 72 chars, NO Co-Authored-By line.
- Commit authorization: executors run the commit themselves on this branch with the exact message each task specifies.
- Comments explain why, not what; match the file's existing prose-heavy style.
- Every task leaves the crate compiling and `cargo test` green with no new clippy warnings beyond pre-existing ones.
- Run from repo root: `cd /Users/at-a/Repos/fleetz`.

## Practical per-unit caps (verbatim values)

| Unit | max_count |
|------|-----------|
| Slot | 1 (count ignored) |
| Hours | 48 |
| Days | 31 |
| Weeks | 52 |
| Months | 24 |
| Years | 10 |

Min is always 1.

---

### Task 1: Unit-aware count caps

**Files:**
- Modify: `src/time.rs` (add `WindowUnit::max_count`, retarget clamps, tests)
- Modify: `src/app.rs` (`TimeWindowOverlay` step/digit/unit-change use per-unit max, tests)

**Interfaces:**
- Produces: `WindowUnit::max_count(&self) -> u8`.
- Changes: `TimeWindow::clamped`, `cutoff_from`, `label` clamp against `unit.max_count()`. `TimeWindowOverlay::step_up/step_down/push_digit/after_unit_change` bound `n` to `1..=unit.max_count()`.

- [ ] **Step 1: Write failing tests in time.rs**

Append to the `tests` module in `src/time.rs`:

```rust
    #[test]
    fn window_unit_max_count_per_unit() {
        assert_eq!(WindowUnit::Slot.max_count(), 1);
        assert_eq!(WindowUnit::Hours.max_count(), 48);
        assert_eq!(WindowUnit::Days.max_count(), 31);
        assert_eq!(WindowUnit::Weeks.max_count(), 52);
        assert_eq!(WindowUnit::Months.max_count(), 24);
        assert_eq!(WindowUnit::Years.max_count(), 10);
    }

    #[test]
    fn window_clamped_bounds_per_unit() {
        assert_eq!(TimeWindow::clamped(40, WindowUnit::Days).n, 31);
        assert_eq!(TimeWindow::clamped(40, WindowUnit::Hours).n, 40);
        assert_eq!(TimeWindow::clamped(60, WindowUnit::Weeks).n, 52);
        assert_eq!(TimeWindow::clamped(0, WindowUnit::Hours).n, 1);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test window_unit_max_count window_clamped_bounds 2>&1 | tail -8`
Expected: compile error, `max_count` not found.

- [ ] **Step 3: Implement in time.rs**

Add a method to `impl WindowUnit` (create the impl block after the enum if none exists):

```rust
impl WindowUnit {
    /// The largest count that reads naturally for this unit. Practical
    /// caps: they still allow useful overlap ("36 hours", "18 months")
    /// rather than snapping to the next unit. Slot ignores the count, so
    /// its max is a nominal 1.
    pub fn max_count(&self) -> u8 {
        match self {
            WindowUnit::Slot => 1,
            WindowUnit::Hours => 48,
            WindowUnit::Days => 31,
            WindowUnit::Weeks => 52,
            WindowUnit::Months => 24,
            WindowUnit::Years => 10,
        }
    }
}
```

In `TimeWindow::clamped`, change the clamp to use the unit max:

```rust
    pub fn clamped(n: u8, unit: WindowUnit) -> Self {
        Self { n: n.clamp(WINDOW_N_MIN, unit.max_count()), unit }
    }
```

In `cutoff_from`, change the first line's clamp:

```rust
        let n = self.n.clamp(WINDOW_N_MIN, self.unit.max_count()) as i64;
```

In `label`, change the clamp near the end:

```rust
        let n = self.n.clamp(WINDOW_N_MIN, self.unit.max_count());
```

Leave `WINDOW_N_MAX` defined (still used as the weeks cap value and the two-digit ceiling); it is simply no longer the universal clamp.

- [ ] **Step 4: Run time.rs tests**

Run: `cargo test --lib time 2>&1 | tail -6`
Expected: all time tests pass, including the two new ones. The pre-existing `window_clamped_bounds_n` test (which asserted 99 -> 31 for Days) still passes since Days max is 31.

- [ ] **Step 5: Write failing overlay tests in app.rs**

Append to the `tests` module in `src/app.rs`:

```rust
    #[test]
    fn window_overlay_number_wraps_at_unit_max() {
        // Hours cap is 48, so 48 -> step_up wraps to 1.
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 48, unit: WindowUnit::Hours });
        o.step_up();
        assert_eq!(o.n, 1);
        o.step_down();
        assert_eq!(o.n, 48);
    }

    #[test]
    fn window_overlay_digit_clamps_to_unit_max() {
        // Years cap is 10: typing 4 then 0 (=40) clamps to 10.
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 1, unit: WindowUnit::Years });
        o.push_digit(4);
        o.push_digit(0);
        assert_eq!(o.n, 10);
    }

    #[test]
    fn window_overlay_unit_change_clamps_n_down() {
        // 52 weeks, switch to hours (max 48): n clamps to 48.
        let mut o = TimeWindowOverlay::new(TimeWindow { n: 52, unit: WindowUnit::Weeks });
        o.toggle_field(); // focus unit
        // Weeks -> step_up cycles backward to Days (max 31): n clamps to 31.
        o.step_up();
        assert_eq!(o.unit, WindowUnit::Days);
        assert_eq!(o.n, 31);
    }
```

- [ ] **Step 6: Run to verify failure**

Run: `cargo test window_overlay_number_wraps_at_unit_max window_overlay_digit_clamps_to_unit_max window_overlay_unit_change_clamps 2>&1 | tail -8`
Expected: assertion failures (current code wraps/clamps at flat 31).

- [ ] **Step 7: Implement overlay changes in app.rs**

In `TimeWindowOverlay::step_up`, the Number arm becomes:

```rust
            WindowField::Number => {
                let max = self.unit.max_count();
                self.n = if self.n >= max { WINDOW_N_MIN } else { self.n + 1 };
            }
```

In `step_down`, the Number arm becomes:

```rust
            WindowField::Number => {
                let max = self.unit.max_count();
                self.n = if self.n <= WINDOW_N_MIN { max } else { self.n - 1 };
            }
```

In `push_digit`, change the clamp line:

```rust
        self.n = parsed.clamp(WINDOW_N_MIN, self.unit.max_count());
```

In `after_unit_change`, clamp n down to the new unit's max (add the clamp before the Slot focus handling):

```rust
    fn after_unit_change(&mut self) {
        self.n = self.n.clamp(WINDOW_N_MIN, self.unit.max_count());
        if self.unit == WindowUnit::Slot {
            self.field = WindowField::Unit;
        }
    }
```

The `use crate::time::{...}` line at the top of app.rs already imports `WindowUnit`; ensure `WINDOW_N_MAX` is still imported only if still referenced. If removing the last `WINDOW_N_MAX` use in app.rs causes an unused-import warning, drop it from the import list (it remains defined and used in time.rs).

- [ ] **Step 8: Run full suite**

Run: `cargo test 2>&1 | tail -5`
Expected: all pass (114 + 5 new = 119).
Run: `cargo clippy 2>&1 | tail -8`
Expected: no new warnings; confirm no unused-import warning for `WINDOW_N_MAX`.

- [ ] **Step 9: Commit**

```bash
git add src/time.rs src/app.rs
git commit -m "feat: unit-aware count caps in the time-window selector"
```

---

### Task 2: Progress plumbing (Clone derives, ApplyProgress, assemble_view channel)

Behavior-preserving: after this task the app runs exactly as before (all callers pass no progress sender). It only adds the ability to report progress.

**Files:**
- Modify: `src/cli.rs` (derive Clone on Args)
- Modify: `src/config.rs` (derive Clone on Config)
- Modify: `src/app.rs` (ApplyProgress enum, App field, a pure ratio helper + test)
- Modify: `src/main.rs` (assemble_view gains an optional progress sender; callers pass None)

**Interfaces:**
- Produces: `app::ApplyProgress` (enum), `App.apply_progress: Option<ApplyProgress>`, `ApplyProgress::ratio(&self) -> f64`, `ApplyProgress::label(&self) -> String`.
- Changes: `assemble_view(..., git_fetch: bool, progress: Option<&tokio::sync::mpsc::UnboundedSender<app::ApplyProgress>>)`.

- [ ] **Step 1: Derive Clone**

`src/cli.rs`: change `#[derive(Parser, Debug)]` on `Args` to `#[derive(Parser, Debug, Clone)]`.
`src/config.rs`: change `#[derive(Debug, Deserialize)]` on `Config` to `#[derive(Debug, Clone, Deserialize)]`.

- [ ] **Step 2: Write failing test for ApplyProgress in app.rs**

Append to the `tests` module in `src/app.rs`:

```rust
    #[test]
    fn apply_progress_ratio_and_label() {
        assert_eq!(ApplyProgress::Discovering.ratio(), 0.0);
        assert_eq!(ApplyProgress::Discovering.label(), "discovering…");
        let s = ApplyProgress::Statting { done: 12, total: 48 };
        assert!((s.ratio() - 0.25).abs() < 1e-9);
        assert_eq!(s.label(), "12/48 repos");
        // total 0 must not divide by zero.
        let z = ApplyProgress::Statting { done: 0, total: 0 };
        assert_eq!(z.ratio(), 0.0);
        assert_eq!(ApplyProgress::Finishing.ratio(), 1.0);
        assert_eq!(ApplyProgress::Finishing.label(), "finishing…");
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test apply_progress_ratio_and_label 2>&1 | tail -6`
Expected: compile error, `ApplyProgress` not found.

- [ ] **Step 4: Implement ApplyProgress in app.rs**

Add near the top-level types (after the `Overlay` enum is fine):

```rust
/// Progress of an in-flight time-window apply, shown as a footer gauge.
/// `Statting` is the determinate phase over the GitHub-matched repos;
/// `Discovering` and `Finishing` bracket it (their counts are not known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyProgress {
    Discovering,
    Statting { done: usize, total: usize },
    Finishing,
}

impl ApplyProgress {
    /// Fill fraction for the gauge, in 0.0..=1.0.
    pub fn ratio(&self) -> f64 {
        match self {
            ApplyProgress::Discovering => 0.0,
            ApplyProgress::Statting { done, total } => {
                if *total == 0 {
                    0.0
                } else {
                    (*done as f64 / *total as f64).clamp(0.0, 1.0)
                }
            }
            ApplyProgress::Finishing => 1.0,
        }
    }

    /// Gauge label text.
    pub fn label(&self) -> String {
        match self {
            ApplyProgress::Discovering => "discovering…".to_string(),
            ApplyProgress::Statting { done, total } => format!("{done}/{total} repos"),
            ApplyProgress::Finishing => "finishing…".to_string(),
        }
    }
}
```

Add the App field after `window_apply_requested`:

```rust
    /// Progress of an in-flight window apply, or None when idle. Drives the
    /// footer gauge; set/cleared by apply_time_window's animate loop.
    pub apply_progress: Option<ApplyProgress>,
```

And in `App::new`'s struct literal:

```rust
            apply_progress: None,
```

- [ ] **Step 5: Add the progress param to assemble_view (main.rs)**

Change the signature:

```rust
async fn assemble_view(
    dataset: &[github::GithubRepo],
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
    header: String,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
    git_fetch: bool,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<app::ApplyProgress>>,
) -> Result<(Vec<RepoStatus>, String), Box<dyn std::error::Error>> {
```

Right after `elsewhere_found` is computed (after the pass-2 line and before the "Parallel fetch" block), add:

```rust
    // Determinate total: the GitHub-matched repos are the expensive stat
    // loop. The pass-3 local tail is counted only as it is walked, so it is
    // reported as the Finishing phase rather than folded into the fraction.
    let stat_total = primary_found.len() + elsewhere_found.len();
    if let Some(tx) = progress {
        let _ = tx.send(app::ApplyProgress::Discovering);
        let _ = tx.send(app::ApplyProgress::Statting { done: 0, total: stat_total });
    }
```

In the primary stat loop, after `repos.push(repo);`, add a running count send. Change the loop to track done:

```rust
    // Process primary repos
    let mut stat_done = 0usize;
    for found in &primary_found {
        let repo = build_repo_status(found, &active, args, RepoLocation::Primary).await;
        checked_names.insert(found.name.to_lowercase());
        repos.push(repo);
        stat_done += 1;
        if let Some(tx) = progress {
            let _ = tx.send(app::ApplyProgress::Statting { done: stat_done, total: stat_total });
        }
    }
```

In the elsewhere stat loop, keep incrementing the same `stat_done` after each push:

```rust
    // Process elsewhere repos
    for found in &elsewhere_found {
        if checked_names.contains(&found.name.to_lowercase()) {
            continue;
        }
        let repo = build_repo_status(found, &active, args, RepoLocation::Elsewhere).await;
        checked_names.insert(found.name.to_lowercase());
        repos.push(repo);
        stat_done += 1;
        if let Some(tx) = progress {
            let _ = tx.send(app::ApplyProgress::Statting { done: stat_done, total: stat_total });
        }
    }
```

Immediately before the Pass 3 comment/`let all_local = ...`, add:

```rust
    if let Some(tx) = progress {
        let _ = tx.send(app::ApplyProgress::Finishing);
    }
```

- [ ] **Step 6: Update the two existing assemble_view callers to pass None**

In `load_repos` (the non-dirty path):

```rust
    let (repos, full_header) =
        assemble_view(&dataset, cutoff, header, args, config, repo_dir, true, None).await?;
```

In `apply_time_window` (current inline caller) for now pass None to keep it compiling; Task 3 replaces this whole function:

```rust
    match assemble_view(&app.dataset, Some(cutoff), label, args, config, repo_dir, false, None).await {
```

- [ ] **Step 7: Verify behavior-preserving**

Run: `cargo test 2>&1 | tail -5`
Expected: all pass (119 + 1 new = 120), every pre-existing test unchanged.
Run: `cargo clippy 2>&1 | tail -8`
Expected: no new warnings. `apply_progress` may be unused until Task 3 (footer reads it); an acceptable dead_code warning for this task only.

- [ ] **Step 8: Commit**

```bash
git add src/cli.rs src/config.rs src/app.rs src/main.rs
git commit -m "feat: thread optional apply-progress channel through assemble_view"
```

---

### Task 3: Spawn + animate + footer gauge

Activates the bar: apply runs on a task, the event loop animates the footer gauge.

**Files:**
- Modify: `src/main.rs` (`apply_time_window` rewrite; the `Overlay::TimeWindow` event-loop arm)
- Modify: `src/ui/footer.rs` (render the gauge when `apply_progress` is Some)

**Interfaces:**
- Consumes: `ApplyProgress` (Task 2), `assemble_view`'s progress param (Task 2), `Args`/`Config` Clone (Task 2).
- Changes: `apply_time_window` signature gains `terminal`.

- [ ] **Step 1: Rewrite apply_time_window in main.rs**

Replace the entire `apply_time_window` function with:

```rust
/// Apply the just-chosen window on a background task so the UI can paint a
/// progress bar as repos are stat'd, instead of freezing. Spawns the
/// assemble work with owned inputs, animates the footer gauge from the
/// progress channel until the task finishes, then swaps in the result and
/// persists the window. No git fetch and no GitHub call: this is a pure
/// re-filter of the already-fetched dataset.
async fn apply_time_window(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    args: &Args,
    config: &config::Config,
    repo_dir: &Path,
) {
    let Some(w) = app.window else { return };
    let cutoff = w.cutoff(config.work_start, config.work_end);
    let label = w.label(config.work_start, config.work_end);

    // Owned copies so the assemble work can move onto a spawned task.
    let dataset = app.dataset.clone();
    let args_owned = args.clone();
    let config_owned = config.clone();
    let repo_dir_owned = repo_dir.to_path_buf();
    let header = label.clone();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    // Box<dyn Error> is not Send, so map the assemble error to a String to
    // keep the task's output Send.
    let mut handle = tokio::spawn(async move {
        assemble_view(
            &dataset,
            Some(cutoff),
            header,
            &args_owned,
            &config_owned,
            &repo_dir_owned,
            false,
            Some(&tx),
        )
        .await
        .map_err(|e| e.to_string())
    });

    app.apply_progress = Some(ApplyProgress::Discovering);
    let _ = terminal.draw(|frame| ui::render(frame, app));

    let result = loop {
        match tokio::time::timeout(std::time::Duration::from_millis(80), &mut handle).await {
            Ok(joined) => {
                // Task finished; drain any last progress then break with it.
                while let Ok(p) = rx.try_recv() {
                    app.apply_progress = Some(p);
                }
                break joined;
            }
            Err(_) => {
                // Still running: pull the latest progress and repaint.
                while let Ok(p) = rx.try_recv() {
                    app.apply_progress = Some(p);
                }
                let _ = terminal.draw(|frame| ui::render(frame, app));
            }
        }
    };

    app.apply_progress = None;

    match result {
        Ok(Ok((repos, new_header))) => {
            app.repos = repos;
            apply_groups(&mut app.repos, config);
            app.header = new_header;
            app.clamp_after_reload();
            window_state::WindowState::new().save(&w);
            app.set_status(format!("Window: {label}"), false);
        }
        Ok(Err(e)) => app.set_status(format!("Window apply failed: {e}"), true),
        Err(e) => app.set_status(format!("Window apply task failed: {e}"), true),
    }
}
```

Add `ApplyProgress` to the existing `use app::{...}` line at the top of main.rs.

- [ ] **Step 2: Update the event-loop arm to pass terminal and drop the static status**

In `run_event_loop`, the `Overlay::TimeWindow` arm becomes (pass `terminal` so apply can drive its own redraw loop):

```rust
                        Overlay::TimeWindow(_) => {
                            handle_time_window_input(app, key);
                            // Apply inline (this arm `continue`s past the
                            // bottom service block, and static mode then
                            // blocks in event::poll). apply_time_window drives
                            // its own progress-bar redraw loop while it runs.
                            if app.window_apply_requested {
                                app.window_apply_requested = false;
                                apply_time_window(terminal, app, args, config, repo_dir).await;
                            }
                            continue;
                        }
```

- [ ] **Step 3: Render the gauge in footer.rs**

Add imports at the top of `src/ui/footer.rs`:

```rust
use ratatui::widgets::Gauge;
```

At the very start of `pub fn render`, before `let summary = ...`, add:

```rust
    // An in-flight window apply takes over the whole footer with a gauge,
    // so the user sees real-time progress instead of a frozen list.
    if let Some(progress) = app.apply_progress {
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(colors::SURFACE)),
            )
            .gauge_style(Style::default().fg(colors::GREEN))
            .ratio(progress.ratio())
            .label(progress.label());
        frame.render_widget(gauge, area);
        return;
    }
```

- [ ] **Step 4: Verify**

Run: `cargo build 2>&1 | tail -5`
Expected: compiles, and the `apply_progress` dead_code warning from Task 2 is gone.
Run: `cargo test 2>&1 | tail -5`
Expected: 120 passing (this task adds no unit tests; it is integration wiring covered by the smoke test).
Run: `cargo clippy 2>&1 | tail -8`
Expected: no new warnings.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/ui/footer.rs
git commit -m "feat: real-time progress gauge while applying a time window"
```

---

### Task 4: README, manual smoke test, final verification

**Files:**
- Modify: `README.md` (note the progress bar and the unit-aware caps)

- [ ] **Step 1: README**

In the Modes section note added by the parent feature, extend the live-window sentence:

```markdown
Applying a window shows a progress bar in the footer as repos are re-stat'd.
The count field is capped per unit (hours up to 48, days 31, weeks 52,
months 24, years 10).
```

- [ ] **Step 2: Automated verification**

Run: `cargo test 2>&1 | tail -5` (120 pass), `cargo clippy 2>&1 | tail -8` (no new warnings), `cargo build --release 2>&1 | tail -3` (builds).

- [ ] **Step 3: Manual smoke test (human runs; needs a real terminal)**

1. `cargo run`, press `t`, set a wide window (e.g. 4 weeks) over many repos, Enter.
2. Footer shows a gauge: briefly `discovering…`, then a filling `N/M repos`, then `finishing…`, then the normal footer with `Window: since 4 weeks`. The list is not frozen; the bar visibly moves.
3. A narrow window (few repos) still works: the bar may flash quickly.
4. Caps: in the overlay, on `hours` the count wraps at 48; on `years` it wraps at 10; type `40` on `days` -> clamps to 31; set weeks to 52, switch to months -> count clamps to 24.
5. `-d` mode: `t` still shows the "needs GitHub data" message, no bar.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: document apply progress bar and per-unit count caps"
```
