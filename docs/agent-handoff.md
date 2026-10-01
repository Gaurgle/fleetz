# Fleetz coordinator handoff

Last written 2026-10-01 19:40 by `claude-fable-5-1`, on Andreas's instruction
to pause at a safe point, dispatch nothing new, record the full state and
stop. **Nothing is running: no worker, no reviewer, no background job.** Read this file, the repository
instructions, [agent-workflow.md](agent-workflow.md),
[release-plan.md](release-plan.md) and
[first-wave-tickets.md](first-wave-tickets.md) before acting. GitHub (the
issues and the [release Project](https://github.com/users/Gaurgle/projects/1))
is authoritative for status; this file says why things are where they are and
what to do next.

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

Three things Andreas said on 2026-10-01 that a new lead must not assume carry
over. Each was said to one session; confirm it once before relying on it.

- **Standing instruction**, in his words: "i dont want to have to tell you
  what to run. we have a harness and the reviewers, keep on going as planned
  as long as I dont have to make a design decition or similar." Under it the
  coordinator dispatched the next ticket in plan order when its dependencies
  held, closed issues at Done, created issues for defined tickets and
  follow-ups, and went to him only for design decisions, interaction or look
  acceptance, and what the safe-merge rule reserves for him.
  `agent-workflow.md` still says to dispatch only what he names.
- **Coordination docs** may be committed and pushed to `main` without asking,
  for that session. A Claude session's permission check can refuse such a
  commit otherwise.
- **CI changes** were approved ticket by ticket (#1, #45), never in general.

Do not automatically switch to API billing or paid usage credits. Stop at a
usage limit and ask Andreas for routing.

## What needs Andreas

Nothing else moves on the `main.rs` line until the first item is settled.

1. **FZ-04 (#17): a second look at the branch build**, once the fix below is
   reviewed and committed. He tested it twice on 2026-10-01 and has not
   accepted it.
2. **#45: a repository setting.** Fork pull requests run no workflow while
   the repository is private unless Settings > Actions > "Run workflows from
   fork pull requests" is on. Turn it on, or leave it until the repository is
   public; and say whether the YAML merges now as preparation. The
   coordinator's recommendation: leave it off and merge the YAML now.
3. **Was Wi-Fi off or just toggled** when he saw `⚠ timeout` on three rows
   in his first FZ-04 run? Asked twice, not answered.
4. **FZ-10 owner items**, to be presented together once the design review is
   through: what the header says when polling is slowed, paused or rate
   limited (O1); manual `r` and `R` under a rate limit skipping the request
   (O2 with A6); no new config key (O3); and amending freshness contract
   rule 6 from "one normal interval later" to "at least one normal interval
   later".
5. **Try `main`** (`cargo run --release`). M0's exit asks for his
   verification of FZ-02; he has run the FZ-03 and FZ-04 branch builds, which
   contain it, but has not said so about `main`.
6. Not blocking: whether setting Status through `gh` belongs in FZ-08's
   first version; whether #46 may change an existing test's setup order.

## Last verdicts, and what is on disk uncommitted

Both reviews that were running when the pause was asked for have reported.
Nothing was dispatched or integrated after them.

- **FZ-04 fix: accepted by the reviewer, not committed.** Worktree
  `.claude/worktrees/fz-04` has one uncommitted change in `src/git.rs` on
  top of `4839891`: `classify_fetch_error` treats any line starting
  `ssh: connect to host ` or `ssh: could not resolve hostname ` as
  `Unreachable` (Auth still wins when both appear), and `first_error_line`
  prefers an `ssh: ` line that comes before the first `fatal:` or `error:`
  line. Worker and reviewer both ran fmt, clippy with warnings denied and
  the suite: 754 passed, 0 failed, 1 ignored. The reviewer confirmed with a
  copy of the old function that Andreas's pasted text was `Other` before
  and is `Unreachable` now. Verdict and details are on #17.
- **FZ-10 design: changes requested, nothing blocking.** Worktree
  `.claude/worktrees/fz-10` has the untracked `docs/api-budget-design.md`
  with decisions A1 to A7 recorded. The reviewer confirmed the measurements
  (cost 3 and 1030 nodes at `repo_limit` 50; its own run took about 7.3 s
  where the draft says 6.3 s) and asked for seven changes: FZ-10a's file
  ownership is too narrow and it must be serial after FZ-04; the contract
  changes are understated (rule 9, rule 1 through A6, new failure kinds, the
  client timeout moving from FZ-11); scenario S20 contradicts S3; four
  unspecified transitions; server epoch against the local clock; overclaimed
  wording about the server deadline; user-visible effects missing from the
  owner section. **The findings are recorded in full on #23**, because the
  worker's session will not be resumable.
- **#45:** uncommitted `ci.yml` and `docs.yml` in
  `.claude/worktrees/issue-45`; reviewer verdict blocked on a repository
  setting, diff judged correct. Unchanged since.
- **FZ-09:** worktree `.claude/worktrees/fz-09` clean, nothing written.

## Next steps, in order

1. **The next authorized step.** FZ-04: commit the accepted hunk
   (`src/git.rs` only) on `fix/fz-04-fetch-outcomes` as a second commit,
   push, wait for CI on the branch, rerun fmt, clippy
   with warnings denied, the full suite and `scripts/check-docs.sh` on the
   combined result with current `main` in a throwaway clone, then give
   Andreas the build:
   `cargo run --release --manifest-path ~/repos/fleetz/.claude/worktrees/fz-04/Cargo.toml`.
   With Wi-Fi off and `F` he should see `⚠ unreachable` on rows,
   `⚠ fetch failed: remote unreachable, as of HH:MM` in the detail pane with
   the ssh line under it, and a count in the status line. Keep every agent
   idle while he tests. Merge only after he accepts; then Done, close #17.
2. FZ-10: dispatch a fresh `fz-worker` to revise the draft against the
   findings on #23 (the original worker's context is gone), then have a
   reviewer re-review. On acceptance commit, push, merge the document, add
   its row to the `CLAUDE.md` index, move #23 to Done and close it, and
   create issues FZ-10a, b and c from its ticket section. Bring the owner
   items to Andreas in plain language: they now include the effect on rule 9
   and on A6, not only rule 6.
3. #45: act on his decision. If the YAML merges, it needs a reviewer
   acceptance first: the verdict so far is blocked on the setting, with the
   diff judged correct.
4. FZ-09 (#22): dispatch again. Nothing was written. Handoff as recorded on
   the issue: output `docs/coverage-cache-design.md`, at most 6 live GraphQL
   requests, the real cache read-only, no cache schema change authorized.
   Add what FZ-10 found: `repo_limit` 100 fails with a 502 at GitHub's server
   deadline, so "what to do when the query no longer fits" is FZ-09's to
   design.
5. After FZ-04 merges, the `main.rs` line, one ticket at a time:
   FZ-06b (#38), FZ-06e (#41), FZ-06c (#39), FZ-06d (#40); then the identity
   series FZ-05b to f (#32 to #36); then FZ-10a, b, c. #43 (`git.rs`) fits
   between any two. #46 needs Andreas's OK first.

## The FZ-04 story, because it is not finished

FZ-04 makes a failed `git fetch` visible. Contract:
[fetch-outcome-contract.md](fetch-outcome-contract.md), accepted by the
coordinator and amended during review. Ticket commit
`48398919d8d97afbd23c05e8481130ae8b1fef9c` on `fix/fz-04-fetch-outcomes`
(base `3ca7ae5`, nine files, +1682 -47), pushed, CI green on the branch,
combined result with `main` at `4fc5ff7` passing 753 tests. Not merged.

Five review passes found and fixed: the `ssh://` login name being stripped
from the drawn URL, git's real "repository not found" text not recognised, a
password containing `,` `)` or `'` reaching the screen, and a panic on a
multi-byte character before the URL scheme (a remote pasted with smart
quotes). Fetch facts are a row field (`RepoStatus.fetch`) owned by the fetch
alone and carried by path across a load.

Andreas's own testing then found what review had not:

- With Wi-Fi off and `f` on `fleetz` (remote `git@github.com:...`), the row
  showed `⚠ fetch` and the detail `fetch failed: git error` over git's
  generic `fatal: Could not read from remote repository.`. His fetch by hand
  printed `ssh: connect to host github.com port 22: Undefined error: 0`,
  which no list of error texts contained. The fix is described above:
  reviewed and accepted, uncommitted.
- In an earlier run he saw `⚠ timeout` on three rows. Not reproduced: the
  same hardened fetch on 44 repos outside the app took under 9 s each, and
  two background runs of the build (one and five minutes) showed every fetch
  completing in about a second. Reproduced separately: ssh to an address it
  cannot reach hangs for over a minute with no output, fleetz's 30 s limit
  then reports a timeout, and the ssh child outlives the killed git. So the
  three rows were most likely SSH remotes while the network was down or just
  toggled. Unconfirmed until he answers.
- Only one row was marked in his screenshot because `f` fetches the selected
  repo; `F` fetches all.

The probe clone `~/repos/fz-probe` he made for the test was removed at his
request after checking it held nothing of its own.

## Done on 2026-10-01

All merged into `main` with `git merge --no-ff`, CI green on `main`, issues
closed with the evidence in a comment.

| Ticket | Issue | Ticket commit | Merge | What landed |
|---|---|---|---|---|
| FZ-02 | #15 | `8027d3b` | `28ebba7` | Sweep and poll each write only the fields they own |
| Pin | #1 | `1c3701a` | `6dd260f` | `dtolnay/rust-toolchain` pinned to a commit in `ci.yml` and `release.yml` |
| FZ-01 | #14 | `1cfd89c` | `b084d21` | [performance-baseline.md](performance-baseline.md) |
| FZ-05 | #18 | `100cb6e` | `ee65eb7` | [repository-identity-design.md](repository-identity-design.md) |
| FZ-06 | #19 | `f2f3aa7` | `ff56a0d` | [cache-invalidation-design.md](cache-invalidation-design.md) |
| FZ-05a | #31 | `5972d50` | `8c1546c` | `RemoteRef { host, owner, name }` in `model.rs`; the comparison key moved to FZ-05b |
| Tests | #44 | `3b38c49` | `375bc7a` | `Config::load_from_candidates` seam and missing-config tests |
| FZ-06a | #37 | `520e77e` | `28fd17f` | `git::repo_dirs`, the git and common directories in one helper |
| FZ-03 | #16 | `5217d30`, `963f575` | `407aa49` | GitHub freshness as three facts; header `github cached` and `⚠ github <kind>, as of HH:MM` |
| Tests | #42 | `497ee62` | `d564fc7` | repoz parity tests run repoz from outside any repository |

Facts from these that later tickets need:

- **Baseline (FZ-01).** First paint 6 to 7 ms; first useful row with a warm
  cache 257 ms for 14 rows and 1148 ms for 70; a cold cache costs about 6 s;
  no cache and no network shows no rows at all; all rows arrive in one
  event; about 11 git processes per local repo at startup. Wall time moved 4
  to 5% between two sessions at equal CPU, so compare a base and a candidate
  binary interleaved in one session, never against the table.
- **Freshness (FZ-03).** `App::github_freshness` holds data time, last
  attempt and outcome with attempt ids. `fetch_repos` returns
  `Result<GithubFetch, FetchError>` and a cached fallback is an `Err` that
  carries data. A reload supersedes polls started before it lands. Andreas
  accepted the header's colours, weight and the words `cached` and
  `offline`, and chose `as of` over `data`.
- **Identity (FZ-05).** GitHub-owned fields keyed by owner and name,
  sweep-owned fields by checkout path, one row per remote carrying the
  GitHub fields and marked by the load through `owner`. Andreas signed off
  on 2026-10-01 ("its a yes from me") on the four `CLAUDE.md` invariant
  changes in section 3.10 and on FZ-05f without a veto; recorded on #32,
  #34 and #36. The coordinator edits the invariant text as each ticket
  lands: change 1 with FZ-05b, 3 and 4 with FZ-05d, 2 with FZ-05f.
  **Between the merges of FZ-05b and FZ-05e nothing is tagged and Andreas is
  told in every report not to install from `main`**: a fleet with two
  same-named repos can lose a row in that span.
- **Cache invalidation (FZ-06).** Thirteen confirmed gaps where a gated
  product changed and the `WorkKey` did not, the everyday `git push` among
  them. Adopted: a ref-content signature from the `for-each-ref` calls the
  sweep already makes, one widened to every ref namespace, plus the shallow
  file's content; every key input read before the probes it gates.
- **Config (#44).** An invalid config file is silently replaced by
  defaults; pinned by a test as current behavior, noted on #2.

## Tickets that exist and wait

On the board in Backlog, milestone M1 unless noted. None dispatched.

- **Identity series:** FZ-05b #32 (`discovery.rs` plus named
  `assemble_view` call sites, and the comparison key in `model.rs`), FZ-05c
  #33 (test seam, pure refactor), FZ-05d #34 (assembly and poll, the largest;
  its reviewer traces both), FZ-05e #35 (`merge_sources`), FZ-05f #36 (pass 3
  by path only). Serial, in that order.
- **Cache fixes:** FZ-06b #38 (ref signature), FZ-06c #39 (worktree
  freshness, after a and b), FZ-06d #40 (histogram key, after b), FZ-06e #41
  (load-path order). All touch `src/main.rs`.
- **Follow-ups (P2):** #43 local contribution count includes stash commits;
  #46 the FZ-02 failed-poll test also passes on the old code (edits an
  existing test, needs Andreas's OK).
- **#45** (M5): described above.
- **FZ-09 #22** is Ready, to be dispatched again. **FZ-10 #23** is in Review
  with changes requested.
- **FZ-08 #21** (M4) was reshaped at Andreas's request into a read-only
  GitHub Project board view: columns and table with status, priority,
  milestone, worker and ticket reviewer; `Enter` opens a ticket's details in
  the inspector with an expand to the full pane; summary fields for the
  list, body and comments fetched on open; editing through `gh`, the browser
  as a fallback; its own slower poll. Measured: every query costs one point,
  and `fieldValueByName` keeps the list at 100 nodes. He agreed to the
  placement: design ticket right after M1 alongside M2, data and the table
  first in M4, columns and details after M3's pane work, `gh` handoffs after
  FZ-16. **He said not to start it.**
- Planning issues with inputs added as comments today: #17, #22, #24, #25,
  #2.

## Contracts and designs

| Document | Status |
|---|---|
| [freshness-contract.md](freshness-contract.md) | Accepted by Andreas, implemented by FZ-03. Its example strings predate the build; the code and README have the accepted wording |
| [fetch-outcome-contract.md](fetch-outcome-contract.md) | Accepted by the coordinator, amended in review; Andreas may veto; the look is his on the FZ-04 build |
| [repository-identity-design.md](repository-identity-design.md) | Accepted; Andreas signed off on the invariant changes |
| [cache-invalidation-design.md](cache-invalidation-design.md) | Accepted by the coordinator; nothing in it needs Andreas |
| [performance-baseline.md](performance-baseline.md) | Measured baseline |
| `docs/api-budget-design.md` | Draft in the `fz-10` worktree, in review, not on `main` |

Astra reviewed none of these. A disagreement goes to Andreas, not into the
code.

## How this session ran, and what it cost

- **The usage limit was hit** at about 17:30 with five agents in flight,
  after Andreas asked for #42 and #45 on top of three running agents. Four
  `claude-opus-5-5` agents died mid-run and three had written nothing. The
  limit reset at 18:50. No paid usage and no model switch. Keep to two
  agents at a time; a design draft costs a few hundred thousand tokens.
- **Workers were cut off, not wrong.** A failed agent can be resumed with
  its context intact; check its worktree first and tell it what is on disk.
- **A reviewer can be continued** for a narrow re-review of its own
  findings. Use a fresh one for a first review.
- **Reviews earned their cost.** Every substantive ticket changed in
  review, and three reviewer findings were against the coordinator's own
  rulings (the poll-before-assembly order in FZ-05, the working directory in
  #42, a redaction tweak in FZ-04 that opened a leak).
- **The owner's test found what five review passes did not**: a real
  environment's error text. Give him the build as early as the safe-merge
  rule allows on tickets whose behavior depends on the machine.

## Practical notes for the next lead

- **Use absolute paths and `git -C`; do not `cd` in the shell.** A `cd` into
  a worktree moved the session's working directory and a worker's with it.
- **Split long commands.** A compound commit, push, merge and wait was
  declined by the permission check twice; the same steps one at a time went
  through.
- **`scripts/check-docs.sh` only scans tracked files.** Stage a new document
  before running it, or apply its dash and link checks by hand.
- **Combined checks run in a throwaway clone** under the session scratchpad,
  with the merge committed there so the tree is clean, and with
  `CARGO_TARGET_DIR` pointed at one shared target directory.
- **Nothing else may build while someone measures or tests.** Say so in the
  handoff of any agent that runs beside a measurement, and keep agents idle
  while Andreas tries a build.
- **Board edits:** read item ids with
  `gh project item-list 1 --owner Gaurgle --limit 100 --format json --jq ...`
  directly; piping the JSON through `echo` breaks on control characters.
  Project `PVT_kwHOCU842c4BlQ1e`. Status field
  `PVTSSF_lAHOCU842c4BlQ1ezhj-XiE`: Backlog `34f9165b`, Ready `79ac8034`,
  In progress `a2e70ebb`, Review `40798e53`, Ready to integrate `d28a1321`,
  Done `5114f2e5`. Priority field `PVTSSF_lAHOCU842c4BlQ1ezhj-ZPA`: P0
  `64e82877`, P1 `63d976f2`, P2 `be37b935`. Worker field
  `PVTF_lAHOCU842c4BlQ1ezhj-ZPE`, Ticket reviewer field
  `PVTF_lAHOCU842c4BlQ1ezhj-ZPI`.
- **GitHub budget:** the account's GraphQL usage was high today with up to
  three fleetz instances running. Cap live requests in every handoff.

## Worktrees and branches

All under `.claude/worktrees/` in the main checkout. Removing a worktree or
a remote branch is Andreas's call; none was removed.

- **Active:** `fz-04` (commit `4839891` plus one uncommitted change in
  `src/git.rs`), `fz-10` (untracked `docs/api-budget-design.md`), `fz-09`
  (clean, nothing written), `issue-45` (uncommitted `ci.yml` and `docs.yml`).
- **Integrated and kept:** `fz-01`, `fz-02`, `fz-03`, `fz-05`, `fz-05a`,
  `fz-06`, `fz-06a`, `issue-1`, `issue-42`, `issue-44`, each with its branch
  pushed.
- Not pushed: `docs/fz-09-coverage-cache-isolation`, `docs/fz-10-api-budget`,
  `ci/45-pull-request-trigger`.
- The session scratchpad holds throwaway integration clones and diagnostic
  captures. Nothing there is needed.

## Follow-ups found and not ticketed

Creating issues was done under the standing instruction; a new lead confirms
before creating more.

- **ssh for fetches:** an unreachable SSH remote hangs to the 30 s limit and
  leaves an ssh child running, and ssh can still prompt on the TUI's
  terminal (`GIT_TERMINAL_PROMPT=0` does not stop ssh). Candidate, from the
  FZ-04 reviewer: pass `-o ConnectTimeout=10 -o BatchMode=yes`, but only
  when none of `GIT_SSH_COMMAND`, `GIT_SSH` or `core.sshCommand` is set,
  since overriding those would drop a user's key or agent wrapper. Its own
  ticket. The same reviewer noted the failure detail prefers any `ssh: `
  line while the kind looks only at connect and resolve lines.
- **From the FZ-04 review:** the fetch tally's 180 s expiry is not extended
  by a later batch; one failing repo widens the compact grid's badge column;
  old fetch facts stay on a path that now holds a different repo; bidi
  characters are not stripped from the failure detail.
- **A row for a checkout that was deleted** keeps showing `✓` beside
  `⚠ fetch` until a reload; seen after the probe clone was removed.
- **`CLAUDE.md` text that the code contradicts:** the tick is 100 ms, not
  250 ms; `r` does not rebuild from cache; it still names
  `last_github_poll` and `GithubFetch::from_cache`, which FZ-03 replaced; "the owner is part of the key"
  and "reports the undotted name" (both corrected by the identity series);
  `cache_ttl` decides nothing observable (a question for FZ-09).
- **Tests:** the real-repo tests in `src/git.rs` inherit the machine's
  global git config; fixed temp directory names can collide between two
  `cargo test` processes; the parity tests still assume the system temp
  directory is not inside a repository.
- **From FZ-03:** cancelling a hung request and a client timeout (FZ-10 and
  FZ-11), per-row staleness (FZ-13), rows that drop out of a poll (FZ-09).
- **From #45's review:** a skipped job reports as Success, and both
  workflows name their job `check`; matters if required checks are ever
  added.
- **From FZ-05:** matching through a non-origin remote (FZ-05j in the
  design, optional); the local calendar double-counts an extra checkout.

## Start Fable manually

From this repository, run:

```sh
claude --agent fz-coordinator
```

The agent definition pins the model and effort and carries the takeover
instructions: it reads this file, inspects the live board and working tree,
and reports the next step before any dispatch.

Use normal permissions with no bypass. Claude subscription Max authentication
was verified through macOS keychain outside the sandbox, with no active
API-key, auth-token or third-party provider overrides. Recheck status and
override presence before starting Claude; never print credentials, change
billing or assume subscription headroom. Fable availability and remaining
allowance require a successful current subscription request. Stop if
unavailable or limited; ask Andreas for routing.
