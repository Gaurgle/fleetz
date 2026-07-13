# Time window selector (in-TUI)

Date: 2026-07-13
Status: approved for implementation

## Problem

fleetz's default (auto) mode filters GitHub activity to the previous time-slot
boundary, which never reaches further back than yesterday's `work_end`. Sitting
down at a machine after a quiet day (or after syncing a week of pulls) shows an
empty activity list, and after pulling there is no divergence either, so the
week's work is invisible. The only way to widen the window today is to quit and
relaunch with `--since`. fleetz is an interactive TUI: the window must be
changeable live, from inside the UI.

## Decisions (made during brainstorming)

- Scope: time window selector only. Unattended-repo indicators and an
  "old repo" cutoff are separate follow-up designs.
- UI form: a number + unit spinner in a small centered modal overlay.
- Persistence: remember the last applied window across launches.
- Apply cost: re-filter only, no per-repo git fetch on window change.
- Architecture: approach A, split `load_repos` into dataset fetch and view
  assembly; window state lives in `App`.
- Input: the overlay is fully modal. hjkl / arrows work inside it without
  colliding with list navigation because all input routes to the overlay
  while it is open (same pattern as the branches overlay). No modifier keys.

## 1. TimeWindow model (`time.rs`)

```rust
pub enum WindowUnit { Slot, Hours, Days, Weeks, Months, Years }

pub struct TimeWindow {
    pub n: u8,      // clamped 1..=31; ignored when unit == Slot
    pub unit: WindowUnit,
}
```

- `cutoff(&self, work_start, work_end) -> DateTime<Utc>`
  - `Slot`: delegates to the existing `auto_cutoff`.
  - `Hours` / `Days` / `Weeks`: `Utc::now() - n * duration`.
  - `Months` / `Years`: chrono calendar-aware `Months` arithmetic
    (years = 12 * n months), saturating on overflow.
- `label(&self, work_start, work_end) -> String` for the header:
  `since 2 weeks`, `since 3 months`, or the existing slot label for `Slot`.
- Default: `TimeWindow { n: 1, unit: Slot }` (identical to today's auto mode).

## 2. load_repos refactor (`main.rs`)

Split the current `load_repos` into:

- `fetch_dataset(config, args, cache, bypass_cache) -> Result<Vec<GithubRepo>>`
  The GitHub GraphQL call + disk cache. Unfiltered, top `repo_limit` repos
  by `pushed_at`.
- `assemble_view(dataset, cutoff, before, args, config, repo_dir, git_fetch: bool)
  -> (Vec<RepoStatus>, String)`
  Everything after the fetch: since/before filtering, discovery passes 1-3,
  optional per-repo `git fetch`, statting, not-cloned rows, header assembly.

`load_repos` remains as a thin composition of the two, so startup and `r`
behavior is unchanged. Dirty mode (`-d`) is untouched.

`App` gains:

- `dataset: Vec<GithubRepo>`: the last fetched unfiltered repo list.
- `window: Option<TimeWindow>`: `None` = follow CLI flags for cutoff.

Behavior:

- Applying a window from the overlay: compute cutoff from the new window,
  run `assemble_view` over the in-memory `dataset` with `git_fetch: false`,
  swap `app.repos`, re-apply groups, clamp selection, update header, persist
  the window. Instant; ahead/behind uses on-disk remote-tracking refs
  (`f` / `F` / `r` freshen as today).
- `r` (full reload): unchanged fetch behavior (cache-bypassed GitHub fetch,
  per-repo git fetch), but the cutoff now comes from `app.window` when set,
  falling back to the launch flags. `r` must not silently reset the window.

## 3. Selector overlay (`app.rs`, `ui/`)

New `Overlay::TimeWindow(TimeWindowOverlay)` alongside `Help` and `Branches`.

```
+- time window ----------------+
|   since:  [ 2 ] [ weeks ]    |
|   h/l field . j/k adjust     |
|   Enter apply . Esc cancel   |
+------------------------------+
```

State: `{ n: u8, unit: WindowUnit, field: Field /* Number | Unit */ }`,
prefilled from the active window (or the CLI-derived one).

Keys (modal; a dedicated handler consumes all input while open, like
`handle_branch_input`):

- `t` opens the overlay (`t` is currently unbound in normal mode).
- `h` / `l` / `Left` / `Right` / `Tab`: switch between number and unit field.
- `j` / `k` / `Down` / `Up`: adjust the focused field. Number wraps within
  1..=31. Unit cycles Slot, Hours, Days, Weeks, Months, Years.
- Digits `0-9`: set the number directly (two-digit entry buffer; values
  clamp to 1..=31).
- `Enter`: apply + persist + close. `Esc`: cancel (no change).
- When unit is `Slot`, the number field renders dimmed and h/l skips it.

Rendering: centered box like the help overlay; the focused field is
highlighted. The help overlay and README both gain the `t` binding.

## 4. Persistence

- File: `window.toml` in the same directory as the API cache.
- Content: `n = 2` and `unit = "weeks"`.
- Written only when a window is applied from the overlay.
- Read at startup only when no explicit mode flag was passed; missing or
  corrupt file falls back to the default (auto slot). IO errors are ignored
  (fall back, never crash).
- Explicit mode flags (`--since`, `--before`, `-t`, `-a`, `-d`) override for
  that session and never overwrite the saved choice.

## 5. repo_limit honesty

The dataset is the top `repo_limit` (default 50) repos by push date. If the
chosen cutoff is older than the oldest `pushed_at` in the dataset, the view
may be missing repos while looking complete. In that case the header gets a
suffix: `(window exceeds fetched repos, repo_limit=50)`.

## 6. Error handling

- State file read/write failures: silent fallback to default window.
- Month/year arithmetic: saturating, never panics.
- Empty dataset (offline, no cache): assemble_view already handles an empty
  active list; the local-changes pass still runs.

## 7. Tests

- `TimeWindow::cutoff` per unit, including month-end and year edges, and
  n clamping.
- Overlay state machine: field switching, j/k wrapping in both fields, digit
  entry clamping, Slot dimming/skip behavior.
- Persistence: round-trip write/read, corrupt file falls back to default,
  mode flags suppress both read and write.
- repo_limit warning: triggers exactly when cutoff predates the oldest
  dataset entry.
- Refactor is behavior-preserving: existing tests keep passing.

## Out of scope (follow-ups)

- Indicators for unattended repos / pending diffs / pulls outside the window.
- A configurable "truly old repo" cutoff.
- Making auto mode scan backward for the latest slot with activity (README
  currently overpromises this).
