# Fleetz public release plan

Prepared 2026-10-01. Planning only: no implementation or release is authorized
by moving an idea into this document.

## Product direction

Fleetz answers what needs attention across repositories, explains why, and
hands off to the right tool. Preserve fast keyboard interaction, character
based presentation, and cooperation with lazygit. Commit, merge, and PR
authoring workflows remain outside this release effort.

Success means useful information arrives quickly, stale or incomplete
information is explicit, and ordinary terminal setups work well.

## Milestones

| Milestone | Scope | Exit evidence |
|---|---|---|
| M0: Prove the workflow | Baseline measurements and one independently reviewed correctness fix | FZ-01 results recorded; FZ-02 integrated and verified by the owner |
| M1: Trust the monitor | Update ownership, freshness, fetch errors, repository identity, cache invalidation, coverage and API budget | Deterministic regression scenarios pass; offline, incomplete and failed states remain distinguishable |
| M2: Make it rapid | Local-first progressive loading, selected-repo history, shared scheduling, network cadence, HTTP reuse and deadlines | Comparable before/after measurements; no loss of correctness or input responsiveness |
| M3: Clarify the terminal UI | Intro default, normal-font fallback, compact layouts, focus, colors, pane maximize | Owner acceptance at agreed terminal sizes, dark/light backgrounds and ordinary fonts |
| M4: Deepen monitoring | Attention view, Git operation state, branch/PR checks, changes since seen, snooze and handoffs | Each included feature has a bounded ticket and accepted interaction behavior |
| M5: Release | Binary distribution, diagnostics, demo, terminal support and installation documentation | Clean-machine install and documented smoke scenarios pass; owner authorizes release |

M4 is a menu, not a requirement to ship every idea. Choose the first public
release scope after M1 and the performance baseline. CI and distribution
changes need explicit approval before editing their configuration.

## Board

The [Fleetz public release Project](https://github.com/users/Gaurgle/projects/1)
is the live source for status, priority, assignment and review ownership. Ticket
bodies are available in GitHub Issues and the detailed first-wave contracts are
also retained in [first-wave-tickets.md](first-wave-tickets.md). This document
records sequencing and scope rather than maintaining a competing status copy.

Flow: Backlog -> Ready -> In progress -> Review -> Ready to integrate -> Done.
Use a blocked flag with the exact dependency, reason and next action.

| ID | Deliverable | Dependency / restriction |
|---|---|---|
| [FZ-01](https://github.com/Gaurgle/fleetz/issues/14) | Reproducible performance baseline | Measurement only; can accompany FZ-02 |
| [FZ-02](https://github.com/Gaurgle/fleetz/issues/15) | Local refresh preserves newer GitHub state | First implementation pilot |
| [FZ-03](https://github.com/Gaurgle/fleetz/issues/16) | Cached fallback remains visibly stale | Blocked on integrated FZ-02 and freshness contract |
| [FZ-04](https://github.com/Gaurgle/fleetz/issues/17) | Fetch failures remain explicit | Blocked on integrated FZ-03; shares refresh state |
| [FZ-05](https://github.com/Gaurgle/fleetz/issues/18) | Repository identity design and collision cases | Design only; implementation follows accepted design |
| [FZ-06](https://github.com/Gaurgle/fleetz/issues/19) | Cache invalidation inventory and scenarios | Investigation only; coordinate with FZ-05 |

Each active assignment also records its base commit, branch, worktree path, allowed
files, chosen model and current blocker on the ticket. Only the coordinator
updates the board. Workers report changes instead of editing shared status.

## Setup checkpoint: 2026-10-01

Setup writes completed for the private
[Fleetz release Project](https://github.com/users/Gaurgle/projects/1): all
23 issue additions, status and priority fields, unassigned worker/reviewer
fields, six milestones, and native dependencies (#16 blocked by #15;
#17 blocked by #16). Existing issues remain open. No product implementation
has started.

Created [Release table](https://github.com/users/Gaurgle/projects/1/views/1)
and [Release board](https://github.com/users/Gaurgle/projects/1/views/2), with
Title, Status, Priority, Milestone, Worker and Ticket reviewer visible.

Live reads confirmed the views, privacy, milestones, dependencies and Status
grouping for the board, and a later read found all 23 items with their fields
set. Visual layout remains unverified because browser automation failed to
start.

See [agent-handoff.md](agent-handoff.md) for coordinator takeover and the
next FZ-02 pilot. GitHub remains the live status source.

## M0 checkpoint and retrospective: written 2026-10-10

M0's exit evidence was met on 2026-10-01: FZ-01's results are in
[performance-baseline.md](performance-baseline.md) (merge `b084d21`), and
FZ-02 was integrated (`28ebba7`) and verified by the owner, who closed it.
The pilot cost: FZ-02 ran from dispatch at 12:30 to Ready to integrate at
12:38 UTC (one worker fix cycle, review accepted first time) and merged at
13:10; FZ-01 needed one review fix cycle (a comparison rule that failed across
sessions), and its reviewer reproduced case A from the document alone with
wall time 4 to 5% higher at equal CPU, which is why later tickets compare
binaries interleaved in one session rather than against the table.

What the method showed over M0 and M1 (data in the issues and in the
handoff's "How the days ran"):

- **Throughput.** 40 ticket merges on `main` between 2026-10-01 15:06 and
  2026-10-09 20:09, over four working sessions: 17 on 10-01, 13 on 10-02,
  6 on 10-03, 5 on 10-09. The 10-01 and 10-02 runs used two or three agents
  at a time; 10-03 and 10-09 at most two workers, mostly one.
- **Review earns its cost.** On 10-02 five of thirteen tickets needed a fix
  cycle after first review; on 10-03 three of six; on 10-09 two of five. The
  usual finding is a rule with no test that fails when the rule is removed
  (reviewers run mutants). Two tickets that passed the worker's own checks
  had tests that could not fail (FZ-09c). One coordinator ruling was wrong
  and the reviewer caught it (the strict owner rule, FZ-05d).
- **What creaked.** Ticket bodies' "existing tests unaffected" lists went
  stale three times in one day; stale handoff times written from memory;
  a reviewer that could not find a mid-ticket ruling because it was only in
  the worker's message; CI's clippy newer than the machine's; a `sed -i`
  slip against the file rule; one `fz-small` that wrote a file through an
  inline script. Each has a rule in the handoff now.
- **Usage.** The limit was hit once (10-01, five agents in flight); 10-02
  ended at 82% by the owner's call. One worker and one reviewer at a time
  ran a full afternoon on 10-09 without reading the limit.
- **Owner decisions per day:** about five, all on test edits, option
  choices in a design, or words on screen. Nothing waited on him for more
  than one turn when he was present.

Keep for the template: one coordinator, worker and reviewer as separate
invocations, mutants as part of review, rulings posted on the issue when
made, cleanup as part of Done, the standing test-edit exception.

## M1 checkpoint: written 2026-10-10

Every M1 implementation ticket is Done: FZ-03, FZ-04, FZ-05a to f, FZ-06a to
e, FZ-09a to d and f, FZ-10a to c, plus #42, #43 and #44 (29 issues closed
under the milestone). The last merge was FZ-09d at `5b25efb` on 2026-10-09.
Two M1 issues stay open and do not block the exit: #46 (a regression test
that also passes on the old code; editing it is the owner's call) and #61
(FZ-09e, deliberately not promoted).

Exit evidence, against the table above:

- **Deterministic regression scenarios pass.** `cargo test` on `main` at
  `fe39168`: 1050 passed, 0 failed, 1 ignored (the ignored test needs a
  live `repoz`). The accepted designs' numbered scenarios are tests:
  identity (`S1` to `S22` across `discovery.rs` and `main.rs`), cache keys
  (`E` scenarios in `git.rs` and `main.rs`), coverage and cache (`CK`
  scenarios in `github.rs` and `cache.rs`), API budget (`S1` to `S3` with
  sub-cases, 34 tests in `app.rs` and `main.rs` plus five from review). Each
  behavior change landed with a test that fails on the old code; the one
  known exception is #46.
- **Offline, incomplete and failed states remain distinguishable.** A
  cache-served launch records no attempt and the header shows the data's
  age (`DatasetSource::Cache`, freshness contract); a failed load says why
  instead of drawing nothing (`a_failed_load_says_why_instead_of_drawing_nothing`);
  rate limited, paused and slowed each have their own header words with the
  resume time (FZ-10c, `github_segment`, accepted by the owner); `access
  refused` and `not authenticated` are named; a backing-off header keeps the
  failure and its age; the coverage note says `(fetched 50 of 95 repos)`
  when the window is smaller than the account; a carrier row the last poll
  did not return is dimmed with "GitHub data from an earlier refresh" and
  never shown as "no CI" (FZ-09d, look accepted); a failed `git fetch` stays
  marked per repo until that repo is fetched again (FZ-04); a signed-out
  `R` leaves the rows and says the load failed. Every one of these has a
  named test in `src/ui/mod.rs`, `src/main.rs` or `src/github.rs`.

Carried out of M1 as follow-ups, not exit conditions: the ssh fetch hang
(`ConnectTimeout`, `BatchMode`), refetch on recovery (fetch rule 6), the ref
query cost at 20,000 refs, one key for "same remote", the `,` interval row
that is never applied, and the review follow-ups listed in the handoff.
Note for the first launch of a build from this `main`: the first successful
poll writes the keyed cache file beside the legacy one, and the installed
binary keeps using the legacy file until it is reinstalled.

Checkpoint verdict: M1's exit evidence is met on the record above. Andreas
confirmed it on 2026-10-10 at 12:52 ("all looks good"). The release scope
decision the plan asks for "after M1 and the performance baseline" is his
next call.

## First dispatch

1. Coordinator checks the working tree, captures the base commit and prepares
   `fix/fz-02-preserve-github-state` in an isolated worktree for FZ-02,
   including its ticket and rules.
2. One implementation agent reproduces and fixes FZ-02 with regression tests.
3. A separate reviewer checks the exact result against the same base.
4. Coordinator commits the accepted diff on the feature branch and pushes it,
   verifies the combined result against current `main` in an isolated checkout,
   and merges under the safe-merge rule in [agent-workflow.md](agent-workflow.md).
5. Record what the pilot cost in time, usage where available, review findings
   and rework. Adjust task size before allowing two implementation workers.

FZ-01 can run in parallel as read-only measurement work with output confined
to its report. Avoid benchmarking while builds or competing workloads distort
results. FZ-05 and FZ-06 need no coding agents until their designs are accepted.

## Follow-on backlog

These are retained ideas, not ready implementation tickets. Promote them when
dependencies and acceptance criteria are known. Consult [ideas.md](ideas.md)
for prior decisions before proposing a different approach.

| Area | Retained work | Prerequisite |
|---|---|---|
| Trust | Pagination and coverage indicators; per-account/host versioned atomic cache | Identity and freshness decisions |
| API budget | Record query cost and budget; reset/retry handling; reserve for other clients; multi-instance behavior | FZ-03 and request lifecycle design |
| Scheduling | Shared limits, duplicate-work coalescing, stale-result rejection, selected-repo priority | Local/GitHub update ownership |
| Startup | Local discovery independent of GitHub; summary rows before deep history | FZ-01 and identity |
| Local cache | On-demand history, correct invalidation, bounded retention, reduced cloning | FZ-06 |
| Refresh | Separate local probes, GitHub polls and fetch reconciliation; reuse HTTP client; cancel timed-out work | Freshness and scheduler contracts |
| Filesystem | Debounced notifications with periodic reconciliation | Measurements justify the added complexity |
| Rendering | Measure composition and terminal bytes; consider synchronized output and derived-view caching | FZ-01 |
| UI | Intro opt-in; normal-font/ANSI modes; dark/light readability; contextual help; maximize/restore; economical folds | Owner reviews representative layouts |
| Monitoring | Attention view; interrupted Git operations; explicit default-branch CI scope; selected PR/branch checks | Trust foundations |
| Awareness | Changes since last seen; pins; saved filters; activity-based snooze; opt-in notifications | Baseline and transition semantics |
| Integration | Configurable editor, shell, lazygit and worktree handoffs | Explicit command and error-handling contract |
| Release | Doctor command; supported terminal matrix; downloadable binaries; demo and installation docs | Agreed release scope; approval for workflow changes |

The retained work is represented by broad planning issues. These issues preserve
direction without acting as implementation contracts:

| ID | Planning issue | Milestone |
|---|---|---|
| [FZ-07](https://github.com/Gaurgle/fleetz/issues/20) | Monitor assigned GitHub issues across repositories | M4 |
| [FZ-08](https://github.com/Gaurgle/fleetz/issues/21) | Read-only GitHub Project board view (columns, table, ticket details) | M4 |
| [FZ-09](https://github.com/Gaurgle/fleetz/issues/22) | Make GitHub coverage and cache isolation explicit | M1 |
| [FZ-10](https://github.com/Gaurgle/fleetz/issues/23) | Define GitHub API budget and retry behavior | M1 |
| [FZ-11](https://github.com/Gaurgle/fleetz/issues/24) | Coordinate refresh work with a shared scheduler | M2 |
| [FZ-12](https://github.com/Gaurgle/fleetz/issues/25) | Load fleet summaries and history progressively | M2 |
| [FZ-13](https://github.com/Gaurgle/fleetz/issues/26) | Improve rendering and terminal UI clarity | M3 |
| [FZ-14](https://github.com/Gaurgle/fleetz/issues/27) | Add an actionable repository attention view | M4 |
| [FZ-15](https://github.com/Gaurgle/fleetz/issues/28) | Track changes since seen and support quieting work | M4 |
| [FZ-16](https://github.com/Gaurgle/fleetz/issues/29) | Define safe editor, shell, lazygit, and worktree handoffs | M4 |
| [FZ-17](https://github.com/Gaurgle/fleetz/issues/30) | Add release diagnostics and clean-machine verification | M5 |

Existing issues [#1](https://github.com/Gaurgle/fleetz/issues/1),
[#2](https://github.com/Gaurgle/fleetz/issues/2),
[#3](https://github.com/Gaurgle/fleetz/issues/3),
[#4](https://github.com/Gaurgle/fleetz/issues/4),
[#5](https://github.com/Gaurgle/fleetz/issues/5), and
[#7](https://github.com/Gaurgle/fleetz/issues/7) remain open for triage. The
planning issues link related work rather than duplicating those scopes.

## Dispatch order after M1: written 2026-10-16

Andreas's direction (2026-10-16): every ticket is to be completed eventually;
the release is not in a hurry; fleetz is a project he uses daily, so
functionality and reliability come first, the look second ("sleek, still
Rust/TUI style, with passion"), and polish is deferred or picked up in
parallel when a slot is free. Optimizations are wanted, scheduled by what and
when. A blocker is a blocker. The release scope decision stays open; it is
not needed to keep building.

| Batch | Tickets | Why here |
|---|---|---|
| 0 (2026-10-20) | #83 FZ-34 fetch timeout leaks (Done); then FZ-35a #84 progress-aware fetch and FZ-35b #85 the `fetch_max_mb` cap, spec approved, each on the owner's word | A P0 disk fill and its design follow-up, ahead of everything below |
| 1 (running) | #70 ssh fetch bound; #72 worktrees section in Info (spec first); #71 Info header worktree colour | The one daily reliability bug, and the owner's two UI ideas |
| 2 | Refetch on network recovery (changes fetch rule 6, owner decision); dirty worktree in the list row (after #72); FZ-13 wave 4 (hints follow focus #81, economical folds #80; maximize/restore dropped by the owner on 2026-10-17, "we dont need a focus mode") | Reliability and look, each needing one decision |
| 3 | #67 CI pane, FZ-14 attention view, FZ-15 changes since seen, FZ-16 worktree overlay | The daily-use features |
| 4 | FZ-12 progressive load, FZ-11 scheduler, the ref query cost at 20,000 refs | Optimizations, each measured against the FZ-01 baseline before and after |
| filler | One key for "same remote", stale `CLAUDE.md` lines, help wording, test sharpening (the handoff's follow-up list) | `fz-small` whenever its slot is free |
| release | #3 distribution, FZ-17 doctor, #2 graceful degradation, #5 config schema, #45 | When the owner asks for the release |

Three slots are in force: two `fz-worker` plus one `fz-small`, each with its
own reviewer, merges one at a time.

## repoz convergence: queued 2026-10-23

Andreas's direction (2026-10-23): bash repoz becomes a quick command
version of fleetz, built on fleetz's code, with repoz's look unchanged;
the command name and any repository renames come last and are his. The
proposed design, its tickets (FZ-46 to FZ-51, not yet filed) and owner
items are in [repoz-print-design.md](repoz-print-design.md). Its place
against the batches above is his call. Publishing fleetz, which replacing
the public repoz needs, belongs to the release row.

## Completion rule

An agent finishing a turn is not a completed ticket. Done requires accepted
criteria, independent review, combined checks, the feature-branch commit and
integration into `main`. Record the commit and verification evidence. Commits,
pushes and merges follow the safe-merge rule; no agent publishes issues,
changes CI, tags or releases as a side effect of this plan.

See [agent-workflow.md](agent-workflow.md) for handoff templates and controls.
