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
- Active worktrees (`WT` badge, drawn in the dirty colour when a linked worktree has uncommitted work) and stashes (archive-glyph badge)
- Open PR count and numbers on the repo row, in the repo pane, and in the footer
- Repo pane header reads local-to-remote: local path, branch and stashes on the left of each row, remote URL, PRs and CI flush right
- A slot digest: stand on a repo list section header ("github activity",
  "local changes", "also active") and pane 2 shows what happened across that
  section's repos since the slot boundary, as repoz's default mode heads it.
  A totals line (`since 2026-10-26  Evening (18-00)   5 repos · 14 commits ·
  2 PRs · 1 red CI`), then one line per touched repo, most recent push first:
  its commit count in the window (with the authors' initials when more than
  one), the newest subject, its CI and open PRs. A repo not cloned, or not
  fetched since the window began, says only `pushed 2h ago`. "local changes"
  lists the worklist instead: dirty files, stashes and unpushed commits. The
  inspector details the line under pane 2's cursor (commits with author and
  time, PR titles, the CI run), `Enter` goes to that repo's row and `t` cycles
  the window: slot, today, yesterday (since its midnight), week (since
  midnight seven days ago). Counts come from `git log --since` over the local
  and remote-tracking branches, read off the UI thread once per window and
  refs; nothing extra is asked of GitHub
- Repo groups defined in your config (cycle with `g`)
- Repo ownership in the detail pane: `owner/name` flush right on the status
  line, colored and glyphed by kind (org, private, public)

The dashboard opens immediately: the first frame paints before any GitHub or
filesystem work, and a spinner in the header marks the load until repos land.

The header says when the GitHub data (CI, PRs, the contribution strip) is not
current, in one segment beside the watch eye:

- nothing, when the last request to GitHub succeeded and polling keeps its
  interval;
- `github cached 14:02` (dim), when launch painted from the disk cache and the
  request that confirms it is still on its way. One request follows a cached
  launch even with watch off;
- `⚠ github offline at 14:30, as of 14:02` (orange), when the last request
  failed. The word says how (`offline`, `rate limited`, `server error`,
  `access refused`, `http error`, `api error`, `not signed in`,
  `bad response`, `timed out`), the first time is when that request failed
  and the second is when the data on screen left GitHub, both written like
  the `swept` time. A cache whose age cannot be read says `age unknown`
  instead. While a rate limit lasts it says when polling resumes in place of
  when it failed: `⚠ github rate limited until 15:00, as of 14:02`;
- `github slowed, allowance low` (dim), with watch on, when the account's
  hourly GitHub allowance is running low and polls come further apart than
  `github_poll_interval`;
- `github paused until 15:00, allowance nearly used` (orange), with watch on,
  when the allowance is nearly gone and no automatic poll goes before it
  resets.

A failed request never blanks or rewinds what is on screen and never touches
the cache. With watch on, the first retry comes one `github_poll_interval`
later; after that the wait doubles with each failure in a row, up to about ten
minutes (8 to 12, randomised so several fleetz windows do not retry
together). Offline and signed out keep retrying every interval, so the
dashboard recovers as soon as the network or the sign-in is back. With watch
off, none follows on its own, and `r` retries at once. The segment clears on
the first success. A narrow header shortens it to `⚠ github`,
`github cached`, `github slowed` or `github paused` rather than dropping it.

fleetz also reads GitHub's hourly allowance off every answer and spaces its
automatic polls to last until the allowance resets, leaving a tenth of it for
your own `r` and `R` and other tools on the same account. While the
allowance is plentiful this changes nothing; when it runs low polls come
further apart, and near the end they stop until the reset. A rate limit
(a 403 or 429 that GitHub marks as a rate limit, or a `RATE_LIMITED` error)
holds polling until GitHub says it lifts.

While GitHub is known to be rate limiting, `r` and `R` send nothing, since a
refused request can make GitHub hold the limit longer: `r` re-checks the local
repos only and `R` rescans your folders against the GitHub data already on
screen. Both say `GitHub rate limited, resumes at 15:00` in the status line.
`R` pressed while a reload is running, or under a limit while a GitHub
refresh is still out, does nothing and says `Already refreshing`.

A repo the last GitHub poll did not return keeps its CI and PRs, drawn dimmed:
its `Pending` lamp stops blinking, the detail pane says the data is from an
earlier refresh, and it no longer holds polling at the 15 second floor.
Switching `gh` accounts while fleetz runs rebuilds the list from the new
account's data on the next poll, with no git fetch and no change to the time
window.

The saved copy of GitHub's answer lives in the `fleetz` folder of your
platform cache directory (`~/Library/Caches/fleetz` on macOS,
`$XDG_CACHE_HOME/fleetz` or `~/.cache/fleetz` on Linux), one file per `gh`
account, for example `github-v2-github.com-2a8d4ab865f3ca20-50.json`: format
version, host, a 16 character fingerprint computed from your `gh` token (never
the token itself) and `repo_limit`. Files are readable only by you. Launch asks
`gh auth token --hostname github.com` which account is signed in, waiting at
most 2 seconds, and paints only that account's copy, whatever its age, which
the first poll replaces a moment later (with watch off too); an account with
no saved copy yet paints nothing until GitHub answers. Signed out, or when
`gh` does not answer in time, launch paints the most recently saved copy on
the machine.
The single `github-{repo_limit}.json` that earlier versions wrote is read only
until any account has saved its own copy. A copy superseded by another account,
token or format version is deleted at launch once it has gone 30 days without
being written; nothing else in the folder is touched.

A `git fetch` that fails (from `f`, `F`, `r`, `R`, launch or a watch sweep) is
marked on the repo rather than passed off as a fetch:

- the row, in the list and the compact grid, gets an orange marker beside the
  status, `⚠ unreachable 3↓`. The word says how: `timeout`, `auth`,
  `unreachable` (network, DNS or host), `no remote` (missing, or not a
  repository), or `fetch` for anything else;
- the repo pane spells it out under the counts, `⚠ fetch failed: remote
  unreachable, as of 14:02`, where the time is the last fetch that worked:
  behind and ahead are counted against the remote refs it brought. When that
  is not known it says `age unknown`. Below it, git's own first error line,
  with any credential in a URL removed;
- after `f`, `F` or `r`, the status line says what failed: `fetch failed:
  fleetz, remote unreachable` for one repo, `2 of 12 fetches failed` for
  several. Nothing extra is drawn when every fetch worked.

A failure keeps the repo's local state and its existing remote refs, and
starts nothing new: the repo is fetched again when it would have been anyway
(the next watch sweep, a push GitHub reports, `f`, `F`, `r` or `R`). The marker
clears on the next fetch that works. Credentials never reach the screen: the
remote URL in the repo pane is drawn without credentials too (all userinfo,
user and password, except for ssh remotes, whose login name stays and only a
password goes).

A fetch runs as long as git keeps reporting progress, however big it is; while
it runs, the row says how much has arrived, `fetching 312 MB`. One that reports
nothing for 30 seconds is stopped together with every process it started
(`ssh`, `index-pack`), and the half-written `tmp_pack_*` files it left in the
repo's pack directory are removed. Such a repo is then left out of the
automatic fetches (the watch sweep, a push GitHub reports, the refetch after
an outage) for two watch intervals, then four after a second timeout in a row,
doubling up to ten minutes. `f` and `F` fetch it regardless; a fetch that
works, or `r` or `R`, lifts the wait.
A repo is never fetched twice at once: while its fetch runs or waits its turn,
a sweep, a poll, `f`, `F`, launch or `R` refreshes its local state and lets
that fetch finish.

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
- a [Nerd Font](https://www.nerdfonts.com/) in your terminal, unless you set
  `icons = "plain"` in the config or the `,` settings, which draws the repo
  kind, stash and watch marks as plain ASCII instead (the contribution strip's
  day squares still need one)

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
in the header instead. The row opens with the focused pane's own keys; as
the terminal narrows, the global keys lose their descriptions first, then the
pane's keys lose theirs, then the pane's keys go, and the summary goes last.

| Key | Action |
|-----|--------|
| `1` / `2` / `3` / `4` | Focus repo list / detail / inspector / contributions. On the focused inspector or strip, its number folds it to one row, its title (still focused) and unfolds it again; on a folded one from elsewhere, it unfolds and focuses it |
| `Tab` | Cycle panel focus |
| mouse | Click a panel to focus; click the `[3]` or `[4]` tag in the inspector's or the strip's title to fold it to one row, its title, and again to unfold and focus it; drag a pane border to resize it (a `⠿` grip marks each draggable border, and lights up while you drag it); scroll wheel scrolls whichever panel the cursor is over. Scrolling moves the view, never the selection: the highlight stays with the row it is on and travels off the pane if you scroll past it, and the next `j`/`k` re-enters at the edge you are looking at |
| `j` / `k`, `↑` / `↓` | Repo list: move selection, stopping on the section headers in the detail view (a header shows its slot digest in pane 2; clicking a header, or resting the pointer on one, lands there too, and a click on a repo row selects it) · Detail: pick commit, or the digest line on a header · Inspector: scroll · Contributions: move the day cursor up/down a weekday (a caret above the grid marks the column) |
| `J` / `K` | Drive the **next** pane without leaving the one you are in: repo list → window 2, window 2 → the inspector, inspector → back to the list. Aimed at window 2 it steps that pane's commit cursor, so the inspector reads out each commit in turn; past the last commit it falls through to scrolling, so the sections below a short list stay reachable. Two exceptions: `graph` docks no inspector, so there the keys stay on the pane and walk its **rows** a line at a time where plain `j`/`k` jumps whole commit blocks; and the contributions strip is skipped, being a horizontal timeline with nothing below the fold to scroll into |
| `PgUp` / `PgDn` | Scroll the focused pane itself by a page, leaving its selection where it is |
| `v` | Toggle detail vs compact view. The compact grid packs as many columns as the terminal has room for |
| `[` / `]` | Repo pane: step its body between six views. `info` is the glance (branches; a `ci` section, below; worktrees, one row each with its branch or detached HEAD, dirty count, ahead/behind, last commit age and path, relative when inside the repo; stashes; newest 6 commits with diffstats); `commits` the complete history with a computed graph column, one row per commit; `reflog` every recorded HEAD movement; `graph` git's own expanded `log --graph`, wrapped to the pane and given the whole column (its blocks already carry what the inspector would repeat, so no inspector is docked there); `branches` every local branch and every remote-tracking branch in one list, read-only, with the inspector showing the highlighted branch's tip commit (a remote branch nothing local claims is marked `not fetched`, and its tip is not on any local branch, so the inspector says so rather than drawing a blank); `ci` the selected repo's recent workflow runs on every branch, newest first, one row each: a lamp in the CI colours (pulsing while a run is in progress, under watch), the workflow, the branch or tag, the event, the title, the duration (ticking while it runs) and the age. Its first line says how current the list is (`as of 12:02`), or when and why the last attempt failed (`⚠ 12:05  offline`, `rate limited until 12:40`, `not signed in`), or why there is nothing to list (`loading`, `no GitHub remote`, `no workflow runs`, `GitHub off under -d`). Only `ci` polls: the selected repo, while the view is on screen, over GitHub's REST API with conditional requests (a `304` costs nothing); runs read on an earlier visit stay for the session, so coming back paints at once. `info`'s `ci` section polls nothing: the default branch's check state as the fleet poll left it (`◉ default branch passed`, or `no checks`), then the latest run of each of the three most recently run branches once `ci` has read them, else a `] for runs` hint; a repo with no GitHub remote has no section. Each keeps its own scroll position and selection, and the inspector follows whichever is active. The active view is named in the pane's top border in its own colour (the tabs are capitalised; the pane's own headliner repeats the same name over a divider that fades from the view's colour into the background across the pane), and the repo header below it stays pinned, so both survive scrolling. In `info` the local branches pin with it where the pane has the rows to spare, since they are what the repo's behind count is a sum of; the remote-only list below them collapses to a count, being bounded by what everyone else has pushed rather than by what you have: `B` expands it in place, and `branches` lists them all |
| `[` / `]` on the inspector, in `ci` | While pane 2 shows `ci`, the inspector details the highlighted run in three bodies, and on the focused inspector holding a run `[` / `]` step between them (anywhere else they step pane 2's views, and a run of them pressed on the inspector in another view goes on through `ci` until another key); the title names the one shown, `[3] inspector  jobs`. `jobs`: one row per job with its lamp and duration; `j` / `k` move the `▸` job cursor (the first failed job, else the first) and the body follows it; only the marked job shows its steps on one line (`✓` passed, `✗` failed, `●` running, `·` queued or skipped) and, when it failed, its failing step and the last 12 lines of its log, dimmed, read once the run has been highlighted for a moment; the jobs never wait for the log. `log`: the marked job's whole log (its last 5000 lines when it is longer), timestamps and colours stripped, once the job has finished. `script`: the workflow file the run ran, at the run's commit, read from your clone when it has the commit and otherwise from GitHub, with the failing step's block marked in red when the run failed. Each body keeps its own scroll, reset when the highlighted run changes; `J` / `K` from pane 2 scroll it. Each log and file is fetched once and held for the session; nothing is fetched until the run has been highlighted for a moment |
| `B` | `info`: expand or collapse the remote branch summary. The full list, with each branch's state, is the `branches` view |
| `y` | Copy the hash of the commit pane 2's cursor is on to the clipboard; the status line says `copied <hash>`. Clicking a row in `info`'s recent commits, `commits`, `reflog` or `graph` moves the cursor there, and clicking the hash itself also copies it. Uses the first of `pbcopy`, `wl-copy`, `xclip`, `xsel` on `PATH`, else an OSC 52 escape to the terminal (works over ssh and tmux where the terminal allows it) |
| `c` | Switch the contributions strip's source, while it is unfolded (the footer's `[c]` lights up in that source's colour): GitHub's own calendar, or your commits in the listed repos. The strip is always on screen, folded or not: three years of activity, GitHub-style. Each day is a Nerd Font square icon, which sits level with the `Mon`/`Wed`/`Fri` labels where a half block cannot, in three sizes (`contributions_cell_size`); the grid scrolls rather than shrinking, with the newest week pinned to the right edge. It opens on today: the title reads out the marked day's count, and the total for the span rides the bottom border beside the legend |
| `4` | Focus the contributions strip, unfolding it if it is folded; on the focused strip, fold or unfold it |
| `h` / `l`, `←` / `→` | Contributions: move the day cursor back/forward a week. The grid scrolls with it, oldest weeks hiding off to the left |
| mouse | Contributions: click picks a day, drag pans the grid, and any wheel axis (including shift-wheel) scrolls it sideways |
| `Home` / `End` | Contributions: jump to the oldest / newest week |
| `Enter` | Open repo in lazygit (pull / merge live here). On a section header: go to the row of the digest line pane 2's cursor is on |
| `e` | Open repo in `$EDITOR` |
| `o` | Open shell in repo dir |
| `O` | Open PR in browser |
| `l` / `a` / `x` / `O` | `ci`, with pane 2 focused, on the highlighted run: `l` its log in the inspector (the `log` body, opened and focused, showing the failed job of a failed run), `a` rerun it with `gh run rerun` (`--failed` when it failed), `x` cancel it with `gh run cancel`, `O` open its page in the browser. `gh run rerun` and `gh run cancel` take over the terminal as `Enter` does for lazygit and name the repo with `-R`; back in fleetz the repo's runs are polled at once. A run that does not allow the action says why in the status line (`run has not finished`, `run is not in progress`). In every other view these keys keep their usual meaning |
| `f` | Fetch selected repo (updates ahead/behind, no merge), with no size cap: how you pull a repo marked `fetch ~N MB, press f` or `fetch over N MB, press f` (see `fetch_max_mb`). The row shows `fetching N MB` while it runs. A failure is marked on the repo and named in the status line |
| `F` | Fetch all repos, each capped at `fetch_max_mb`; a repo held for its size is skipped until `f`. If any fail, the status line says how many |
| `b` / `s` | Branches overlay: preview + switch (`Enter`), clean up merged (`space` to mark, `d` to delete) |
| `t` | Time window: pick how far back to show activity (slot / hours / days / weeks / months / years). On a section header it cycles the digest's window instead: slot / today / yesterday / week |
| `g` | Cycle group filter |
| `G` | Show all (clear group) |
| `/` | Fuzzy-search repos by name (best match first, `flz` finds `fleetz`), or type `#dirty` `#behind` `#ahead` `#ci` `#prs` `#stash` to filter |
| `0` | Clear `#tag` filters (keeps free-text search) |
| `,` | Settings overlay: refresh interval, startup view, work hours, pane persistence, contributions strip and its cell size, icon set (`nerd` or `plain`), loading screen. `Enter` saves to `config.toml` |
| `<` / `>` | Narrow / widen the repo list |
| `+` / `-` | Grow / shrink the inspector pane |
| `=` | Reset both panes to their default sizes |
| `r` | Refresh: re-stat the listed repos and re-poll GitHub, so CI status updates, and fetch the repos whose remote moved. No rediscovery. A failed poll keeps the rows as they are and marks the header; failed fetches are marked on their repos and counted in the status line. Goes at once even while automatic polling is slowed or backing off; under a rate limit it re-checks the local repos only and says when GitHub resumes |
| `R` | Reload: re-fetch GitHub (cache-bypassed) + rediscover repos, so new ones appear. If GitHub cannot be reached it rebuilds from the signed-in account's saved copy and says `Reloaded from cache: GitHub offline` (or why) instead of `Reloaded`; with no saved copy for that account the reload fails and says so, rather than showing another account's data. Under a rate limit it rescans your folders against the GitHub data already on screen, sends nothing, and says `Rebuilt locally: GitHub rate limited, resumes at 15:00`. Pressed while a reload is running, or under a rate limit while a GitHub refresh is still out, it says `Already refreshing` and does nothing |
| `w` | Toggle watch: sweep the listed repos (fetch included) every `refresh_interval` seconds and re-poll GitHub. A fetch that fails is marked on its repo, not in the status line. While it is on, a red eye (`󰈈 watch`) in the header pulses between two shades so the dashboard says it is live without anything moving; off, it is a grey crossed-out eye (`󰈉 watch`). With `icons = "plain"` the eye is `o watch` on and `- watch` off |
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

# Glyph set for the repo kind, stash and watch marks: "nerd" (default) draws
# Nerd Font icons, "plain" draws ASCII any monospace font has: `@` org, `-`
# private, `+` public, `$` stash (as git's own prompt marks one), and `o` / `-`
# for watch on / off. An unknown value keeps "nerd". Also in the `,` menu,
# where it applies live.
icons = "nerd"

# Start in watch mode. On by default: a dashboard you leave open should keep
# itself current. --watch / --no-watch override for one session, `w` toggles live.
watch = true

# Seconds between watch sweeps: local git status plus a fetch from every
# remote. This one costs real network traffic, so keep it generous -- the
# GitHub poll below already notices when a remote moves and fetches just that
# repo, so the blanket sweep is a backstop rather than the main path.
refresh_interval = 60

# The most any fetch you did not ask for by name may download, in MB: launch,
# `R`, the watch sweep, the targeted fetch after a push, and `F`. A repo
# GitHub says grew by more since its last fetch is not fetched, and a fetch
# that passes it is stopped (what it got so far is discarded: a fetch cannot
# resume). Either way its row reads `fetch ~N MB, press f` or `fetch over N
# MB, press f`, and `f` on it fetches it in full. 0 means no cap. Anything
# but a whole number from 0 up keeps 200.
fetch_max_mb = 200

# Seconds between GitHub re-polls while watching. One GraphQL call covers
# every repo, so this is the cheap way to learn what changed: it keeps CI
# status current, and any repo whose remote moved (you pushed from another
# machine, someone else pushed) gets a targeted fetch straight away rather
# than waiting for the next blanket sweep. Automatically tightened to 15s
# while a CI run is in flight, forced immediately after a local push is
# detected, and floored at 15s. While polls are failing it is at least this
# interval, doubling per failure in a row up to about ten minutes, with no
# 15s floor and no push shortcut. A low hourly allowance or a rate limit
# stretches it too.
github_poll_interval = 30

# Write pane sizes back on exit and restore them next launch. Off by
# default: every launch starts from the built-in split, so a layout nudged
# for one repo doesn't silently become permanent. Toggle it in the `,` menu.
persist_pane_sizes = false

# Show the loading screen while the first load of a launch runs: a field of
# scrambled characters with LOADING held still in block capitals in the
# middle. When the repos land the dashboard replaces it in one frame. Any
# keypress dismisses it. Set false to start on the empty dashboard instead.
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
| GitHub calls | Uncached, one GraphQL call per run | Disk cache painted at launch, `R` bypasses it |
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

First-class worktrees (today: a read-only `WT` badge, in the dirty colour when a linked worktree is dirty, and in `info` a
`worktrees` section with each linked worktree's branch or detached HEAD, dirty
count, ahead/behind, last commit age and path):

- A `w` overlay like the branch overlay, reusing those rows: Enter opens
  lazygit or a shell there.
- Create and prune from the overlay (`git worktree add` under a configured
  root; prune merged ones with the existing y/n confirm flow).
- Per-worktree dirty state surfaced in the repo list (badge + `-d` mode), so
  uncommitted work in a worktree is visible without opening `info`.

Shared core with repoz: extract discovery, git porcelain parsing, and the
GitHub client + cache into a library crate both tools consume; a repoz TUI (or
rewrite) becomes a thin one-shot frontend over the same `RepoStatus` snapshot,
with `--json` output for free.

## History

This tool was previously named `epoz`. The `epoz` name now belongs to [epoz](https://github.com/Gaurgle/epoz), a modular dashboard app; the TUI lives on as `fleetz`, the interactive sibling of `repoz`.

## See also

- [repoz](https://github.com/Gaurgle/repoz): the headless one-shot version
- [notez](https://github.com/Gaurgle/notez): local-first notes and todos, CLI/TUI plus the epoz desktop app

## Credits

The commit graph in the repo pane is a port of [lazygit](https://github.com/jesseduffield/lazygit)'s,
from `pkg/gui/presentation/graph/` at `v0.59.0` -- including the insight that
makes it work, which is to compute the topology from parent hashes instead of
parsing `git log --graph`. That is what lets the history draw one row per
commit instead of interleaving git's filler rows. Nine of lazygit's own test
fixtures are ported with it. lazygit is MIT licensed; see [NOTICES.md](NOTICES.md).

[repoz](https://github.com/Gaurgle/repoz) is fleetz's sibling and the source of
its discovery rules, time-slot logic and status vocabulary.
