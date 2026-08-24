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
  The human commits and pushes.
- **Never edit, delete or disable a test to make code pass.** Fix the code. If a
  test looks wrong, stop and say so rather than changing it.
- **Ask before** adding or removing dependencies, changing schema or API
  contracts, touching CI config, or deleting files.
<!-- house-rules:end -->

## Current state

Used daily. All code is unit-tested in `#[cfg(test)]` modules in the same file
as the code they cover. There is no integration test suite and no CI workflow.

The event loop is channel-driven and async: the TUI opens instantly, GitHub and
local discovery load in the background (`AppEvent`), a 250ms tick drives spinner
and blink, and watch mode re-sweeps on an interval.

In `docs/ideas.md`, **Status: shipped** sections are kept for their reasoning
only; everything else is unbuilt, and its line references drift.

## Where things are written down

| Path | What it holds |
|---|---|
| `README.md` | User-facing: install, flags, keybindings, config keys |
| `docs/ideas.md` | Design backlog. Problem, options, rejected alternatives, open questions per feature |
| `docs/superpowers/` | Dated plans and specs, paired, for landed work |

Read the relevant `docs/ideas.md` section before building anything it covers:
each one records alternatives already rejected and why, which is the expensive
part to rediscover.

## Working conventions

- **Commit straight to `main`.** No feature branches, no PRs. The `(#10)`
  suffixes on older commits are history, not current policy.
- Tests live beside the code in `#[cfg(test)] mod tests`, not in `tests/`.
- Every behavior change lands with a test that would have caught the old
  behavior. Several existing tests name the regression they guard in a comment;
  match that.

## Build

```
cargo test                    # whole suite; `cargo test <name>` for one
cargo clippy --all-targets    # must be clean
cargo fmt                     # must be clean before committing
cargo install --path .        # to ~/.cargo/bin (the `fleetz` on PATH)
```

Needs a Nerd Font in the terminal: repo-kind glyphs are Nerd Font codepoints
(`ui/badges.rs::kind_glyph`) and render as tofu without one. Needs `gh` on PATH
and authenticated, since the GitHub fetch shells out to it.

## Domain invariants - do not violate

**`ui::panel_rects` must produce the same rectangles `render` draws.** It exists
so mouse clicks can be hit-tested against what is on screen. Shared geometry
lives in one function called by both (`detail::split_right_column`, and
`ui::contributions::split_main_area` for the strip); never reintroduce a second
copy of that arithmetic. Drift here misroutes clicks silently, with nothing
failing.

**`App::visible_repos` order must match the order the repo list draws.** The
cursor walks the former while the eye walks the latter. When a free-text query
is active both switch to one ranked list; when it is not, both group by section.
Both sides key off `App::has_text_query()`.

**Discovery pass order is load-bearing** (`main.rs::assemble_view`). Pass 1
scans the configured roots, pass 2 sweeps `$HOME`, pass 3 picks up local-only
repos, and only then is anything labeled "not cloned". A local clone must win
over the not-cloned fallback, or a repo under a hidden path reports as absent.

**A local clone may be dot-prefixed when its GitHub repo is not** (`dotfiles`
lives at `~/.dotfiles`). Discovery matches both spellings and reports the
undotted name: the GitHub match, the glyph lookup and dedupe all key on the
GitHub name (`discovery::matched_name`, `discovery::repo_name_for_path`).

**Hidden directories are traversed inside a configured scan root, but not in
the `$HOME` sweep.** A root you named (`repo_dir`, `extra_dirs`) yields its
repos hidden or not, as repoz does; the `$HOME` sweep blanket-skips
dot-directories, because descending every `~/.local` and `~/.npm` costs far
more than it finds. `PRUNE_DIRS` applies to both and mirrors repoz's
`PRUNE_EXPR` -- keep the two lists in step.

**Discovery parity with repoz is enforced by test, not by memory.**
`discovery::repoz_parity` runs `repoz -d` against a fixture tree and diffs the
paths against fleetz's own, skipping loudly when repoz is not on PATH. Every
fixture repo is deliberately dirty so a pure-local repoz run reports the full
set without touching GitHub.

**One color ladder per concept.** `badges::status_color` is the single source
for what a sync state looks like; the detail pane calls it rather than keeping
its own. `ui::filter_color` maps a quick filter to the same color its badge
already uses. Two ladders always drift.

Contribution intensity is the one place with two ladders, green for the GitHub
source and blue for the local one, because the two count different things and
should not be mistaken for each other. They are still reached through a single
`colors::contrib_ladder(source)`, which the cells and the Less/More legend both
call, so a source can never leave half the strip on the other palette.

The greens are sampled from github.com's own dark-dimmed graph, not picked or
interpolated, and a test pins the exact values. The blue is derived by holding
each step's saturation and solving its lightness for the same luminance, so
both ladders climb at identical rates. Do not hue-rotate (step 1 lands within
4% of the blue-hued empty slate, losing a level) and do not interpolate between
two palette colours (it reads as one flat tone, defeating a heatmap).

**The compact history graph is computed, not parsed.** `git::graph` does not
pass `--graph`; it asks for `%p` and `crate::graph` derives the topology, the
way lazygit does. Re-adding `--graph` would bring back the filler rows that
carry no commit -- which is what forced row indices and commit indices apart
everywhere -- and its single-column diagonals, which cannot be drawn in box
characters. `%p` not `%P`: a parent hash must be spelled the same way the
child's own `%h` is, or every pipe dangles. The expanded view is the one place
that still reproduces git's own drawing.

**Status vocabulary is split by kind.** Counters are numeric (`3↓ 2↑ 4!`),
categorical states are words (`not cloned`). A repo that is not cloned has all
counters at zero and must never render as synced: `RepoStatus::is_synced()`
excludes it deliberately.

**Batch on GraphQL, target on git.** The two have opposite cost shapes and the
code leans on both. One GraphQL call returns every repo's state, so never split
it into per-repo queries: the rate limit has a floor of one point per call,
making fifty small queries cost more than one big one and tell you less.
`git fetch` is priced per remote, so the reverse holds: use the poll's
`pushed_at` diff to find which remotes moved (`remotes_that_moved`) and fetch
only those.

**GraphQL bills the `first:` you ask for, not what comes back**, and nested
connections multiply. The org fallback asked 25 orgs x 50 repos to fetch 17
repos from 4 orgs: 83% of the query's nodes, pricing the poll at 26 points, or
3120/hour at the 30s interval against a 5000/hour budget. One fleetz idled at
62% of the account and four exhausted it. Sized to what it retrieves
(`MAX_ORGS`, `ORG_REPO_LIMIT`) the identical result costs 3. Measure with
`rateLimit { cost nodeCount }` before and after; two comments in `github.rs`
disagreed about this for months and the optimistic one was wrong.

The first poll has no previous dataset to diff, and fetching every repo past
the cutoff cost ~0.8s each. It uses a disk baseline instead: `FETCH_HEAD`'s
mtime is when that clone last fetched, so a remote pushed before then is
already in hand (`stale_remotes`). Unknowns must fetch -- no `pushed_at` means
no GitHub match, no `FETCH_HEAD` means never fetched -- or the fleet renders as
more current than it is.

**Launch paints from the cache, however stale.** The repo list is derived from
the GraphQL dataset, so blocking on it cost 6-9s before the first frame --
dwarfing discovery (0.1s) and statting the fleet (0.6s). Startup passes
`CacheMode::AnyCached`; the poll on the first tick replaces it. Only a
never-run install waits, and it has nothing to show anyway. Keyed by
`repo_limit` alone: never re-add `-p` (see `cache.rs`), which decides whether
PR data is *drawn*, not whether it is fetched.

**Never widen the GitHub fetch without checking the cache.** Results are cached
on disk with a TTL (`cache.rs`, `cache_ttl` config key); `r` rebuilds from cache
and `R` bypasses it. A field with no `first:` is genuinely free -- the four
contribution calendars ride along at zero cost -- a field with one is not, and
a request never is.

**Only a response carrying `data` may be cached** (`accept_response`). GraphQL
errors arrive as HTTP 200, so a rate limit is a plausible-looking 95-byte body.
Caching it before checking `errors` overwrote the good payload and took the
`read_stale` fallback with it, which the offline and non-2xx paths both need:
the dashboard went blank and could not heal until a poll succeeded.

## Relationship to repoz

Behavioral parity with repoz is a design goal, not an accident. `EXTRA_DIRS`,
the dot-prefixed name match, the `section_div` list headers, the org/private/
public glyphs, and the time-slot logic all mirror it. When the two disagree
about what a repo's state is, repoz has usually been right longer: check its
implementation at `~/repos/repoz/repoz` before assuming fleetz is correct.
