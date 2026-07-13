# Apply progress bar + unit-aware window caps

Date: 2026-07-13
Status: approved for implementation
Parent feature: 2026-07-13-time-window-selector-design.md (on branch feat/time-window)

## Problem

Two follow-ups to the in-TUI time-window selector, both approved in discussion:

1. **Apply freezes the UI.** Pressing Enter to apply a window runs
   `assemble_view` (local discovery + a `git` stat of every repo entering the
   window) awaited inline on the UI thread, so nothing redraws until it
   finishes. A wide window looks frozen. We want a determinate progress bar
   that fills in real time as repos are stat'd.

2. **The count spinner is unintuitive.** The overlay clamps the count to a
   flat 1-31 for every unit, so "29 hours" or "31 years" are selectable. The
   max should be unit-aware.

## Decisions

- Determinate `N/M` bar (not indeterminate), driven by a spawned tokio task
  that reports progress over a channel while the event loop animates.
- Bar tracks the GitHub-matched repos (the expensive stat loop). Phases:
  `discovering...` (indeterminate, brief), then `N/M` fill, then
  `finishing...` for the small local-only/not-cloned tail whose count is not
  known until the end.
- Input is frozen while the bar runs (no navigation, no cancel in v1).
- The bar renders in the footer (where the status line already lives), not a
  popup overlay.
- Practical per-unit caps: hours <=48, days <=31, weeks <=52, months <=24,
  years <=10. (Slot ignores the count.) Min stays 1.
- Both changes land on the existing `feat/time-window` branch.

## Part A: unit-aware caps

`time.rs`:
- `WindowUnit::max_count(&self) -> u8`: Slot -> 1 (count ignored), Hours ->
  48, Days -> 31, Weeks -> 52, Months -> 24, Years -> 10.
- `TimeWindow::clamped` clamps `n` to `unit.max_count()` (not the flat
  `WINDOW_N_MAX`), so a persisted or hand-edited count is bounded per unit.
- `cutoff_from` / `label` clamp against `unit.max_count()` too.
- `WINDOW_N_MAX` const stays (still the ceiling for a two-digit entry buffer
  and the weeks cap) but is no longer the universal clamp.

`app.rs` (`TimeWindowOverlay`):
- `step_up` / `step_down` wrap the number within `1..=unit.max_count()`.
- `push_digit` clamps typed values to `1..=unit.max_count()`.
- On a unit change (`after_unit_change`), clamp `n` down to the new unit's
  max so switching weeks(52) -> hours can't leave n at 52.

## Part B: apply progress bar

### Owned inputs for spawning
- `#[derive(Clone)]` on `cli::Args` and `config::Config` so owned copies move
  into the spawned task. Both are small (Args is CLI flags; Config is a few
  scalars plus two maps).

### Progress type (`app.rs`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyProgress {
    Discovering,
    Statting { done: usize, total: usize },
    Finishing,
}
```
- `App` gains `pub apply_progress: Option<ApplyProgress>` (None when idle),
  defaulted None in `App::new`.

### assemble_view instrumentation (`main.rs`)
- New trailing param `progress: Option<&tokio::sync::mpsc::UnboundedSender<ApplyProgress>>`.
- After discovery (passes 1+2) compute `total = primary_found.len() +
  elsewhere_found.len()` and send `Discovering` then the first
  `Statting { done: 0, total }`.
- In the primary and elsewhere stat loops, after each `build_repo_status`,
  send `Statting { done, total }` with the running count.
- Before pass 3 send `Finishing`.
- All existing callers (`load_repos` startup/reload) pass `None`: behavior
  and the return type are otherwise unchanged. The send is best-effort
  (`if let Some(tx) = progress { let _ = tx.send(...); }`), never an error
  path.

### apply_time_window restructure (`main.rs`)
- Signature gains `terminal: &mut Terminal<...>`; called from the event-loop
  `Overlay::TimeWindow` arm.
- Clone owned inputs (`dataset`, `args`, `config`, `repo_dir.to_path_buf()`),
  create an unbounded channel, `tokio::spawn` a task that runs
  `assemble_view(..., false, Some(&tx))` and maps its `Box<dyn Error>` to a
  `String` (so the task output is `Send`).
- Animate: loop on `tokio::time::timeout(80ms, &mut handle)`. On timeout,
  drain `rx.try_recv()` into `app.apply_progress` and redraw. On completion,
  drain any final progress, then apply the result: swap `app.repos`,
  re-apply groups, clamp, `WindowState::save`, set the `Window: <label>`
  status. Always clear `app.apply_progress = None` at the end.
- The pre-draw `set_status("Applying window...")` added earlier is removed:
  the bar replaces it.

### Footer gauge (`ui/footer.rs`)
- At the top of `render`, if `app.apply_progress` is `Some`, render a
  `ratatui::widgets::Gauge` in the footer area (keeping the existing TOP
  border block) and return early:
  - `Discovering` -> ratio 0.0, label `discovering...`
  - `Statting { done, total }` -> ratio `done/total` (guard total 0), label
    `<done>/<total> repos`
  - `Finishing` -> ratio 1.0, label `finishing...`
- Normal footer rendering is unchanged when `apply_progress` is None.

## Error handling
- Progress sends are best-effort; a closed receiver never breaks the worker.
- The spawned task maps assemble errors to `String`; a `JoinError` (panic)
  surfaces as an error status, and `apply_progress` is still cleared.
- Empty result (offline) already handled by assemble_view; the bar simply
  fills to 0/0 -> finishing quickly.

## Testing
- Part A: `WindowUnit::max_count` per unit; `TimeWindow::clamped` bounds per
  unit (e.g. 40 days -> 31, 40 hours -> 40, 60 weeks -> 52); overlay wrap and
  digit clamp per unit; unit-change clamps n down (weeks 52 -> switch to
  hours -> n becomes 48).
- Part B: the progress-emission and spawn/animate paths are integration-level
  (need real git) and are covered by the manual smoke test, matching the
  codebase's existing no-UI-render-test convention. Unit tests assert the
  `None`-progress path leaves assemble_view behavior unchanged (existing
  suite) and that `ApplyProgress` ratio math (a small pure helper if
  extracted) is correct.

## Out of scope (later)
- Esc-to-cancel during apply.
- The same bar for the `r` reload (keeps its `Refreshing...` line for now).
- Truly non-blocking apply that also processes input during the bar.
