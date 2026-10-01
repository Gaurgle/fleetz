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

Done and closed on 2026-10-01: FZ-02 (#15), FZ-01 (#14), FZ-05 (#18),
FZ-06 (#19), FZ-03 (#16), FZ-05a (#31), FZ-06a (#37), issue #1 and issue
#44. In progress: FZ-04 (#17, in review), #45 (in review), #42, FZ-10 (#23)
and FZ-09 (#22); the last three were interrupted and wait to be dispatched
again. All are described below.
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

FZ-05a (#31) is Done and closed: ticket commit `5972d50`, merge `8c1546c`,
`src/model.rs` only. `RemoteRef { host, owner, name }` and its parser exist;
`repo_from_remote_url` is built on it and returns what it always did (the
reviewer compared old and new over ten million inputs). The lowercased
comparison key moved to FZ-05b (#32), because clippy with warnings denied
rejects an uncalled method in a binary crate; #32 has a comment adding
`src/model.rs` to its files for that method.

Andreas signed off on 2026-10-01 ("its a yes from me"): the four `CLAUDE.md`
invariant changes in section 3.10, and FZ-05f's pass 3 change without a
veto. He was told plainly that nothing has been executed yet, that two cases
get slightly worse on purpose (a clone whose origin is the upstream, a
transferred repo) and that the series has a no-install window. The sign-off
is recorded on #32, #34 and #36. The coordinator edits the invariant text in
`CLAUDE.md` as the ticket that makes it true lands: change 1 with FZ-05b,
changes 3 and 4 with FZ-05d, change 2 with FZ-05f.

**Constraint on the series:** between the merge of FZ-05b and the merge of
FZ-05e a fleet with two same-named repos can lose a row it shows today,
including one with uncommitted work. In that span nothing is tagged and
Andreas is told in every report not to install from `main`. Lift it when
FZ-05e is integrated.

Its index row in `CLAUDE.md`, and rows for the two contracts, are added by
the coordinator after the FZ-01 branch merges, since that branch adds a row
to the same table.

## FZ-06: Done, design merged

[cache-invalidation-design.md](cache-invalidation-design.md) is on `main`:
ticket commit `f2f3aa7`, merge `ff56a0d`, #19 Done and closed. Two review
passes, final verdict accepted; the reviewer reran the documented
reproduction and found two gaps the worker had missed.

It confirms thirteen gaps where a gated product's answer changed and the
`WorkKey` did not (the everyday `git push` among them) and adopts a
ref-content signature built from the `for-each-ref` calls the sweep already
makes, widened to every ref namespace, plus the shallow file's content. The
coordinator's decisions OD1 to OD7 are in the document; none needs Andreas.

Fix tickets, created as issues on 2026-10-01 (Backlog, M1, P1), IDs in
landing order: FZ-06a #37 (FIX-C in the design, the common-directory helper,
`src/git.rs` only), FZ-06b #38 (FIX-A, ref-content signature), FZ-06c #39
(FIX-B, worktree freshness), FZ-06d #40 (FIX-D, histogram key), FZ-06e #41
(FIX-E, load-path order). b to e touch `src/main.rs` and wait for FZ-03.
None runs beside FZ-04, which will also own `src/git.rs`.

FZ-06a (#37) is Done and closed: ticket commit `520e77e`, merge `28fd17f`.
`git::repo_dirs` returns the git and common directories; `work_key` and
`last_fetch_at` use it; behavior unchanged. FZ-06b (#38) is next in this
series, after FZ-04.

## FZ-03: Done, integrated into `main`

Andreas tried the branch build on 2026-10-01 and accepted the colours, the
weight and the words `cached` and `offline`. He asked for one wording change:
the stale segment reads `⚠ github <kind>, as of 14:02`, not `data 14:02`.

- Commits `5217d30` (implementation) and `963f575` (the wording change,
  re-reviewed) on `fix/fz-03-github-freshness`; merge `407aa49` on `main`;
  CI green on the branch and on `main`; #16 Done and closed.
- Combined result before the merge: fmt, clippy with warnings denied, 704
  tests passed, docs check.
- GitHub freshness is now `App::github_freshness` (`GithubFreshness`: data
  time, last attempt, outcome, attempt ids). `fetch_repos` returns
  `Result<GithubFetch, FetchError>` where a cached fallback is an `Err` that
  carries data. A reload supersedes polls started before it lands.
- Left for later tickets: cancelling a hung request and a client timeout
  (FZ-11), per-row staleness (FZ-13), rows that drop out of a poll (FZ-09).

The main line is open again: tickets that touch `src/main.rs` run one at a
time. Order chosen by the coordinator: FZ-04 first (P0, and it needs Andreas
to look at the result, which is easiest while he is active), then FZ-06b,
e, c, d, then the FZ-05 series once he has signed off its invariant changes.

## FZ-04: in progress

Dispatched on 2026-10-01 under the standing instruction.

- Contract: [fetch-outcome-contract.md](fetch-outcome-contract.md), accepted
  by the coordinator (Astra not consulted, Andreas may veto any rule). One
  finding in it was executed: a failed `git fetch` empties `FETCH_HEAD` and
  advances its mtime, so `git::last_fetch_at` counts a failed fetch as a
  fetch.
- Branch `fix/fz-04-fetch-outcomes`, worktree `.claude/worktrees/fz-04`,
  worker `fz-worker` (`claude-opus-5-5`, medium). The base and the allowed
  files are in the assignment comment on #17.
- State when last written: implemented, four review passes so far. Pass one
  asked for two fixes; pass two found a credential leak in
  `git::redact_userinfo` (a password containing `,` `)` or `'` was drawn in
  full), fixed; pass three confirmed that and found a panic the fix
  introduced (R4: a multi-byte character directly before the scheme, for
  example a URL pasted with smart quotes, crashes the detail pane). The
  worker is fixing R4; then a final pass by the same reviewer. The change is
  uncommitted in the worktree. Board: #17 Review.
- Decisions taken in review are in the contract: fetch facts are a row
  field carried by path across a load; the drawn remote URL loses all
  userinfo except an ssh login name; the row badge says `unreachable`.
- Follow-ups from the review, not ticketed: the fetch tally's 180 second
  expiry is not extended by a later batch; one failing repo widens the
  compact grid's badge column; bidi characters are not stripped from the
  failure detail.
- It **stops at Ready to integrate**: how a failed fetch looks on a row and
  in the detail pane is Andreas's to accept on the branch build. Until it is
  merged nothing else that touches `src/main.rs`, `src/git.rs`, `src/app.rs`,
  `src/model.rs` or the status display can start.

## Usage limit hit on 2026-10-01, and what was interrupted

At about 17:30 the Claude session limit was reached with five agents in
flight (Andreas had asked for #42 and #45 on top of FZ-04's fix and the two
design drafts). Four `claude-opus-5-5` agents were cut off mid-run; the
limit reset at 18:50 and work resumed, at no more than two agents at a time.
What each left behind, checked on disk:

- FZ-04 worker (fix R4): nothing applied. `git::redact_userinfo` still had
  the byte-index scheme scan that panics on a multi-byte character before
  the scheme. Resumed with the same instruction.
- FZ-10 and FZ-09 workers: nothing written. Their worktrees are clean at
  base `6794962`. To be dispatched again, one at a time, after FZ-04.
- #42 worker: nothing in the worktree; a scratch reproduction clone exists
  under the session scratchpad. To be dispatched again.
- #45 worker (`fz-small`): finished before the limit. Its change is
  uncommitted in `.claude/worktrees/issue-45` and is with `fz-reviewer`.

No Fable routing change was made and no paid usage was used.

## FZ-10 and FZ-09: design drafts in progress

The last two M1 planning issues, both documentation only, dispatched on
2026-10-01 beside FZ-04 because they share no files with it.

- FZ-10 (#23), API budget and retry: base `67949620512b20419c0b71a9ed9b147f3eb1bdb2`,
  branch `docs/fz-10-api-budget`, worktree `.claude/worktrees/fz-10`, output
  `docs/api-budget-design.md`. At most 10 live GraphQL requests.
- FZ-09 (#22), coverage and cache isolation: same base, branch
  `docs/fz-09-coverage-cache-isolation`, worktree `.claude/worktrees/fz-09`,
  output `docs/coverage-cache-design.md`. At most 6 live requests; the real
  cache is read-only; no cache schema change is authorized.
- **Both were cut off by the usage limit before writing anything and must
  be dispatched again**, with the same handoff (the issue comments carry the
  base, branch, worktree and limits).
- Both: worker `fz-worker` (`claude-opus-5-5`, medium) maps and drafts with
  options; the coordinator decides the architecture; anything that changes a
  cache format, a config key or what the user sees goes to Andreas. Then
  `fz-reviewer`, commit, push, merge, Done and close.
- Board: #23 and #22 In progress, assignments on the issues.

## FZ-08: reshaped, not started

On 2026-10-01 Andreas asked for FZ-08 (#21) to be reshaped into a read-only
GitHub Project board view inside fleetz: columns and table, with status,
priority, milestone, worker and ticket reviewer per item; `Enter` opens a
ticket's details (body, comments, fields, labels, linked PRs, dependencies)
in the inspector with an expand to the full pane; summary fields for the
list, body and comments fetched on open ("Enrich-on-select" in `ideas.md`);
editing through the `gh` CLI, the browser only as a fallback; a slower poll
than repo state. **He said not to start it.**

The issue body is rewritten with the scope, the measured GraphQL cost (every
query costs the floor of one point; field values through `fieldValueByName`
keep the list at 100 nodes), six proposed tickets and the dependencies. The
placement he agreed to ("sounds good to me"): the design ticket right after
M1 alongside M2, data and the table as the first item of M4, columns and
ticket details after M3's pane work, the `gh` handoffs after FZ-16. Priority
stays P2. Left open for the design ticket: whether setting Status through
`gh` is in the first version. No sub-issues are created and nothing is
dispatched.

## Freshness contract for FZ-03: accepted

[freshness-contract.md](freshness-contract.md) is the contract FZ-03
implemented. Andreas accepted it on 2026-10-01: D1 (header information
content) and D2 (a cache-served launch polls once even with watch off)
himself, D3 to D6 by the coordinator's recommendations. It defines attempt,
outcome and data time as three separate facts, the transitions and what the
header must say. Its example strings predate the build; the code and the
README have the wording he accepted. Astra has not reviewed it; a
disagreement goes to Andreas, not into the code.

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
  verification. He has run the FZ-03 branch build, which contains it.
- Accept the FZ-04 failed-fetch display on its branch build when it reaches
  Ready to integrate.
- The `fz-02`, `fz-01`, `fz-03`, `fz-05`, `fz-05a`, `fz-06`, `fz-06a`,
  `issue-1` and `issue-44` worktrees and their remote branches are
  integrated and kept. Removing them is his call.
- Follow-ups ticketed on 2026-10-01 (Backlog, P2): #42 `repoz_parity` tests
  fail from a dirty checkout outside `$HOME` (edits an existing test's
  harness, needs his OK); #43 the local contribution count includes stash
  commits; #44 a test for a missing config file; #45 CI on pull requests
  (a CI change, needs his approval); #46 the FZ-02 failed-poll test also
  passes on the old code (edits an existing test, needs his OK). Routed to
  planning issues as comments instead: overlapping sweeps and the reload
  race (#24), the drawn remote URL and `FETCH_HEAD` findings (#17), coverage
  (#22), the baseline's findings (#25).
- #44 is Done and closed: ticket commit `3b38c49`, merge `375bc7a`. The loop
  in `Config::load()` is extracted into `load_from_candidates` so the
  fallback is testable. A test pins, without endorsing it, that an invalid
  config file is silently replaced by defaults; noted on #2.

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
