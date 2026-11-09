# Fetch outcome contract

Status: **accepted by the coordinator (`claude-fable-5-1`) on 2026-10-01** as
the per-repo fetch state contract that
[FZ-04 / #17](https://github.com/Gaurgle/fleetz/issues/17) implements. Written
against `main` at `32ffb2b`; line numbers drift, follow the symbols. It reuses
the vocabulary of [freshness-contract.md](freshness-contract.md), which
Andreas accepted and FZ-03 implemented: attempt, outcome, and a time that
only a success may advance. Astra has not reviewed it and Andreas may veto
any rule; what the user sees is his to accept on the branch build.

Scope: what fleetz knows and says about `git fetch` for one checkout. Not in
scope: fetch cadence, retries, authentication, a shared scheduler.

## What happens today

Read from the code at `32ffb2b`; the two rows marked "run" were also executed
with git 2.39.3 in a throwaway repository.

| Situation | Today | Where |
|---|---|---|
| Any fetch result | `git::fetch` returns a `bool` and `spawn_sweep` drops it. | `git.rs::fetch` 222, `main.rs::spawn_sweep` 1937 |
| The event that lands | `RepoUpdated.fetched` is `do_fetch`: "a fetch was requested", not "a fetch succeeded". | `main.rs::spawn_sweep` 1944 |
| Failure, timeout, could not start | Indistinguishable: `run_git` returns `None` for a timeout and for a process that could not run, and stderr is never read. | `git.rs::run_git` 32 |
| Manual `r` waiting on fetches | Settles on any requested fetch, failed or not. The spinner stops correctly, but nothing says the fetch failed. | `app.rs::settle_refresh_fetch` 1444 |
| Counts after a failed fetch | Behind and ahead are recomputed against the old remote refs and drawn as current. | `main.rs::rebuild_repo_status` |
| "When did this clone last fetch" | `git::last_fetch_at` is the mtime of `FETCH_HEAD`. **A failed fetch truncates `FETCH_HEAD` and advances its mtime** (run: a remote path that does not exist, and a host that does not resolve, both left a 0 byte file with a new mtime). So a failed fetch counts as a fetch. | `git.rs::last_fetch_at` 178 |
| First poll's fetch decision | `stale_remotes` skips a remote pushed before `last_fetch_at`. After a failed fetch that baseline is wrong, and the repo is skipped as "already in hand". | `engine/assembly.rs::stale_remotes` (`main.rs` 2241 when written) |
| Credentials in errors | Not shown today, because nothing is shown. git itself printed the URL without its userinfo in the case tried (run). | |

## State

Per checkout, keyed by its path (the checkout identity in
`docs/repository-identity-design.md`, FZ-05). Three facts, as for GitHub:

| Fact | Meaning | Changed by |
|---|---|---|
| `last_success: Option<time>` | When remote refs were last actually updated. | A fetch that succeeded in this session. At load, from `FETCH_HEAD` only when that file is not empty; otherwise unknown. |
| `last_attempt: Option<time>` | When a fetch last finished or was given up on. | Every finished fetch. |
| `outcome` | `NotAttempted`, `Success`, or `Failed(kind, detail)`. Kinds: `TimedOut`, `Auth` (credentials wanted or refused), `Unreachable` (network, DNS, host), `NoRemote` (remote missing or not a repository), `Other`. | Every finished fetch. |

Rules, each one a test:

1. **Requested is not successful.** The landing event carries the outcome of
   the fetch: not requested, succeeded, or failed with a kind. The boolean
   goes away.
2. **A sweep that did not fetch says nothing about fetching.** It leaves all
   three facts alone. Under the FZ-02 ownership split the fetch facts are a
   third owner beside the poll and the sweep, and `apply_local_sweep` must
   not carry them.
3. **Only success advances `last_success`.** Unknown stays unknown and is never
   shown as a time.
4. **A failure settles.** Any finished fetch ends the spinner for that repo,
   as today. A failure keeps every local field and the existing remote refs.
5. **Counts against stale refs say so.** While `outcome` is `Failed`, behind
   and ahead are "as of the last successful fetch", and the row or the detail
   pane says the fetch failed.
6. **No retry storm, one refetch on recovery.** A failed repo is fetched
   again when it would have been anyway: the next watch sweep, a moved
   remote, `f` or `r`. One exception: when a GitHub poll succeeds and the
   previous poll's outcome was `Offline`, the repos whose fetch mark is
   `Unreachable` or `TimedOut` are fetched once, in the same batch as a
   moved-remote fetch would be. A refetch that fails again keeps its mark
   and waits for the next trigger; the recovery refetch never repeats on its
   own, and a repo marked for any other reason (`Auth`, `NoRemote` or
   `Other`, a git error) is not touched. Amended: see [amendment 1](#amendments).
7. **An empty `FETCH_HEAD` is not a fetch.** `stale_remotes` treats it as
   never fetched, so the repo is fetched instead of skipped. This keeps the
   existing rule that unknowns must fetch.
8. **No credential reaches the screen or a log.** The detail is a short phrase
   chosen by kind plus, at most, git's first error line with any URL userinfo
   removed. Classification reads stderr under the C locale the command already
   sets.
9. **A mixed sweep is reported as mixed.** A manual refresh whose fetches
   partly failed says how many failed, not `Refreshed`.

## What the user sees

Wording and look are Andreas's to accept on the branch build, so FZ-04 stops
at Ready to integrate. The information that must be on screen:

- On the repo: that its last fetch failed, and the kind, in a form short
  enough for the row (for example a marker) and spelled out in the detail pane
  with the age of the last successful fetch, or "unknown".
- For the fleet: how many repos failed after a manual refresh.
- Nothing extra when every fetch succeeded.

## File ownership for FZ-04

`src/git.rs` (a fetch result with kind and sanitized detail; `run_git` keeps
its signature for other callers), `src/model.rs` (the fetch facts and their
owner), `src/main.rs` (`spawn_sweep`, `RepoUpdated`, `apply_repo_updated`,
`stale_remotes`), `src/app.rs` (settling and the refresh summary), the status
display in `src/ui/`, `README.md`. Serial with FZ-03 and with the FZ-05
tickets that touch `main.rs`.

Regressions, with local test remotes or controlled process results and no
GitHub credentials: single fetch success; failure by missing remote; timeout;
a mixed sweep; recovery after a failure; a non-fetching sweep leaving the
fetch facts alone; an empty `FETCH_HEAD` treated as never fetched; a URL with
userinfo never appearing in the detail.

## Decisions

- **Look and wording.** Andreas, on the branch build. FZ-04 stops at Ready to
  integrate.
- **Rule 7 in `stale_remotes`.** Adopted as written. Leaving the baseline
  alone would keep a known way for the fleet to look more current than it is.
- **No fleet-level marker for watch-mode failures.** The per-repo marker is
  enough, and the header already carries the GitHub segment.
- **Where the facts live.** A field on the row (`RepoStatus.fetch`), owned by
  the fetch and by nothing else: the fourth writer beside the load, the poll
  and the sweep.
- **A load carries the facts by path.** Amended on 2026-10-01 after review:
  when a load replaces the rows, a row at the same path keeps its fetch
  facts. A window apply fetches nothing and must not wipe a failure while
  the counts are still stale. A newer non-empty `FETCH_HEAD` on disk clears
  an older failure, and a fetch the load itself ran is recorded only when it
  is newer than the kept attempt. Known limit: when a path now holds a
  different repo, the old facts stay on it until its next fetch.
- **The drawn remote URL.** The detail pane drew `remote_url` as stored, so
  an HTTPS remote with userinfo put a credential on screen (confirmed by a
  failing test). It is redacted where it is drawn: all userinfo for `http`
  and `https`, only the password part for other schemes, because an ssh
  login name is not a credential and the drawn URL should stay one that
  works.
- **Linked worktrees.** `FETCH_HEAD` is per worktree (FZ-06, executed), so a
  fetch in the main checkout does not advance a linked worktree's
  `last_success`. Accepted: extra fetches, not wrong data.

## Amendments

1. **2026-10-16, rule 6: one refetch on recovery** (Andreas, "Rule 6 yes for
   refetch"; [#76](https://github.com/Gaurgle/fleetz/issues/76)). "Nothing
   new retries it" becomes the exception in rule 6: when the network drops,
   every clone's fetch fails as `Unreachable` or `TimedOut`, and outside
   watch mode nothing fetched it again, so the marks stayed until `f` or
   `r`. Now the first successful GitHub poll after an `Offline` one fetches
   those repos once, through the moved-remote batch
   (`main.rs::recovery_refetch`, called from `apply_github_poll_at`), so the
   fetch tally, the per-repo marks and the status line are a moved-remote
   batch's. Of the fetch kinds in [State](#state), `Auth`, `NoRemote` and
   `Other` (a git error) are not refetched. No fetch backoff, no new config key, and `r`, `f`,
   `F`, the watch sweep and `stale_remotes` are unchanged.
2. **2026-10-20, rule 6: a backoff after timeouts** (FZ-34,
   [#83](https://github.com/Gaurgle/fleetz/issues/83), coordinator ruling R3
   under the owner's P0 instruction). A repo whose fetch never finished
   within the limit started the same download on every sweep and left a
   temp pack each time (185 of them, 22 GB, in one clone). Now, per checkout
   path, after the `n`th consecutive `TimedOut` fetch every automatic fetch
   (the watch sweep, a moved remote, the recovery refetch of amendment 1)
   skips the repo for `backoff_secs(n + 1, refresh_interval, 1.0)` seconds
   from that timeout: two watch intervals, then four, doubling up to
   `BACKOFF_CAP_SECS` (`app::FetchBackoff`). A successful fetch clears the
   path, `r` and `R` clear every path, and `f` and `F` fetch as before. Only
   `TimedOut` counts; the other kinds fail fast and leave the count alone.
   A timeout from a launch or `R` fetch counts too. The row keeps its
   `TimedOut` mark as rule 5 says, and the hold lives in memory only. With
   it, `git::fetch` has its own limit, `FETCH_TIMEOUT` (120 s, R4; every
   other git call keeps `GIT_TIMEOUT`, 30 s), runs in its own process group
   that a timeout kills whole (R1), and removes the temp packs the timed-out
   fetch created (R2).
   Ruling R5 (same day) adds one fetch per checkout at a time: `App`
   lists the paths a sweep is fetching (`App::fetches_in_flight`), and
   every sweep selection, automatic or `f`/`F`, sweeps such a path without
   fetching it again. The landed outcome ends the entry, as does the
   `SWEEP_STUCK_AFTER` expiry. A launch or `R` load runs its fetches itself
   and does not take part.
3. **2026-10-20, a progress-aware limit and the claim at permit** (FZ-35a,
   [#84](https://github.com/Gaurgle/fleetz/issues/84), the spec
   `docs/superpowers/specs/2026-10-20-fetch-progress-and-cap-design.md`
   section A, approved by Andreas). Amends amendment 2. The fetch runs with
   `--progress` instead of `--quiet` and its stderr is read while it runs:
   each `\r`- or `\n`-separated update (`Receiving objects`, `Resolving
   deltas`, the server's `remote:` phases) is a `git::FetchProgress`; what
   `--quiet` used to keep off stderr is dropped, and the rest is what rule
   8's classification and detail read, as before. `FETCH_TIMEOUT` is gone:
   `FETCH_STALL_LIMIT` (30 s) restarts on every update, so a fetch still
   moving is never killed, and one silent for 30 s is killed and cleaned as
   amendment 2 kills a timeout, with the same `TimedOut` kind and the same
   backoff. The ssh connect timeout stays. While a fetch runs, the received
   size goes to its row at most twice a second (`AppEvent::FetchProgress`,
   held on the row's facts as `FetchFacts::progress`, in memory only) and
   the row reads `fetching 312 MB` in the place of the failure marker of
   rule 5 once bytes have arrived; the fetch's landing clears it. The
   one-fetch-per-checkout entry of R5 becomes a claim with a token
   (`engine::FetchClaims`): reserved when a sweep is scheduled, taken when the
   fetch's permit is granted (a reservation that expired while queued is
   taken again if the path is free, and the fetch does not run if a newer
   claim holds it), kept alive by each update, and ended only by its own
   landing, so a late landing cannot end a newer claim. `SWEEP_STUCK_AFTER`
   expires a claim from its last sign of life (reserved, permit granted, or
   an update); the refresh spinner and the manual fetch report followed in
   amendment 4. A launch or `R` load now claims its fetches too and skips a
   path a sweep is fetching; its fetches report no progress to the rows.
4. **2026-10-20, the automatic-fetch cap** (FZ-35b,
   [#85](https://github.com/Gaurgle/fleetz/issues/85), the spec
   `docs/superpowers/specs/2026-10-20-fetch-progress-and-cap-design.md`
   section B, approved by Andreas: no question on `f`, `F` keeps the cap,
   200 MB). Amends rule 6 and amendment 2's "`f` and `F` fetch as before".
   The config key `fetch_max_mb` (MB, default 200, `0` no cap) bounds every
   fetch but `f`: launch, `R`, the watch sweep, a moved remote, the recovery
   refetch of amendment 1, and `F`.
   - **The kind.** `TooBig` joins the kinds in [State](#state): a fetch
     whose received bytes (amendment 3's progress) pass the cap is killed
     and cleaned as amendment 2 kills a timeout, and its detail is the bytes
     it had received. Row: `fetch over N MB, press f`; detail pane and
     status line: `over the size cap`.
   - **The baseline.** The fetch facts keep GitHub's `diskUsage` at the
     last successful fetch in this session (`FetchFacts::disk_baseline`, in
     memory only). The poll asks for `diskUsage` on every repo (free: cost 3
     / nodeCount 1030 at `repo_limit` 50 with and without it, 2026-10-20);
     a payload cached without it parses unchanged. When the current value
     exceeds the baseline by more than the cap, the repo is held before any
     download (`FetchFacts::grown`). Row: `fetch ~N MB, press f`, N the
     growth. A missing current value, an unknown baseline, or an unchanged
     or shrunken size decide nothing: the fetch runs under the cap.
     `grown` is derived state: recomputed from the dataset and the baseline
     on every poll apply and every load apply (`main.rs::assess_size_holds`),
     never set by a fetch (a successful one only clears it). A sweep that started before a push and lands
     after the poll that saw it records the new size as its baseline, so the
     pre-check misses that growth once and the in-fetch cap bounds it.
   - **The hold.** A repo held either way is skipped by every fetch but
     `f`, which fetches it uncapped under amendment 3's stall limit and
     shows `fetching N MB` meanwhile. The hold lasts until `f` on that row or
     any successful fetch, which also records the new baseline. It is not
     amendment 2's backoff: `TooBig` is not counted there, and `r` and `R`
     do not lift it.
   - **Reports outlive long fetches.** The refresh spinner's fetch set and
     the manual fetch report (rule 9) expire `SWEEP_STUCK_AFTER` after the
     last progress of a fetch they wait on, not after they started: an `f`
     on a big repo can run well past 180 s and still owes its "N of M
     fetches failed" line.
