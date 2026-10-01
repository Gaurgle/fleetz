# Fleetz coordinator handoff

Last written 2026-10-01 23:20 by `claude-fable-5-1`. Merged today in this
session: FZ-04, FZ-06b, FZ-06c and the FZ-10 design; FZ-06e closed as
delivered by FZ-06b. **One worker is running: the FZ-09 (#22) design draft in
`.claude/worktrees/fz-09`, not yet reported. FZ-06d (#40) is stopped, blocked
on a decision from Andreas about test builders.** Read this file, the repository
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
  `agent-workflow.md` still says to dispatch only what he names. In the
  19:34 session he renewed it for the tickets below with "if this is normal,
  we can push? and get along with the rest", after which FZ-04 was merged
  and FZ-06b and the FZ-10 revision were dispatched in plan order.
- **Coordination docs** may be committed and pushed to `main` without asking,
  for that session. A Claude session's permission check can refuse such a
  commit otherwise.
- **CI changes** were approved ticket by ticket (#1, #45), never in general.

Two rules he gave the 19:34 session that do carry over:

- **Existing tests.** A ticket that cannot compile or pass without editing an
  existing test stops and the question goes to him, even when the edit is
  mechanical. He said yes for FZ-06b (three FZ-06a tests and `tests::key`)
  and for FZ-10a (four test builders, O5). Tell every worker to look for
  forced test edits before writing the fix, and every design worker to name
  them per ticket.
- **Where his testing matters.** At 23:30, in his words: "these 'small'
  tickets like these cache fixes, coverage etc, are not much for me to test
  in the actual software, i think you're tests tell a bigger and more true
  picture of the situation. I think my opinions will matter more when we
  start to look at the UI updates". So: do not ask him to try a build for a
  ticket with nothing to see (cache, identity, budget policy, coverage
  plumbing); the reviewer's run, the combined checks and CI are the evidence,
  and reports for those stay short. Do bring him every ticket that changes
  what is on screen or how a key behaves, with the branch build, before it
  merges: FZ-10c is the next one, then M3. Design decisions, contract
  changes and edits to existing tests still go to him.
- **Writing files.** File contents are written with the Write or Edit tool,
  never with shell heredocs, `echo` or `printf` redirection, or an inline
  script. Text a command needs goes to a scratchpad file and is passed with
  `--body-file`. It is in `.claude/agents/fz-coordinator.md`.

Do not automatically switch to API billing or paid usage credits. Stop at a
usage limit and ask Andreas for routing.

## What needs Andreas

In order of weight:

1. **Nothing blocks the running work.** He accepted the FZ-10 owner items O1
   to O5 as recommended at 22:55 ("ok, all sound good to me") and allowed
   the issues FZ-10a #47, FZ-10b #48 and FZ-10c #49, which now exist in
   Backlog. O2 and O4 are amendments 1 to 6 in
   [freshness-contract.md](freshness-contract.md). O1's words and look are
   still his on the FZ-10c branch build.
2. **May the unticketed follow-ups below become issues?** Not asked
   separately: his yes covered FZ-10a to c. Ask before creating more.
3. **Should a recovered network clear fetch marks sooner?** Today a failed
   fetch stays marked until that repo is fetched again (`f`, `F`, a moved
   remote, or the watch sweep every `refresh_interval`, 300 s in his config).
   The alternative, refetching the repos that failed as unreachable or timed
   out when the GitHub poll recovers, changes fetch contract rule 6 and is
   its own ticket. Offered at 19:55; he merged without choosing it.
4. **Was Wi-Fi off or changing state** when he saw `⚠ timeout` on three rows
   in his first FZ-04 run? Still unanswered; it only matters for the ssh
   follow-up below.
5. **Try `main`**: no longer asked of him for these tickets (see "Where his
   testing matters" above). `main` holds FZ-02, FZ-03, FZ-04, FZ-06b and
   FZ-06c. For the record, M0's exit asks for his verification of FZ-02; he has run the
   branch builds that contain it but has not said so about `main`.
6. **#45: decided.** Leave it until the repository goes public.
7. Not blocking: whether setting Status through `gh` belongs in FZ-08's
   first version; whether #46 may change an existing test's setup order.

## Last verdicts, and what is on disk uncommitted

- **FZ-06d (#40): worker stopped at 23:35, blocked on Andreas.** Base
  `24b2210`, branch `fix/fz-06d-histogram-reuse-key`, worktree
  `.claude/worktrees/fz-06d`. The new field next to `daily` on `RepoStatus`
  forces one `None` line into six existing test builders (`model.rs`
  `make_repo`, `app.rs` `make_app`, `main.rs` `test_repo`, and `repo` in
  `ui/badges.rs`, `ui/compact.rs`, `ui/detail.rs`), four of them outside the
  ticket's files; no assertion changes. Asked at 23:37, together with the
  standing form: may a compile-forced new field in a test builder, with no
  assertion changed and the reviewer confirming it, go ahead without asking
  each time. On disk, uncommitted: `src/main.rs`, five new tests, E7 and E14
  failing on the base by design (789 passed, 2 failed, 1 ignored). Details
  on #40. On a yes: resume the same worker with the six builders allowed,
  then a separate `fz-reviewer`, commit, push, CI, combined checks, merge.
- **FZ-09 (#22): worker drafting since 22:35.** Worktree
  `.claude/worktrees/fz-09` fast-forwarded to `769c1fd`; output
  `docs/coverage-cache-design.md`, untracked; at most 6 live GraphQL
  requests, the real cache read-only, no builds. It was told to leave the
  architectural options to the coordinator and to name every existing test
  a proposed ticket would force to change. Next: the coordinator rules on
  the options, then a reviewer. Its worktree is at `769c1fd`, before the
  freshness contract was amended (`702e546`): tell it, or its reviewer, that
  amendments 1 to 6 exist.
- **#45:** uncommitted `ci.yml` and `docs.yml` in
  `.claude/worktrees/issue-45`; reviewer verdict blocked on a repository
  setting, diff judged correct. Parked by Andreas until the repository is
  public; the files stay where they are.

## Next steps, in order

1. FZ-06d: review, integrate and close as above. It is the last cache fix.
2. Then the identity series FZ-05b to f (#32 to #36), serial, one worker at
   a time; then FZ-10a, b, c (#47 to #49). #43 (`git.rs`) fits between any
   two. #46 needs Andreas's OK first. **From the merge of FZ-05b until the
   merge of FZ-05e, every report tells Andreas not to install from `main`
   and nothing is tagged** (see Identity under "Facts" below). The
   coordinator edits the `CLAUDE.md` invariant text as each identity ticket
   lands: change 1 with FZ-05b, 3 and 4 with FZ-05d, 2 with FZ-05f.
3. FZ-09 (#22): when the worker reports, rule on its options, send the
   draft to a reviewer, and land it as FZ-10 was landed. Any cache format
   change, config key or visible change in it goes to Andreas.
4. #45: nothing until the repository goes public.

## The FZ-04 story, for what it left behind

FZ-04 (#17, closed) makes a failed `git fetch` visible. Contract:
[fetch-outcome-contract.md](fetch-outcome-contract.md). Five review passes
fixed four defects, and Andreas's own test with Wi-Fi off then found a fifth
that no review had: macOS ssh printing
`ssh: connect to host github.com port 22: Undefined error: 0`, which
classified as a plain git error. The second commit `3469f67` fixed it.

What a later lead needs from it:

- **One marked row is normal after `f`.** `f` fetches the selected repo
  only; a full sweep (`F` or the watch timer) fetches every clone and marks
  each row whose fetch fails. He asked about this twice.
- **A mark outlives the outage.** Nothing retries because the network came
  back (rule 6). In his run the mark set at 19:51 cleared by itself at the
  19:56 watch sweep.
- **The earlier `⚠ timeout` rows are unexplained but understood.** ssh to an
  address it cannot reach hangs past fleetz's 30 s limit and the ssh child
  outlives the killed git. With Wi-Fi fully off ssh fails at once instead.
  The ssh follow-up below is the fix.
- **Commit `3469f67` has no recorded author session.** It appeared at
  19:33:51, after the 19:31 handoff called the change uncommitted and
  before the 19:34 session started; Andreas says he did not make it. Its
  content is the file as last modified at 19:25:40, before the reviewer's
  acceptance was recorded, so it was merged. If a session other than the
  one holding this file is committing, find out which before dispatching.

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
| FZ-06b | #38 | `e33914d` | `f3663e6` | `WorkKey` hashes every ref name and object id plus the shallow file; key inputs read before the probes on both paths |
| FZ-06e | #41 | `e33914d` (FZ-06b) | `f3663e6` | Closed without a dispatch: FZ-06b reordered `build_repo_status` and fixed its comment |
| FZ-10 | #23 | `9494938` | `f627a84` | [api-budget-design.md](api-budget-design.md), decisions A1 to A7, tickets FZ-10a to c (#47 to #49), owner items accepted |
| FZ-06c | #39 | `5c83fb0` | `24b2210` | The worktree list is read on every sweep when the common directory has a `worktrees` folder; the branch-delete overlay sweeps its repo |
| FZ-04 | #17 | `4839891`, `3469f67` | `f8a54c0` | Fetch outcomes as three facts per checkout; `⚠ unreachable` and its kin on the row, the reason in the detail pane, no credential drawn |

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

On the board in Backlog, milestone M1 unless noted. None dispatched except
FZ-06d and FZ-09, which are In progress.

- **Identity series:** FZ-05b #32 (`discovery.rs` plus named
  `assemble_view` call sites, and the comparison key in `model.rs`), FZ-05c
  #33 (test seam, pure refactor), FZ-05d #34 (assembly and poll, the largest;
  its reviewer traces both), FZ-05e #35 (`merge_sources`), FZ-05f #36 (pass 3
  by path only). Serial, in that order.
- **Cache fixes:** FZ-06d #40 (histogram key, running) is the last one.
  FZ-06a, b, c and e are done.
- **API budget:** FZ-10a #47 (`github.rs`, after FZ-05e), FZ-10b #48
  (`app.rs`, `main.rs`), FZ-10c #49 (`ui/mod.rs`, README; stops at Ready to
  integrate for Andreas's look). Serial, after the identity series.
- **Follow-ups (P2):** #43 local contribution count includes stash commits;
  #46 the FZ-02 failed-poll test also passes on the old code (edits an
  existing test, needs Andreas's OK).
- **#45** (M5): described above.
- **FZ-09 #22** is In progress: the design draft.
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
| [freshness-contract.md](freshness-contract.md) | Accepted by Andreas, implemented by FZ-03. Its example strings predate the build; the code and README have the accepted wording. Amended on 2026-10-01 by his acceptance of FZ-10's O2 and O4: six amendments, each in force when its FZ-10 ticket lands |
| [fetch-outcome-contract.md](fetch-outcome-contract.md) | Accepted by the coordinator, amended in review; Andreas may veto; the look is his on the FZ-04 build |
| [repository-identity-design.md](repository-identity-design.md) | Accepted; Andreas signed off on the invariant changes |
| [cache-invalidation-design.md](cache-invalidation-design.md) | Accepted by the coordinator; nothing in it needs Andreas |
| [performance-baseline.md](performance-baseline.md) | Measured baseline |
| [api-budget-design.md](api-budget-design.md) | On `main`. Accepted by the reviewer after four revisions; decisions A1 to A7 are the coordinator's; owner items O1 to O5 accepted by Andreas as recommended |

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

- **Active:** `fz-06d` (worker writing, base `24b2210`), `fz-09` (worker
  drafting, base `769c1fd`), `issue-45` (uncommitted `ci.yml` and
  `docs.yml`, parked).
- **Integrated and kept:** `fz-01`, `fz-02`, `fz-03`, `fz-04`, `fz-05`,
  `fz-05a`, `fz-06`, `fz-06a`, `fz-06b`, `fz-06c`, `fz-10`, `issue-1`,
  `issue-42`, `issue-44`, each with its branch pushed.
- Not pushed: `docs/fz-09-coverage-cache-isolation`,
  `ci/45-pull-request-trigger`, `fix/fz-06d-histogram-reuse-key`.
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
- **Ref query cost on repos with many refs** (from the FZ-06b review,
  medium): the widened `for-each-ref` computes `%(refname:short)` for every
  ref each sweep, about 74 ms per call at 5,000 refs and 300 ms at 20,000
  against about 10 ms before. The largest repo in this fleet has 50 refs.
  Candidates: derive the short remote name in Rust, or
  `%(refname:lstrip=2)`; either changes the short name of an ambiguous ref
  and needs a decision. Matters for a public release; record the number in
  [performance-baseline.md](performance-baseline.md) when ticketed.
- **From the FZ-06b review, small:** `parse_other_refs` keeps a line with a
  missing id as an empty string while `parse_branch_refs` skips it;
  `dirty_mode_status` and `local_only_status` start with `work_key: None`,
  so their first sweep always re-probes; no deterministic E13 test exists
  (it needs a seam).
- **From the FZ-06c review, small:** `handle_branch_input` (the delete and
  the switch arm) takes the repo index from the selection, not from the
  overlay's `repo_path`, so a reorder while the overlay is open sweeps the
  wrong row until the next sweep; the new tests' scratch directories under
  `$TMPDIR/fleetz_wt_*` are never removed.
- **From the FZ-10 review, nits left in the merged document:** O5 calls two
  test helpers "tests"; one over-long line in FZ-10c's acceptance.
- **Refetch on recovery:** when the GitHub poll goes from failing to
  healthy, refetch the repos whose last fetch failed as unreachable or
  timed out, so a mark does not outlive the outage by a sweep interval.
  Changes fetch contract rule 6; Andreas has not asked for it.
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
