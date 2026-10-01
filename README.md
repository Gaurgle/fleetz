# fleetz

![Rust](https://img.shields.io/badge/rust-stable-dea584?logo=rust)
![Platform](https://img.shields.io/badge/platform-macOS%20·%20Linux-lightgrey)
![License](https://img.shields.io/badge/License-MIT-green)
![TUI](https://img.shields.io/badge/TUI-ratatui-cba6f7)

![GitHub](https://img.shields.io/badge/GitHub-API%20·%20cache%20·%20stale--if--offline-89b4fa)
![Git](https://img.shields.io/badge/git-fetch%20·%20branches%20·%20worktrees%20·%20stash-fab387)

Multi-repo dashboard TUI. The interactive sibling of [repoz](https://github.com/Gaurgle/repoz).

`repoz` answers "what's the state right now?" in one shot. `fleetz` is the dashboard you leave open: live status across all your repos, with fetches, branch cleanup, PR details, and CI badges one keystroke away.

## What it shows

- Repos with recent GitHub activity (commits behind, ahead, divergence). A push
  from another machine shows up as `behind` without waiting for a full sweep:
  the GitHub poll notices the remote moved and fetches that one repo
- Repos with local uncommitted, untracked, or unpushed changes. They are listed
  above the "also active" section, since work sitting only on this machine is a
  worklist and repos you merely have a copy of are not. `info` dates each one
  with a `last touched` row: the newer of your last commit and the newest change
  in the working tree
- Open pull requests per repo
- CI status as a `◉` status lamp: green passed, red failed, blinking orange while running. It pulses on the same phase in the repo row and in the repo pane, since both are reading one repo's CI. From GitHub's `statusCheckRollup`, kept live in watch mode
- Active worktrees (`wt` badge) and stashes (archive-glyph badge)
- Open PR count and numbers on the repo row, in the repo pane, and in the footer
- Repo pane header reads local-to-remote: local path, branch and stashes on the left of each row, remote URL, PRs and CI flush right
- Repo groups defined in your config (cycle with `g`)
- Repo ownership in the detail pane: `owner/name` flush right on the status
  line, colored and glyphed by kind (org, private, public)

The dashboard opens immediately: the first frame paints before any GitHub or
filesystem work, and a spinner in the header marks the load until repos land.

The header says when the GitHub data (CI, PRs, the contribution strip) is not
current, in one segment beside the watch eye:

- nothing, when the last request to GitHub succeeded;
- `github cached 14:02` (dim), when launch painted from the disk cache and the
  request that confirms it is still on its way. One request follows a cached
  launch even with watch off;
- `⚠ github offline, data 14:02` (orange), when the last request failed. The
  word says how (`offline`, `rate limited`, `http error`, `api error`,
  `not signed in`, `bad response`, `timed out`) and the time is when the data
  on screen left GitHub, written like the `swept` time. A cache whose age
  cannot be read says `age unknown` instead.

A failed request never blanks or rewinds what is on screen and never touches
the cache. With watch on, the next one comes a full `github_poll_interval`
later; with watch off, none follows on its own, and `r` retries at once. The
segment clears on the first success. A narrow header shortens it to
`⚠ github` or `github cached` rather than dropping it.

## Install

Requires the Rust toolchain (`cargo`).

```bash
git clone https://github.com/Gaurgle/fleetz.git
cd fleetz
cargo install --path .
```

You also need:

- `git`
- `gh` (GitHub CLI, authenticated via `gh auth login`)
- `lazygit` (optional, for the `Enter` keybind)

## Usage

```bash
fleetz [options]
```

### Modes (pick one)

| Flag | Description |
|------|-------------|
| *(default)* | Show activity since the previous time slot boundary |
| `--since DATE` | Show all activity since DATE (e.g. `2026-03-15`, `yesterday`, `3d`, `1w`) |
| `--before DATE` | Only show repos pushed before DATE (same relative formats as `--since`) |
| `-t, --today` | Show all activity since midnight |
| `-a, --all` | Show all divergence regardless of time |
| `-d, --dirty` | Local only: uncommitted/untracked/unpushed (no GitHub call) |

The window can also be changed live inside the TUI with `t`. A window applied
there is remembered across launches; explicit mode flags override it for that
session without overwriting it. Applying a window shows a progress bar in the
footer as repos are re-stat'd. The count field is capped per unit (hours up to
48, days 31, weeks 52, months 24, years 10).

The window gates both sections, each by the clock that means something to it.
**github activity** goes by when the remote was last pushed to. **local
changes** goes by local work: the newer of your last commit and the newest
change in the working tree, so a repo you dirtied this morning stays put at a
one-day window while one abandoned dirty months ago drops out. Repos with
unpushed commits are never hidden by the window, however old, because that
work exists nowhere but this machine. Widen to `-a` / `all` to see everything
again.

### Options (combinable)

| Flag | Description |
|------|-------------|
| `--author NAME` | Filter displayed commits by author (substring match) |
| `-p, --prs` | Draw the detailed PR section (title, author, age, review state). Counts and numbers always show; the data is fetched either way, since a one-PR page costs exactly what a five-PR page with every field costs |
| `--watch` | Force watch mode on for this session |
| `--no-watch` | Force watch mode off for this session |
| `--interval SECS` | Watch refresh interval, overriding `refresh_interval` |

### Examples

```bash
fleetz                                # what changed since I last sat down
fleetz --since 1w                     # last week
fleetz --since 1w --author andreas    # only my commits
fleetz -a -p                          # everything plus PR counts
fleetz -d                             # local dirty repos only
fleetz --watch                        # live dashboard, auto-refreshes
```

## Keybindings

The footer hints the global keys. A key whose action has an "on" state lights
up green while it is on (`r` and `R` in flight, `v` in the compact view, `?`
with help open); `c` lights in the strip's own colour. Watch shows its state
in the header instead.

| Key | Action |
|-----|--------|
| `1` / `2` / `3` / `4` | Focus repo list / detail / inspector / contributions. On the focused inspector or strip, its number folds it to an empty frame (still focused) and unfolds it again; on a folded one from elsewhere, it unfolds and focuses it |
| `Tab` | Cycle panel focus |
| mouse | Click a panel to focus; click the `[3]` or `[4]` tag in the inspector's or the strip's title to fold it to an empty frame, and again to unfold and focus it; drag a pane border to resize it (a `⠿` grip marks each draggable border, and lights up while you drag it); scroll wheel scrolls whichever panel the cursor is over. Scrolling moves the view, never the selection: the highlight stays with the row it is on and travels off the pane if you scroll past it, and the next `j`/`k` re-enters at the edge you are looking at |
| `j` / `k`, `↑` / `↓` | Repo list: move selection · Detail: pick commit · Inspector: scroll · Contributions: move the day cursor up/down a weekday (a caret above the grid marks the column) |
| `J` / `K` | Drive the **next** pane without leaving the one you are in: repo list → window 2, window 2 → the inspector, inspector → back to the list. Aimed at window 2 it steps that pane's commit cursor, so the inspector reads out each commit in turn; past the last commit it falls through to scrolling, so the sections below a short list stay reachable. Two exceptions: `graph` docks no inspector, so there the keys stay on the pane and walk its **rows** a line at a time where plain `j`/`k` jumps whole commit blocks; and the contributions strip is skipped, being a horizontal timeline with nothing below the fold to scroll into |
| `PgUp` / `PgDn` | Scroll the focused pane itself by a page, leaving its selection where it is |
| `v` | Toggle detail vs compact view. The compact grid packs as many columns as the terminal has room for |
| `[` / `]` | Repo pane: step its body between five views. `info` is the glance (branches, worktrees, stashes, newest 6 commits with diffstats); `commits` the complete history with a computed graph column, one row per commit; `reflog` every recorded HEAD movement; `graph` git's own expanded `log --graph`, wrapped to the pane and given the whole column (its blocks already carry what the inspector would repeat, so no inspector is docked there); `branches` every local branch and every remote-tracking branch in one list, read-only, with the inspector showing the highlighted branch's tip commit (a remote branch nothing local claims is marked `not fetched`, and its tip is not on any local branch, so the inspector says so rather than drawing a blank). Each keeps its own scroll position and selection, and the inspector follows whichever is active. The active view is named in the pane's top border in its own colour (the tabs are capitalised; the pane's own headliner repeats the same name over a divider that fades from the view's colour into the background across the pane), and the repo header below it stays pinned, so both survive scrolling. In `info` the local branches pin with it where the pane has the rows to spare, since they are what the repo's behind count is a sum of; the remote-only list below them collapses to a count, being bounded by what everyone else has pushed rather than by what you have: `B` expands it in place, and `branches` lists them all |
| `B` | `info`: expand or collapse the remote branch summary. The full list, with each branch's state, is the `branches` view |
| `c` | Switch the contributions strip's source, while it is unfolded (the footer's `[c]` lights up in that source's colour): GitHub's own calendar, or your commits in the listed repos. The strip is always on screen, folded or not: three years of activity, GitHub-style. Each day is a Nerd Font square icon, which sits level with the `Mon`/`Wed`/`Fri` labels where a half block cannot, in three sizes (`contributions_cell_size`); the grid scrolls rather than shrinking, with the newest week pinned to the right edge, and no day is marked until you move the cursor |
| `4` | Focus the contributions strip, unfolding it if it is folded; on the focused strip, fold or unfold it |
| `h` / `l`, `←` / `→` | Contributions: move the day cursor back/forward a week. The grid scrolls with it, oldest weeks hiding off to the left |
| mouse | Contributions: click picks a day, drag pans the grid, and any wheel axis (including shift-wheel) scrolls it sideways |
| `Home` / `End` | Contributions: jump to the oldest / newest week |
| `Enter` | Open repo in lazygit (pull / merge live here) |
| `e` | Open repo in `$EDITOR` |
| `o` | Open shell in repo dir |
| `O` | Open PR in browser |
| `f` | Fetch selected repo (updates ahead/behind, no merge) |
| `F` | Fetch all repos |
| `b` / `s` | Branches overlay: preview + switch (`Enter`), clean up merged (`space` to mark, `d` to delete) |
| `t` | Time window: pick how far back to show activity (slot / hours / days / weeks / months / years) |
| `g` | Cycle group filter |
| `G` | Show all (clear group) |
| `/` | Fuzzy-search repos by name (best match first, `flz` finds `fleetz`), or type `#dirty` `#behind` `#ahead` `#ci` `#prs` `#stash` to filter |
| `0` | Clear `#tag` filters (keeps free-text search) |
| `,` | Settings overlay: refresh interval, startup view, work hours, pane persistence, contributions strip and its cell size, intro animation. `Enter` saves to `config.toml` |
| `<` / `>` | Narrow / widen the repo list |
| `+` / `-` | Shrink / grow the inspector pane |
| `=` | Reset both panes to their default sizes |
| `r` | Refresh: re-stat the listed repos and re-poll GitHub, so CI status updates. No rediscovery. A failed poll keeps the rows as they are and marks the header |
| `R` | Reload: re-fetch GitHub (cache-bypassed) + rediscover repos, so new ones appear. If GitHub cannot be reached it rebuilds from the cache and says `Reloaded from cache: GitHub offline` (or why) instead of `Reloaded` |
| `w` | Toggle watch: sweep the listed repos (fetch included) every `refresh_interval` seconds and re-poll GitHub. While it is on, a red eye (`󰈈 watch`) in the header pulses between two shades so the dashboard says it is live without anything moving; off, it is a grey crossed-out eye (`󰈉 watch`) |
| `?` | Toggle help overlay |
| `Esc` | Close search or help |
| `q` | Quit |

## Config

Optional config at `~/.config/fleetz/config.toml` (a legacy `~/.config/epoz/config.toml` is still read as a fallback):

```toml
# Time slot boundaries (24h)
work_start = 9
work_end = 18

# How many repos to fetch from GitHub, newest push first. Repos reached only
# through org membership come from a separate fallback query capped at 8 orgs
# of 12 repos each, independent of this: GraphQL bills node count on the page
# size asked for, so that branch is sized to what it retrieves rather than to
# repo_limit.
#
# This also bounds how far the time window can be widened before a re-fetch is
# needed; past it the header says so. Lowering it to match a narrow window
# saves no API cost (the per-connection floor dominates) and costs that
# headroom, so leave it generous.
repo_limit = 50

# Where to look for local clones (~ expanded). A clone is matched to its
# GitHub repo by its `origin` remote, so the folder can be named anything; one
# whose remote matches nothing falls back to matching on the folder name. After
# renaming a repo on GitHub, run `git remote set-url` so the remote matches.
repo_dir = "~/repos"

# Extra scan roots, searched alongside repo_dir (repoz's EXTRA_DIRS).
# A clone whose folder is dot-prefixed matches its undotted GitHub repo, so
# ~/.dotfiles is reported as "dotfiles". Hidden directories inside a configured
# root are scanned normally; only the wider $HOME sweep skips them, so a clone
# living outside repo_dir under a hidden path needs naming here.
extra_dirs = ["~/.dotfiles"]

# Contribution strip: "github" (default, your GitHub calendar), "local"
# Covers three years. GitHub caps one contributionsCollection at a year, so
# the poll asks for one aliased window per year -- still a single request, and
# measured at the same rate-limit cost as asking for none.
# (your commits in the repos currently listed), or "off" to start it folded.
# `c` switches source and `4` folds it; this is what a fresh launch starts from. The two
# sources use different colour ladders (green for github, blue for local)
# because they count different things and will never agree. The github source
# shades each day with GitHub's own level, so it matches github.com (which
# scales each year to itself and trims outliers first); local buckets by
# quartile of its busiest day.
contributions = "github"

# Git author regex the "local" source filters by, overriding each repo's own
# user.email. --author takes a regex, so several identities can be joined.
contributions_author = "me@work.com\\|me@home.com"

# Size of the square each day is drawn as: "small", "medium" (default) or
# "large". Three Nerd Font icons of measured size, all sitting level with the
# weekday labels. Also in the `,` menu, where it applies live.
contributions_cell_size = "medium"

# Start in watch mode. On by default: a dashboard you leave open should keep
# itself current. --watch / --no-watch override for one session, `w` toggles live.
watch = true

# Seconds between watch sweeps: local git status plus a fetch from every
# remote. This one costs real network traffic, so keep it generous -- the
# GitHub poll below already notices when a remote moves and fetches just that
# repo, so the blanket sweep is a backstop rather than the main path.
refresh_interval = 60

# Seconds between GitHub re-polls while watching. One GraphQL call covers
# every repo, so this is the cheap way to learn what changed: it keeps CI
# status current, and any repo whose remote moved (you pushed from another
# machine, someone else pushed) gets a targeted fetch straight away rather
# than waiting for the next blanket sweep. Automatically tightened to 15s
# while a CI run is in flight, forced immediately after a local push is
# detected, and floored at 15s. While polls are failing it is this interval
# exactly: no 15s floor and no push shortcut.
github_poll_interval = 30

# GitHub API cache TTL in seconds. Launch does not wait on this: it paints
# from whatever is cached, however old, and the poll that fires a moment
# later replaces it (with watch off too). The TTL bounds how long a *running*
# fleetz will reuse a payload before going back to the network.
cache_ttl = 120

# Write pane sizes back on exit and restore them next launch. Off by
# default: every launch starts from the built-in split, so a layout nudged
# for one repo doesn't silently become permanent. Toggle it in the `,` menu.
persist_pane_sizes = false

# Play the intro wave on the first painted screen of a launch. The load is
# spent on a field of scrambled characters with LOADING held still in block
# capitals in the middle; when the repos land, a diagonal wave washes the
# scramble away character by character and leaves the dashboard behind, over
# about a second and a half. Any keypress skips it. Set false to never see it again.
intro = true
repo_list_pct = 35
inspector_pct = 40

# Startup view: "detail", "compact", or "auto" (default). Auto drops to
# compact only when the terminal is too small for three panels; pin it here
# if you always want one or the other. `v` still toggles for the session.
view = "detail"

# Optional: named repo groups for the g/G filter
[groups]
work = ["api", "infra", "frontend-app"]
personal = ["dotfiles", "notez-cli"]
```

All keys are optional. Without a config file, defaults are used.

The three time slots are derived from `work_start` and `work_end`:

| Slot | Default |
|------|---------|
| Work | 09:00-18:00 |
| Evening | 18:00-00:00 |
| Night | 00:00-09:00 |

## How it relates to repoz

| | repoz | fleetz |
|---|---|---|
| Form | Bash one-shot | Rust TUI |
| Output | Static text dump | Interactive dashboard |
| Fetch | Yes, every repo on every run | On demand (`f` / `F`) and in watch mode |
| GitHub calls | Uncached, one GraphQL call per run | Disk cache with a TTL, `R` bypasses it |
| Branch cleanup | No | Yes (`b`) |
| CI status | No | Yes (◉ green passed, red failed, blinking orange running) |
| Worktrees / stashes | No | Yes (badges) |
| Watch mode | No | `--watch` for tmux |
| Single repo | `repoz /owner/repo` | No |
| Release status | `-r` (tag, commits since) | No |

Same discovery rules and the same time-slot logic; the flag sets overlap but
are not identical (`-r`, `-s`, `--no-glyphs`, `--update` and `/owner/repo` are
repoz-only, `--watch` and `--interval` are fleetz-only).

`repoz` is the script you pipe, cron, or point at one repo. `fleetz` is the
dashboard you act from: it caches, so it opens instantly and stays cheap to
leave running, whereas repoz pays full fetch cost on every invocation.

The two implement discovery separately, in two languages, and have drifted
apart before (a clone at `~/.dotfiles` was visible to repoz and invisible to
fleetz for months). `cargo test repoz_parity` now builds a fixture tree, runs
`repoz -d` against it, and diffs the discovered paths against fleetz's own. It
skips with a notice when repoz is not on `PATH`.

## Stack

Rust, [ratatui](https://github.com/ratatui/ratatui), [crossterm](https://github.com/crossterm-rs/crossterm), [reqwest](https://github.com/seanmonstar/reqwest), [tokio](https://github.com/tokio-rs/tokio), [clap](https://github.com/clap-rs/clap), [chrono](https://github.com/chronotope/chrono), [serde](https://github.com/serde-rs/serde) + [toml](https://github.com/toml-rs/toml).

## Status

Personal project, used daily. Public API and config keys may shift before a 1.0 tag.

## Roadmap

Toward a true realtime dashboard. Background sweeps, fetch in the refresh
cycle, tick-based redraw, and GitHub re-polling all landed; what is left is
reacting to change rather than polling for it:

- Filesystem events (`notify` on `.git/HEAD`, `.git/refs/`, index) with a
  debounce: dirty/branch changes appear in milliseconds, not on the interval.
- GitHub conditional polling with stored ETags (`If-None-Match`) so PR and CI
  badges refresh live without burning rate limit.

First-class worktrees (today: a read-only `Nw` badge and a path list, only for
GitHub-matched repos):

- A `w` overlay like the branch overlay: per-worktree dirty/ahead-behind,
  Enter opens lazygit or a shell there.
- Create and prune from the overlay (`git worktree add` under a configured
  root; prune merged ones with the existing y/n confirm flow).
- Per-worktree dirty state surfaced in the main views (badge + `-d` mode), so
  uncommitted work in a worktree stops being invisible.

Shared core with repoz: extract discovery, git porcelain parsing, and the
GitHub client + cache into a library crate both tools consume; a repoz TUI (or
rewrite) becomes a thin one-shot frontend over the same `RepoStatus` snapshot,
with `--json` output for free.

## History

This tool was previously named `epoz`. The `epoz` name now belongs to the desktop app in the notez2 project; the TUI lives on as `fleetz`, the interactive sibling of `repoz`.

## See also

- [repoz](https://github.com/Gaurgle/repoz): the headless one-shot version
- [notez](https://github.com/Gaurgle/notez-cli): local-first CLI note-taking with interactive todos

## Credits

The commit graph in the repo pane is a port of [lazygit](https://github.com/jesseduffield/lazygit)'s,
from `pkg/gui/presentation/graph/` at `v0.59.0` -- including the insight that
makes it work, which is to compute the topology from parent hashes instead of
parsing `git log --graph`. That is what lets the history draw one row per
commit instead of interleaving git's filler rows. Nine of lazygit's own test
fixtures are ported with it. lazygit is MIT licensed; see [NOTICES.md](NOTICES.md).

[repoz](https://github.com/Gaurgle/repoz) is fleetz's sibling and the source of
its discovery rules, time-slot logic and status vocabulary.
