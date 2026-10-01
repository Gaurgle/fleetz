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

Done and closed on 2026-10-01: FZ-02 (#15), FZ-01 (#14), FZ-05 (#18) and
issue #1. FZ-03 (#16) is Ready to integrate and waits for Andreas to try its header.
In review: FZ-06 (#19) and FZ-05a (#31). All are described below.
No release has started.

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

## FZ-01: Done, baseline merged

[performance-baseline.md](performance-baseline.md) is on `main`: ticket commit
`1cfd89c`, merge `b084d21`, #14 Done and closed. Two review passes, final
verdict accepted; the reviewer rebuilt case A from the document alone.

What later tickets need from it: first paint is 6 to 7 ms; first useful row
with a warm cache is 257 ms for 14 rows and 1148 ms for 70; a cold cache
costs about 6 s; with no cache and no network no rows show at all, local ones
included, because the load waits on the GitHub fetch before discovery; all
rows arrive in one event; a large startup spawns about 11 git processes per
local repo. Wall time moved 4 to 5% between two sessions at equal CPU, so a
later ticket compares a base and a candidate binary interleaved in one
session and never against the table. The scratch fixtures under the session
scratchpad may be gone; the document inlines every script.

## FZ-05: Done, design merged

[repository-identity-design.md](repository-identity-design.md) is on `main`:
ticket commit `100cb6e`, merge `ee65eb7`, #18 Done and closed. Three review
passes by `fz-reviewer`, final verdict accepted. Nothing was executed for it;
its last section lists the claims that need a test to confirm.

Findings: `github::merge_sources` dedupes the dataset by bare name, so of two
same-named repos the older-pushed one vanishes; the folder-name fallback in
`Wanted::match_clone` lets a fork or a foreign-host clone claim another
repo's row; the poll writes GitHub fields onto local-only rows by bare name.

Design, decided by the coordinator, Astra not consulted, Andreas may veto:
GitHub-owned fields keyed by remote identity derived from the existing
`owner` and `name`, sweep-owned fields by checkout path, exactly one row per
remote carrying the GitHub fields and marked by the load through `owner`.

Implementation tickets it defines, serial, created as issues on 2026-10-01
and on the board in Backlog (M1, P1): FZ-05a #31 (`model.rs`), b #32
(`discovery.rs` plus named `assemble_view` call sites), c #33 (test seam
around `assemble_view`, pure refactor), d #34 (assembly and poll in one
ticket), e #35 (`merge_sources`), f #36 (pass 3 by path only). Optional g,
h, i, j have no issues. b to f run after FZ-03 is integrated and serially
with FZ-04; only FZ-05a can run beside FZ-03.

FZ-05a (#31) is in progress: base `69df13300bbda2ee322e07065af61b9de7f71250`,
branch `feat/fz-05a-remote-ref`, worktree `.claude/worktrees/fz-05a`, worker
`fz-worker` (`claude-opus-5-5`, medium), allowed file `src/model.rs`. Next:
review, commit, push, combined checks, merge under the safe-merge rule, Done
and close. It needs nothing from Andreas.

Before those tickets run, Andreas must see two things, both in the document:
four `CLAUDE.md` invariant texts change (section 3.10), and FZ-05f changes
what pass 3 shows, which he may veto.

**Constraint on the series:** between the merge of FZ-05b and the merge of
FZ-05e a fleet with two same-named repos can lose a row it shows today,
including one with uncommitted work. In that span nothing is tagged and
Andreas is told in every report not to install from `main`. Lift it when
FZ-05e is integrated.

Its index row in `CLAUDE.md`, and rows for the two contracts, are added by
the coordinator after the FZ-01 branch merges, since that branch adds a row
to the same table.

## FZ-06: in progress

Dispatched on 2026-10-01 once FZ-01's timing runs were over.

- Base `6dd260f9a253dc2ba37b0efe6c1315fecd0dde60`, branch
  `docs/fz-06-cache-invalidation`, worktree `.claude/worktrees/fz-06`.
- Worker `fz-worker` (`claude-opus-5-5`, medium). Allowed file:
  `docs/cache-invalidation-design.md`. Production code is read-only; mutation
  experiments run in temporary repositories under the session scratchpad.
- State when last written: in Review. The document (untracked in the
  worktree) confirms eleven gaps where a gated product's answer changed and
  the `WorkKey` did not, the everyday `git push` among them, and adopts a
  ref-content signature built from the `for-each-ref` calls the sweep
  already makes. The coordinator's decisions OD1 to OD7 are recorded in it;
  none needs Andreas. `fz-reviewer` is repeating gap G1 from the document.
- Board: #19 Review, assignment on the issue.
- Next: on acceptance commit, push, merge, Done and close; then create the
  fix tickets FIX-A to FIX-E as issues. FIX-C (`src/git.rs` only) can run
  first, but not beside FZ-04.

## FZ-03: Ready to integrate, waiting for Andreas

Reviewed, committed and pushed, **not merged**: the header's wording and
look are Andreas's to accept on the branch build.

- Ticket commit `5217d30308be8aff5c8bf308c5c7215cf17d3254` on
  `fix/fz-03-github-freshness`, base `6dd260f`, worktree
  `.claude/worktrees/fz-03`: `README.md`, `src/app.rs`, `src/cache.rs`,
  `src/github.rs`, `src/main.rs`, `src/ui/mod.rs`, +1632 -148.
- Review: two passes, final verdict accepted. The first found that an `R`
  did not supersede an outstanding poll; fixed with regressions.
- CI green on the branch (`ci` and `docs`). Combined result with `main` at
  `0ab1890`: fmt, clippy with warnings denied, 688 tests passed, docs check.
  **Rerun the combined checks if `main` has gained source changes since**,
  which it will once FZ-05a merges.
- Board: #16 Ready to integrate, with the evidence, the try command and a
  table of how to see each header state.
- Try: `cargo run --release --manifest-path
  .claude/worktrees/fz-03/Cargo.toml`.
- When Andreas accepts: rerun the combined checks, `git merge --no-ff`,
  push, confirm CI on `main`, Done, close #16. If he wants wording or colour
  changes, they are in `ui::github_freshness_segment` and
  `ui::github_failure_label`; a change goes back through a worker and a
  re-review of that hunk.
- Until it is merged, nothing that touches `src/main.rs`, `src/app.rs`,
  `src/github.rs`, `src/cache.rs` or `src/ui/mod.rs` can start: FZ-04,
  FZ-05b to f and the FZ-06 fixes other than the common-directory helper.

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
FZ-04 follows FZ-03 and must not run beside it. Its per-repo contract is
drafted in [fetch-outcome-contract.md](fetch-outcome-contract.md), not yet
accepted. One finding in it was executed: a failed `git fetch` empties
`FETCH_HEAD` and advances its mtime, so `git::last_fetch_at` counts a failed
fetch as a fetch.

## Issue #1: Done

`dtolnay/rust-toolchain` is pinned to commit `6bed076` with `toolchain:
stable` in `ci.yml` and `release.yml`: ticket commit `1c3701a`, merge
`6dd260f`, CI green on the branch and on `main`, #1 Done and closed with a
summary. Andreas approved the CI change for that ticket only. The summary
names two things the issue's acceptance does not cover: the workflows have
no `pull_request` trigger, and no test loads a config file that does not
exist.

## Open items that are Andreas's

- Try FZ-02 on `main` (`cargo run --release`): M0's exit asks for his
  verification.
- Accept the FZ-03 header on its branch build when it reaches Ready to
  integrate.
- Before the FZ-05 implementation tickets: the four `CLAUDE.md` invariant
  changes and the veto on FZ-05f, both in the identity design.
- The `fz-02`, `fz-01`, `fz-05` and `issue-1` worktrees and their remote
  branches are integrated and kept. Removing them is his call.
- Not ticketed yet: the `repoz_parity` tests failing from a dirty checkout
  outside `$HOME`; overlapping local sweeps; the FZ-02 failed-poll test that
  also passes on the old code; a credential in an HTTPS remote URL being
  drawn in the detail pane (unconfirmed); the two gaps named under issue #1.

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
