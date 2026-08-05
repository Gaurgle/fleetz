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

- Repos with recent GitHub activity (commits behind, ahead, divergence)
- Repos with local uncommitted, untracked, or unpushed changes
- Open pull requests per repo
- CI status (✓ / ✗ / ○) from GitHub's `statusCheckRollup`
- Active worktrees (`w` badge) and stashes (`s` badge)
- Repo groups defined in your config (cycle with `g`)

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
| `--watch` | Auto-refresh mode for a persistent tmux pane |

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
| `1` / `2` / `3` | Focus repo list / detail / inspector |
| `Tab` | Cycle panel focus |
| mouse | Click a panel to focus; scroll wheel scrolls whichever panel the cursor is over |
| `j` / `k`, `↑` / `↓` | Repo list: move selection · Detail: pick commit · Inspector: scroll |
| `J` / `K` | Scroll the detail pane (works from the repo list too) |
| `PgUp` / `PgDn` | Scroll focused pane by a page |
| `v` | Toggle detail vs compact view |
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
| `/` | Search repos, or type `#dirty` `#behind` `#ahead` `#ci` `#prs` `#stash` to filter |
| `0` | Clear `#tag` filters (keeps free-text search) |
| `r` | Refresh: re-stat the listed repos only (fast, offline, no new repos) |
| `R` | Reload: re-fetch GitHub (cache-bypassed) + rediscover repos, so new ones appear |
| `w` | Toggle watch: auto-refresh listed repos every `refresh_interval` seconds |
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

# Auto-refresh interval in seconds (used with --watch)
refresh_interval = 300

# GitHub API cache TTL in seconds
cache_ttl = 120

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
| Fetch | No (read-only) | Yes (`f` / `F`, no merge) |
| Branch cleanup | No | Yes (`b`) |
| CI status | No | Yes (✓/✗/○) |
| Worktrees / stashes | No | Yes (badges) |
| Caching | None | GitHub API disk cache |
| Watch mode | No | `--watch` for tmux |

Same flag set, same time-slot logic. `repoz` is the script you pipe; `fleetz` is the dashboard you live in.

## Stack

Rust, [ratatui](https://github.com/ratatui/ratatui), [crossterm](https://github.com/crossterm-rs/crossterm), [reqwest](https://github.com/seanmonstar/reqwest), [tokio](https://github.com/tokio-rs/tokio), [clap](https://github.com/clap-rs/clap), [chrono](https://github.com/chronotope/chrono), [serde](https://github.com/serde-rs/serde) + [toml](https://github.com/toml-rs/toml).

## Status

Personal project, used daily. Public API and config keys may shift before a 1.0 tag.

## Roadmap

Toward a true realtime dashboard (today `--watch` only re-reads local git state
on an interval, inline on the UI task, and GitHub data is fetched once at startup):

- Background refresh: per-repo `tokio::spawn` tasks feeding the UI over an mpsc
  channel, `tokio::select!` with the crossterm `EventStream` so refresh never
  freezes the UI; tick-based redraw so updates paint without a keypress.
- `git fetch` in the refresh cycle so ahead/behind stops going stale mid-session.
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
