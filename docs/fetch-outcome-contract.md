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
| First poll's fetch decision | `stale_remotes` skips a remote pushed before `last_fetch_at`. After a failed fetch that baseline is wrong, and the repo is skipped as "already in hand". | `main.rs::stale_remotes` 2241 |
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
6. **No retry storm.** A failed repo is fetched again when it would have been
   anyway: the next watch sweep, a moved remote, `f` or `r`. Nothing new
   retries it.
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
