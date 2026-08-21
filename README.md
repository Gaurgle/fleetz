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
- Repos with local uncommitted, untracked, or unpushed changes
- Open pull requests per repo
- CI status as a `◉` status lamp: green passed, red failed, blinking orange while running. From GitHub's `statusCheckRollup`, kept live in watch mode
- Active worktrees (`w` badge) and stashes (`s` badge)
- Repo groups defined in your config (cycle with `g`)
- Repo ownership in the detail pane: `owner/name` flush right on the status
  line, colored and glyphed by kind (org, private, public)

The dashboard opens immediately: the first frame paints before any GitHub or
filesystem work, and a spinner in the header marks the load until repos land.

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
| *(default)* | Auto: find the latest time slot with activity and show those repos |
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

### Options (combinable)

| Flag | Description |
|------|-------------|
| `--author NAME` | Filter displayed commits by author (substring match) |
| `-p, --prs` | Include open pull request counts |
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

| Key | Action |
|-----|--------|
| `1` / `2` / `3` / `4` | Focus repo list / detail / inspector / contributions |
| `Tab` | Cycle panel focus |
| mouse | Click a panel to focus; drag a pane border to resize it; scroll wheel scrolls whichever panel the cursor is over |
| `j` / `k`, `↑` / `↓` | Repo list: move selection · Detail: pick commit · Inspector: scroll · Contributions: move the day cursor up/down a weekday (a caret above the grid marks the column) |
| `J` / `K` | Scroll the detail pane (works from the repo list too) |
| `PgUp` / `PgDn` | Scroll focused pane by a page |
| `v` | Toggle detail vs compact view |
| `c` | Toggle the contributions strip: three years of activity, GitHub-style. Each day is a half-block square with its own padding, so the grid fits in 11 rows; it scrolls rather than shrinking, with the newest week pinned to the right edge, and no day is marked until you move the cursor |
| `C` | Switch its source: GitHub's own calendar, or your commits in the listed repos |
| `4` | Focus the contributions strip (only while it is open) |
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
| `,` | Settings overlay: refresh interval, startup view, work hours, pane persistence, contributions strip. `Enter` saves to `config.toml` |
| `<` / `>` | Narrow / widen the repo list |
| `+` / `-` | Shrink / grow the inspector pane |
| `=` | Reset both panes to their default sizes |
| `r` | Refresh: re-stat the listed repos and re-poll GitHub, so CI status updates. No rediscovery |
| `R` | Reload: re-fetch GitHub (cache-bypassed) + rediscover repos, so new ones appear |
| `w` | Toggle watch: sweep the listed repos (fetch included) every `refresh_interval` seconds and re-poll GitHub |
| `?` | Toggle help overlay |
| `Esc` | Close search or help |
| `q` | Quit |

## Config

Optional config at `~/.config/fleetz/config.toml` (a legacy `~/.config/epoz/config.toml` is still read as a fallback):

```toml
# Time slot boundaries (24h)
work_start = 9
work_end = 18

# How many repos to fetch from GitHub
repo_limit = 50

# Where to look for local clones (~ expanded)
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
# (your commits in the repos currently listed), or "off". `c` toggles it and
# `C` switches source; this is what a fresh launch starts from. The two
# sources use different colour ladders (green for github, blue for local)
# because they count different things and will never agree.
contributions = "github"

# Git author regex the "local" source filters by, overriding each repo's own
# user.email. --author takes a regex, so several identities can be joined.
contributions_author = "me@work.com\\|me@home.com"

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
# detected, and floored at 15s.
github_poll_interval = 30

# GitHub API cache TTL in seconds
cache_ttl = 120

# Write pane sizes back on exit and restore them next launch. Off by
# default: every launch starts from the built-in split, so a layout nudged
# for one repo doesn't silently become permanent. Toggle it in the `,` menu.
persist_pane_sizes = false
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
