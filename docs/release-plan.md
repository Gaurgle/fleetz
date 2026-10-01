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
| [FZ-08](https://github.com/Gaurgle/fleetz/issues/21) | Preview GitHub Project status across the fleet | M4 |
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

## Completion rule

An agent finishing a turn is not a completed ticket. Done requires accepted
criteria, independent review, combined checks, the feature-branch commit and
integration into `main`. Record the commit and verification evidence. Commits,
pushes and merges follow the safe-merge rule; no agent publishes issues,
changes CI, tags or releases as a side effect of this plan.

See [agent-workflow.md](agent-workflow.md) for handoff templates and controls.
