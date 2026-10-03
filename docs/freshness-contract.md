# GitHub freshness contract

Status: **accepted by Andreas on 2026-10-01**, with the decisions recorded at
the end. Written by the coordinator (`claude-fable-5-1`) against `main` at
`6c72926`. It is the contract
[FZ-03 / #16](https://github.com/Gaurgle/fleetz/issues/16) implements. Astra
has not reviewed it; a second opinion is still welcome, and a disagreement
goes to Andreas rather than into the code.

**Amended on 2026-10-01** by Andreas's acceptance of owner items O2 and O4 of
[api-budget-design.md](api-budget-design.md) (FZ-10). The amendments are
listed in [Amendments](#amendments) and each takes effect when the ticket
named there lands. Until then the code implements the text as first written,
which the amended text also allows. Rules 6, 9 and 10 and the vocabulary
carry a pointer.

Scope: the one GitHub dataset (repos, PRs, CI, contribution calendar) that a
single GraphQL call returns. Per-repo `git fetch` outcomes are FZ-04 and reuse
the vocabulary here. Nothing in this contract changes the cache file format,
the cache key or the polling budget.

## The problem in one paragraph

fleetz keeps one timestamp and one flag for GitHub: `App::last_github_poll`
and `App::github_poll_failing`. Three different things are folded into them:
that a request was tried, that GitHub answered, and how old the data on screen
is. `github::fetch_repos` hides the difference at the source, because a
transport failure or a non-2xx answer returns `Ok` with the cached payload, so
the caller sees a success.

## What happens today

Read from the code, not executed. Line numbers are at `6c72926` and drift.

| Situation | Today | Where |
|---|---|---|
| Background poll, network down or HTTP non-2xx, cache file exists | `fetch_repos` returns `Ok` with `from_cache: true`. `apply_github_poll` ignores `from_cache` ("network data by construction"), clears `github_poll_failing`, and stamps `last_github_poll`. **The header shows nothing. Offline looks healthy.** | `github.rs::fetch_repos` 555-569, `main.rs::apply_github_poll` 1687-1700 |
| Background poll, GraphQL error body (rate limit) | `Err`. `⚠ github` appears in the header and stays until a poll succeeds. The cache file is not overwritten. | `github.rs::accept_response` 600-603, `ui/mod.rs` 810 |
| Launch with any cached payload | Painted at once, whatever its age. The age is not carried anywhere and nothing on screen says the data is cached. The poll clock is left unset so the first tick polls. | `github.rs::fetch_repos` 528-533, `main.rs::apply_load_result` 539 |
| Launch with a cached payload and watch off (`--no-watch`, or `watch = false`) | Same paint, but **no poll ever follows**: the only automatic poll is inside the watch branch of the tick. The data stays as old as the cache until `r` or `R`, silently. | `main.rs` 665-675; call sites of `start_github_poll` are 674 and 1511 only |
| `R` with the network down | Falls back to the cache and reports `Reloaded`. | `main.rs::fetch_dataset` 2212, `apply_load_result` 554 |
| A poll that never returns | After 60 seconds the in-flight flag is dropped. No failure is recorded, the poll clock is not advanced, so the next tick starts another request while the first is still open. The HTTP client has no timeout. | `main.rs` 658-664, `GITHUB_POLL_STUCK_AFTER`; `github.rs` 543 |
| Any failing state while a repo's CI is `Pending` | The poll interval stays at the 15 second floor, because the frozen `Pending` keeps asking for it. | `main.rs::github_poll_secs` 1799 |
| No usable cache and the first load fails | The error panel explains it. This case is already right. | `App::load_error`, `ui/mod.rs::render_load_error` |

## Vocabulary

- **Attempt**: one network request to GitHub that fleetz started and that has
  finished, successfully or not. Reading the cache is not an attempt.
- **Outcome**: how the most recent attempt ended. `Success`, or `Failed` with
  a kind: `Offline` (transport: DNS, TLS, no route), `Http` (non-2xx status),
  `Api` (GraphQL `errors`, the rate limit among them), `Auth` (no token from
  `gh`), `Malformed` (unreadable body), `TimedOut` (given up on). Two kinds
  are added by [amendment 3](#amendments).
- **Data time**: when the data now on screen left GitHub. For a network answer
  it is the moment the answer was accepted. For a cached payload it is the
  cache file's write time, which is the same thing because only an accepted
  answer is ever written (`accept_response`). If the write time cannot be
  read, the data time is **unknown** and stays unknown.
- **Fresh**: the most recent attempt succeeded and its data is what is shown.
- **Cached**: what is shown was read from disk and no attempt has finished
  since.
- **Stale**: the most recent attempt failed, so what is shown is older than
  the last time fleetz tried.
- **Offline** is one kind of failure, not a synonym for stale. A rate limit is
  never reported as offline.

## State

Three independent facts replace `last_github_poll` and `github_poll_failing`.
They live together in one small type so the transitions are pure functions
that tests can drive without a network.

| Fact | Meaning | Changed by |
|---|---|---|
| `data_time: Option<time>` | Data time as defined above. `None` means unknown or no data. | A successful attempt (now); a cache-served load (file write time or unknown). Nothing else. |
| `last_attempt: Option<time>` | When the most recent attempt finished or was given up on. Drives the retry clock and nothing else. | Every finished attempt, whatever its outcome. Never by a cache read. |
| `outcome` | `NotAttempted`, `Success` or `Failed(kind, message)`. | Every finished attempt. |

Rules that follow, each one a test:

1. **A cache read is not an attempt.** It sets `data_time` from the file and
   leaves `last_attempt` and `outcome` alone. This keeps the existing rule that
   a cache-served launch leaves the poll due at once.
2. **A fallback is a failure that happens to have data.** The fetch layer must
   report "request failed, here is the cached payload" as exactly that, never
   as `Ok`. The result of a fetch carries the payload if any, where it came
   from (network, or cache with its write time), and the failure if an attempt
   was made and failed.
3. **Only a successful attempt advances `data_time`.** A failure of any kind
   leaves it where it was. Launch time is never used as a data time.
4. **A failed background poll changes no row.** The data in memory is at least
   as new as the cache file, so the fallback payload is not applied over it.
   The cached payload is used only when there is nothing in memory to keep:
   the initial load and `R`.
5. **A failure never blanks good data and never touches the cache file.** The
   existing protection against GraphQL error bodies stays as it is.
6. **Every finished attempt advances the retry clock, including a failure and
   a give-up.** The next automatic attempt comes one normal interval later.
   No immediate retry and no backoff scheme: backoff and budget are FZ-10.
   Amended: see [amendment 1](#amendments).
7. **A success clears the failure.** `outcome` becomes `Success`, `data_time`
   becomes now, and the header marker goes away on the same frame.
8. **A cache-served launch is always followed by one attempt**, with watch on
   or off. Watch decides whether polling continues after it, not whether the
   launch data gets confirmed.
9. **While `outcome` is `Failed`, the poll interval is the configured one.**
   The 15 second floor for a `Pending` repo applies only while polls succeed,
   since a frozen `Pending` says nothing about what CI is doing now.
   Amended: see [amendment 2](#amendments).
10. **Data time never goes backwards.** A poll given up on after 60 seconds is
    recorded as `Failed(TimedOut)`. If its answer arrives later, it is applied
    only when no newer attempt has been applied since; otherwise it is
    discarded. Cancelling the hung request and a client timeout are FZ-11.
    Amended: see [amendment 4](#amendments).

## Transitions

| Event | Rows | `data_time` | `last_attempt` | `outcome` | Cache file |
|---|---|---|---|---|---|
| Launch, cache served | From cache | File write time, or unknown | Unchanged (unset), so one attempt follows at once | `NotAttempted` | Untouched |
| Launch, no usable cache, request succeeds | From network | Now | Now | `Success` | Written |
| Launch, no usable cache, request fails | None; error panel | None | Now | `Failed` | Untouched |
| Poll succeeds | GitHub-owned fields replaced | Now | Now | `Success` | Written |
| Poll fails (any kind) | Unchanged | Unchanged | Now | `Failed(kind)` | Untouched |
| Poll given up on after 60 seconds | Unchanged | Unchanged | Now | `Failed(TimedOut)` | Untouched |
| Answer arrives from an attempt older than the one applied | Unchanged | Unchanged | Unchanged | Unchanged | Untouched |
| `R` succeeds | Rebuilt from network | Now | Now | `Success` | Written |
| `R` fails, cache exists | Rebuilt from cache | File write time, or unknown | Now | `Failed(kind)` | Untouched |

## What the user sees

Andreas accepted the information content below. The exact wording, glyph and
colour are his to accept on a running build, so FZ-03 stops at Ready to
integrate. Everything sits in one header segment where `⚠ github` is today.

| State | Header | Example (wording open) |
|---|---|---|
| Fresh | Nothing, as today | |
| Cached, attempt on its way | Quiet, says cached and how old | `github cached 3h` |
| Stale | Marker, failure kind, age of the data | `⚠ github offline, data 12m` |
| Stale, data time unknown | Same, age stated as unknown | `⚠ github rate limited, age unknown` |

- The segment keeps today's rule that it is never dropped by the header's fit
  loop; a narrow header gets a shorter form, not no form.
- Age uses the same wording as the `swept` label beside it.
- `R` that fell back to the cache says so in its status message instead of
  `Reloaded`.
- No per-row marking in FZ-03. Whether stale rows should look different (dimmed
  CI lamps, a `Pending` lamp that stops blinking) is raised under FZ-13.
- With watch off and a successful launch poll, the header shows nothing more:
  the watch indicator already says the dashboard is not polling.

## File ownership for FZ-03

One worker, serial with FZ-04 because both change the refresh state in
`main.rs` and `app.rs`.

| File | Change |
|---|---|
| `src/github.rs` | Fetch result reports payload, origin and failure separately; failure kinds |
| `src/cache.rs` | A stale read also returns the file's write time. No format or key change |
| `src/app.rs` | The freshness type and its pure transitions, replacing the two fields |
| `src/main.rs` | `apply_github_poll`, `apply_load_result`, `LoadResult`, the stuck guard, the poll-due check |
| `src/ui/mod.rs` | The header segment |
| `README.md` | The visible behaviour |

Required regressions, all with controlled results and no network or sleeps:
launch from a cache of a planted age; launch from a cache whose age cannot be
read; background fallback with a cache present (the case that is invisible
today); no usable cache; GraphQL error body leaves the cache file intact;
recovery after a failure; no attempt sooner than one interval after a failure;
a give-up recorded as a failure; `R` falling back without claiming a reload;
a cache-served launch with watch off makes exactly one attempt; a `Pending`
repo does not shorten the interval while polls fail; an answer from an older
attempt is discarded once a newer one has been applied.

Existing invariants this keeps (see `CLAUDE.md`): launch paints from the cache
however stale; a cache-served load leaves the poll due; only a response
carrying `data` is cached; the cache is keyed by `repo_limit` alone.

## Decisions

Answered on 2026-10-01. D1 and D2 by Andreas; D3 to D6 he left to the
coordinator's recommendations.

- **D1. Header wording, glyph and colours.** Information content accepted as
  in the table above. The look is settled on the FZ-03 branch build.
- **D2. A cache-served launch polls once even when watch is off.** Yes
  (rule 8). The rejected alternative kept `--no-watch` strictly quiet and left
  a `cached` segment highlighted until `r`, at the cost of showing old CI by
  default.
- **D3. No `Pending` floor while polls fail.** Yes (rule 9).
- **D4. The poll that never returns.** Recorded as `Failed(TimedOut)`, and a
  late answer cannot move data time backwards (rule 10). Cancellation and a
  client timeout stay with FZ-11.
- **D5. A failed background poll applies nothing.** Yes (rule 4). Applying the
  cached payload would only matter when another fleetz instance wrote a newer
  cache, and then this instance is probably not failing either.
- **D6. Per-row staleness.** Header only in FZ-03. The blinking `Pending` lamp
  on stale data is raised under FZ-13.

## Amendments

Accepted by Andreas on 2026-10-01 as owner items O2 and O4 of
[api-budget-design.md](api-budget-design.md), which holds the reasoning, the
numbers and the scenarios. Each amendment names the ticket that puts it into
the code.

1. **Rule 6: at least one interval.** "The next automatic attempt comes one
   normal interval later" becomes "at least one normal interval later": it
   may be longer under backoff, a rate limit or a low budget. Both guarantees
   stay: every finished attempt advances the retry clock, and there is never
   an immediate retry. Backoff is floored at the interval. Takes effect with
   FZ-10b.
2. **Rule 9: at least the configured interval.** "While `outcome` is `Failed`,
   the poll interval is the configured one" becomes "at least the configured
   one". The first failure still waits exactly the configured interval; after
   that the wait doubles per consecutive failure up to a cap of 8 to 12
   minutes. `Offline` and `Auth` are the exception and keep the configured
   interval. The rule's purpose, no `Pending` floor while failing, is kept and
   extended: a stretched or paused pace also suspends the floor and the push
   shortcut. Takes effect with FZ-10b.
3. **Vocabulary: two failure kinds.** `Server` (a 5xx answer) and `Refused`
   (credentials or permissions refused: a 401, or a 403 that is not a limit)
   join the kinds. "A rate limit is never reported as offline" holds; a rate
   limit can now be an `Http` failure (a 403 or 429) as well as an `Api` one,
   and both read as rate limited. A connection that cannot be opened,
   including a connect timeout, is `Offline`. Takes effect with FZ-10a.
4. **Rule 10 and D4: the client timeout is FZ-10's.** "Cancelling the hung
   request and a client timeout are FZ-11" becomes "cancelling the hung
   request is FZ-11; the client timeout (30 s in total, 10 s to connect) is
   FZ-10". The 60 second give-up stays as the backstop. Takes effect with
   FZ-10a.
5. **Transitions: a manual refresh under a known rate limit sends nothing.**
   One row is added. Rule 1 and the definition of an attempt are unchanged:
   nothing was sent, so nothing is recorded. Takes effect with FZ-10b.

   | Event | Rows | `data_time` | `last_attempt` | `outcome` | Cache file |
   |---|---|---|---|---|---|
   | `r` or `R` under a known limit, no poll in flight | `r`: local state only. `R`: rebuilt from the dataset in memory | Unchanged | Unchanged | Unchanged | Untouched |

6. **What the user sees: one line.** `r` or `R` under a known limit says in
   its status line that GitHub is rate limited and when it resumes, instead
   of `Refreshing…` or `Reloaded`. The wording, and the header's words for a
   slowed, paused or rate limited poll (O1), are settled on the FZ-10c branch
   build.

Accepted by Andreas on 2026-10-02 with the owner items of
[coverage-cache-design.md](coverage-cache-design.md), whose section 6 goes
through every rule, changed or not. Amendments 7 to 9 took effect with FZ-09c
on 2026-10-03. The changes for coverage marks and for an account change
follow with FZ-09b and FZ-09d and are added here when those land.

7. **Rule 2: the fallback is this credential's file.** A fallback after a
   failed attempt reads only the cache file keyed to the credential that made
   the attempt. A credential with no saved copy has no fallback, and neither
   has an attempt made with no credential (signed out of `gh`): the failure
   then carries no data. Rule 1 is unchanged and covers a launch that is
   signed out, or whose `gh` lookup did not answer within 2 s: it is served
   from the newest saved copy as a cache read, and no attempt is recorded.
8. **Rule 5: cleanup is not a failure path.** A failure still never touches
   the cache file. Superseded files (the legacy unkeyed file, a file of an
   older format version, another account's file) are deleted only when they
   have gone 30 days unwritten, only at a launch that has data and a known
   account with its own current file, and never the file that launch just
   served.
9. **Transitions: what "cache" means.** "Launch, cache served" reads this
   credential's keyed file; or the legacy unkeyed file while no account has a
   keyed file; or, signed out or after a timed-out lookup, the newest keyed
   file. "Launch, no usable cache" includes a credential with no saved copy
   while other accounts' copies exist: nothing is painted from them. "`R`
   fails, cache exists" becomes two rows:

   | Event | Rows | `data_time` | `last_attempt` | `outcome` | Cache file |
   |---|---|---|---|---|---|
   | `R` fails, this credential's keyed file exists | Rebuilt from that file | File write time, or unknown | Now | `Failed(kind)` | Untouched |
   | `R` fails, no keyed file for this credential, or signed out | Unchanged; the status line says the load failed and why | Unchanged | Now | `Failed(kind)` | Untouched |

   The second row is owner item O12, and Andreas accepted it for the
   signed-out case too on 2026-10-03 ("Merge as it is"): before FZ-09c a
   signed-out `R` rebuilt from the shared file.

## Not in this contract

Budget and backoff beyond the amendments above
([api-budget-design.md](api-budget-design.md)), cache isolation by account
and host (FZ-09), a shared scheduler and cancellation (FZ-11), per-repo fetch
outcomes ([fetch-outcome-contract.md](fetch-outcome-contract.md)), and
partial coverage of the dataset (FZ-09).
