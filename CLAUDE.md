# CLAUDE.md - fleetz

## What this is

A Rust TUI dashboard over many git repos at once: what changed, what is dirty,
what is behind, what has open PRs, and what CI says. It is the interactive
counterpart to [repoz](https://github.com/Gaurgle/repoz), a bash script that
prints the same picture once and exits. Same flag set, same time-slot logic;
repoz is the thing you pipe, fleetz is the thing you sit in.

In scope now: the three-pane detail view, the compact grid, discovery and git
probing, the GitHub GraphQL fetch with a disk cache, watch mode, and the
overlays (help, branches, time window). Not built yet: a settings menu, themes,
hidden repos. See `docs/ideas.md`.

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
  The human commits and pushes.
- **Never edit, delete or disable a test to make code pass.** Fix the code. If a
  test looks wrong, stop and say so rather than changing it.
- **Ask before** adding or removing dependencies, changing schema or API
  contracts, touching CI config, or deleting files.
<!-- house-rules:end -->

## Current state

Used daily. Roughly 7.5k lines of Rust across 21 files, all unit-tested in
`#[cfg(test)]` modules in the same file as the code they cover. There is no
integration test suite and no CI workflow.

The event loop is channel-driven and async: the TUI opens instantly and GitHub
plus local discovery load in the background (`AppEvent`), with a 250ms tick
driving spinner and blink. Watch mode re-sweeps on an interval.

`docs/ideas.md` is the backlog. Sections marked **Status: shipped** are kept for
their reasoning only; everything else is unbuilt. Line references inside it drift
as the code moves, so verify one before trusting it.

## Where things are written down

| Path | What it holds |
|---|---|
| `README.md` | User-facing: install, flags, keybindings, config keys |
| `docs/ideas.md` | Design backlog. Problem, options, rejected alternatives, open questions per feature |
| `docs/superpowers/plans/` | Dated implementation plans for landed work |
| `docs/superpowers/specs/` | Dated design specs, paired with the plans |

Read the relevant `docs/ideas.md` section before building anything it covers:
each one records alternatives already rejected and why, which is the expensive
part to rediscover.

## Working conventions

- **Commit straight to `main`.** No feature branches, no PRs. Older commits
  carry `(#10)`-style suffixes from an earlier PR-based phase; that is history,
  not the current policy.
- Tests live beside the code in `#[cfg(test)] mod tests`, not in `tests/`.
- Every behavior change lands with a test that would have caught the old
  behavior. Several existing tests name the regression they guard in a comment;
  match that.

## Build

```
cargo test                       # whole suite
cargo test <name>                # one test by substring
cargo clippy --all-targets       # must be clean
cargo fmt                        # must be clean before committing
cargo install --path .           # install to ~/.cargo/bin (what the `fleetz` on PATH is)
```

Needs a Nerd Font in the terminal: repo-kind glyphs are Nerd Font codepoints
(`ui/badges.rs::kind_glyph`) and render as tofu without one. Needs `gh` on PATH
and authenticated, since the GitHub fetch shells out to it.

## Domain invariants - do not violate

**`ui::panel_rects` must produce the same rectangles `render` draws.** It exists
so mouse clicks can be hit-tested against what is on screen. Shared geometry
lives in one function called by both (`detail::split_right_column`); never
reintroduce a second copy of that arithmetic. The contribution strip is
carved off the bottom by `ui::contributions::split_main_area`, called by both
for the same reason; it lives in its own module rather than beside the other
two because it applies to Compact view as well. Drift here misroutes clicks
silently, with nothing failing.

**`App::visible_repos` order must match the order the repo list draws.** The
cursor walks the former while the eye walks the latter. When a free-text query
is active both switch to one ranked list; when it is not, both group by section.
Both sides key off `App::has_text_query()`.

**Discovery pass order is load-bearing** (`main.rs::assemble_view`). Pass 1
scans the configured roots, pass 2 sweeps `$HOME`, pass 3 picks up local-only
repos, and only then is anything labeled "not cloned". A local clone must always
win over the not-cloned fallback, or a repo under a hidden path gets reported as
absent.

**A local clone may be dot-prefixed when its GitHub repo is not.** The repo
`dotfiles` lives at `~/.dotfiles`. Discovery matches both spellings and reports
the undotted name, because the GitHub match, the glyph lookup, and dedupe all
key on the GitHub name (`discovery::matched_name`, `discovery::repo_name_for_path`).

**Hidden directories are traversed inside a configured scan root, but not in
the `$HOME` sweep.** A root you named (`repo_dir`, `extra_dirs`) should yield
its repos whether or not they are hidden, which is what repoz does. The `$HOME`
sweep still blanket-skips dot-directories, because descending into every
`~/.local` and `~/.npm` costs far more than it finds. `PRUNE_DIRS` applies to
both and mirrors repoz's `PRUNE_EXPR` -- keep the two lists in step.

**Discovery parity with repoz is enforced by test, not by memory.**
`discovery::repoz_parity` builds a fixture tree, runs `repoz -d` against it,
and diffs the discovered paths against fleetz's own. It skips (loudly) when
repoz is not on PATH. Every fixture repo is deliberately dirty so a pure-local
repoz run reports the full set without touching GitHub.

**One color ladder per concept.** `badges::status_color` is the single source
for what a sync state looks like; the detail pane calls it rather than keeping
its own. `ui::filter_color` maps a quick filter to the same color its badge
already uses. Two ladders always drift.

**Status vocabulary is split by kind.** Counters are numeric (`3↓ 2↑ 4!`),
categorical states are words (`not cloned`). A repo that is not cloned has all
counters at zero and must never render as synced: `RepoStatus::is_synced()`
excludes it deliberately.

**Batch on GraphQL, target on git.** The two have opposite cost shapes and the
code leans on both. One GraphQL call returns every repo's state, so never split
it into per-repo queries: the rate limit is priced per node with a floor of one
point per call, making fifty small queries cost more than one big one and tell
you less. `git fetch` is priced per remote, so the reverse holds: use the poll's
`pushed_at` diff to find which remotes moved (`remotes_that_moved`) and fetch
only those.

**Never widen the GitHub fetch without checking the cache.** Results are cached
on disk with a TTL (`cache.rs`, `cache_ttl` config key); `r` rebuilds from cache
and `R` bypasses it. Adding a field to the GraphQL query is free, adding a
request is not.

## Relationship to repoz

Behavioral parity with repoz is a design goal, not an accident. `EXTRA_DIRS`,
the dot-prefixed name match, the `section_div` list headers, the org/private/
public glyphs, and the time-slot logic all mirror it. When the two disagree
about what a repo's state is, repoz has usually been right longer: check its
implementation at `~/repos/repoz/repoz` before assuming fleetz is correct.
