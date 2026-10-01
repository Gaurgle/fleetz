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

Used daily. All code is unit-tested in `#[cfg(test)]` modules in the same file
as the code they cover. There is no integration test suite. CI is
`.github/workflows/ci.yml` (fmt, clippy, tests on a push to any branch, skipped
when only prose changed), `docs.yml` (runs `scripts/check-docs.sh` on a prose
push: links, the index below, no dashes) and
`release.yml`, which turns a pushed `v*` tag into a GitHub
release: it checks the tag against `Cargo.toml`, runs fmt, clippy and tests,
and writes commit-based notes with `scripts/release-notes.sh`. PRs are optional.

The event loop is channel-driven and async: the TUI opens instantly, GitHub and
local discovery load in the background (`AppEvent`), a 250ms tick drives spinner
and blink, watch mode re-sweeps on an interval.

## Where things are written down

| Path | What it holds |
|---|---|
| `README.md` | User-facing: install, flags, keybindings, config keys |
| `docs/ideas.md` | Design backlog: problem, options, rejected alternatives per feature |
| `docs/release-plan.md` | Release milestones, ticket status and dispatch order |
| `docs/agent-workflow.md` | Shared worker/reviewer instructions and integration rules |
| `docs/agent-handoff.md` | Coordinator state between leads: read first when taking over |
| `.claude/agents/` | Claude-native workforce: `fz-coordinator`, `fz-worker`, `fz-reviewer`, `fz-small` |
| `docs/superpowers/` | Dated plans and specs, paired, for landed work |

Read the relevant `docs/ideas.md` section before building anything it covers:
each records alternatives already rejected and why, the expensive part to
rediscover. Its **Status: shipped** sections are kept for reasoning only;
everything else is unbuilt, and its line references drift.

## Working conventions

- **One feature branch and worktree per implementation ticket.** Branch from
  the agreed `main` commit, for example `fix/fz-02-preserve-github-state`.
  PRs are optional. Andreas commits reviewed work on its branch and merges
  into `main`; agents do not implement tickets directly in the main checkout.
- **Ship policy:** prepare and review the branch changes, then validate the
  combined result against current `main` (`cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`). Present commit
  and integration commands for Andreas to run. Do not commit, merge or push.
- **Release policy:** prepare the requested version bump and What's Changed
  notes, run the release checks, and present commands for Andreas. The
  `release.yml` workflow publishes when he pushes the version tag. Never
  recommend releasing a dirty tree or a red build.
- Tests live beside the code in `#[cfg(test)] mod tests`, not in `tests/`.
- Every behavior change lands with a test that would have caught the old
  behavior; many existing tests name the regression they guard. Match that.

## Build

```
cargo test                    # whole suite; `cargo test <name>` for one
cargo clippy --all-targets    # must be clean
cargo fmt                     # must be clean before committing
cargo install --path .        # to ~/.cargo/bin (the `fleetz` on PATH)
```

Needs a Nerd Font (`ui/badges.rs::kind_glyph` and the contribution strip's
day squares, `app::CellSize`, use Nerd Font codepoints, tofu without one) and
an authenticated `gh` on PATH, which the GitHub fetch uses.

## Domain invariants - do not violate

**`ui::panel_rects` must produce the same rectangles `render` draws.** It exists
so mouse clicks can be hit-tested against what is on screen. Shared geometry
lives in one function called by both (`detail::split_inspector`, and
`ui::contributions::split_main_area` for the strip); never reintroduce a second
copy of that arithmetic. Drift here misroutes clicks silently, with nothing
failing.

**`App::visible_repos` order must match the order the repo list draws.** The
cursor walks one while the eye walks the other. A free-text query switches both
to one ranked list, otherwise both group by section; both key off
`App::has_text_query()`. Within a section the order is push recency, as repoz
lists it (`sort_by_activity`), applied after the pass dedupe below.

**Discovery pass order is load-bearing** (`main.rs::assemble_view`). Pass 1
scans the configured roots, pass 2 sweeps `$HOME`, pass 3 picks up local-only
repos, and only then is anything "not cloned". A local clone must win over that
fallback, or a repo under a hidden path reports as absent.

**A clone is matched by its `origin` remote first, its folder name second**
(`discovery::Wanted::match_clone`). A folder name is only a convention: a clone
of `portfolio-site` in `~/repos/DevOps_Portfolio` reported as not cloned. The
remote is read from `.git/config`, not `git remote get-url`, since it runs for
every candidate in both sweeps. The owner is part of the key, so a clone of
someone else's same-named repo can't claim yours. A linked worktree has no
config of its own and keeps the name match, or it would race its main clone for
the row. Pass 3 dedupes by path as well as name (`main.rs::unclaimed_local`),
because a remote-matched folder name is not the row's name. This diverges from
repoz on purpose: repoz still matches on names only.

**A local clone may be dot-prefixed when its GitHub repo is not** (`dotfiles`
lives at `~/.dotfiles`). Discovery matches both spellings and reports the
undotted name: match, glyph lookup and dedupe all key on the GitHub name
(`discovery::matched_name`, `discovery::repo_name_for_path`).

**Hidden directories are traversed inside a configured scan root, but not in
the `$HOME` sweep.** A root you named (`repo_dir`, `extra_dirs`) yields its
repos hidden or not, as repoz does; the `$HOME` sweep blanket-skips
dot-directories, since descending every `~/.local` and `~/.npm` costs far more
than it finds. `PRUNE_DIRS` applies to both and mirrors repoz's `PRUNE_EXPR`.

**Discovery parity with repoz is enforced by test, not by memory.**
`discovery::repoz_parity` runs `repoz -d` against a fixture tree and diffs the
paths, skipping loudly when repoz is absent. Fixture repos are deliberately
dirty so a pure-local repoz run reports them all, and the fixture root is
canonicalized: repoz resolves `/var` to `/private/var` and fleetz does not.

**One color ladder per concept.** `badges::status_color` is the single source
for what a sync state looks like; the detail pane calls it rather than keeping
its own. `ui::filter_color` maps a quick filter to the same color its badge
already uses. Two ladders always drift.

Contribution intensity is the one place with two ladders, green for GitHub and
blue for local, because the two count different things. Both are reached
through one `colors::contrib_ladder(source)`, which the cells and the Less/More
legend call, so a source cannot leave half the strip on the other palette.

Both ladders are spaced evenly in CIELAB **L***, in saturation as well as
lightness: even *luminance* is the wrong metric and leaves the middles too
dark. The greens are tuned from github.com's, not sampled. The blue tops out
lower, carrying 0.07 of luminance to green's 0.72, so at the green's top the
bluest colour sRGB has reads as paper. Even L*, rate parity and chroma are the
invariants, not equal luminance; tests pin each, and `colors.rs` has the why.

**A contribution day is a Nerd Font square icon, not a block element.** The
icon is drawn by the font to sit on the text line, so it lines up with the
`Mon`/`Wed`/`Fri` labels. A half block cannot: it fills the top or bottom half
of the cell and sits a quarter row off the label's centre either way, which is
how the strip looked for a long time. The three sizes are three icons measured
in the font (`CellSize::glyph`), each one column wide as ratatui lays it out,
which the grid's column arithmetic and `slot_at`'s click routing depend on.

**The GitHub strip draws GitHub's `contributionLevel`, not a bucket of the
count.** github.com trims outliers before scaling each year and the rule is
not reproducible: measured over three years, a year whose busiest day had 83
contributions was scaled as if the top were 47, and one with an 86-day as if
it were 10. Quartiles of the max drew a 38-commit day two shades paler than
github.com. The field is free (cost 1, nodeCount 0, with and without it). The
`recent` window's levels are never used: its `from` rotates, so they are on a
scale that changes every few minutes. Days it overwrites are put on the year
window's scale by example (`Calendar::derive_missing_levels`), and a payload
cached before the field was asked for falls back to `intensity`.

**The compact history graph is computed, not parsed.** `git::graph` asks for
`%p` and `crate::graph` derives the topology, as lazygit does. Re-adding
`--graph` brings back filler rows carrying no commit -- which forced row and
commit indices apart everywhere -- and single-column diagonals, undrawable in
box characters. `%p` not `%P`, or a parent hash is spelled differently from the
child's own `%h` and every pipe dangles. The expanded view still reproduces
git's own drawing.

**The commit list and the graph must walk the same refs.** `git::commits` and
`git::graph` are two `git log` runs and the detail pane joins them by hash: the
churn beside a subject, and everything the inspector draws, is a graph row
looked up in the commit list. Reading HEAD in one and the union of the local
branches in the other left a side-branch commit drawn but unfindable, and the
views that indexed rather than resolved it named the row below the caret. `-n`
caps the union, not each ref, so the wider read returns the same 200 commits
for the same cost.

**Status vocabulary is split by kind.** Counters are numeric (`3↓ 2↑ 4!`),
categorical states are words (`not cloned`). A not-cloned repo has all counters
at zero and must never render as synced: `RepoStatus::is_synced()` excludes it
deliberately.

**Batch on GraphQL, target on git.** Opposite cost shapes, and the code leans
on both. One GraphQL call returns every repo's state, so never split it into
per-repo queries: the rate limit has a floor of one point per call, so fifty
small queries cost more than one big one and tell you less. `git fetch` is
priced per remote, so use the poll's `pushed_at` diff to find which remotes
moved (`remotes_that_moved`) and fetch only those.

**GraphQL bills the `first:` you ask for, not what comes back**, and nested
connections multiply. The org fallback asked 25 orgs x 50 repos to fetch 17
from 4: 83% of the query's nodes, pricing the poll at 26 points, or 3120/hour
at the 30s interval against a 5000/hour budget. One fleetz idled at 62% of the
account and four exhausted it; sized to what it retrieves (`MAX_ORGS`,
`ORG_REPO_LIMIT`) the identical result costs 3. Measure with `rateLimit { cost
nodeCount }` before and after: two comments in `github.rs` disagreed about this
for months and the optimistic one was wrong.

The first poll has no previous dataset to diff, and fetching every repo past
the cutoff cost ~0.8s each. It uses a disk baseline: `FETCH_HEAD`'s mtime is
when that clone last fetched, so a remote pushed before then is already in hand
(`stale_remotes`). Unknowns must fetch -- no `pushed_at` means no GitHub match,
no `FETCH_HEAD` means never fetched -- or the fleet looks more current than it
is.

**Launch paints from the cache, however stale.** Blocking on the GraphQL
dataset cost 6-9s before the first frame, so startup passes
`CacheMode::AnyCached` and the first tick's poll replaces it. That only works
if the poll is due: a cache-served load must leave `last_github_poll` unset
(`GithubFetch::from_cache`), or launch shows the last session's numbers for a
whole interval. Keyed by `repo_limit` alone: never re-add `-p` (`cache.rs`),
which decides whether PR data is *drawn*, not whether it is fetched.

**Never widen the GitHub fetch without checking the cache.** Results are cached
on disk with a TTL (`cache.rs`, `cache_ttl`); `r` rebuilds from cache, `R`
bypasses it. A field with no `first:` is free, which is how the contribution
calendars ride along at no cost; one with a `first:` is not, and a request
never is.

**GitHub caches `contributionsCollection` against the `from` date alone**, so
the `recent` window rotates its `from` and must keep doing so: a fixed one
re-reads its own stale answer all day, and the strip drew 3 against
github.com's 11. The merged range is fixed at seven days, not derived from the
window, because a cached payload was built under a rotation the parser cannot
recover. Measurements, and the undocumented behaviour this leans on, are at
`contributions.rs::ROTATION_HOLD_SECS`.

**Only a response carrying `data` may be cached** (`accept_response`). GraphQL
errors arrive as HTTP 200, so a rate limit is a plausible 95-byte body. Caching
it before checking `errors` overwrote the good payload and took the
`read_stale` fallback with it, which the offline and non-2xx paths both need:
the dashboard went blank until a poll succeeded.

## Relationship to repoz

Behavioral parity with repoz is a design goal, not an accident: `EXTRA_DIRS`,
the dot-prefixed name match, `section_div` headers, the glyphs, push ordering
and the time-slot logic all mirror it. When the two disagree, repoz has usually
been right longer: read `~/repos/repoz/repoz` before assuming fleetz is right.
