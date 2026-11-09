# fleetz architecture

The map of the code: what each module owns, which way dependencies point,
how data moves at runtime, who may change which state, and where the
structure has drifted from where it should be. It is the overview for
Andreas and the reference for every agent that plans or reviews a change.

**Status.** Sections 1 to 6 describe the code as it is, measured on `main`
at `270bd71` (2026-11-09, after L1, T1, E1, E2a and D1). Section 7 is the
target, accepted by Andreas on 2026-11-07 with every owner item in section 8.

**How to read it.** Everyone reads section 1. Agents then read the sections
their change touches, by heading, never the whole file. Domain rules live in
the design docs and `.claude/rules/`; this file links them and does not
repeat them (section 6).

## 1. Overview

fleetz is one Rust binary with two front ends over the same data:

- **TUI** (default): an async, channel-driven dashboard. The terminal opens
  on an empty `App`; background tasks load discovery, git and GitHub data
  and send results as `AppEvent`s; one event loop applies them and redraws.
- **Print mode** (`--print`, `src/report/`): repoz's one-shot text report
  rebuilt on fleetz's own discovery, git and GitHub code. It branches off
  in `main` before any terminal setup (`docs/repoz-print-design.md`, D1).

```mermaid
flowchart LR
  subgraph sources[Background tasks, tokio]
    L[spawn_load]
    S[spawn_sweep]
    P[spawn_github_poll]
    C[spawn_ci_poll / ci_detail]
    N[notes, note read, render]
    D[digest, worktree log]
  end
  IN[input thread] --> CH
  TK[tick task, 100 ms] --> CH
  sources --> CH[(mpsc channel<br/>AppEvent)]
  CH --> LOOP[run_event_loop<br/>event.rs]
  LOOP -->|handlers mutate| APP[App<br/>app.rs]
  APP -->|read by| R[ui::render<br/>ui/mod.rs]
  R --> TERM[terminal]
  R -. writes layout back<br/>through Cell fields .-> APP
  LOOP -->|starts| sources
```

External systems: `git` as a subprocess (`git.rs`, one `Command::new("git")`
helper), GitHub over HTTPS with `reqwest` (GraphQL in `github.rs`, REST
Actions and the default branch tip's commit statuses, check runs and
status history in `ci.rs`) using the token from `gh auth token`, and the disk:
the API cache (`cache.rs`), the config and settings file (`config.rs`),
the saved time window (`window_state.rs`) and the notes walk (`notes.rs`).

## 2. Module map

Production lines are a file's lines with every `#[cfg(test)]` item removed
(blank and comment lines count), and the files a `#[cfg(test)] mod x;`
reads are left out entirely: the sibling `tests.rs` files (FZ-104),
`discovery/repoz_parity.rs` and `src/architecture.rs` (the layer test,
470 lines). "Effects" is what the module does beyond computing: `proc`
runs a program, `net` talks HTTP, `fs` touches the disk, `term` draws or
reads the terminal. Every row below was measured on `270bd71` by one
script over the whole tree, but for the seven `main.rs` rows, measured
after F1 (FZ-109) split it, and the two `model` rows, measured after
FZ-111 split it; rows are in size order.

| Module | Lines | Owns | Effects |
|---|---|---|---|
| `ui/` (28 files) | 13,402 | Drawing every view from `&App`; `detail/` is ten files by pane since FZ-108, 4,751 together: `pane.rs` 1,030, `notes.rs` 612, `mod.rs` 530 (the entry `render` and the helpers the panes share), `commits.rs` 501, `header.rs` 482, `list.rs` 403, `inspector.rs` 377, `worktrees.rs` 355, `changes.rs` 301, `layout.rs` 160; then `mod.rs` 1,541, `markdown.rs` 1,245, `ci.rs` 1,072, `digest.rs` 699, the other 14 files 2,992 together | term |
| `app.rs` | 6,184 | `App`: all TUI state (115 fields), the overlays, filters and query parsing, API budget and backoff, per-view caches for ci, notes, worktrees, changes and digest | none (holds ratatui types) |
| `git.rs` | 2,444 | Every git read and fetch, reading a fetch's outcome and progress and a checkout's `WorkKey` (the types live in `model`), the `changes` read (`changes`, `changed_file_diff`), the note body read | proc, fs |
| `report/` (3 files) | 2,057 | Print mode: run (`mod.rs` 910), golden parity harness (`parity.rs` 765), render byte for byte as repoz (`render.rs` 382) | proc, fs |
| `github.rs` | 1,567 | The GraphQL query and its cost, the HTTP client, token lookup, failure kinds; parsing the payload into `model::GithubRepo` | net, proc |
| `engine/` (6 files) | 1,562 | `assembly.rs` 559: view assembly (discovery passes, carriers, activity order, the startup fetch baseline); `fetch.rs` 333: fetches under a claim (`FetchClaims`) and the types the sweep plans with (`SweepTarget`, `FetchAsk`, `FetchTally`, `FetchReport`, FZ-106); `status.rs` 313: a row's status probe; `load.rs` 191: the load (GitHub dataset or dirty mode); `carriers.rs` 126; `mod.rs` 40: `Sink`, which a front end's channel implements (FZ-105) | proc, fs, net |
| `ci.rs` | 1,538 | Workflow runs, jobs and log tails over REST, with ETags; the default branch tip's `TipStatus`, `TipChecks` and `StatusEvent` history, and `tip_verdict` over them | net |
| `input.rs` | 1,081 | Every input handler: keys, mouse, the overlays' keys, the settings overlay, `r`, `R` and `f` | proc, term |
| `model.rs` | 953 | `RepoStatus` and the row types the views draw; the types the sources fill (`WorkKey`, `GithubRepo`), re-exporting the fetch record from `model/fetch.rs`; remote identity (`identity_key`, `RemoteRef`, `RemoteKey`, `carrier_key`) | none |
| `poll.rs` | 747 | GitHub poll scheduling and apply, the account rebuild, the CI tick, spawning the background reads (CI, worktree log, notes, changes, digest) | none |
| `event.rs` | 634 | `AppEvent`, `LoadResult`, the input thread, the tick and loading tasks, the event loop (`run_event_loop`) | term |
| `discovery.rs` | 611 | Finding clones under roots and `$HOME`, clone matching (`Wanted`, `CheckoutRank`) | fs |
| `cache.rs` | 517 | The keyed on-disk API cache | fs |
| `sweep.rs` | 511 | The local sweep: targets, fetch planning and claims, spawning it, landing a repo's result, the activity order | none |
| `graph.rs` | 471 | Commit graph lanes and glyphs | none |
| `notes.rs` | 443 | Walking a repo for notes and markdown, the capped body read | fs |
| `contributions.rs` | 431 | The 53 by 7 contribution grid | none |
| `load.rs` | 390 | Spawning the load, the window apply and the memory rebuild; applying a `LoadResult` | none |
| `config.rs` | 349 | Config file, defaults, settings writes; the values its keys select (`View` and `initial_view`, `Icons`, the pane defaults) | fs |
| `handoff.rs` | 347 | Editor, opener, `gh` and lazygit handoffs: suspending and reclaiming the terminal | proc, fs, term |
| `digest.rs` | 333 | The slot digest model | none |
| `time.rs` | 249 | Time windows and slots; the poll `Pace` | none |
| `model/fetch.rs` | 249 | The record of a fetch (FZ-111): `FetchFailureKind`, `FetchFailure`, `FetchOutcome`, `FetchPhase`, `FetchProgress`, `FetchFacts` and `SizeHold` | none |
| `main.rs` | 217 | Startup and teardown (`main`), the `mod` lines | term |
| `clipboard.rs` | 137 | Copying a hash to the system clipboard | proc |
| `cli.rs` | 115 | Flags (`clap`) | none |
| `fuzzy.rs` | 81 | Fuzzy match for search | none |
| `window_state.rs` | 44 | The remembered time window | fs |

Module docs (`//!`) exist on `engine/`, `report/`, `ci`, `notes`, `graph`,
`digest`, `contributions`, `fuzzy`, `clipboard` and nine `ui/` files
(`ci`, `contributions`, `digest`, `grips`, `highlight`, `intro`,
`markdown`, `scroll`, `todo`), but not on `main`, `app`, `git`, `github`,
`model`, `model/fetch`, `discovery`, `cache`, `config`, `cli`, `time`,
`window_state`,
`ui/mod`, `ui/detail`, `ui/badges`, `ui/branches`, `ui/colors`,
`ui/compact`, `ui/footer`, `ui/help`, `ui/settings` or `ui/timewindow`:
22 files, the first nine the ones a newcomer most needs.

**Structure notes** (measured drift from the limits in `CLAUDE.md`: about
1200 production lines per file, 150 per function; the seams are tickets in
`docs/module-split-design.md`):

- Files over the limit: `app.rs` 6,184 (A1), `git.rs` 2,444 (G1),
  `github.rs` 1,567, `ui/mod.rs` 1,541, `ci.rs` 1,538, `ui/markdown.rs`
  1,245 (the last four are not planned: revisit if they grow).
  `ui/detail.rs` (U1, FZ-108) and `main.rs` (F1, FZ-109) are split.
- `model.rs` is split (FZ-111): the fetch record moved to
  `model/fetch.rs`, which `model` re-exports, so every `model::` path and
  the `git` re-export keep resolving. `WorkKey` stays in `model.rs`, beside
  the `RepoStatus` field it fills. A pure move inside the domain layer; no
  edge changes.
- Functions over 200 lines, with their homes at `270bd71` (S1, one ticket
  each, after F1 and U1): `ui::detail::render_detail_pane`
  (`src/ui/detail.rs:816-1513`, 698 lines), `main::handle_normal_input`
  (`src/main.rs:1749-2119`, 371), `engine::assembly::assemble_view_held`
  (`src/engine/assembly.rs:324-579`, 256), `main::run_event_loop`
  (`src/main.rs:961-1197`, 237) and `ui::detail::header_lines`
  (`src/ui/detail.rs:1976-2197`, 222). Four more sit between 150 and 200:
  `ui::branches::render` 193, `ui::detail::graph_list_lines` 184,
  `ui::detail::render_inspector` 175, `main::main` 156.

## 3. Dependencies as they are

Who uses whom, from `crate::` paths in production code (`main.rs` is the
crate root and names modules bare, so its edges come from `<module>::`
paths). Arrows point from user to used. No edge breaks the rules in
section 7: the layer test's allowlist is empty since FZ-107 (`report` to
`main` left with FZ-105, `report` to `app` with D1's `Icons`, `ci` and
`config` to `app` and `model` to `git` and `github` with FZ-107, by moving
the shared types down). What remains is one cycle inside one layer,
drawn dotted, which A1 does not forbid:

- `app` and `ui`, both front ends. `app` names `ui` at 11 sites: three
  field types (`src/app.rs:1749` `ui::detail::DetailHits`, `:1772`
  `ui::intro::Loading`, `:1831` `ui::Divider`), `ui::colors` (`:2640`),
  `ui::ci::run_status` and `shown_log` (`:4036`, `:5107`),
  `ui::detail::repo_list_width`, `RECENT_COMMIT_COUNT` and `DetailClick`
  (`:4511`, `:4687`, `:5130`) and `ui::contributions::weeks_dragged` and
  `cols_for_weeks` (`:5436`, `:5441`); `app.rs` holds `ratatui` types at
  four sites. `ui` names `app` in ten of its files (`footer.rs` 22
  sites, `mod.rs` 17, `detail.rs` 13, `compact.rs` 13). A4 is the fix
  (the A1 ticket): the view state and render feedback get owned types.

The `cache` and `github` cycle ended with FZ-112: `fnv1a_64` lives in
`cache`, and `github.rs` takes `cache::ApiCache` and `cache::KeyedCache`
at 11 sites, so the edge runs one way.

Sideways edges inside a layer, allowed and worth knowing: `ci` to `git`
and `github`; `git` to `notes` (the note body read, FZ-60); `notes` to
`discovery` (`PRUNE_DIRS`); `model` to `contributions` and `graph`;
`digest` to `model` and `time`.

```mermaid
flowchart TB
  subgraph front[Front ends]
    main
    report
    ui
    app
  end
  subgraph eng[Engine]
    engine
  end
  subgraph io[Sources]
    git
    github
    ci
    cache
    config
    discovery
    notes
  end
  subgraph dom[Domain, pure]
    model
    contributions
    graph
    digest
    time
    fuzzy
  end
  main --> app & ui & report
  ui --> app
  app -.-> ui
  main --> engine
  report --> engine
  app --> engine
  engine --> io
  app --> io
  main --> io
  report --> io
  git --> model
  github --> model
  github --> cache
  io --> dom
```

The edges that matter, with what crosses them:

| Edge | What crosses | Why it is a problem |
|---|---|---|
| `app` to `ui`, `app` to ratatui | `DetailHits`, `DetailClick`, `Divider`, `Loading`, colors, `run_status`, `shown_log`, layout widths; `Rect`, `Line`, `Color` | State depends on drawing, so neither can change alone |

## 4. Runtime and data flow

**Startup** (`main.rs::main`): parse flags; print mode returns here; load
config; build an empty `App` and apply config and flags to it; install the
panic hook that restores the terminal; enter raw mode and the alternate
screen; open the channel; start the input thread and the tick task; remove
abandoned cache temp files; `spawn_load` the initial load; run the event
loop; restore the terminal; save pane sizes if `persist_pane_sizes` is set.

**Events.** Every background result reaches state through one channel and
one loop (`run_event_loop`), so state changes happen on one task in a
defined order. Sent by functions in `event.rs`, `load.rs`, `sweep.rs` and
`poll.rs` unless noted.

| `AppEvent` | Sent by | Applied by |
|---|---|---|
| `Input` | `event::spawn_input_thread` | `handle_key`, `handle_mouse_input` and the handlers they call |
| `Tick` | `event::spawn_tick_task` | the loop: `App::on_tick`, status expiry, the watch sweep and GitHub poll when due |
| `YankFrame` | `event::next_event` (the loop's own wait) | the loop |
| `LoadingFrame` | `event::spawn_loading_task` | `on_loading_frame` |
| `Loaded` | `load::spawn_load`, `spawn_window_apply`, `spawn_memory_rebuild` | `apply_load_result` |
| `Progress` | `engine::assemble_view_held`, through `engine::Sink` | the loop |
| `RepoUpdated` | `sweep::spawn_sweep` | `apply_repo_updated` |
| `FetchProgress` | `engine::fetch_under_claim_capped`, through `engine::Sink` | the loop |
| `SweepDone` | `sweep::spawn_sweep` | `apply_sweep_done` |
| `Github` | `poll::spawn_github_poll` | `apply_github_poll` |
| `Ci`, `CiDetail` | `poll::spawn_ci_poll`, `spawn_ci_detail` | `App::apply_ci`, `App::apply_ci_detail` |
| `WorktreeLogRead` | `poll::spawn_worktree_log` | `App::apply_worktree_log` |
| `NotesWalked` | `poll::spawn_notes_walk` | `App::apply_notes_walk` |
| `NoteRead`, `NoteCommit` | `poll::spawn_note_read` | `App::apply_note_read`, `apply_note_facts`, `apply_note_rendered`, `apply_note_commit` |
| `NoteRendered` | `poll::spawn_note_render` | the loop |
| `ChangesRead` | `poll::spawn_changes_read` | `App::apply_changes_read` |
| `ChangeDiff` | `poll::spawn_change_diff` | `App::apply_change_diff` |
| `DigestLog` | `poll::spawn_digest_log` | `App::apply_digest_log` |

**Stale answers.** A task's answer can land after a newer one. The guards:
the poll's `attempt` id (`GithubFreshness::begin_attempt`), fetch claim
tokens (`FetchClaims`), and per-view generations and cursors for notes,
worktrees, changes and ci. Contracts: `docs/freshness-contract.md`,
`docs/fetch-outcome-contract.md`.

## 5. State and who changes it

`App` is the whole TUI state: 115 fields, all `pub`. Grouped by concern:

| Group | Examples | Changed by |
|---|---|---|
| Fleet data | `repos`, `dataset`, `github_meta`, `not_in_last_poll`, `window` | load, sweep and poll handlers in `load.rs`, `sweep.rs` and `poll.rs` |
| GitHub sync | `github_freshness`, `api_budget`, `github_backoff`, poll timing fields | poll scheduling and apply in `poll.rs`, `App` methods |
| Local fetch | `fetches_in_flight`, `fetch_backoff`, `manual_fetches`, refresh fields | sweep and fetch planning in `sweep.rs`, over the engine's fetch types (FZ-106) |
| Navigation and view | `view`, `overlay`, `selected`, `focused_panel`, `history_view`, scrolls, pane percentages | input handlers in `input.rs`, `App` methods |
| Per-view caches | `ci` (each repo's `CiTip` with it), `notes`, `note_body`, `worktree_logs`, `digest_logs` | `App::apply_*` |
| Render feedback | 18 `Cell` and `RefCell` fields such as `detail_hits`, `list_hits`, `reader_width` | `ui::render`, while drawing |

As it is, any code holding `&mut App` may change any field, and `ui`
changes some through `Cell` while it holds only `&App` (27 `.set(` and
`.borrow_mut(` sites, 19 of them in `ui/detail/`: 8 in `pane.rs`, 4 in
`inspector.rs`, 2 each in `list.rs` and `notes.rs`, 1 each in
`changes.rs`, `mod.rs` and `worktrees.rs`; then two each in
`contributions.rs`, `digest.rs` and `footer.rs`). What keeps the
behavior right today is the invariants in `.claude/rules/` and the tests,
not the types. `ApiBudget`, `GithubBackoff`, `FetchClaims` (in `engine`
since FZ-105) and
`GithubFreshness` are the exception: concerns with their own type, methods
and tests, and the pattern section 7 builds on.

## 6. Where the domain rules live

| Area | Rules and contracts |
|---|---|
| Discovery, carriers, clone matching | `.claude/rules/discovery.md`, `docs/repository-identity-design.md` |
| GitHub fetch, budget, cache | `.claude/rules/github.md`, `docs/api-budget-design.md`, `docs/coverage-cache-design.md`, `docs/freshness-contract.md` |
| Sweep and cache invalidation | `docs/cache-invalidation-design.md`, `docs/fetch-outcome-contract.md` |
| Layout, render order, status vocabulary | `.claude/rules/ui.md` |
| Commit list and graph | `.claude/rules/git-graph.md` |
| Print mode | `docs/repoz-print-design.md` |

## 7. Target shape (accepted 2026-11-07)

Four layers. Dependencies point down only.

```mermaid
flowchart TB
  F["Front ends<br/>tui: event loop, input, App, ui<br/>print: report"]
  E["Engine (no terminal)<br/>assembly and carriers, sweep and fetch planning,<br/>poll scheduling, budget, backoff, fetch claims"]
  S["Sources<br/>git, github, ci, cache, config, discovery, notes"]
  D["Domain (pure)<br/>model, time, graph, contributions, digest, fuzzy"]
  F --> E --> S --> D
  F --> D
  E --> D
```

Rules:

- **A1. Down only.** No module imports from a layer above it. It holds
  since FZ-107 (the allowlist is empty); the dotted edge left in
  section 3 is a cycle inside one layer, which A1 permits and A4 (`app`
  and `ui`) removes; the `cache` and `github` cycle ended in FZ-112.
- **A2. No terminal below the front ends.** `ratatui` and `crossterm`
  appear only in `ui/`, the event loop and `App`'s view state.
- **A3. One engine, two front ends.** The assembly that `report` borrows
  from `main.rs` today moves into the engine, and print mode and the TUI
  both call it. Print mode never imports the TUI.
- **A4. State has owners.** A concern with rules of its own gets its own
  type that owns its fields, the way `ApiBudget` does, and `App` holds
  those types instead of loose fields. Render feedback is one declared
  struct, not cells scattered across `App`.
- **A5. The engine is a module now, a crate later.** Inside the one crate,
  as print mode's D1 decided, but shaped so it can become the UI-free
  `fleetz-core` that epoz needs without a rewrite.
- **A6. Rules are checked, not remembered.** A script fails when an import
  breaks A1 or A2, in the spirit of Relay's gates in tools rather than
  prose. The check is the unit test `architecture::layers_hold_but_for_the_allowlist`
  (`src/architecture.rs`), which also fails on A3 and on an allowlist entry
  that no longer occurs.

What this means for the refactor: the seams come from these layers, not
from the order functions sit in today's files. The tickets, their order and
the gate are in `docs/module-split-design.md`.

## 8. Owner items

- **O1** Accepted 2026-11-07: the four layers, with A1 to A5.
- **O2** Accepted 2026-11-07: the engine is an in-crate module now (keeps
  D1); the `fleetz-core` crate split waits until epoz needs it.
- **O3** Accepted 2026-11-07: A6 is a unit test in the normal suite, so CI
  runs it through `cargo test` with no CI change. Today's violations start
  on an allowlist that may only shrink. It is the first code ticket
  (`docs/module-split-design.md`, L1).
- **O4** Accepted 2026-11-07: an architect role keeps this file current,
  run by the lead at set triggers and by Andreas on demand, never as a
  standing agent.

## 9. Keeping this file true

- A ticket that adds a module, moves a responsibility, adds an `AppEvent`
  or changes a dependency edge updates the matching section here in the
  same ticket.
- The numbers in sections 2 and 3 are measured by script, not by hand;
  rerun them at each milestone and after each split.
- When this file and the code disagree, the code is the fact and this file
  is the bug.
