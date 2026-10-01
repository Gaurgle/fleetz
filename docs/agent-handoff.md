# Fleetz coordinator handoff

Prepared 2026-10-01. Read this file, applicable repository instructions,
[agent-workflow.md](agent-workflow.md), [release-plan.md](release-plan.md), and
[first-wave-tickets.md](first-wave-tickets.md) before acting.

## Authority and routing

Andreas owns priorities, acceptance, what he installs and runs, tags and
releases. Since commit `7005579` on 2026-10-01 the coordinator commits the
reviewed diff on the ticket branch, pushes it and merges into `main` under the
safe-merge rule in [agent-workflow.md](agent-workflow.md), and commits
coordination docs straight to `main`. When a condition of that rule fails or
is unclear, stop at Ready to integrate and ask. Workers and reviewers never
commit. No agent tags, releases, runs `cargo install`, force-pushes, rewrites
pushed history or deletes remote branches.
Astra is the preferred lead. Fable 5.1 may take over coordination when Astra
is unavailable or its usage needs preserving. This is an authorized manual
handoff; there is no automatic coordinator switch or background service.

- Lead: `gpt-6-astra` or `claude-fable-5-1` (Fable effort `high`).
- Codex workhorse: `gpt-6.1-sol`, effort `medium`.
- Substantive Claude worker or independent reviewer: `claude-opus-5-5`,
  effort `medium`.
- Truly small work: Sonnet or `gpt-6-luna`, effort `medium`; record the
  resolved model instead of silently substituting.

Standing instruction from Andreas, 2026-10-01, in his words: "i dont want to
have to tell you what to run. we have a harness and the reviewers, keep on
going as planned as long as I dont have to make a design decition or
similar." The coordinator of that session therefore dispatches the next
ticket in [release-plan.md](release-plan.md) order when its dependencies
hold, closes issues at Done, and goes to Andreas only for design decisions,
interaction or look acceptance, and what the safe-merge rule reserves for
him. It was said to one session; a new lead confirms it once with Andreas
before relying on it, since `agent-workflow.md` still says to dispatch only
what he names.

Start with one implementation worker and a separate reviewer. Do not recursively
expand delegation. Use architectural review at bounded checkpoints, not routine
board updates. Do not automatically switch to API billing or paid usage credits.

## Current state

Setup writes succeeded for all 23 issues on the private
[Fleetz release Project](https://github.com/users/Gaurgle/projects/1).
Status, priority, Worker and Ticket reviewer fields were filled. All workers
and reviewers are Unassigned. Milestones M0 through M5 exist. Native
blocking links are #16 blocked by #15 and #17 blocked by #16.

- [Release table](https://github.com/users/Gaurgle/projects/1/views/1)
- [Release board](https://github.com/users/Gaurgle/projects/1/views/2)

Both views show Title, Status, Priority, Milestone, Worker and Ticket reviewer.
A later read on 2026-10-01 by Fable found all 23 items on the Project,
including #29 and #30, with Status, Priority, Worker and Ticket reviewer set,
both dependencies in place, the Project private and the board grouped by
Status. Issue #7 has no milestone. Visual layout in the browser is the one
setup check still open. GitHub is authoritative for current status and
assignment.

FZ-02 is integrated and #15 is closed. FZ-01 and FZ-05 are in flight; FZ-06
and the pin for issue #1 are authorized and queued; the freshness contract
for FZ-03 is accepted. All are described below. No release has started.

Fable is acting coordinator on 2026-10-01 while Astra's usage recovers, and
Andreas asked to lean on Claude for the day. The Claude-native workforce is
defined in `.claude/agents/` and described in `agent-workflow.md`. Under it,
tickets run on `fz-worker` with a separate `fz-reviewer` session instead of
Sol. Capture `git status --short` and the actual base SHA before dispatch.

## FZ-02: Done, integrated into `main`

Andreas instructed Fable to run FZ-02 on 2026-10-01 and, in a later session
the same day, to finish it from this file and close #15 at Done.

- Base: `27abb030a024e5d6c5ef3d0618188cb86292d928`. Worker `fz-worker` and
  reviewer `fz-reviewer`, both `claude-opus-5-5`, separate invocations.
  Verdict accepted, no blockers, no changes after the review.
- Ticket commit `8027d3b7437c3a39b8bd064bafe31fe91707c120` on
  `fix/fz-02-preserve-github-state`: exactly the reviewed diff, `src/main.rs`
  and `src/model.rs`, 432 insertions, 77 deletions. Pushed; CI green on it
  ([run](https://github.com/Gaurgle/fleetz/actions/runs/36866002614)).
- Combined result with `main` at `7005579`, in a throwaway clone: `cargo fmt
  --check` exit 0, `cargo clippy --all-targets -- -D warnings` exit 0,
  `cargo test` exit 0 with 649 passed, 0 failed, 1 ignored.
- Merge commit `28ebba796ab6b1bea72e96f908e0082b61456ce9` on `main`
  (`git merge --no-ff`), pushed. Its tree is identical to the one checked.
  CI on `main` at that commit: `ci` and `docs` both green. The push also
  published `7005579`.
- Board: #15 is Done and the issue is closed, with the commits and evidence
  in a comment. Closing it lifted the native block on #16; #16 is still
  Backlog and still needs the freshness contract before it is Ready.
- Worktree `.claude/worktrees/fz-02` and the remote branch are kept. Removing
  either is Andreas's call.
- Try it: `cargo run --release` from `main`. The installed `fleetz` is
  unchanged until Andreas runs `cargo install --path .`.

What the change does: the `RepoUpdated` and `Github` loop arms became
`apply_repo_updated` and `apply_github_poll`. A sweep result now lands
through `RepoStatus::apply_local_sweep`, which takes only the fields the sweep
probes onto the row as it is at that moment, and the poll writes through
`RepoStatus::apply_github`. Both destructure exhaustively, so a new field
does not compile until its owner is decided. The split is documented on
`RepoStatus`.

Follow-ups found, not ticketed (creating issues needs Andreas's approval):

- Overlapping local sweeps can still put older local fields over newer ones.
  A single-repo sweep (`f`, or a moved remote) can overlap a full sweep.
- A late sweep result after a reload merges by path. A path reused by a
  different repo is FZ-05 identity work.
- Optional test tightening: the failed-poll regression takes its snapshot
  after the good poll, so it also passes on the old code.
- The two `discovery::repoz_parity` tests fail when the suite runs from a
  dirty checkout outside `$HOME`: repoz adds the current repo's toplevel to
  its scan, and the test picks up that absolute path. Seen in the throwaway
  integration clone while the merge was staged but not recorded; the same
  tree passes once clean, and CI is unaffected. Keep an integration checkout
  clean before running the suite.

Pilot cost, as the client reported it: worker about 5 minutes, 29 tool uses,
about 79k tokens; reviewer about 1.5 minutes, 14 tool uses, about 45k tokens.
No rework and no review findings beyond the optional one above. The ticket
size was right for one worker. With the pilot integrated, the limit is now
two workers with disjoint files.

## FZ-01: in progress

Andreas instructed Fable to run FZ-01 after FZ-02 on 2026-10-01.

- Base: `28ebba796ab6b1bea72e96f908e0082b61456ce9` (`main`, equal to
  `origin/main`). The main checkout had one uncommitted file at dispatch,
  this one.
- Branch `docs/fz-01-performance-baseline`, worktree
  `.claude/worktrees/fz-01` in the main checkout.
- Worker `fz-worker`, requested `claude-opus-5-5`, effort medium. Reviewer
  will be `fz-reviewer`, same model, separate invocation, and reruns at
  least one measured case.
- Allowed files: `docs/performance-baseline.md` (new) and one index row in
  `CLAUDE.md`. No source, harness or instrumentation is committed.
- Limits given to the worker: isolated `HOME` so the real cache, config and
  Git configuration are untouched; synthetic repos with local bare remotes;
  at most 8 live GraphQL fetches, none in a loop; no installs.
- Board: #14 is In progress with worker and reviewer fields set; the
  assignment is a comment on the issue.

State when last written: the worker is dispatched and has not reported. The
worktree holds whatever it has written so far, uncommitted. Next, in order:

1. Read the worker's report and the document; check that only the allowed
   files changed and that `scripts/check-docs.sh` passes in the worktree.
2. Dispatch `fz-reviewer` with the report. It reruns at least one case.
3. On acceptance, commit the reviewed diff on the branch and push it. The
   push runs `docs.yml`, not `ci.yml`, because only prose changed.
4. Merge under the safe-merge rule, move #14 to Done and record the commits.
   Whether to close #14 has not been asked; leave it open unless Andreas
   says otherwise.

## FZ-05: in progress

Andreas instructed Fable to run FZ-05 alongside FZ-01 on 2026-10-01.

- Base: `6c729265d2214a55979c4ab7e8bf307941184667` (`main`, equal to
  `origin/main`, clean tree at dispatch).
- Branch `docs/fz-05-repository-identity`, worktree
  `.claude/worktrees/fz-05` in the main checkout.
- Worker `fz-worker`, requested `claude-opus-5-5`, effort medium. It traces
  identity through the code and drafts `docs/repository-identity-design.md`
  with alternatives and open decisions. The design decisions are the
  coordinator's, as an architectural checkpoint, and want Astra's second
  opinion when Astra is back.
- Allowed files: `docs/repository-identity-design.md` only. The worker was
  told not to build or run tests, because FZ-01 measures on the same machine;
  claims that need execution are marked unconfirmed in the document.
- Board: #18 is In progress, assignment recorded on the issue.

State when last written: the worker delivered the document (untracked in the
worktree) and is revising it with the coordinator's decisions. The findings:
`github::merge_sources` dedupes the dataset by bare name, so of two repos
with one name the older-pushed one vanishes; the folder-name fallback in
`Wanted::match_clone` lets a fork or a foreign-host clone claim a row; and
the poll writes CI onto local-only rows by bare name. The coordinator
spot-checked those against the code.

Decisions taken by the coordinator, Astra not consulted, Andreas may veto:
rows keyed by remote identity derived from the existing `owner` and `name`
(model A); host parsed and carried but matching stays host-agnostic; the
folder-name fallback for a clone whose origin matches nothing is kept only
when the origin's owner equals the wanted repo's owner and the origin is not
itself a repo in the dataset (so a rename keeps matching, a fork does not);
an ambiguous name claims no row and says so once; pass 3 dedupes by path
only, as repoz does, which changes a `CLAUDE.md` invariant; GitHub fields on
a local-only row only by exact origin; `owner/name` group members deferred;
adding a neutral field value to existing test fixture literals is allowed
when a struct gains a field, with no assertion changed.

Next: read the revised document, dispatch `fz-reviewer` to trace scenarios S1
(collision) and S3 (local-only), then commit, push and merge it. The
implementation tickets FZ-05a to FZ-05f it defines run after FZ-03, serially
where they share `main.rs`. Its index row in `CLAUDE.md` is left to the
coordinator, since the FZ-01 branch also adds a row to that table.

## FZ-06: authorized, not started

Andreas instructed Fable to start FZ-06 once FZ-01's measurements are done.
Dispatch it after the FZ-01 reviewer has finished rerunning its case, so the
mutation experiments do not disturb a timing run. Output is
`docs/cache-invalidation-design.md`; production code is read-only; identity
assumptions are coordinated with FZ-05. Nothing is created for it yet: no
branch, no worktree, #19 still Ready.

## Freshness contract for FZ-03: accepted

[freshness-contract.md](freshness-contract.md) is the contract FZ-03
implements. Andreas accepted it on 2026-10-01: D1 (header information content;
the look is settled on the branch build) and D2 (a cache-served launch polls
once even with watch off) himself, D3 to D6 by the coordinator's
recommendations. It defines attempt, outcome and data time as three separate
facts, the transitions, what the header must say and file ownership. Astra
has not reviewed it; a disagreement goes to Andreas, not into the code.

Two findings behind it, read from the code and not executed: a background
poll that falls back to the cache is reported as a success, so offline shows
no marker at all; and with watch off a cache-served launch never polls.

Board: #16 is Ready, with the contract and file ownership in a comment.
FZ-03 is next in line for an implementation worker under the standing
instruction above. It waits until FZ-01's timing runs are over, because its
worker builds and tests. It stops at Ready to integrate for Andreas to try
the header, and it must not run beside FZ-04.

## Issue #1: pin the Rust toolchain action, then close

Andreas instructed Fable on 2026-10-01 to close out #1: pin
`dtolnay/rust-toolchain` in `ci.yml` and `release.yml`, then close the issue
with a summary. He approved the CI change for this ticket, which the
safe-merge rule requires.

- Pin: commit `6bed0761d98439e5a578e2877258200ad565ba87`, the tip of the
  action's `stable` branch on 2026-09-03, with `toolchain: stable` stated
  explicitly because the ref no longer names it.
- Branch `ci/1-pin-rust-toolchain`, worktree `.claude/worktrees/issue-1`.
  Worker `fz-small`, reviewer `fz-reviewer`.
- Worker and reviewer are done: verdict accepted, no blockers. Committed as
  `1c3701ad551d2499f32019823238381e3d63307b` on the branch and pushed; CI on
  the branch is the real test of the pinned action.
- Still to do: the local fmt, clippy and test run on the combined result,
  which waits until FZ-01's timing runs are over; then merge, confirm CI on
  `main`, and close #1 with a summary of how its acceptance is met.
- `release.yml` only runs on a tag, so its pinned line is checked by reading;
  the identical line in `ci.yml` is exercised by the branch push.

This file is committed and pushed through `89946f5`. A Claude session's
permission check can refuse a docs commit to `main` that Andreas did not ask
for in that session, whatever this file says. On 2026-10-01 he allowed
coordination-doc commits and pushes to `main` for that session only; a new
session asks him once before its first one.

## Running the next ticket

The pilot was [FZ-02 / #15](https://github.com/Gaurgle/fleetz/issues/15). For
any later ticket the flow is the one in `agent-workflow.md`:

1. Prepare the ticket branch in an isolated worktree under
   `.claude/worktrees/<ticket>` from current `main`; record its path and base
   SHA on the issue.
2. Dispatch the worker with the ticket, rules, allowed files and exact base.
3. Use a separate reviewer session against the exact diff and base.
4. After the reviewer accepts, commit the reviewed diff on the branch, push
   it, check the combined result against current `main` and merge under the
   safe-merge rule. Stop at Ready to integrate and ask when a condition fails,
   which always includes tickets whose acceptance is about interaction,
   layout or look.

Do not start implementation simply because an issue is Ready. Dispatch only
a ticket Andreas has told the current session to run.

## Start Fable manually

From this repository, run:

```sh
claude --agent fz-coordinator
```

The agent definition pins the model and effort and carries the takeover
instructions: it reads this file, inspects the live board and working tree,
and reports the next step before any dispatch. Tell it which ticket to run.

Use normal permissions with no bypass. Claude subscription Max authentication
was verified through macOS keychain outside the sandbox, with no active API-key,
auth-token or third-party provider overrides. No model request was run during
board setup. Recheck status and override presence before starting Claude;
never print credentials, change billing or assume subscription headroom.
Fable availability and remaining allowance require a successful current
subscription request. Stop if unavailable or limited; ask Andreas for routing.
