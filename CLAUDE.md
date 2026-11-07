# CLAUDE.md - fleetz

## What this is

A Rust TUI dashboard over many git repos at once: what changed, what is dirty,
what is behind, what has open PRs, and what CI says. The interactive
counterpart to [repoz](https://github.com/Gaurgle/repoz), which prints the same
picture once and exits: repoz is what you pipe, fleetz is what you sit in.

In scope now: the three-pane detail view, the compact grid, discovery and git
probing, the GitHub GraphQL fetch with a disk cache, watch mode, and the
overlays. Unbuilt work lives in `docs/ideas.md`.

<!-- house-rules:start v1 -->
## House rules

These mirror the global config at `~/claude-config`, which is machine-local and
therefore invisible to cloud and mobile sessions. They apply here regardless of
where the session runs.

- **Never use em-dashes or en-dashes** in any output: chat, files, or code. Use
  a hyphen, a colon, parentheses, or rewrite the sentence.
- **Conventional Commits** (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`,
  `cleanup:`), first line under 72 characters. Body is for the why, not the what.
- **No `Co-Authored-By` lines.** Ever.
- **Present the full `git commit -m "..."` command for review; do not commit.**
  Andreas owns commits, merges and pushes. **"Ship it"** means prepare and
  validate the change and present the commands for review; it does not grant
  agents permission to commit, merge, push, tag, release or deploy.
- **Never edit, delete or disable a test to make code pass.** Fix the code. If a
  test looks wrong, stop and say so rather than changing it.
- **Ask before** adding or removing dependencies, changing schema or API
  contracts, touching CI config, or deleting files.
<!-- house-rules:end -->

## Current state

Used daily. All code is unit-tested in `#[cfg(test)] mod tests` child modules
of the code they cover. There is no integration test suite. CI is
`.github/workflows/ci.yml` (fmt, clippy, tests on a push to any branch, skipped
when only prose changed), `docs.yml` (runs `scripts/check-docs.sh` on a prose
push: links, the index below, no dashes) and
`release.yml`, which turns a pushed `v*` tag into a GitHub
release: it checks the tag against `Cargo.toml`, runs fmt, clippy and tests,
and writes commit-based notes with `scripts/release-notes.sh`. PRs are optional.

The event loop is channel-driven and async: the TUI opens instantly, GitHub and
local discovery load in the background (`AppEvent`), a 100ms tick drives spinner
and blink, watch mode re-sweeps on an interval.

## Where things are written down

| Path | What it holds |
|---|---|
| `README.md` | User-facing: install, flags, keybindings, config keys |
| `AGENTS.md` | Codex entrypoint to these project rules and the shared Relay workflow |
| `docs/ideas.md` | Design backlog: problem, options, rejected alternatives per feature |
| `docs/release-plan.md` | Release milestones, ticket status and dispatch order |
| `docs/agent-workflow.md` | Shared worker/reviewer instructions and integration rules |
| `docs/agent-handoff.md` | Coordinator state between leads: read first when taking over |
| `docs/agent-log.md` | Finished tickets and earlier lead records, moved out of the handoff, each with a metrics line |
| `scripts/relay-status` | The Relay status block: version, lead and baton, `main` and CI, worktrees, next step |
| `scripts/relay-check` | Reports Relay template drift; the coordinator runs it on start |
| `scripts/relay-sweep` | Removes merged, clean ticket worktrees once `main` CI is green |
| `docs/performance-baseline.md` | Measured startup, idle, refresh and input baseline, with fixtures and commands to rerun, and the print mode comparison against bash repoz |
| `docs/repository-identity-design.md` | Accepted design: remote and checkout identity, collision scenarios, ordered tickets FZ-05a to f |
| `docs/cache-invalidation-design.md` | Accepted design: what the sweep's `WorkKey` misses, the ref-content signature, fix tickets FIX-A to E |
| `docs/freshness-contract.md` | Accepted contract for GitHub data freshness: attempt, outcome, data time, header states, and the FZ-10 amendments |
| `docs/fetch-outcome-contract.md` | Accepted contract for per-repo `git fetch` outcomes (FZ-04): last success, attempt, outcome |
| `docs/api-budget-design.md` | Accepted design for the GitHub API budget, pacing and retry (FZ-10): decisions A1 to A7, owner items O1 to O5, tickets FZ-10a to c |
| `docs/repoz-print-design.md` | Proposed design: bash repoz rebuilt as a print mode of this binary, repoz's look held by a golden test, decisions D1 to D9, tickets FZ-46 to FZ-51, owner items O1 to O8 (naming deferred) |
| `docs/architecture.md` | The code map: modules and what they own, dependencies, runtime flow, state ownership, measured drift, and the proposed target layers. Read section 1 first |
| `docs/module-split-design.md` | Proposed plan from the target layers: the layer test (L1), tests into sibling files, the engine out of `main.rs` and `app.rs`, the front-end splits; the gate on open trees and where each lives |
| `docs/coverage-cache-design.md` | Design for GitHub coverage and cache isolation (FZ-09): what the payload covers, the cache keyed by account, host and version, decisions C1 to C9, tickets FZ-09a to f, owner items accepted 2026-10-02 |
| `.claude/agents/` | Claude-native workforce: `fz-coordinator`, `fz-worker`, `fz-reviewer`, `fz-small`, `fz-architect` |
| `.claude/rules/` | Domain invariants, one path-scoped file per area; loaded when a matching source file is touched |
| `docs/superpowers/` | Dated plans and specs, paired, for landed work |

Read the relevant `docs/ideas.md` section before building anything it covers:
each records alternatives already rejected and why, the expensive part to
rediscover. Its **Status: shipped** sections are kept for reasoning only;
everything else is unbuilt, and its line references drift.

## Working conventions

- Claude and Codex follow this file and `docs/agent-workflow.md`. On a Relay
  takeover, read the live handoff and honor its stop and confirmation
  conditions; entering a lead session is not authorization to resume tickets.
- **One feature branch and worktree per implementation ticket.** Branch from
  the agreed `main` commit, for example `fix/fz-02-preserve-github-state`.
  PRs are optional. Agents do not implement tickets directly in the main
  checkout.
- **Ship policy:** this specializes the house rule above for the release
  effort. The coordinator commits the reviewed diff on the ticket branch,
  pushes it, validates the combined result against current `main`
  (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`) and merges under the safe-merge rule in
  `docs/agent-workflow.md`. Workers and reviewers never commit. When a
  condition of that rule fails, or outside the release effort, present the
  commands for Andreas. `cargo install`, tags and releases stay his.
- **Release policy:** prepare the requested version bump and What's Changed
  notes, run the release checks, and present commands for Andreas. The
  `release.yml` workflow publishes when he pushes the version tag. Never
  recommend releasing a dirty tree or a red build.
- **Versioning:** `main` keeps the last released version until a release
  is cut; the bump is one commit, the last before the tag, never per
  ticket and never left uncommitted. While the major is 0: a new feature,
  key, config key, cache format or contract change makes the next release
  a minor (`0.X.0`); fixes and docs alone make it a patch (`0.x.Y`). The
  class is decided from everything since the previous tag, so a prepared
  bump stays valid as long as later work fits its class; `1.0.0` is the
  public release (M5).
- Tests live in a `#[cfg(test)] mod tests` child module of the code they
  cover, not in `tests/`, so they reach private items: inline while the file
  is small, in a sibling file once they push it past about 1200 lines
  (`mod tests;` in `src/app.rs` reads `src/app/tests.rs`).
- **Files stay under about 1200 lines of production code**, functions under
  about 150. Past either, flag the seam; never split in passing. Splits are
  refactor tickets (`docs/agent-workflow.md`, `docs/module-split-design.md`).
- Every behavior change lands with a test that would have caught the old
  behavior; many existing tests name the regression they guard. Match that.

## Build

```
cargo test                    # whole suite; `cargo test <name>` for one
cargo clippy --all-targets    # must be clean
cargo fmt                     # must be clean before committing
cargo install --path .        # to ~/.cargo/bin (the `fleetz` on PATH)
```

Needs a Nerd Font for five uses of its codepoints: the repo kind glyph
(`ui/badges.rs::kind_glyph`), the stash mark (`badges::STASH_GLYPH`), the watch
eye (`WATCH_ON_GLYPH` and `WATCH_OFF_GLYPH` in `ui/mod.rs`), the failed-fetch
marker's warning icon (`badges::FETCH_WARNING_GLYPH`) and the contribution
strip's day squares (`app::CellSize::glyph`). Without one they are tofu. The
`icons = "plain"` config key, or the `,` settings row (`app::Icons::Plain`),
swaps the first three for ASCII stand-ins and the warning icon for `⚠`; the
day squares have no plain fallback. It also needs an authenticated `gh` on PATH, which the GitHub fetch
uses.

## Domain invariants - do not violate

The invariants are binding wherever the session runs. They live in
`.claude/rules/`, one file per area, path-scoped so a session loads a file
when it reads or edits a matching source file and pays for nothing else. A
loader without path scoping (Codex, a subagent that skips project rules)
reads the file for the module it is about to change:

| File | Loads for | Holds |
|---|---|---|
| `.claude/rules/discovery.md` | `discovery.rs`, `main.rs`, `model.rs` | pass order, carrier rows, origin-first clone matching, pass 3 dedupe, dot-prefixed clones, hidden directories, repoz parity |
| `.claude/rules/github.md` | `github.rs`, `cache.rs`, `contributions.rs`, `app.rs`, `main.rs` | batch on GraphQL, `first:` billing, launch from cache, fetch widening, `contributionsCollection` caching, cache only `data` |
| `.claude/rules/ui.md` | `ui/`, `app.rs`, `contributions.rs`, `model.rs` | `panel_rects` and `render`, `visible_repos` order, one color ladder, contribution day glyph, `contributionLevel`, status vocabulary |
| `.claude/rules/git-graph.md` | `git.rs`, `graph.rs`, `ui/detail.rs` | computed graph, commit list and graph on the same refs |

A new invariant goes into the file whose paths it governs; a cross-cutting
one goes here.

## Relationship to repoz

Behavioral parity with repoz is a design goal, not an accident: `EXTRA_DIRS`,
the dot-prefixed name match, `section_div` headers, the glyphs, push ordering
and the time-slot logic all mirror it. When the two disagree, repoz has usually
been right longer: read `~/repos/repoz/repoz` before assuming fleetz is right.
fleetz filters archived repositories server-side (`isArchived: false`) and
repoz does not, a deliberate divergence (#69).
