# epoz

![Rust](https://img.shields.io/badge/rust-stable-dea584?logo=rust)
![Platform](https://img.shields.io/badge/platform-macOS%20·%20Linux-lightgrey)
![License](https://img.shields.io/badge/License-MIT-green)
![TUI](https://img.shields.io/badge/TUI-ratatui-cba6f7)

![GitHub](https://img.shields.io/badge/GitHub-API%20·%20cache%20·%20stale--if--offline-89b4fa)
![Git](https://img.shields.io/badge/git-pull%20·%20branches%20·%20worktrees%20·%20stash-fab387)

Multi-repo dashboard TUI. The interactive sibling of [repoz](https://github.com/Gaurgle/repoz).

`repoz` answers "what's the state right now?" in one shot. `epoz` is the dashboard you leave open: live status across all your repos, with pulls, branch cleanup, PR details, and CI badges one keystroke away.

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
git clone https://github.com/Gaurgle/epoz.git
cd epoz
cargo install --path .
```

You also need:

- `git`
- `gh` (GitHub CLI, authenticated via `gh auth login`)
- `lazygit` (optional, for the `Enter` keybind)

## Usage

```bash
epoz [options]
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

### Options (combinable)

| Flag | Description |
|------|-------------|
| `--author NAME` | Filter displayed commits by author (substring match) |
| `-s, --stat` | Show diff stats per commit |
| `-p, --prs` | Include open pull request counts |
| `--watch` | Auto-refresh mode for a persistent tmux pane |

### Examples

```bash
epoz                                # what changed since I last sat down
epoz --since 1w                     # last week
epoz --since 1w --author andreas    # only my commits
epoz -a -p                          # everything plus PR counts
epoz -d                             # local dirty repos only
epoz --watch                        # live dashboard, auto-refreshes
```

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `k`, `↑` / `↓` | Move selection |
| `v` | Toggle detail vs compact view |
| `Enter` | Open repo in lazygit |
| `e` | Open repo in `$EDITOR` |
| `o` | Open shell in repo dir |
| `O` | Open PR in browser |
| `p` | Pull selected repo (ff-only) |
| `P` | Pull all clean repos (ff-only) |
| `b` | Branch cleanup overlay (multi-select delete) |
| `g` | Cycle group filter |
| `G` | Show all (clear group) |
| `r` | Refresh data |
| `/` | Search / filter |
| `?` | Toggle help overlay |
| `Esc` | Close search or help |
| `q` | Quit |

## Config

Optional config at `~/.config/epoz/config.toml`:

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
| Work | 09:00–18:00 |
| Evening | 18:00–00:00 |
| Night | 00:00–09:00 |

## How it relates to repoz

| | repoz | epoz |
|---|---|---|
| Form | Bash one-shot | Rust TUI |
| Output | Static text dump | Interactive dashboard |
| Pulls | No (read-only) | Yes (`p` / `P`) |
| Branch cleanup | No | Yes (`b`) |
| CI status | No | Yes (✓/✗/○) |
| Worktrees / stashes | No | Yes (badges) |
| Caching | None | GitHub API disk cache |
| Watch mode | No | `--watch` for tmux |

Same flag set, same time-slot logic. `repoz` is the script you pipe; `epoz` is the dashboard you live in.

## Stack

Rust, [ratatui](https://github.com/ratatui/ratatui), [crossterm](https://github.com/crossterm-rs/crossterm), [reqwest](https://github.com/seanmonstar/reqwest), [tokio](https://github.com/tokio-rs/tokio), [clap](https://github.com/clap-rs/clap), [chrono](https://github.com/chronotope/chrono), [serde](https://github.com/serde-rs/serde) + [toml](https://github.com/toml-rs/toml).

## Status

Personal project, used daily. Public API and config keys may shift before a 1.0 tag.

## See also

- [repoz](https://github.com/Gaurgle/repoz): the headless one-shot version
- [notez](https://github.com/Gaurgle/notez-cli): local-first CLI note-taking with interactive todos
