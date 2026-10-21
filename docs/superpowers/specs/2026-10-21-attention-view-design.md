# Attention: repositories that need action, and why (FZ-14)

Written 2026-10-21 by the coordinator (`claude-fable-5-1`) during the
unattended run Andreas asked for (18:52), under the batch 3 approval of
2026-10-17 ("3. yes, you decide order", each spec first). Planning issue:
[#27](https://github.com/Gaurgle/fleetz/issues/27) (FZ-14, M4). Status:
**draft, for Andreas's approval; no worker before his word.**

## What the issue asks for

Surface repositories that need action because of interrupted Git
operations, branch state, pull requests or CI, with explicit reasons and a
stable order; keep the trust states (incomplete, stale, unavailable) from
M1; coordinate with FZ-07 (assigned issues); never an undifferentiated
alert count.

## What fleetz has today

- Pane 1 groups repos into three sections by location (`github activity`,
  `local changes`, `also active`), each in push order; a `/` query with
  `#tags` (`#dirty #behind #ahead #ci #prs #stash`) ANDs filters and ranks
  free text. Rows carry badges: CI lamp, `nPR`, stashes, `nWT`, the fetch
  slot and the counters.
- Signals held per row: dirty lines, ahead and behind per branch, stashes,
  worktrees with their dirty state, CI on the default branch, up to five
  open PRs with `reviewDecision`, the fetch outcome, the location, and
  `not_in_last_poll`. Nothing detects an interrupted git operation.
- The footer summary counts behind, ahead, dirty, synced, not cloned and
  PRs over the visible rows.

## Design

### A. A reason, not a score

A repo "needs attention" when at least one **reason** holds. Reasons are
few, explicit and ordered; a reason always has words on screen. Dirty, ahead
and stashes are not reasons: they are work in progress, and a dashboard
that nags about them is ignored within a day.

| Rank | Reason | Fact | Words |
|---|---|---|---|
| 1 | Git operation in progress | `MERGE_HEAD`, `rebase-merge/`, `rebase-apply/`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `BISECT_LOG` in the checkout's git dir | `merge in progress`, `rebase in progress (3/7)`, `cherry-pick in progress`, `revert in progress`, `bisecting` |
| 2 | Conflicts | unmerged paths in `git status --porcelain` (`UU`, `AA`, `DU`...) | `4 conflicts` |
| 3 | CI failed on the default branch | `ci_status == Failed` | `CI failed on main` (the branch name once the payload carries it; `CI failed` until then) |
| 4 | Changes requested on a PR | a held PR with `reviewDecision == ChangesRequested` | `changes requested on #12` |
| 5 | Fetch failing | `fetch.outcome` failed on the last two attempts | `fetch failing: offline` (the FZ-04 wording) |
| 6 | Behind on the checked-out branch | the current branch's `behind > 0` (not the sum over branches) | `3 behind on main` |
| 7 | Detached HEAD in the main checkout | `HEAD` not a ref | `detached at a1b2c3d` |

A repo's **reason** is its highest-ranked holding reason; the rest are
listed after it in pane 2's `info` header line. Rank 4 needs `reviewDecision`
only, already fetched. Review *requests* on me, draft state and per-PR
checks are not fetched today; they would widen the query and are left to
FZ-07 or a measured follow-up (section F).

### B. Where it shows

1. **A section `needs attention` at the top of pane 1**, above `github
   activity`, drawn only when non-empty, in the existing `section_div`
   look. Its rows are the repos with a reason, ordered by rank then push
   recency; they leave their location section while a reason holds (one
   row per repo, as today). The row keeps its badges; the reason's words
   are drawn in the fetch slot's place when that slot is empty, dimmed, so
   a glance reads `fleetz  rebase in progress  2! 1↑`. When the fetch slot
   is busy (a fetch running or held), the badge wins and the words wait.
2. **A badge for rank 1 and 2** regardless of the section: `MERGE`,
   `REBASE`, `PICK`, `REVERT`, `BISECT` in RED, and `n⚡` for conflicts
   (plain mode: `n!!`). These are the states where a wrong key loses work;
   they deserve a badge even in a filtered or ranked list.
3. **`#attention` as a tag** (`#att` by prefix): the list shows only the
   section's rows, ranked mode off, so the order is the section's. Free, as
   the filter machinery exists.
4. **Footer summary** gains `N need attention` in PEACH, first, only when
   N > 0. It is a count of rows each of which shows its reason: not an
   undifferentiated alert count.
5. **Pane 2 `info` header** gains one line while any reason holds:
   `attention: rebase in progress (3/7), 3 behind on main`.

No new key, no new pane, no overlay. The cursor and the eye walk the same
list as today, so `visible_repos` and the drawn order stay one function.

### C. Trust states

- A reason from GitHub (ranks 3 and 4) is held only while the row is in the
  last poll; a `not_in_last_poll` row keeps the words but dimmed and leaves
  the section (its GitHub facts are not current). A failed poll changes no
  row, as the freshness contract says.
- A reason from disk (ranks 1, 2, 6, 7) is re-read on every sweep; between
  sweeps it stands.
- Rank 5 needs two consecutive failures so one bad DNS lookup does not
  move a row.
- A not-cloned repo has no reason. A local-only repo has ranks 1, 2, 7 only.

### D. Stable ordering, and snooze

Within the section: rank, then push recency (`sort_by_activity`), then
name; a repo whose reason changes rank moves at the next sweep or poll, not
on a tick. Snooze (FZ-15, `docs/ideas.md` "Snooze, not hide") removes a row
from the section like any other; a reason of rank 1 or 2 wakes a snoozed
repo at once (a rebase you left half-done is exactly what must resurface).

### E. The git-operation probe

A few `Path::exists` calls on `repo_dirs(path).git_dir` in the always-run
tier of `rebuild_repo_status`, beside `linked_worktrees`: no process, a
microsecond each. `rebase-merge/msgnum` and `end` give the `(3/7)`. Conflicts
come from the porcelain output the dirty count already reads: count lines
whose two-letter code has `U` or is `AA`/`DD`. The new fields ride on
`RepoStatus` (`git_op: Option<GitOp>`, `conflicts: u32`) and through
`apply_local_sweep` and the other builders; test fixtures that construct
`RepoStatus` literally gain the two fields (compile-forced, no assertion
changed).

### F. Not in this design

- Review requests addressed to me, draft PRs and per-PR checks: a query
  widening to measure first (`reviewRequests` is a connection with a
  `first:`); FZ-07 territory.
- A score or a single number per repo.
- Notifications: the "CI completion notification" idea covers that.

## Tickets

1. **FZ-14a, the git-operation probe and badges** (no new view): the
   probe, `git_op` and `conflicts` on `RepoStatus`, the `MERGE`/`REBASE`/
   `PICK`/`REVERT`/`BISECT` and `n⚡` badges with plain fallbacks, tests on
   a fixture repo with a real stopped rebase and a real conflict. Small
   look change (a badge).
2. **FZ-14b, reasons and the section**: `Reason` and the rank table, the
   `needs attention` section, the reason words in the fetch slot, the
   `#attention` tag, the footer count, the `info` line, the trust rules of
   section C. Stops for his look.
3. **FZ-14c, the default branch name** (shared with the `info` CI line of
   FZ-30d): `name` on `defaultBranchRef`, free, `#[serde(default)]` so
   cached payloads parse; `CI failed on main` and `3 behind on main` read
   the real name. No screen change beyond the words.

## Open with Andreas

1. The reason table: anything to add or drop? (Dirty and ahead left out on
   purpose; "behind" counts only on the checked-out branch.)
2. The section at the top of pane 1 (recommended) versus the tag alone.
3. Rank 5's two-failure rule, and whether a held fetch (`fetch_max_mb`)
   should count as attention (proposed: no, it already shows `press f`).
4. Creating FZ-14a to c as P2 in M4, and the word to run FZ-14a.
