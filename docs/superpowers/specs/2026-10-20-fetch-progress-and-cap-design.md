# Fetch progress and the automatic-fetch cap (FZ-35a, FZ-35b)

Written 2026-10-20 by the coordinator (`claude-fable-5-1`) for Andreas's
approval. Tickets: [#84](https://github.com/Gaurgle/fleetz/issues/84)
(FZ-35a) and [#85](https://github.com/Gaurgle/fleetz/issues/85) (FZ-35b).
Follows [FZ-34 (#83)](https://github.com/Gaurgle/fleetz/issues/83), which
stops the disk fill: process-group kill, temp-pack cleanup, a backoff after
a timeout and one fetch per repo at a time.

## What Andreas asked for

2026-10-20, in chat: why a fetch that did not finish starts over instead of
resuming; whether a big fetch can run to completion on its own without the
poll restarting it; and "i DO want some kind of warning for extra big
fetches. we can just automatically download big amounts of data without
permissions or warnings?" His answers to the coordinator's questions: create
FZ-35 a and b, P1 in M1, spec first; default cap 200 MB, on by default; the
cap applies to every automatic fetch, launch included. He floated a safety
question on `f` ("are you sure you want to fetch this big ass repo?"); see
"Open with Andreas".

## Constraints that shape the design

1. **A fetch cannot resume.** The server builds the pack for one
   negotiation and streams it; `tmp_pack_*` is `index-pack` writing that
   stream. Nothing on either side lets a later fetch continue it. Every
   killed fetch wastes what it received. So the design never kills a fetch
   it intends to let finish, and any cap it enforces costs at most the cap
   once.
2. **The size is unknown before the first byte.** `git fetch --dry-run`
   still downloads. The only pre-signal is GitHub's `diskUsage` (kB), free
   in the poll, updated lazily on GitHub's side and absent on the first
   poll. It can stop a fetch from starting; it cannot promise to.
3. **The size is known early, in two forms.** `--progress` on stderr gives
   `Receiving objects:  12% (123/1000), 45.00 MiB | 5.00 MiB/s` as it
   runs: the object total within the first update, the bytes as they come.
4. **fleetz runs unattended** in watch mode. A modal question would park
   the fetch until he returns; a row state he reads when he looks does not.
5. **Fetch outcomes are a contract** ([fetch-outcome-contract.md](../../fetch-outcome-contract.md)).
   A new kind and new row labels are amendments, not drift.

## Design

### A. Progress-aware fetch (FZ-35a)

- `FETCH_ARGS` drops `--quiet` and adds `--progress` (git only reports
  progress to a tty otherwise). stderr is piped and read while the child
  runs; updates are `\r`-separated, so the reader splits on `\r` and `\n`.
- `git::FetchProgress { phase, objects_done, objects_total, bytes }` is
  parsed from `Receiving objects` and `Resolving deltas`; anything else is
  kept as the final stderr for `classify_fetch_error`, unchanged.
- **Inactivity limit replaces the wall clock.** `FETCH_STALL_LIMIT` (30 s)
  restarts on every progress update. A fetch that is still receiving is
  never killed; one that goes quiet for 30 s is killed exactly as FZ-34
  kills a timed-out one (group SIGKILL, temp packs removed) and reports
  `TimedOut`, so the FZ-04 row and the FZ-34 backoff see no new kind.
  `FETCH_TIMEOUT` (120 s) goes away. The ssh connect timeout (10 s) stays:
  it is what makes a dead host fail fast with a reason.
- **Progress reaches the row.** A new `AppEvent::FetchProgress { path,
  progress }`, throttled in the fetch task to at most two per second per
  repo, lands on the row's fetch facts. The existing fetch spinner on the
  row (FZ-04) carries the received size once it is above zero:
  `fetching 312 MB`. No status-line change; the header's "checking remote"
  wording stays.
- **Claim at permit.** FZ-34 claims a path when the sweep is scheduled, but
  only `FETCH_CONCURRENCY` (4) fetches run at once; in a large fleet a
  queued fetch can outlive `SWEEP_STUCK_AFTER` (180 s) and lose its claim
  while still queued. The claim moves to the moment the permit is granted,
  carried back on the progress event's first arrival; and the load path
  (`assemble_view`) claims the fetches it runs, which FZ-34 left unclaimed.
  Both gaps are recorded on #83.

### B. The cap (FZ-35b)

- **Config key** `fetch_max_mb`, default 200, `0` disables. Documented in
  the README's config table. No settings row: it is a limit you set once.
- **Pre-check from `diskUsage`.** The poll adds `diskUsage` to the
  per-repo fields (free: no `first:`; measured with
  `rateLimit { cost nodeCount }` before and after, recorded on the ticket).
  The row's fetch facts keep the value seen at the last successful fetch.
  When the current value exceeds it by more than the cap, every automatic
  selection skips the repo and the row shows `fetch ~N MB, press f`, where
  N is the growth. A missing, unchanged or shrunken value decides nothing:
  the fetch runs under the in-fetch cap. Two caveats, accepted: GitHub
  updates the field lazily, so the pre-check is sometimes late (the
  in-fetch cap then catches it); the growth over-estimates the fetch when
  GitHub repacked, so a repo may be held once for a fetch that would have
  fit (one `f` clears it).
- **In-fetch cap.** An automatic fetch whose received bytes pass the cap is
  killed like a stalled one and reports the new
  `FetchFailureKind::TooBig`, with the bytes seen as its detail. The row
  shows `fetch over N MB, press f`. The FZ-34 backoff ignores `TooBig`: the
  hold is explicit and lasts until `f` on the row or a success, not a
  doubling timer. At most the cap is wasted, once per push burst.
- **`f` is the permission.** `f` on a held row fetches it with no cap and
  the inactivity limit, so it finishes however big it is, with
  `fetching 312 MB` on the row meanwhile. A successful fetch clears the
  hold and records the new `diskUsage` baseline. `F` (fetch all), launch,
  the watch sweep, the moved-remote batch and the recovery refetch all keep
  the cap: nothing but a deliberate per-row key pulls a big one.
- **Contract.** Amendment 4 in the fetch outcome contract (FZ-35a's is
  3): the `TooBig` kind, the two labels, the baseline in the facts, the
  rule that the backoff skips it, and the manual tally timed from the
  last sign of life (criterion 7, from the FZ-35a review).

### What the user sees, in order

1. Poll: `bonsai/content` grew 300 MB. Row: `fetch ~300 MB, press f`.
   Nothing downloaded.
2. Without the tell: the sweep fetches; at 200 MB received the fetch is
   killed, cleaned up. Row: `fetch over 200 MB, press f`. Nothing retries.
3. `f`: row shows `fetching 48 MB`, `fetching 212 MB`, ..., then the normal
   synced or behind state. The hold is gone.
4. A repo that stalls (dead network mid-pack): killed after 30 s of
   silence, `timed out`, FZ-34 backoff as before.

## Rejected alternatives

- **A confirmation overlay on `f`.** The held row already says what and how
  much; `f` on it is the answer to that question. A second question is the
  same question. Kept open for Andreas below.
- **Stop at the cap, ask, continue.** Continuing is a new fetch from zero
  (constraint 1); the design would download the cap, ask, then download the
  cap again plus the rest.
- **`diskUsage` as the only gate.** Lazy and absent on the first poll
  (constraint 2); a gate that sometimes never fires is not a gate.
- **Object count as the cap.** Known from the first update, but objects are
  not bytes: a 1,000-object fetch can be 1 MB or 1 GB.
- **A per-repo cap or an allow-list.** More keys for a case `f` already
  covers.
- **Progress in the status line.** It is one line for the fleet; the row is
  where one repo's state lives (FZ-04).

## Tests

FZ-35a: the parser on captured git output (a `\r`-separated update, the
final summary line, a non-progress error line left for classification); a
fetch whose upload-pack hook writes a progress-shaped line every second for
longer than 30 s finishes; one whose hook stops writing is cut at the stall
limit with FZ-34's cleanup; the claim is taken at permit and the load path
claims. FZ-35b: the pre-check with a grown, unchanged, shrunken and missing
`diskUsage`; a hook that writes progress past the cap is killed, cleaned and
reports `TooBig`; `f` bypasses and `F` does not; the backoff ignores
`TooBig`; the config default, `0`, and a bad value. All local, no
credentials.

## Decisions

Approved by Andreas 2026-10-20 at about 12:05 CEST ("no safety question, F
keeps the cap, 200 is fine"):

1. **No safety question on `f`.** The held row's label is the warning and
   `f` on it is the answer.
2. **`F` keeps the cap**, as do launch, the watch sweep, the moved-remote
   batch and the recovery refetch. Only a per-row `f` fetches uncapped.
3. **`fetch_max_mb` defaults to 200.**

#84 and #85 are Ready on this approval; dispatch waits for his word, with
FZ-34 integrated first.
