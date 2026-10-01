# Performance baseline

Ticket FZ-01 (issue #14). A measured baseline of startup, idle, refresh and
input behaviour, taken before any concurrency, caching or rendering change, so
later tickets have something to compare against. Nothing in `src/` was changed
to take it. Every number below was measured on a release build of one commit,
on synthetic fixtures, with the commands in the appendix; anything inferred
rather than measured says so.

## Summary

| Case (254x74, release build) | Small fleet (14 rows) | Large fleet (70 rows) |
|---|---|---|
| First paint, any cache state (range of case medians) | 6.3 to 6.9 ms | 6.3 to 6.7 ms |
| First useful row, warm cache, intro off | 257 ms | 1148 ms |
| Completed initial summary, warm cache, intro off | 257 ms | 1149 ms |
| Screen settled, warm cache, intro on | 1788 ms | 2742 ms |
| First useful row, cold cache, live GitHub (one fleet, n=5) | 6273 ms | not run |
| Offline, cold cache: error screen, no rows | 14 ms | 13 ms |
| Manual refresh `r`, offline | 138 ms | 496 ms |
| Keypress to first output byte, median / max | 4.3 / 11.4 ms | 5.7 / 11.9 ms |
| Idle CPU, `--no-watch` | 0.12% | 0.12% |
| Idle CPU (fleetz process only), watch on, offline | 0.80 to 1.03% | 1.17 to 1.51% |
| Terminal output to settled screen, intro off / on | 79 KB / 6.3 MB | 83 KB / 9.0 MB |

Medians unless stated; sample sizes and spreads are in the sections below.
These are one session's numbers. A reviewer's rerun of the same binary in a
later session came out about 4 to 5% slower in wall time at equal CPU (see
case A), so compare a candidate with the base binary in one session rather
than with this table (see [Recommended targets](#recommended-targets)).

Ranked bottlenecks, by evidence (details in [Bottlenecks](#bottlenecks-ranked-by-evidence)):

1. A cold cache blocks every row on the GraphQL fetch: about 6.2 s of the
   6.3 s cold start (`load_repos` awaits `fetch_dataset` before discovery).
2. git subprocess count times per-spawn cost: 707 git processes for the large
   fleet's startup (counted). One `git rev-parse HEAD` costs 8.4 ms through
   `/usr/bin/git` against 3.1 ms called directly, measured in isolation as
   sequential calls from Python, not inside fleetz under concurrency. With git
   resolved past the Xcode stub the same startup drops from 1148 ms to 404 ms
   (measured). That the stat loop is where this time goes is inferred from the
   code, since per-phase timing is unavailable.
3. The intro and the loading field write 6 to 9 MB to the terminal per launch
   and put the first legible row about 0.65 s later than with the intro off.
4. Remotes that look stale at launch add a `git fetch` per repo before any
   row appears (+105 ms small, +429 ms large against local remotes).
5. Watch mode at idle: about 1 to 1.5% CPU for the fleetz process, a repaint
   every 500 ms (215 to 325 bytes/s), plus the periodic sweep's git children.

## Environment

| Item | Value |
|---|---|
| Commit | `28ebba796ab6b1bea72e96f908e0082b61456ce9` (main, includes FZ-02) |
| Build | `cargo build --release`, default release profile (`Cargo.toml` has no `[profile]` section) |
| Binary | `target/release/fleetz`, 6.7 MB, sha256 `abfe2daf644159f6d2ae96fb1a2a0fd3b8a6cbff95983bb00433039c9b59060b` |
| Toolchain | rustc 1.96.1, cargo 1.96.1 |
| OS | macOS 14.2.1 (23C71), Darwin 23.2.0 arm64 |
| Hardware | Apple M1 Max, 10 cores (8 performance, 2 efficiency), 64 GB; on AC power, low power mode off |
| Owner's terminal | Ghostty, tmux 3.6a inside it (`TERM=tmux-256color`, client `xterm-ghostty`), window 254x74 |
| Measured terminal | A pty owned by the Python harness, sized 254x74 (and 120x40 in case J), `TERM=tmux-256color`; cross-checked in a private tmux 3.6a server at 254x74 (case M) |
| git | 2.39.3 (Apple Git-146), resolved on `PATH` as `/usr/bin/git`, the xcrun stub that dispatches to `/Library/Developer/CommandLineTools/usr/bin/git` |
| Python (harness only) | 3.12.1 |
| Background load | Two instances of the owner's installed fleetz were running in watch mode for the whole session (sampled at 0.3 to 0.5% CPU). Nothing else was building or testing. Each case ran alone, sequentially. |
| Measured on | 2026-10-01, 15:20 to 16:15 CEST |

### Fixtures

Two synthetic fleets built by `mkfleet.py` (appendix). Every repo is local:
`origin` of each clone is a bare repo under the fixture's `remotes/`
directory, so a sweep's `git fetch` never leaves the machine.

| | Small | Large |
|---|---|---|
| GitHub-matched clones (pass 1) | 10 | 60 (50 in `viewer.repositories`, 10 in an organization) |
| Local-only dirty repos (pass 3) | 2 | 5 |
| Not cloned (payload only) | 2 | 5 |
| Rows on screen | 14 | 70 |
| Size on disk | 4.8 MB | 26 MB |

Per matched repo: 300 linear commits on `main` touching 20 small files (above
`HISTORY_COUNT` = 200, so every `-200` cap engages). By index `i`: every third
repo has a tracked `feature` branch of 5 commits; `i % 7 == 3` is 1 behind;
`i % 5 == 2` is 2 ahead; `i % 6 == 4` has one stash; `i % 4 == 1` is dirty (one
modified, two untracked files). Every `FETCH_HEAD` is newer than every
`pushedAt`, so `stale_remotes` fetches nothing at launch unless a case says so.

The canned GitHub payload (`github-50.json`) has the shape `fetch_repos`
parses and accepts as a usable cache: all three calendar year windows, a
`pullRequests` field on every node (0 to 2 open PRs), and CI states cycling
SUCCESS, SUCCESS, FAILURE, SUCCESS, PENDING. Every `pushedAt` is five minutes
before generation, so the default Auto window keeps every repo. Two side
effects of the fixture are visible on screen and in the numbers:

- The large payload holds 65 repos against `repo_limit = 50`, so its header
  carries "(window exceeds fetched repos, repo_limit=50)".
- A PENDING repo is present in both fleets, so a watch-mode run polls GitHub
  at the 15 s floor (`github_poll_secs`) instead of every 30 s.

### Configuration, intro and cache conditions

- Config file: only `intro = true` or `intro = false`, written per run. Every
  other key is at its default (`refresh_interval` 60, `github_poll_interval`
  30, `cache_ttl` 120, `repo_limit` 50, contribution strip on GitHub).
- Watch: `--no-watch` unless a case says watch on (`--watch`).
- Time window: Auto (no `window.toml`), which at the time of the runs was
  "since 2026-09-30 Evening (18-00)".
- Intro: off unless the case says on.
- Warm cache: the canned payload is copied into the isolated cache directory
  before each run. Startup reads it with `CacheMode::AnyCached`, which serves
  any cached payload regardless of age, so warm means "a usable payload is
  present", not "within TTL".
- Cold cache: the isolated cache directory is emptied before each run.

### Isolation

Every measured process ran with `HOME` and `REPO_CHECK_DIR` inside the
fixture, a fresh environment (no inherited `XDG_*` or git variables), and a
`gh` shim first on `PATH`.

- `gh` is only used for `gh auth token` (`github::get_gh_token`); the request
  itself goes from reqwest straight to `https://api.github.com/graphql`.
- Offline shim: `gh auth token` prints a dummy token. Offline runs also set
  `HTTPS_PROXY` (and the other proxy variables) to `http://127.0.0.1:9`, where
  nothing listens. A check run against a logging listener on another port
  received `CONNECT api.github.com:443`, which shows reqwest honours the
  variable; with port 9 the connection is refused locally and nothing reaches
  the network. This exercises the transport-failure branch of `fetch_repos`.
- Live shim: forwards `gh auth token` to the real `gh` under the real `HOME`
  and refuses every other subcommand. Used only for the five runs of case E.
- Proof the real state was not written: an isolated run that applied a time
  window wrote `window.toml` into the fixture's cache directory while the real
  `~/Library/Caches/fleetz/window.toml` kept its mtime (1790184085, Sep 23).
  The real `github-50.json` changed mtime during the session, but it also
  changed every 30 s or so with no fz-01 process running (18 s wait observed),
  because the owner's own fleetz instances rewrite it on each poll; it contains
  none of the fixture names. In-app paths showed `~/repos/fx-000` resolving to
  the fixture.

## Milestones

What the code does on launch, at the base commit:

1. `main` loads config, enters raw mode and the alternate screen, spawns the
   input thread, the 100 ms tick and `spawn_load(LoadKind::Initial)`, then
   calls `run_event_loop`.
2. `run_event_loop` draws once before reading any event. With the intro on that
   frame is the loading field (`ui::intro::Loading`), repainted every 16 ms by
   `spawn_loading_task`; with it off it is `ui::render` of an empty app, with
   the `loading` spinner in the header.
3. `load_repos` runs on a background task: `fetch_dataset` (cache or network),
   then `assemble_view`: pass 1 and pass 2 discovery, `git fetch` of
   `stale_remotes`, `build_repo_status` for every target behind a
   `probe_concurrency()` semaphore (10 on this machine), pass 3
   (`local_only_status`), then the not-cloned rows.
4. Only when all of that is done does it send one `AppEvent::Loaded`.
   `apply_load_result` swaps in every row and clears `loading`; the next draw
   is the first frame with data. With the intro on, that draw captures the
   frame and starts the wave (`spawn_intro_task`, 16 ms frames for 1.5 s).

The three milestones, as measured:

| Milestone | Definition in code | How it was observed |
|---|---|---|
| First paint | The `terminal.draw` before the event loop's first `recv` | First output write after which the screen model holds any non-blank cell |
| First useful row | The first draw after `AppEvent::Loaded` | First write after which a repo-list row contains a repo name (`fx-`, `lo-`) or `not cloned`. Intro on: the first moment such a row is legible through the wave |
| Completed initial summary | The same draw, finished: header without `loading`, footer summary drawn | First write after which the header lacks `loading` and the footer holds `synced` or `not cloned` |

Because one event carries every row, the first useful row and the completed
summary come from the same frame; they differ only by the time it takes to
write that frame (about 1 ms here). There is no progressive state in between:
a 70-row fleet shows nothing until its slowest repo and pass 3 are done. With
the intro on, the footer resolves before the first row, because the wave
starts at the bottom-left corner.

The harness (`run.py`) forks fleetz in a pty, timestamps every read of its
output with `time.perf_counter`, and replays the recording through a small
screen model (`vt.py`) after the process exits, so the analysis cannot slow the
run. Times are from `fork` to the read that completed the condition. They are
the moment fleetz wrote the frame to its terminal, not the moment a terminal
drew it; case M checks the same milestones as tmux sees them.

## Startup

### Warm cache, intro off (case A, n=20 per fleet, first session)

| Measure | Small min / median / max | Large min / median / max |
|---|---|---|
| First paint | 5.8 / 6.3 / 9.5 ms | 5.8 / 6.3 / 7.5 ms |
| First useful row | 239 / 257 / 306 ms | 1114 / 1148 / 1191 ms |
| Completed initial summary | 240 / 257 / 307 ms | 1115 / 1149 / 1192 ms |
| Output to settled screen | 79,173 bytes in all 20 runs (see note) | 83,371 bytes in all 20 runs (see note) |
| of which loading frame / data frame | about 25 KB / 54 KB | about 26 KB / 58 KB |
| Process-tree CPU, user / sys (whole run, about 0.8 s and 1.7 s) | 0.63 / 0.65 s | 3.57 / 3.67 s |
| Peak RSS reported by `wait4` | 13.0 MB | 22.3 MB |

The CPU figures come from `wait4`, which on macOS includes waited-for
descendants, so they are fleetz plus every git process it ran. Inferred from CPU over wall time, the large fleet keeps about six cores
busy for the whole load.

The output count is not exact in general: it varies by one spinner frame
(about 59 bytes) depending on whether a 100 ms tick repaints the `loading`
spinner before the data frame. The reviewer saw 79,173 to 79,232 (small) and
83,371 to 83,430 (large).

The very first launch after the build (a dry run, n=1, one-off) took 206 ms to
first paint against 6 to 7 ms on every later launch. Not investigated; likely
first-exec work by the OS on a new binary. The reviewer's first run hit the
same effect (below), which is why the rerun recipe discards a warm-up run.

#### Second session: reviewer rerun

The independent reviewer rebuilt case A from this document alone, same binary
(sha256 matches), same fixture sizes, the owner's two fleetz instances running.
Numbers as reported by the reviewer, not by the worker:

| Measure | Large, batch 1 (n=10) | Large, batch 2 (n=10) | Large, both without run 1 (n=19) | Small (n=20) |
|---|---|---|---|---|
| First paint, min / median / max | 6.0 / 6.4 / 235 ms | 6.0 / 6.7 / 6.9 ms | not reported | 5.9 / 6.6 / 7.7 ms |
| First useful row, min / median / max | 1150 / 1192 / 1466 ms | 1143 / 1209 / 1242 ms | 1143 / about 1200 / 1269 ms | 247 / 269 / 317 ms |
| Output to settled screen | 83,371 to 83,430 bytes | not reported | not reported | 79,173 to 79,232 bytes |
| Process-tree CPU, user / sys | 3.57 / 3.67 s | not reported | not reported | 0.63 / 0.65 s |

Batch 1, run 1 was a first-exec outlier (235 ms first paint, 1466 ms first
row). Without it, 9 of 19 large runs are above the first session's maximum of
1191 ms, at the same CPU. Wall time is about 4 to 5% higher at equal CPU, which
points to session-to-session scheduling differences, not to the code. The
first session's min to max band is therefore not a regression threshold.

### Warm cache, intro on (case B, n=10 per fleet)

| Measure | Small min / median / max | Large min / median / max |
|---|---|---|
| First paint (loading field) | 6.1 / 6.9 / 7.1 ms | 6.3 / 6.7 / 7.8 ms |
| Footer summary legible | 786 / 814 / 852 ms | 1717 / 1751 / 1947 ms |
| First legible row | 897 / 923 / 962 ms | 1779 / 1814 / 2009 ms |
| Screen settled (wave over) | 1761 / 1788 / 1842 ms | 2709 / 2742 / 2938 ms |
| Output to settled screen | 6.22 / 6.31 / 6.40 MB | 8.91 / 9.01 / 9.38 MB |
| Output writes (pty reads of at most 1 KB) | 6140 / 6229 / 6318 | 8799 / 8902 / 9286 |
| Process-tree CPU, user / sys | 1.00 / 0.69 s | 4.07 / 3.72 s |

Settled minus the intro-off data frame is 1.53 s small and 1.59 s large, which
matches the wave's 1.5 s `DURATION`. While the load runs, the loading field
alone wrote about 1.7 MB in the first 0.3 s of the small fleet's launch, before
its data arrived at about 0.26 s (bytes per 100 ms bucket, one probe run, n=1).

### Watch on, warm cache, offline (case D, n=10 per fleet)

| Measure | Small min / median / max | Large min / median / max |
|---|---|---|
| First useful row | 245 / 263 / 284 ms | 1167 / 1211 / 1319 ms |
| Completed initial summary | 246 / 264 / 285 ms | 1168 / 1212 / 1320 ms |

A cache-served launch leaves `last_github_poll` unset, so the first tick
starts a GitHub poll: the shim log shows exactly one `gh auth token` per run.
Offline, the poll's transport error falls back to `read_stale` and returns the
cached payload as a success, so nothing on screen says the poll failed.

### Cold cache, live GitHub (case E, small fleet, n=5)

| Measure | min / median / max |
|---|---|
| First paint | 6.1 / 6.8 / 7.2 ms |
| First useful row and completed summary | 5722 / 6273 / 7254 ms (runs: 7254, 6275, 5999, 5722, 6273) |
| Output to settled screen | 62,233 / 62,528 / 63,118 bytes |

The live payload is the owner's account, so the fixture clones do not match
it: the screen showed 13 rows (the fixture's dirty and ahead repos as local
rows, the account's active repos as not cloned). To separate the network from
the rest, the same payload was then served from the cache (n=10, offline):
first useful row 104 / 116 / 155 ms. The difference, about 6.2 s at the
median, is the token, the GraphQL round trip, parsing and the cache write; it
is inferred by subtraction, not timed directly. `gh auth token` alone, run
directly (n=10), takes 46 / 49 / 55 ms. A large-fleet cold run was not made:
the GitHub response does not depend on the local fleet.

### Offline (case C, cold cache, n=10 per fleet)

With no cache, the transport error has no stale payload to fall back on and
`load_repos` returns before discovery. The screen shows "Could not load the
fleet" and no rows, not even the local repos.

| Measure | Small min / median / max | Large min / median / max |
|---|---|---|
| First paint | 6.5 / 6.7 / 10.9 ms | 6.1 / 6.3 / 7.3 ms |
| Error screen | 12.8 / 13.9 / 26.5 ms | 12.2 / 12.5 / 15.0 ms |
| Output | 41,016 bytes | 41,016 bytes |

Offline with a warm cache is case A: launch reads the cache before it ever
asks for a token, and the shim log stayed empty for all 40 case A runs. The
only offline mode simulated is a refused connection; see the limitations.

### Remotes stale at launch (case F, n=10 per fleet)

Every `FETCH_HEAD` back-dated to 2026-01-01 before each run, so
`stale_remotes` fetches every matched clone before statting.

| Measure | Small min / median / max | Large min / median / max |
|---|---|---|
| First useful row | 338 / 362 / 413 ms | 1543 / 1577 / 1734 ms |
| Difference to case A median (inferred) | +105 ms | +429 ms |

The remotes are local bare repos, so this is the cost of the extra processes
and the `FETCH_CONCURRENCY` = 4 gate, not of a network. A real remote costs
more per fetch; this was not measured.

### Two terminal sizes (case J, large fleet)

| Measure | 254x74 (case A, B) | 120x40 |
|---|---|---|
| First useful row, intro off | 1148 ms (n=20) | 1103 / 1146 / 1220 ms (n=10) |
| Output to settled screen, intro off | 83,371 bytes | 30,731 / 30,789 / 30,847 bytes (n=10) |
| Screen settled, intro on | 2742 ms (n=10) | 2649 / 2655 / 2657 ms (n=3) |
| Output to settled screen, intro on | 9.01 MB | 2.20 / 2.20 / 2.21 MB (n=3) |

Startup time does not depend on the terminal size; output volume scales with
the cell count.

### tmux cross-check (case M, n=10 per fleet)

fleetz launched as the command of a private tmux server
(`tmux -L fz01check new-session -d -x 254 -y 74`), polled with
`capture-pane` until the first row and the footer summary were visible.
`tmuxcheck.py` times everything from just before `new-session`, which blocks
for 0.68 to 0.82 s while the server starts (`session_up`), and reports both
those raw times and the same times minus `session_up` (the `*_after_session`
fields). The table uses the `*_after_session` values. In the first session
the script printed only the raw fields and the subtraction was done in the
analysis, per run: for example a small run printed `session_up` 0.818,
`first_text` 0.836 and `first_row` 1.091, which is 18 ms and 273 ms after the
session came up. The raw medians from the saved logs were `session_up` 0.706 s
(both fleets), `first_row` 1.000 s (small) and 2.008 s (large). When the
pane's command starts relative to `new-session` returning is not known, so
these carry an offset of unknown sign of a few milliseconds, plus the 3 to 5
ms of one `capture-pane` call.

| Measure | Small min / median / max | Large min / median / max |
|---|---|---|
| First text visible | 12 / 13 / 18 ms | 12 / 14 / 25 ms |
| First row visible | 265 / 283 / 320 ms | 1222 / 1300 / 1369 ms |
| Footer summary visible | 266 / 284 / 323 ms | 1222 / 1302 / 1370 ms |

The same order and magnitude as the pty harness, later by roughly 25 ms
(small) and 150 ms (large). Part of that is the wrapper (`env -i` through
`fzenv.sh`) and tmux parsing the frame; how much each contributes is not
separated.

## Steady state

### Input to paint (case G, 3 runs x 20 keypresses per fleet, n=60)

`j` ten times then `k` ten times, 300 ms apart, after the screen had been
quiet for 1 s (`--no-watch`, so nothing else paints). Latency is from the
write of the key to the pty to the first output byte, and to the end of that
burst of output.

| Measure | Small: min / median / p90 / max | Large: min / median / p90 / max |
|---|---|---|
| Key to first output byte | 1.03 / 4.29 / 8.49 / 11.44 ms | 1.11 / 5.67 / 8.99 / 11.93 ms |
| Key to end of frame | 1.07 / 4.47 / 8.68 / 11.71 ms | 1.15 / 5.92 / 9.08 / 12.51 ms |
| Bytes per keypress frame | 564 / 2,882 / 7,964 / 10,899 | 564 / 2,887 / 3,239 / 3,284 |

The fastest presses are the ones that did not move the selection (the end of
the small list). This covers `handle_key`, `ui::render` of the whole frame and
the diff write; it does not include the terminal drawing the frame.

### Idle (case H, n=3 per fleet and mode)

Measured over a window that starts 1 s after the screen went quiet: 60 s with
`--no-watch`, 90 s with watch on (offline, so each poll fails locally and
falls back to the cache). CPU in the window is the fleetz process alone, from
`ps` utime + stime (10 ms resolution). Context switches and idle wakeups are
the deltas of `top -stats csw,idlew`.

| Measure | No watch, small | No watch, large | Watch, small | Watch, large |
|---|---|---|---|---|
| Output in window | 0 bytes, 0 writes | 0 bytes, 0 writes | 19,372 to 19,422 bytes, 198 to 200 writes | 29,235 to 29,385 bytes, 245 to 251 writes |
| Output rate | 0 | 0 | 214 to 215 B/s | 323 to 325 B/s |
| fleetz CPU in window | 0.07 to 0.08 s (0.12 to 0.13%) | 0.07 s (0.12%) | 0.72 to 0.93 s (0.80 to 1.03%) | 1.06 to 1.37 s (1.17 to 1.51%) |
| Context switches | 51 to 52 /s | 51 to 52 /s | 76 to 80 /s | 130 to 142 /s |
| `top` idle wakeups | 1.2 to 1.3 /s | 0 to 1.5 /s | 0 to 0.3 /s | 0.2 to 1.1 /s |
| Process-tree CPU, whole run | 1.36 to 1.39 s (62.2 s wall) | 7.25 to 7.38 s (63.1 s) | 2.98 to 3.22 s (96.7 s) | 12.71 to 13.03 s (97.7 s) |

With `--no-watch` an idle fleetz writes nothing; the roughly 51 context
switches a second plausibly come from (inferred, not traced) the 100 ms tick task and the input thread's 50 ms
`event::poll` loop. With watch on, `SLOW_REPAINT_TICKS` repaints every 500 ms
(about 2 writes a second), a poll runs every 15 s here because a fixture repo
is PENDING (7 polls per run in the shim log), and the 60 s sweep (`spawn_sweep`
with fetch) runs once per window. The sweep's git children are not in the
in-window CPU column; by subtraction of whole-run tree CPU, watch mode cost
about 1.6 s (small) and 5.5 s (large) of extra process-tree CPU over a run
about 35 s longer. That attribution is inferred. `top`'s idle-wakeup counter
was noisy and is reported as read, not interpreted.

### Local refresh (case I, 3 runs x 5 presses per fleet, n=15)

From the keypress to the first write in which the header shows `swept` again
after `checking` appeared, which is when `refresh_phase` returns None: the
sweep has landed and the GitHub poll `r` starts has answered (offline, the
answer is the stale-cache fallback, which is immediate).

| Key | Small min / median / max | Large min / median / max | Bytes written, median |
|---|---|---|---|
| `r` (re-stat, no fetch, plus poll) | 110 / 138 / 174 ms | 424 / 496 / 602 ms | 1,402 / 2,566 |
| `F` (fetch every remote, then re-stat) | 135 / 178 / 267 ms | 608 / 711 / 852 ms | 1,469 / 2,942 |

Nothing changed between presses, so every matched repo hit the `WorkKey`
fast path in `rebuild_repo_status` and only the cheap probes ran; the
local-only rows have no `WorkKey` and re-run everything. A live `r` waits on
the real poll as well, which by case E would add seconds; not measured.

### git process counts (case K, n=1 each, one-off)

A counting `git` wrapper first on `PATH`, in runs that were not timed.

| Run | git processes | By subcommand |
|---|---|---|
| Startup, small | 126 | log 32, for-each-ref 24, status 14, remote 14, reflog 10, stash 10, worktree 10, rev-parse 10, rev-list 2 |
| Startup, large | 707 | log 185, for-each-ref 130, status 70, remote 70, reflog 60, stash 60, worktree 60, rev-parse 60, rev-list 12 |
| One `r`, small | 64 | (190 for startup plus `r`) |
| One `r`, large | 307 | for-each-ref 130, status 65, rev-parse 65, log 15, rev-list 12, remote 5, reflog 5, stash 5, worktree 5 |
| Stale startup plus one `F`, large | 1139 | includes 125 `fetch` (60 at launch, 65 for `F`) |

About 11 git processes per local repo at launch (707 for 65 and 126 for 12;
most come from `build_repo_status`) and about 5 per listed repo per `r`.

Per-spawn cost, measured in isolation: sequential `git rev-parse HEAD` on a
fixture repo from Python, not inside fleetz and not under concurrency (n=30
each): through `/usr/bin/git` 8.14 / 8.43 / 10.07 ms; calling
`/Library/Developer/CommandLineTools/usr/bin/git` directly 2.97 / 3.12 /
3.48 ms. Same binary in the end (`xcrun --find git` resolves to it); the
difference is the xcrun stub.

### Startup with git resolved directly (case L, n=10 per fleet)

Case A again, with a directory holding a symlink to the CommandLineTools git
put first on `PATH`. This changes the environment, not fleetz; it is here as
evidence for ranking, not as a recommendation.

| Measure | Small min / median / max | Large min / median / max |
|---|---|---|
| First useful row | 91 / 97 / 216 ms | 386 / 404 / 447 ms |
| Process-tree CPU, user / sys | 0.27 / 0.28 s | 1.51 / 1.62 s |
| Against case A median | 257 ms, 0.63 / 0.65 s | 1148 ms, 3.57 / 3.67 s |

### `$HOME` sweep size (case N, proxy, n=3)

Pass 2 walks `$HOME` to depth 4. The fixtures' isolated `HOME` is tiny, so
this cost is absent from every case above. `homewalk.py` re-implements
`discovery::scan_recursive` (hidden directories skipped, `PRUNE_DIRS` pruned)
in Python and walked the real home read-only: 7,955 directories visited, 205
git repos found, 0.544 s the first time and 0.080 s and 0.076 s after. This is
a Python proxy for the work, not a fleetz measurement; it also differs from
the 1,238 directories `docs/ideas.md` recorded on 2026-08-08, which may be a
different home or a different counting method.

## Bottlenecks, ranked by evidence

1. **Cold-cache GraphQL fetch blocks every row.** Measured: 6.3 s median
   (5.7 to 7.3 s, n=5) to the first row with a cold cache against 116 ms for
   the same payload from the cache. `load_repos` awaits `fetch_dataset` (then
   `github::fetch_repos`) before `assemble_view` starts discovery, so local
   repos that need no network wait too. Offline with no cache the same
   ordering shows no rows at all (case C). The cached path (`AnyCached`)
   avoids it whenever any usable payload exists.
2. **Per-repo git subprocesses in `build_repo_status`.** Measured: startup
   grows with fleet size (257 ms for 10 matched repos, 1148 ms for 60), uses
   about 7.2 s of process-tree CPU for the large fleet, spawns 707 git
   processes, and drops to 404 ms with git resolved past the xcrun stub. The
   per-spawn figures (8.4 ms through the stub, 3.1 ms direct) were measured in
   isolation as sequential calls from Python, not inside fleetz under
   concurrency. Inferred from the code, since per-phase timing is unavailable:
   the time goes to `assemble_view`'s stat loop (`build_repo_status`, about 11
   git processes per repo, gated by `probe_concurrency()` = 10), and the
   one-shot `AppEvent::Loaded` holds every row until the last probe and pass 3
   finish.
3. **Intro and loading-field output volume.** Measured: 6.3 MB (small) and
   9.0 MB (large) written to reach a settled screen at 254x74, against 79 to
   83 KB with the intro off; about 1.7 MB of it (one probe run) before the
   small fleet's data even arrives. First legible row 923 ms against 257 ms (small). Responsible:
   `spawn_loading_task` and `spawn_intro_task` at `FRAME_INTERVAL` = 16 ms,
   `Loading::paint` and `Intro::paint` changing most cells per frame. Terminal
   cost of drawing it is not measured.
4. **Startup `git fetch` of stale remotes.** Measured against local remotes:
   +105 ms small, +429 ms large. Responsible: the `git_fetch` block in
   `assemble_view` (`stale_remotes`, `FETCH_CONCURRENCY` = 4), which runs
   before any stat. Real network cost per fetch not measured.
5. **Watch-mode idle cost.** Measured: 0.8 to 1.5% CPU for the fleetz process
   and 215 to 325 B/s of output, against 0.12% and nothing without watch.
   Responsible: the `SLOW_REPAINT_TICKS` repaint in `run_event_loop`, the
   periodic `start_sweep` with fetch, and `start_github_poll` (15 s floor while
   anything is PENDING).

Not a bottleneck on this evidence: keypress to frame (median 4 to 6 ms, max
12.5 ms), first paint (6 to 7 ms), and the `r` refresh of an unchanged fleet
(138 ms small, 496 ms large).

## Measurement limitations

- The harness's pty reader is the terminal. Its read speed shapes when the
  later bytes of a large frame arrive; tmux (case M) saw the same milestones
  25 to 150 ms later. A terminal emulator's own paint time is not included
  anywhere.
- The isolated `HOME` has no global git config, a tiny pass-2 sweep and no
  real repos. The owner's daily startup has a larger `$HOME` (case N) and
  whatever his `~/.gitconfig` adds to each git process.
- Fixture histories are synthetic: linear, small files, small working trees.
  `git status` and `git log` on large real repos cost more per call.
- All remotes are local. `git fetch` times here are process and disk cost,
  not network.
- The OS file cache was warm in every run. Fixtures were freshly written and
  read back to back, so every git call hit cached pages. "Cold cache" in this
  document means only fleetz's GitHub cache; a cold-disk launch (after a
  reboot, or with the fixtures evicted from memory) was not measured.
- Results move between sessions: the reviewer's rerun of the same binary was
  about 4 to 5% slower in wall time at equal CPU (case A, second session).
- Offline was simulated one way: a refused connection to a local proxy port,
  with a dummy token. DNS failure, a black-holed route or a captive portal can
  take far longer to fail, and `gh` itself has no timeout (comment at
  `GITHUB_POLL_STUCK_AFTER`); none of that was measured.
- The CPU from `wait4` and `RUSAGE_CHILDREN` covers fleetz plus every git
  child; fleetz's own share during startup cannot be separated without
  in-process accounting.
- Peak RSS is from `wait4` at exit only, the maximum across the process tree.
- The owner's two fleetz instances ran throughout and are background load.
- Spreads come from 3 to 20 runs per case; the git counts and the first-exec
  observation are single runs.

## Not measured, and what would make it measurable

| Measure | Why unavailable | Instrumentation that would make it measurable |
|---|---|---|
| Time per phase inside `load_repos` (cache read, pass 1, pass 2, stale fetch, stat loop, pass 3) | No timing exists in the code and the source was not to be changed | Timestamps at each phase boundary in `load_repos` and `assemble_view`, written to a file named by an environment variable |
| `ui::render` and `terminal.draw` cost per frame | Same; the keypress latency is only an upper bound | `Instant` around the draw call in `run_event_loop`, logged the same way |
| Terminal-side paint time (Ghostty) | Outside fleetz; the harness sees bytes, not pixels | Terminal-side frame timing, or a high-speed screen capture |
| Live GitHub poll in watch mode, live `r`, `R` reload | GitHub budget (at most 8 live fetches; 5 were spent on case E) | A recorded GraphQL response served by a local HTTPS endpoint, which needs a way to point fleetz at another URL or a trusted test certificate |
| Real-network `git fetch` at launch and in sweeps | Fixtures use local bare remotes so no traffic reaches anyone's repo | A dedicated test remote the owner controls |
| Owner's real fleet and real `$HOME` | Isolation rule for this ticket | A run under the real `HOME` with the cache directory redirected, which needs a cache-path override |
| Wakeups as the OS counts them | `powermetrics` needs root; `top`'s IDLEW is noisy | `powermetrics --samplers tasks` run by the owner |
| fleetz-only CPU during startup | Process-tree accounting only | `getrusage(RUSAGE_SELF)` logged at `Loaded` |
| Memory over time | Only peak at exit | Periodic `ps -o rss` sampling, or in-process logging |
| Cold-cache large fleet | The live response is the owner's account and does not change with the fixture | Not needed unless the payload is made to match the fixture |

## Recommended targets

None are proposed as pass thresholds. The tables are orientation only: the
same binary moved by 4 to 5% between two sessions, so a later ticket must not
compare its numbers against them. Instead:

- Build the base binary (this commit, or the ticket's base) and the candidate
  side by side, for example `cp target/release/fleetz $S/fleetz-base` before
  building the candidate.
- In one session, on a quiet machine, with the same fixtures, discard one
  warm-up launch of each binary, then interleave them (base, candidate, base,
  candidate, ...) with at least n=10 each in the same case.
- Report both medians and ranges, and compare the candidate with the base
  from that session.

Questions for the owner rather than targets:

- Whether a cold or failed GitHub fetch should still show local rows (today:
  6.3 s of nothing cold, an error screen and no rows offline).
- Whether rows should appear progressively rather than in one event after the
  slowest repo.
- Whether the intro's 6 to 9 MB of output per launch is acceptable over slow
  links (ssh, tmux over a network), where it was not measured.

## Rerunning

Everything below runs from one scratch directory outside the repo, with no
network access except case E. Apart from the release build, the measured
runs write nothing outside that directory.

```sh
W=/path/to/fleetz/worktree          # at commit 28ebba7
S=/tmp/fz-perf                      # any empty scratch directory
mkdir -p "$S/logs" "$S/shim-offline" "$S/shim-live" "$S/git-count" "$S/git-direct"
# Save the appendix files into $S: mkfleet.py, run.py, vt.py, summarize.py,
# tmuxcheck.py, homewalk.py, shim-offline/gh, shim-live/gh, git-count/git,
# and run the fzenv.sh heredoc. Then:
chmod +x "$S"/shim-*/gh "$S/git-count/git"
ln -sf /Library/Developer/CommandLineTools/usr/bin/git "$S/git-direct/git"
cargo build --release --manifest-path "$W/Cargo.toml"
B="$W/target/release/fleetz"
command -v git   # expected: /usr/bin/git; the runs inherit this PATH
cd "$S"
python3 mkfleet.py small 10 2 2
python3 mkfleet.py large 60 5 5
```

Regenerate the fixtures on the day you rerun: the payload's `pushedAt` must
fall inside the Auto window, which moves. Before any timed case, make one
warm-up launch and discard it (a run with `--out logs/warmup.jsonl`): the
first launch of a freshly built binary is an outlier (206 ms and 235 ms to
first paint in the two sessions, against 6 to 7 ms).

| Case | Command (from `$S`) |
|---|---|
| A, warm, intro off | `for fl in small large; do for i in $(seq 1 20); do python3 run.py --root $fl --bin $B --shim shim-offline --label A-warm-$fl --out logs/A.jsonl --raw logs/A-$fl-$i.raw; done; done` |
| B, intro on | as A with `--intro on --label B-intro-$fl --out logs/B.jsonl` |
| C, offline cold | as A with `--cache cold --label C-offline-cold-$fl --out logs/C.jsonl` |
| D, watch on | as A with `--watch on --label D-warm-watch-offline-$fl --out logs/D.jsonl` |
| E, live cold (spends one GraphQL request per run; run one at a time) | `python3 run.py --root small --bin $B --shim shim-live --net live --cache cold --label E-live-cold-small --out logs/E.jsonl` |
| E, same payload warm | `python3 run.py --root small --bin $B --shim shim-offline --cache keep --label E-warm-livepayload-small --out logs/E.jsonl` (n=10, right after E) |
| F, stale remotes | before each run `touch -t 202601010000 $fl/home/repos/fx-*/.git/FETCH_HEAD`, then as A with `--label F-stale-remotes-$fl --out logs/F.jsonl` |
| G, input | `python3 run.py --root $fl --bin $B --shim shim-offline --scenario input --keys 20 --label G-input-$fl --out logs/G.jsonl` (3 runs) |
| H, idle | `--scenario idle --idle 60` (no watch) and `--watch on --scenario idle --idle 90`, `--label H-...`, 3 runs each |
| I, refresh | `--scenario refresh --repeat 5` and `--scenario fetchall --repeat 5`, 3 runs each |
| J, 120x40 | as A and B with `--cols 120 --rows 40 --root large` |
| K, git counts | as A, I with `--path-prefix $S/git-count`; read `$fl/git-calls.log` |
| L, direct git | as A with `--path-prefix $S/git-direct --label L-warm-directgit-$fl --out logs/L.jsonl` |
| M, tmux | `python3 tmuxcheck.py $S/$fl $B $S/shim-offline 254 74` (10 runs per fleet; the table uses the `*_after_session` fields) |
| N, home walk | `python3 homewalk.py "$HOME"` (reads directory entries only) |

Summaries: `python3 summarize.py logs/A.jsonl first_paint first_row summary_done bytes_to_settled fleetz_cpu_user fleetz_cpu_sys`.
For G, H and I the per-press and per-window values are in the `input`, `idle`
and `refresh` fields of each JSON line. In every record, `fleetz_cpu_*` is
the process-tree CPU from `wait4`.

## Appendix: scripts

### mkfleet.py

```python
#!/usr/bin/env python3
"""Build a synthetic fleetz fixture: isolated HOME, clones, local bare remotes,
and a canned GitHub cache payload whose repo names match the clones.

usage: mkfleet.py ROOT MATCHED LOCAL_ONLY NOT_CLONED

Nothing here touches the real home, global git config or the network.
"""
import datetime as dt
import json
import os
import shutil
import subprocess
import sys

ROOT = os.path.abspath(sys.argv[1])
MATCHED, LOCAL_ONLY, NOT_CLONED = (int(x) for x in sys.argv[2:5])
COMMITS = 300  # above HISTORY_COUNT (200), so every -200 cap engages
FILES = 20
HOME = os.path.join(ROOT, "home")
REPOS = os.path.join(HOME, "repos")
REMOTES = os.path.join(ROOT, "remotes")
CACHE = os.path.join(HOME, "Library", "Caches", "fleetz")

ENV = dict(os.environ)
ENV.update(
    HOME=HOME,
    GIT_CONFIG_NOSYSTEM="1",
    GIT_CONFIG_GLOBAL=os.path.join(ROOT, "gitconfig-empty"),
    GIT_AUTHOR_NAME="Fixture",
    GIT_AUTHOR_EMAIL="fixture@example.invalid",
    GIT_COMMITTER_NAME="Fixture",
    GIT_COMMITTER_EMAIL="fixture@example.invalid",
)


def git(*args, cwd=None, stdin=None):
    subprocess.run(["git", *args], cwd=cwd, env=ENV, input=stdin, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def history(ref, start, count, t0, parent=None, tag="c"):
    """fast-import stream: `count` commits on `ref`, one file touched each."""
    out = []
    for n in range(count):
        mark = start + n
        ts = t0 + n * 3600
        body = "".join(f"{tag} rev {mark} line {k}\n" for k in range(8)).encode()
        msg = f"{tag}: change {mark}\n".encode()
        out.append(f"commit {ref}\nmark :{mark}\n".encode())
        out.append(f"author Fixture <fixture@example.invalid> {ts} +0000\n".encode())
        out.append(f"committer Fixture <fixture@example.invalid> {ts} +0000\n".encode())
        out.append(f"data {len(msg)}\n".encode() + msg)
        if n == 0 and parent:
            out.append(f"from {parent}\n".encode())
        out.append(f"M 100644 inline src/file{mark % FILES}.txt\n".encode())
        out.append(f"data {len(body)}\n".encode() + body + b"\n")
    return b"".join(out)


def make_remote(name, feature):
    bare = os.path.join(REMOTES, f"{name}.git")
    git("init", "--bare", "-q", "-b", "main", bare)
    t0 = int(dt.datetime.now(dt.timezone.utc).timestamp()) - COMMITS * 3600 - 7200
    stream = history("refs/heads/main", 1, COMMITS, t0)
    if feature:
        stream += history("refs/heads/feature", 10001, 5, t0 + (COMMITS - 20) * 3600,
                          parent=f":{COMMITS - 20}", tag="feat")
    git("fast-import", "--quiet", cwd=bare, stdin=stream)
    return bare


def commit_file(repo, path, text, msg):
    with open(os.path.join(repo, path), "a") as f:
        f.write(text)
    git("add", path, cwd=repo)
    git("commit", "-q", "-m", msg, cwd=repo)


def main():
    if os.path.exists(ROOT):
        shutil.rmtree(ROOT)
    os.makedirs(REPOS)
    os.makedirs(REMOTES)
    os.makedirs(CACHE)
    open(ENV["GIT_CONFIG_GLOBAL"], "w").close()

    matched = [f"fx-{i:03d}" for i in range(MATCHED)]
    local_only = [f"lo-{i:03d}" for i in range(LOCAL_ONLY)]
    not_cloned = [f"nc-{i:03d}" for i in range(NOT_CLONED)]

    for i, name in enumerate(matched):
        bare = make_remote(name, feature=(i % 3 == 0))
        repo = os.path.join(REPOS, name)
        git("clone", "-q", bare, repo)
        git("config", "user.email", "fixture@example.invalid", cwd=repo)
        if i % 3 == 0:
            git("branch", "-q", "--track", "feature", "origin/feature", cwd=repo)
        if i % 7 == 3:  # behind 1: a commit lands on the remote after the clone
            tmp = os.path.join(ROOT, "tmp-push")
            git("clone", "-q", bare, tmp)
            commit_file(tmp, "src/remote.txt", "upstream\n", "upstream change")
            git("push", "-q", "origin", "main", cwd=tmp)
            shutil.rmtree(tmp)
        if i % 5 == 2:  # ahead 2
            commit_file(repo, "src/local.txt", "a\n", "local one")
            commit_file(repo, "src/local.txt", "b\n", "local two")
        if i % 6 == 4:  # one stash
            with open(os.path.join(repo, "src/file1.txt"), "a") as f:
                f.write("stashed\n")
            git("stash", "-q", cwd=repo)
        if i % 4 == 1:  # dirty: one modified, two untracked
            with open(os.path.join(repo, "src/file0.txt"), "a") as f:
                f.write("wip\n")
            for k in range(2):
                open(os.path.join(repo, f"scratch{k}.txt"), "w").write("x\n")

    for name in local_only:
        bare = make_remote(name, feature=False)
        repo = os.path.join(REPOS, name)
        git("clone", "-q", bare, repo)
        git("remote", "remove", "origin", cwd=repo)
        shutil.rmtree(bare)
        open(os.path.join(repo, "untracked.txt"), "w").write("x\n")

    # The payload: every name pushed a few minutes ago, so the Auto window
    # keeps them all. Written before the final fetch below, so FETCH_HEAD is
    # newer than every pushedAt and startup's stale_remotes() fetches nothing.
    now = dt.datetime.now(dt.timezone.utc)
    states = ["SUCCESS", "SUCCESS", "FAILURE", "SUCCESS", "PENDING"]

    def node(i, name):
        pushed = (now - dt.timedelta(seconds=300 + i)).strftime("%Y-%m-%dT%H:%M:%SZ")
        prs = [{
            "number": 100 + k, "title": f"{name} change {k}", "author": {"login": "fixture"},
            "createdAt": pushed, "url": f"https://example.invalid/{name}/pull/{100 + k}",
            "reviewDecision": "REVIEW_REQUIRED",
        } for k in range(i % 3)]
        return {
            "name": name, "pushedAt": pushed, "isPrivate": i % 2 == 0,
            "owner": {"__typename": "User", "login": "fz01-fixture"},
            "pullRequests": {"totalCount": len(prs), "nodes": prs},
            "defaultBranchRef": {"target": {
                "statusCheckRollup": {"state": states[i % len(states)]},
                "checkSuites": {"nodes": [{
                    "workflowRun": {"workflow": {"name": "CI"}},
                    "createdAt": pushed, "updatedAt": pushed,
                }]},
            }},
        }

    names = matched + not_cloned
    nodes = [node(i, n) for i, n in enumerate(names)]
    viewer_nodes, org_nodes = nodes[:50], nodes[50:]
    today = now.date()

    def year(k):
        days = []
        for d in range(365):
            day = today - dt.timedelta(days=365 * k + d)
            c = (d * 7 + k) % 5
            days.append({"date": day.isoformat(), "contributionCount": c,
                         "contributionLevel": ["NONE", "FIRST_QUARTILE", "SECOND_QUARTILE",
                                               "THIRD_QUARTILE", "FOURTH_QUARTILE"][c]})
        return {"contributionCalendar": {"weeks": [{"contributionDays": days}]}}

    viewer = {f"y{k}": year(k) for k in range(3)}
    viewer["repositories"] = {"nodes": viewer_nodes}
    viewer["organizations"] = {"nodes": [{"repositories": {"nodes": org_nodes}}]}
    with open(os.path.join(ROOT, "github-50.json"), "w") as f:
        json.dump({"data": {"viewer": viewer}}, f)

    for name in matched:
        git("fetch", "-q", cwd=os.path.join(REPOS, name))

    print(f"{ROOT}: {len(matched)} matched, {len(local_only)} local-only, "
          f"{len(not_cloned)} not cloned; payload at {ROOT}/github-50.json")


main()
```

### shim-offline/gh

```sh
#!/bin/sh
# Stand-in for gh: hands fleetz a dummy token and never touches the network.
echo "$(date +%s) offline-shim $*" >> "${FZ_SHIM_LOG:-/dev/null}"
[ "$1 $2" = "auth token" ] && { echo fz01-dummy-token; exit 0; }
exit 1
```

### shim-live/gh

```sh
#!/bin/sh
# Stand-in for gh: forwards only `gh auth token` to the real gh under the real
# HOME, so fleetz can make one live GraphQL request from an isolated HOME.
echo "$(date +%s) live-shim $*" >> "${FZ_SHIM_LOG:-/dev/null}"
[ "$1 $2" = "auth token" ] || exit 1
exec env HOME="$FZ_REAL_HOME" GH_NO_UPDATE_NOTIFIER=1 /opt/homebrew/bin/gh auth token
```

`run.py` sets `FZ_REAL_HOME` to the caller's `HOME` for this shim.

### git-count/git

```sh
#!/bin/sh
# Counts git invocations (first argument after -C path); timing runs never use this.
echo "$*" >> "${FZ_GIT_LOG:-/dev/null}"
exec /usr/bin/git "$@"
```

### run.py

```python
#!/usr/bin/env python3
"""Drive one fleetz run in a pty and timestamp every byte it writes.

Milestones are found in fleetz's own output stream, so they are the moment
fleetz handed a frame to the terminal, not the moment a terminal drew it.
One JSON line per run is appended to --out.
"""
import argparse
import fcntl
import json
import os
import pty
import resource
import select
import shutil
import signal
import struct
import subprocess
import termios
import time

ap = argparse.ArgumentParser()
ap.add_argument("--root", required=True)
ap.add_argument("--bin", required=True)
ap.add_argument("--shim", required=True, help="dir holding the gh shim")
ap.add_argument("--cols", type=int, default=254)
ap.add_argument("--rows", type=int, default=74)
ap.add_argument("--cache", choices=["warm", "cold", "keep"], default="warm")
ap.add_argument("--net", choices=["offline", "live"], default="offline")
ap.add_argument("--intro", choices=["on", "off"], default="off")
ap.add_argument("--watch", choices=["on", "off"], default="off")
ap.add_argument("--scenario", default="startup",
                choices=["startup", "input", "idle", "refresh", "fetchall"])
ap.add_argument("--keys", type=int, default=20)
ap.add_argument("--idle", type=float, default=60.0)
ap.add_argument("--repeat", type=int, default=5, help="refresh/fetchall presses")
ap.add_argument("--path-prefix", default="", help="extra dir put before PATH")
ap.add_argument("--label", required=True)
ap.add_argument("--out", required=True)
ap.add_argument("--raw", default="")
ap.add_argument("--timeout", type=float, default=60.0)
a = ap.parse_args()

ROOT = os.path.abspath(a.root)
HOME = os.path.join(ROOT, "home")
CACHE = os.path.join(HOME, "Library", "Caches", "fleetz")
CONF = os.path.join(HOME, ".config", "fleetz")
REAL_HOME = os.environ["HOME"]

# --- isolated state ---------------------------------------------------------
os.makedirs(CONF, exist_ok=True)
with open(os.path.join(CONF, "config.toml"), "w") as f:
    f.write(f"intro = {'true' if a.intro == 'on' else 'false'}\n")
os.makedirs(CACHE, exist_ok=True)
if a.cache != "keep":
    for name in os.listdir(CACHE):
        os.remove(os.path.join(CACHE, name))
if a.cache == "warm":
    shutil.copy(os.path.join(ROOT, "github-50.json"), os.path.join(CACHE, "github-50.json"))

env = {
    "PATH": ":".join(p for p in [a.path_prefix, os.path.abspath(a.shim), os.environ["PATH"]] if p),
    "HOME": HOME,
    "REPO_CHECK_DIR": os.path.join(HOME, "repos"),
    "TERM": "tmux-256color",
    "LANG": "en_US.UTF-8",
    "USER": os.environ.get("USER", ""),
    "LOGNAME": os.environ.get("LOGNAME", ""),
    "SHELL": "/bin/zsh",
    "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
    "FZ_SHIM_LOG": os.path.join(ROOT, "gh-shim.log"),
    "FZ_REAL_HOME": REAL_HOME,
    "FZ_GIT_LOG": os.path.join(ROOT, "git-calls.log"),
}
if a.net == "offline":
    # Nothing listens on port 9: every HTTPS request fails to connect locally.
    for k in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]:
        env[k] = os.environ.get("FZ_PROXY", "http://127.0.0.1:9")
args = [a.bin] + (["--no-watch"] if a.watch == "off" else ["--watch"])

# --- spawn ------------------------------------------------------------------
before_children = resource.getrusage(resource.RUSAGE_CHILDREN)
t0 = time.perf_counter()
pid, fd = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", a.rows, a.cols, 0, 0))
    os.execve(a.bin, args, env)

chunks = []  # (seconds since spawn, bytes)
buf = bytearray()
eof = False


def now():
    return time.perf_counter() - t0


def pump(seconds=None, until=None):
    """Read output until `seconds` pass or `until()` is true."""
    global eof
    deadline = None if seconds is None else time.perf_counter() + seconds
    while not eof:
        if until is not None and until():
            return True
        left = 0.05 if deadline is None else deadline - time.perf_counter()
        if left <= 0:
            return False
        r, _, _ = select.select([fd], [], [], min(left, 0.05))
        if r:
            try:
                data = os.read(fd, 65536)
            except OSError:
                data = b""
            if not data:
                eof = True
                break
            chunks.append((now(), data))
            buf.extend(data)
    return False


def first(marker, after_offset=0):
    """(time, chunk index) of the chunk that completes the first `marker`."""
    pos = bytes(buf).find(marker, after_offset)
    if pos < 0:
        return None
    end = pos + len(marker)
    off = 0
    for i, (t, d) in enumerate(chunks):
        off += len(d)
        if off >= end:
            return t, i
    return None


def offset_of(i):
    return sum(len(d) for _, d in chunks[:i])


def burst_end(i, gap=0.003):
    j = i
    while j + 1 < len(chunks) and chunks[j + 1][0] - chunks[j][0] < gap:
        j += 1
    return j


def quiet_for(seconds, cap):
    """Wait until no output has arrived for `seconds` (or `cap` passes)."""
    start = time.perf_counter()
    while time.perf_counter() - start < cap:
        last = chunks[-1][0] if chunks else 0
        if now() - last >= seconds:
            return True
        pump(0.05)
    return False


def proc_counters():
    out = subprocess.run(["top", "-l", "1", "-pid", str(pid), "-stats", "pid,idlew,csw"],
                         capture_output=True, text=True).stdout
    idlew = csw = None
    for line in out.splitlines():
        parts = line.split()
        if parts and parts[0] == str(pid):
            idlew, csw = int(parts[1].rstrip("+")), int(parts[2].rstrip("+"))
    ps = subprocess.run(["ps", "-o", "utime=,stime=", "-p", str(pid)],
                        capture_output=True, text=True).stdout.split()

    def secs(s):
        m, rest = s.split(":")
        return int(m) * 60 + float(rest)
    return {"t": now(), "idlew": idlew, "csw": csw,
            "cpu": secs(ps[0]) + secs(ps[1]) if len(ps) == 2 else None}


def send(key):
    t = now()
    os.write(fd, key)
    return t


res = {"label": a.label, "cols": a.cols, "rows": a.rows, "cache": a.cache, "net": a.net,
       "intro": a.intro, "watch": a.watch, "scenario": a.scenario}

# --- startup milestones (computed from the recording after exit) ----------
MARKERS = (b"fx-", b"lo-", b"not cloned", b"Could not load the fleet")
if a.intro == "off":
    pump(a.timeout, until=lambda: any(m in buf for m in MARKERS))
    res["marker_seen"] = now()
    quiet_for(0.3, 8)
else:
    # The loading field and then the wave paint every 16ms; the run has
    # settled once output stops. Their cells carry per-cell colors, so a
    # byte marker cannot be used here.
    quiet_for(0.6, a.timeout)
res["bytes_at_settle"] = len(buf)
res["writes_at_settle"] = len(chunks)

# --- scenarios --------------------------------------------------------------
if a.scenario == "startup":
    quiet_for(0.5, 3)
elif a.scenario == "input":
    quiet_for(1.0, 5)
    lat = []
    for k in range(a.keys):
        key = b"j" if k < a.keys // 2 else b"k"
        n_before = len(chunks)
        ts = send(key)
        pump(0.3)
        later = [c for c in chunks[n_before:] if c[0] >= ts]
        if later:
            i0 = chunks.index(later[0])
            i1 = burst_end(i0)
            lat.append({"first": later[0][0] - ts, "end": chunks[i1][0] - ts,
                        "bytes": sum(len(d) for _, d in chunks[i0:i1 + 1])})
        else:
            lat.append(None)
    res["input"] = lat
elif a.scenario == "idle":
    quiet_for(1.0, 5)
    c0, n0, b0 = proc_counters(), len(chunks), len(buf)
    pump(a.idle)
    c1 = proc_counters()
    res["idle"] = {"seconds": c1["t"] - c0["t"], "bytes": len(buf) - b0,
                   "writes": len(chunks) - n0,
                   "cpu_s": (c1["cpu"] - c0["cpu"]) if c0["cpu"] is not None else None,
                   "idle_wakeups": (c1["idlew"] - c0["idlew"]) if c0["idlew"] is not None else None,
                   "ctx_switches": (c1["csw"] - c0["csw"]) if c0["csw"] is not None else None}
elif a.scenario in ("refresh", "fetchall"):
    key = b"r" if a.scenario == "refresh" else b"F"
    out = []
    for _ in range(a.repeat):
        quiet_for(2.0, 6)  # also outlasts the 1.5s status-message TTL
        off0 = len(buf)
        ts = send(key)
        busy = lambda: bytes(buf).find(b"checking", off0) >= 0
        pump(10, until=busy)
        pos = bytes(buf).find(b"checking", off0)
        done = lambda: pos >= 0 and bytes(buf).find(b"swept", pos) >= 0
        pump(30, until=done)
        hit = first(b"swept", pos) if pos >= 0 else None
        out.append({"done": (hit[0] - ts) if hit else None,
                    "bytes": len(buf) - off0})
    res["refresh"] = out

# --- quit -------------------------------------------------------------------
res["t_quit_sent"] = send(b"q")
deadline = time.perf_counter() + 5
status = None
while time.perf_counter() < deadline:
    pump(0.05)
    wpid, st, ru = os.wait4(pid, os.WNOHANG)
    if wpid == pid:
        status = st
        res["fleetz_cpu_user"] = ru.ru_utime
        res["fleetz_cpu_sys"] = ru.ru_stime
        res["fleetz_maxrss_mb"] = ru.ru_maxrss / 1048576
        break
if status is None:
    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    res["killed"] = True
after_children = resource.getrusage(resource.RUSAGE_CHILDREN)
res["tree_cpu_user"] = after_children.ru_utime - before_children.ru_utime
res["tree_cpu_sys"] = after_children.ru_stime - before_children.ru_stime
res["exit_status"] = status
res["bytes_total"] = len(buf)
res["wall"] = now()

import vt
raw = bytes(buf)
idx = [(t, len(d)) for t, d in chunks]
tl = vt.timeline(raw, idx, a.cols, a.rows)
final = tl.pop("final_text")
res.update(tl)
# ratatui writes only cells that changed, so the last write before the quiet
# period is when the screen reached its settled state.
n_settle = res["writes_at_settle"]
res["screen_settled"] = idx[n_settle - 1][0]
res["bytes_to_settled"] = sum(l for _, l in idx[:n_settle])
if a.raw:
    with open(a.raw, "wb") as f:
        f.write(bytes(buf))
    with open(a.raw + ".idx.json", "w") as f:
        json.dump([(t, len(d)) for t, d in chunks], f)
with open(a.out, "a") as f:
    f.write(json.dumps(res) + "\n")
print(json.dumps({k: (round(v, 4) if isinstance(v, float) else v) for k, v in res.items()
                  if not isinstance(v, (list, dict))}))
```

### vt.py

```python
"""Minimal screen model for ratatui's crossterm output: cursor moves, SGR
(ignored), text, and screen clears. Enough to ask what was on screen after
each chunk fleetz wrote."""
import unicodedata


class Screen:
    def __init__(self, cols, rows):
        self.cols, self.rows = cols, rows
        self.cells = [[" "] * cols for _ in range(rows)]
        self.r = self.c = 0
        self.pending = b""

    def line(self, r):
        return "".join(self.cells[r])

    def text(self):
        return "\n".join(self.line(r) for r in range(self.rows))

    def blank(self):
        return all(ch == " " for row in self.cells for ch in row)

    def put(self, ch):
        w = 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
        if 0 <= self.r < self.rows and 0 <= self.c < self.cols:
            self.cells[self.r][self.c] = ch
            if w == 2 and self.c + 1 < self.cols:
                self.cells[self.r][self.c + 1] = ""
        self.c += w

    def feed(self, data):
        data = self.pending + data
        self.pending = b""
        i, n = 0, len(data)
        while i < n:
            b = data[i]
            if b == 0x1B:
                if i + 1 >= n:
                    self.pending = data[i:]
                    return
                if data[i + 1] == ord("["):
                    j = i + 2
                    while j < n and not (0x40 <= data[j] <= 0x7E):
                        j += 1
                    if j >= n:
                        self.pending = data[i:]
                        return
                    params, final = data[i + 2:j].decode("latin1"), chr(data[j])
                    if final == "H":
                        parts = (params.split(";") + ["", ""])[:2]
                        self.r = (int(parts[0]) if parts[0] else 1) - 1
                        self.c = (int(parts[1]) if parts[1] else 1) - 1
                    elif final == "J" and params in ("2", "3"):
                        self.cells = [[" "] * self.cols for _ in range(self.rows)]
                    elif final == "K":
                        for x in range(self.c, self.cols):
                            if 0 <= self.r < self.rows:
                                self.cells[self.r][x] = " "
                    i = j + 1
                else:
                    i += 2
                continue
            if b < 0x20:
                if b == 0x0D:
                    self.c = 0
                elif b == 0x0A:
                    self.r += 1
                i += 1
                continue
            # UTF-8 sequence
            length = 1 if b < 0x80 else 2 if b < 0xE0 else 3 if b < 0xF0 else 4
            if i + length > n:
                self.pending = data[i:]
                return
            self.put(data[i:i + length].decode("utf-8", "replace"))
            i += length


def screen_at(raw, idx, cols, rows, n):
    scr = Screen(cols, rows)
    scr.feed(raw[:sum(l for _, l in idx[:n])])
    return scr.text()


def timeline(raw, idx, cols, rows, row_markers=("fx-", "lo-", "not cloned")):
    """Milestones from a recorded run: seconds since spawn of the chunk after
    which each condition first held."""
    scr = Screen(cols, rows)
    out = {}
    off = 0
    frames = []
    for t, length in idx:
        scr.feed(raw[off:off + length])
        off += length
        frames.append((t, off))
        header = scr.line(0)
        footer = scr.line(rows - 1)
        if "first_paint" not in out and not scr.blank():
            out["first_paint"] = t
        if "first_row" not in out and any(
                m in scr.line(r) for r in range(1, rows - 2) for m in row_markers):
            out["first_row"] = t
        if "error_shown" not in out and any(
                "Could not load the fleet" in scr.line(r) for r in range(rows)):
            out["error_shown"] = t
        loaded = "first_paint" in out and "loading" not in header
        if "loading_gone" not in out and loaded:
            out["loading_gone"] = t
        if ("summary_done" not in out and loaded and
                any(w in footer for w in ("synced", "not cloned"))):
            out["summary_done"] = t
        if all(k in out for k in ("first_paint", "first_row", "loading_gone", "summary_done")):
            break
    out["final_text"] = ""
    return out
```

### summarize.py

```python
#!/usr/bin/env python3
"""usage: summarize.py FILE.jsonl [field ...]  -> n, min, median, max per label."""
import json
import statistics
import sys
from collections import defaultdict

path, fields = sys.argv[1], sys.argv[2:] or [
    "first_paint", "first_row", "summary_done", "screen_settled", "bytes_to_settled",
    "fleetz_cpu_user", "fleetz_cpu_sys", "fleetz_maxrss_mb"]
rows = defaultdict(list)
for line in open(path):
    d = json.loads(line)
    rows[d["label"]].append(d)


def fmt(v):
    return f"{v:.4f}" if isinstance(v, float) else str(v)


for label, runs in rows.items():
    print(f"== {label}  (n={len(runs)})")
    for f in fields:
        vals = [r[f] for r in runs if r.get(f) is not None]
        if not vals:
            print(f"  {f:18} unavailable")
            continue
        vals.sort()
        print(f"  {f:18} n={len(vals):3} min={fmt(vals[0])} med={fmt(statistics.median(vals))}"
              f" max={fmt(vals[-1])}")
```

### fzenv.sh (used by tmuxcheck.py)

Written with an unquoted heredoc, so `$PATH`, `$USER` and `$TMPDIR` are
fixed at the values of the shell that writes it.

```sh
cat > "$S/fzenv.sh" <<EOF
# usage: fzenv.sh ROOT SHIMDIR -- cmd...   (offline proxy always set)
R=\$1; SH=\$2; shift 3
exec env -i PATH="\$SH:$PATH" HOME="\$R/home" REPO_CHECK_DIR="\$R/home/repos" TERM=tmux-256color LANG=en_US.UTF-8 USER=$USER SHELL=/bin/zsh TMPDIR="$TMPDIR" FZ_SHIM_LOG="\$R/gh-shim.log" HTTPS_PROXY=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9 HTTP_PROXY=http://127.0.0.1:9 http_proxy=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9 all_proxy=http://127.0.0.1:9 "\$@"
EOF
chmod +x "$S/fzenv.sh"
```

### tmuxcheck.py

```python
#!/usr/bin/env python3
"""Cross-check: launch fleetz inside a private tmux server and poll
capture-pane until the first fixture row and the footer summary are visible.
usage: tmuxcheck.py ROOT BIN SHIMDIR COLS ROWS"""
import os, shutil, subprocess, sys, time, json
root, binary, shim, cols, rows = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5]
here = os.path.dirname(os.path.abspath(__file__))
cache = os.path.join(root, "home/Library/Caches/fleetz")
for f in os.listdir(cache):
    os.remove(os.path.join(cache, f))
shutil.copy(os.path.join(root, "github-50.json"), cache)
with open(os.path.join(root, "home/.config/fleetz/config.toml"), "w") as f:
    f.write("intro = false\n")
T = ["tmux", "-L", "fz01check"]
cmd = f"{here}/fzenv.sh {root} {shim} -- {binary} --no-watch"
t0 = time.perf_counter()
subprocess.run(T + ["new-session", "-d", "-s", "c", "-x", cols, "-y", rows, cmd], check=True)
t_session = time.perf_counter() - t0
res = {"session_up": t_session}
while time.perf_counter() - t0 < 20:
    out = subprocess.run(T + ["capture-pane", "-p", "-t", "c"], capture_output=True, text=True).stdout
    t = time.perf_counter() - t0
    lines = out.split("\n")
    if "first_text" not in res and out.strip():
        res["first_text"] = t
    if "first_row" not in res and any("fx-0" in l for l in lines[1:-3]):
        res["first_row"] = t
    if "summary" not in res and any("synced" in l for l in lines[-3:]):
        res["summary"] = t
    if "first_row" in res and "summary" in res:
        break
subprocess.run(T + ["send-keys", "-t", "c", "q"])
time.sleep(0.5)
subprocess.run(T + ["kill-server"], capture_output=True)
# Everything above is timed from before `new-session`, which blocks while the
# server starts. The documented table uses the `*_after_session` fields: the
# same times minus `session_up`.
for k in ("first_text", "first_row", "summary"):
    if k in res:
        res[k + "_after_session"] = res[k] - res["session_up"]
print(json.dumps(res))
```

### homewalk.py

```python
#!/usr/bin/env python3
"""Read-only re-implementation of discovery::scan_recursive for the pass-2
$HOME sweep (max depth 4, hidden dirs skipped, PRUNE_DIRS pruned). Counts what
the walk visits; it opens no file and writes nothing."""
import os, sys, time
PRUNE = {"Library", ".Trash", ".cache", "node_modules", "vendor", "target", ".venv", "venv",
         "dist", "build", "Pods", "DerivedData", ".terraform", ".gradle"}
stats = {"dirs": 0, "repos": 0}
def walk(d, depth, maxd=4):
    if depth > maxd: return
    stats["dirs"] += 1
    g = os.path.join(d, ".git")
    if os.path.isdir(g) or os.path.isfile(g):
        stats["repos"] += 1; return
    if depth == maxd: return
    try: it = os.scandir(d)
    except OSError: return
    with it:
        for e in it:
            try:
                if not e.is_dir(): continue
            except OSError: continue
            if e.name in PRUNE or e.name.startswith("."): continue
            walk(e.path, depth + 1, maxd)
t = time.perf_counter(); walk(sys.argv[1], 0); el = time.perf_counter() - t
print(f"dirs={stats['dirs']} repos={stats['repos']} seconds={el:.3f}")
```
