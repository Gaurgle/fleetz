# Module split design

**Status: proposed 2026-11-07, redrawn the same day from the target shape in
`docs/architecture.md` section 7 (accepted).** A plan: no ticket here is
authorized until Andreas files and dispatches it. Every split waits for the
gate below; L1 does not.

## Why

`docs/architecture.md` has the measurements: `main.rs` 4,820 production
lines, `app.rs` 5,990, `ui/detail.rs` 3,984, `git.rs` 2,245; seven
dependency edges against the layers; no engine layer, so print mode borrows
the assembly from `main.rs`. The seams below come from the four layers, not
from where functions sit today: first the engine is pulled out of `main.rs`
and `app.rs`, then the backward edges are cut, then the front ends are split.

## Rules for every split

- The limit is about 1200 production lines per file and 150 per function: a
  signal to find a seam, not a quota. A file under it stays.
- Moves only, unless the ticket says otherwise. Visibility widens only as
  far as the move needs (`pub(crate)`, `pub(super)`); no rename, no logic
  change.
- Acceptance: the sorted `cargo test -- --list` output is identical before
  and after, `cargo fmt --check`, `cargo clippy --all-targets -- -D
  warnings` and `cargo test` are clean, `git diff -M --stat` shows moves,
  and the L1 allowlist shrinks or stays, never grows. A moved body is
  compared with the base after whitespace and trailing-comma
  normalization: rustfmt reflows a body that changed indentation, and
  may drop the braces of a single-expression closure (T1, FZ-104).
- `.claude/rules/*.md` follow the code in the same ticket: their `paths:`
  frontmatter must name the new files, or the invariants stop loading, and
  references such as `main.rs::assemble_view` must point at the new home.
  The same goes for `CLAUDE.md` (the Nerd Font list), `docs/ideas.md` and
  the as-is sections of `docs/architecture.md`.
- Tests follow the code they cover.
- One ticket per seam. Each conflicts with every open tree that touches its
  files, so it starts only when none does.

## Gate

No split starts while an open ticket tree touches its files. Each tree is
merged or explicitly dropped first. The lead checks the gate live (the
issues' state, `git worktree list`, `origin` branches) before each split;
the table below is the 2026-11-07 snapshot and was already stale on 11-09
(four of its trees had closed; FZ-91 merged `8ac7ff1`). Where they lived,
checked 2026-11-07 on
RoosMacbookA (this repo's `git worktree list`, `origin` branches, the
handoff and the issue records):

| Ticket | Where the work is | Resumable from RoosMacbookA |
|---|---|---|
| FZ-83 (#140) | Uncommitted tree on at-a (the smbp), diff `5f4c44d114ce` | No: only on at-a until committed and pushed there |
| FZ-65 (#121) | Uncommitted in `issue-121` on at-a, diff `9f4018d7160d`, held for his wheel decision | No: only on at-a |
| FZ-38 (#88) | Uncommitted in `issue-88`, parked; not on RoosMacbookA, so at-a by elimination | No |
| FZ-95 (#152) | Pushed: `origin/feat/fz-95-sections` at `c3c6830` | Yes, after FZ-83 (both rewrite `header_lines`) |
| FZ-91 (#148) | Pushed: `origin/feat/148-strip-median` at `e74fe67` | Yes |
| FZ-87 (#144), FZ-90 (#147) | No code yet; their clean worktrees are gone | Yes: start fresh from the rulings on the issues |
| FZ-60 (#116) | Uncommitted in `issue-116` here, base `41ecd76` (10-30), 112 commits behind `main` | Yes, here only |
| FZ-63 (#119) | Uncommitted in `issue-119` here, same base | Yes, here only |
| #81 | Uncommitted test edits in `issue-81` here, base `84a06b7` (10-17) | Yes, here only; waits on his yes |
| #45 | Uncommitted CI edits in `issue-45` here (no source) | Yes, here only; a CI change, his call |

## Branch check, 2026-11-07

Measured on the trees reachable from RoosMacbookA, each against its own
base: dependency edges, files over 1200 production lines, new `AppEvent`s.
The at-a trees are unmeasured.

| Tree | Edges | Size | Events | Verdict |
|---|---|---|---|---|
| FZ-95 | none | `ui/detail.rs` 3,971 to 4,057; `ui/mod.rs` shrinks | none | fits with note: grows `header_lines`, already one of the five functions over 200 lines |
| FZ-91 | none | `github.rs` +3 | none | fits |
| FZ-60 (`issue-116`) | adds `git` to `notes` (sideways, between sources) | `app.rs` +274, `git.rs` +332, `main.rs` +68, `ui/detail.rs` +308 | adds `ChangeDiff`, `ChangesRead` | fits with note: the new edge and events go into `docs/architecture.md` when it merges; the 10-30 base makes it the costliest tree to carry through a split |
| FZ-63 (`issue-119`) | none | `main.rs` +96, small growth elsewhere; new `ui/todo.rs`, 452 lines | none | fits |
| `issue-81` | none | none over 1200 | none | fits |

No tree adds an edge against the layers. The risk the gate guards is
conflict, not structure.

## Tickets

IDs are placeholders; the coordinator assigns FZ numbers when filing.

**L1, the layer test (first, not gated).** A unit test, `#[cfg(test)] mod
architecture;` in `main.rs` with the body in `src/architecture.rs`, that
reads every `src/**/*.rs`, strips `#[cfg(test)]` items, and fails on:
- an import from a layer above the importer (A1), using a table of module
  to layer kept in the test: front ends `main`, `app`, `ui`, `report`,
  `cli`; engine (none yet); sources `git`, `github`, `ci`, `cache`,
  `config`, `discovery`, `notes`, `clipboard`, `window_state`; domain
  `model`, `time`, `graph`, `contributions`, `digest`, `fuzzy`;
- `ratatui` or `crossterm` outside `main`, `app` and `ui` (A2);
- `report` importing `main`, `app` or `ui` (A3);
- an allowlist entry that no longer occurs, so the list can only shrink.

The allowlist starts with today's violations, measured by the test itself:
`ci` to `app`, `config` to `app`, `model` to `git`, `model` to `github`,
`report` to `main`, `report` to `app`. Std only, no dependency. It touches
`main.rs` by one line, so it can land while other trees are open, and it
guards them as they resume. Section 7 of `docs/architecture.md` then names
it as A6's check.

**T1, tests into sibling files.** Every file whose total passes 1200 lines
moves its `mod tests` body to `<module>/tests.rs` (`src/app.rs` reads
`src/app/tests.rs`; the crate root `src/main.rs` reads `src/tests.rs`,
`ui/mod.rs` reads `src/ui/tests.rs`). Other test modules go to sibling
files of their own name, so test paths stay. Test-only helpers outside
the test modules stay. About
35k lines leave the code files at no design cost; the reviewer checks the
moved text is byte-identical apart from one level of indentation. First
after the gate, so every later diff is readable. Waits on the test
convention ruling (owner items).

**E1, the engine: assembly.** Create `src/engine/` and move the assembly
out of `main.rs`: `load_repos`, `assemble_view*`, `unclaimed_local`,
`mark_local_carriers`, `carries`, `names_remote`, `remote_key`,
`RemoteKey`, `build_repo_status`, `local_only_status`, `probe_concurrency`
and their helpers. `report` calls the engine; `report` to `main` leaves the
allowlist. `.claude/rules/discovery.md` follows.

**E2, the engine: sweep and fetch planning.** From `main.rs`:
`SweepTarget`, `sweep_targets`, `plan_sweep_fetch`, `FetchAsk`, the fetch
skips and held paths. From `app.rs`: `FetchClaims`, `FetchBackoff`,
`FetchReport`, `FetchTally`. `report` to `app` leaves the allowlist.

**E3, the engine: GitHub poll scheduling (needs design first).**
`GithubFreshness`, `ApiBudget`, `GithubBackoff` and the due and backoff
decisions. Unlike E1 and E2 this separates decisions from `App` mutation,
so it starts with an `fz-architect` consult and a short design, then moves.
`.claude/rules/github.md` follows.

**D1, cut the remaining backward edges.** Small tickets, one edge each:
`model` to `git` and `github` (the shared types move down into the domain),
`ci` to `app` (`Pace`, `RemoteKey`, `carrier_key` to the engine), `config`
to `app` (`View`, `Icons` and the pane defaults become config's own value
types). The allowlist is empty after D1.

**F1, split the TUI front end.** What remains of `main.rs` after E1 and E2
splits into `main.rs` (startup, about 200 lines), `event.rs` (`AppEvent`,
the loop, the tick, input and loading tasks), `input.rs` (key and mouse
handlers) and `handoff.rs` (editor, opener, CI handoff).

**A1, `App` by concern (needs design first).** Rule A4: concerns with rules
of their own become owned types, `App` holds them, and render feedback
becomes one declared struct instead of 18 `Cell` fields. An `fz-architect`
consult and a design come first; the move into `app/` follows.

**U1, `ui/detail.rs` by pane.** Layout splits (`split_main`,
`split_inspector`, named in `.claude/rules/ui.md`), header, graph list,
inspector, notes pane.

**G1, `git.rs`.** Seams named by an `fz-architect` survey after E1.

**S1, long functions.** The five above 200 lines, one ticket each, after
F1 and U1. These restructure code, so they get the normal review, not the
move acceptance.

**Not planned:** `github.rs` and `ui/mod.rs` (1.6k), `ui/markdown.rs`,
`ci.rs`, `model.rs` (1.0k to 1.2k). Revisit only if a `Structure` note or a
survey flags them.

## Owner items

- **O1** File and dispatch L1 now, while the gate is still closed.
- **O2** The test convention for T1: sibling `tests.rs` files once a file
  passes about 1200 lines (drafted in `CLAUDE.md`), or tests stay inline.
- **O3** A ruling on each gated tree: merge or drop. The at-a trees (FZ-83,
  FZ-65, FZ-38) can only be resumed or pushed from at-a.
- **O4** The order after L1 and T1: E1 to E3 and D1 before F1, A1 and U1,
  as above.
