# App split design: owned state in the TUI

**Status: accepted 2026-11-09 21:56 CET by Andreas ("I accept all
changes in the design plan"): decisions D1 to D7 and owner items O-A to
O-D as recommended.** From the architect's consult (A1, E3, E2b) on
`main` at `ca8244a`. Tickets are filed from the table below as they come
up. Measured facts are `rg` counts; sizes marked "est." are estimates.

## Why

`src/app.rs` is 6,203 production lines. `App` has 115 `pub` fields, 18 of
them `Cell`/`RefCell` that the renderer writes through `&App`, and 170
methods. Anyone may set any field from anywhere: `main.rs` assigns 44 of
them at about 100 sites, `ui/` touches 114 members. Rule A4 of
`docs/architecture.md` says state has owners; this design says which.

**For the reader coming from Kotlin.** There is no inheritance and no
interface for state: a type's public methods are its whole contract, and
the module boundary is the access modifier (`pub(crate)`, `pub(super)`,
private), so "who may change this" is answered by where the code sits, not
by `private set`. Background work never calls back into the state: it
posts an event, and one loop applies events in order, so there is no
synchronisation to read past. A `&App` in the renderer is a compile-time
read-only view; the `Cell` fields are the one exception, which A1a makes
visible and R2 removes.

## `App` today, by concern

| Concern | Fields | `Cell` | `main.rs` assigns | `ui/` reads directly |
|---|---|---|---|---|
| Fleet data (`repos`, `dataset`, `github_meta`, `window`, `loading`, ...) | 14 | 0 | 13 of them | 7 |
| GitHub sync (`github_freshness`, `api_budget`, `github_backoff`, poll timing) | 9 | 0 | 3, plus two startup copies | 5 |
| Local fetch and refresh (`fetch_backoff`, `fetches_in_flight`, `manual_fetches`, sweep and refresh flags) | 11 | 0 | 7 | 6 |
| Navigation and layout (`view`, `selected`, `focused_panel`, pane sizes, scrolls) | 27 | 10 | 10 | the most: `focused_panel` 36 sites, `view` 20 |
| Overlays, input modes, loop (`overlay`, search, status, `needs_redraw`, yank) | 12 | 0 | 6 | 7 |
| Contributions strip | 8 | 2 | 5 | 13 |
| `ci` view | 9 | 2 | none | through methods |
| `notes` view and reader | 10 | 3 | none | through methods |
| `changes` view | 5 | 0 | none | through methods |
| worktrees | 3 | 0 | none | through methods |
| digest | 7 | 1 | 1 | through methods |

The per-view groups are already the cleanest: nothing in `main.rs` assigns
their fields; the loop calls `App::claim_*` on the tick and `App::apply_*`
on the answer.

## The rule for an owner type

A concern gets its own type when two or more of its fields must agree (a
generation and its cursor, a claim table and its tally, a hold and its
ladder). A field anyone may set alone (`view`, `focused_panel`, a settings
copy) stays a plain `pub(crate)` field on `App`; `&App` already makes it
read-only for `ui/`.

## Owner types

| Type | File | Holds (private) | Verbs | `ui/` reads |
|---|---|---|---|---|
| `GithubSync` | `engine/github_sync.rs` | `GithubFreshness`, `ApiBudget`, `GithubBackoff`, the jitter, `interval_secs`, `poll_cost`, `inflight: Option<Instant>`, `push_detected` | `observe_budget`, `attempt_failed`, `attempt_succeeded`, `pace`, `slowed`, `limit_until`, `poll_due`, `not_before`, `effective_interval`, `claim_attempt`, `expire_stuck`, `note_push`, `take_push` | `freshness()`, `pace()`, `limit_until()`, `interval_secs()`, `poll_cost()` |
| `FetchState` | `engine/fetch.rs` (grows to about 650, est.) | `FetchBackoff`, `FetchClaims`, `FetchTally`, `max_bytes`, `interval_secs`, the sweep lock, the refresh flags | `begin_sweep`, `sweep_landed`, `plan` (today `plan_sweep_fetch` + `fetch_skips_for` + `held_fetch_paths`), `expect_manual`, `settle_refresh_*`, `held`, `claim`, `landed_under`, `expire_claims`, `touch`, `expire_refresh`, `refresh_phase`, `record_outcome`, `clear_holds`, `set_interval` | `refresh_phase()`, `is_refreshing()`, `sweep_manual()`, `interval_secs()` |
| `RenderFeedback` | `app/feedback.rs` | the 18 cells, unchanged at first | none at first (D4) | all, as `app.drawn.<cell>` |
| `CiView`, `NotesView`, `ChangesView`, `WorktreesView`, `DigestView`, `ContribStrip` | `app/ci.rs`, `app/notes.rs`, `app/changes.rs`, `app/worktrees.rs`, `app/digest.rs`, `app/contrib.rs` | the group's fields, `pub(super)` | the group's `impl App` block, moved unchanged (D5) | `app.ci.body` and so on, or the methods as today |

`App` keeps plain: fleet data, navigation and layout (minus the cells),
overlays and loop, the settings copies. `SettingsOverlay`,
`BranchOverlay`, `TimeWindowOverlay`, `QuickFilters` and `ParsedQuery` are
owned types already and only need their own files (`app/overlays.rs`,
`app/query.rs`).

## Decisions

- **D1. `GithubSync` and `FetchState` live in `src/engine/`.** They decide,
  `App` holds them (A3, A5: both are what a `fleetz-core` crate would
  export). Their imports are sources and domain only, so the layer test
  stays green. `app.rs` re-exports the types `ui/` names by `app::` path
  (`GithubFreshness`, `DataTime`, `Outcome`, `RefreshPhase`, `Pace`,
  `FetchReport`), so `ui/` does not change in E3 or E2b. Rejected:
  `src/app/github.rs`, which keeps the decisions in the front end and gives
  `report` and epoz nothing to reuse.
- **D2. Decisions take inputs, never `&App`.** `poll_due(now, ctx)` with
  `PollContext { watch, busy, any_pending }` built by `App::poll_context()`.
  Rejected: a view trait over `App`, which would make the engine import a
  front-end type.
- **D3. The `config` parameter goes.** The four poll functions read only
  `github_poll_interval` and `repo_limit` from it, which `App` already holds
  as `github_poll_secs` and `github_poll_cost`, set once at startup and
  never live. `GithubSync::new(interval_secs, poll_cost, jitter)` holds
  them. This is E3's one real signature change; the reviewer checks that no
  other `config` read is lost and that a startup test pins the equivalence.
- **D4. `RenderFeedback` is first a move, then a redesign.** A1a declares
  the struct with the 18 cells unchanged as `app.drawn`; 24 `ui/` write
  sites and 10 in `app.rs` gain the prefix. R2 (normal review, not a move)
  threads `&mut RenderFeedback` through `ui::render` and the panes, the loop
  stores it, the cells become plain fields and interior mutability leaves
  `App`. Rejected: one `RefCell<RenderFeedback>`, because render reads
  `inspector_max_scroll` and `fleet_drawn` in one pane after writing them
  in another, so a held borrow would panic at runtime where no test looks;
  and `render` returning the struct, since the within-frame reads need it
  during the pass.
- **D5. Per-view groups split by file before they split by type.** Rust
  visibility is by module: `impl App` in `app/ci.rs` reads `App`'s private
  fields without widening anything, so each view ticket is a move of the
  group's types and methods plus the rename `self.ci_cursor` to
  `self.ci.cursor`. A method lifts onto `impl CiView` only when its body
  names no other group; the reviewer checks that count is zero per lifted
  method. Rejected: lifting everything at once, which turns move tickets
  into rewrites with `&self.repos` plumbing.
- **D6. The loop stays the one place events are applied.** No owner type
  sends an event or holds a sender; each returns a decision (an attempt id,
  a `SweepFetchPlan`, a `Vec<SweepTarget>`) and the loop spawns. The `Sink`
  pattern from E1 stays where it is.
- **D7. Visibility.** Engine owner types: private fields, verbs and
  queries, `#[cfg(test)]` setters where tests poke state today (22 GitHub
  and 38 fetch writes in tests). Front-end view structs: `pub(super)`
  fields, readable through `&App`, writable only from `app/`. Plain fields
  on `App`: `pub(crate)`. Getters only where an invariant exists.

## Tickets and order

| # | Ticket | Files | Kind | Beside F1 | Beside U1 |
|---|---|---|---|---|---|
| 1 | E3 `GithubSync` | `app.rs`, `main.rs`, new `engine/github_sync.rs`, `engine/mod.rs`, tests, `.claude/rules/github.md` | signature change (D2, D3) | no | yes |
| 2 | E2b `FetchState` | `app.rs`, `main.rs`, `engine/fetch.rs`, tests | signature change | no | yes |
| 3 | F1 | `main.rs` split | move | | yes |
| 4 | A1b overlays and query | `app.rs` to `app/overlays.rs`, `app/query.rs` | move | yes | yes |
| 5 | A1a `RenderFeedback` | `app.rs`, `app/feedback.rs`, five `ui/` files, one `main.rs` line, tests | move (prefix rename) | after F1 | no |
| 6 | A1c to A1h, one per view: ci, notes, changes with worktrees, digest, contrib, yank | `app.rs`, `app/<view>.rs`, the `ui/` readers, tests | move (field-path rename) | yes | notes and ci after U1 |
| 7 | A1i navigation into `app/panes.rs` and `app/selection.rs` | `app.rs`, tests | move | yes | yes |
| 8 | R2 feedback redesign (D4's second half) | `ui/*`, `app/feedback.rs`, the loop | restructure, normal review | after F1 | after U1 |

E3 before E2b because `FetchBackoff` calls the ladder E3 moves (ruled on
#163). U1 before A1a and the view tickets, since it shrinks `detail.rs`
before they rename sites in it. Gate for every ticket: no open tree on its
files. Acceptance for the moves: the split design's rules. For the two
signature changes the reviewer judges three things: the moved body differs
from the base only by `self.` rewrites and parameter plumbing; every test
that set a field now calls a `#[cfg(test)]` setter with the same value; and
D3's equivalence is pinned by a test on the startup path.

Test cost, counted: 170 `App::new(` sites across 11 test files; 662 direct
`app.<field> =` writes in tests (GitHub 22, fetch 38, cells 145, per-view
104 references). Each ticket's test edit is the same path rename as its
production edit, except the 60 writes into private fields, which become
setters.

## Readability at the end

- `src/app.rs` stays the root: the struct with its field docs, `new`, tick,
  status, search and help; target under 800 production lines. Each
  `app/*.rs` under 600 (est.). `engine/github_sync.rs` about 550.
- A `//!` on every file, three lines: what it owns, what writes it, which
  design doc holds its rules. `app.rs`, `main.rs`, `git.rs`, `github.rs`
  and `model.rs` have none today.
- Names say the role: `app.github`, `app.fetch`, `app.drawn`, `app.ci`,
  `app.notes`; verbs on owners (`claim_attempt`, `begin_sweep`), queries
  as nouns (`pace()`, `limit_until()`).

## Risks

- **The `app <-> ui` cycle is not cut by A1.** `app.rs` references
  `crate::ui` at 11 production sites and holds ratatui types in three
  structs. Both are front end, so the layer test permits it. A1a confines
  the held feedback types; R2 can move `DetailHits` and `ListHits` into
  `app/feedback.rs`; the five geometry and colour helpers are a small
  ticket after U1. It stays a cycle until then.
- **R2 changes `ui::render`'s signature** and every pane function that
  writes feedback (24 sites): the one ticket here that is not a move.
- **Test coupling** is the cost driver (662 writes, 170 constructions). The
  reviewer's risk is a test silently weakened by a setter that no longer
  asserts the same thing, so setters take the same value type as the field.
- `apply_github_poll_at` (151 lines), `handle_normal_input` (375),
  `run_event_loop` (240) and `main` (159) are over the function limit; E3
  shortens the first a little, the rest are S1.

## Owner items

- **O-A** A2 lets `App`'s view state hold ratatui types. After R2 that
  could tighten to "only `ui/` and `RenderFeedback`": a change to A2's
  wording, his.
- **O-B** Whether `app` must sit below `ui` (cutting the in-layer cycle)
  is not in A1 to A6. Options: leave it, or add the rule that `ui` imports
  `app` and never the reverse (one more ticket after U1). Recommendation:
  add the rule after R2, when it is nearly true.
- **O-C** D3 drops the `config` parameter on the poll decisions because
  the interval and cost are fixed per process. A live poll-interval setting
  later hooks into `GithubSync::set_interval`; nothing else changes.
- **O-D** Order: E3 and E2b before F1 (the split design's O4), or F1 first
  so they touch smaller files. Same tickets either way; the architect leans
  to the existing ruling.
