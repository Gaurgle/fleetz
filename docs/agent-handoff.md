# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Session `19b5ad57` (`claude-fable-5-1`, at-a) took over 2026-11-09
15:15 CET; the previous lead `6d8fd7f3` had released the baton at 13:26
on his "stop after fz-95 is complete". `main` at `ee0ba8b`, CI green.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |
| Refactor batch from `docs/module-split-design.md` | 2026-11-09 15:14, "refactor only ... File L1, T1, E1, E2 and D1 and run them in order; F1 and U1 after" | FZ-103 to FZ-107 in that order, then F1 and U1; O1 yes, O2 sibling `tests.rs`, O4 as written; E3 and A1 wait for architect consults; no feature tickets (FZ-100, FZ-102, the build label wait) | the batch lands or revoked by word |

Astra is the preferred lead. No `cargo install`, tag or release. His
FZ-45 look rules (10-27, log) apply. Commit subjects end with the ticket,
`(#n)` (11-07, `00f4ab7`).

## In flight

- FZ-103 (#160, L1): In progress, record on the issue. Worktree
  `.claude/worktrees/fz-103`, branch `test/fz-103-layer-test`, base
  `ee0ba8b`, worker `fz-worker` standard, running since 15:25.
- FZ-104 to FZ-107 (#161 to #164, T1, E1, E2, D1) filed Backlog P1 M5,
  bodies carry gate and allowed files. T1 waits on O3 (below). An
  `fz-architect` branch check runs before T1 starts (workflow trigger).
- Open from the 11-09 reviews (in the log entries): FZ-83's branch-name
  and side-run follow-ups and `ci/tip.rs` split; FZ-95's held-fetch glyph
  (needs his yes on a `badges.rs` test rewrite), two-row header block,
  `detail.rs` seams. FZ-100 (#157), FZ-102 (#159), the build label and
  FZ-97 (#154) wait on his word after the batch.

## Open questions

- O3 for T1: merge or drop each tree that touches the files it moves:
  `issue-116` FZ-60, `issue-119` FZ-63, `issue-81` on RoosMacbookA;
  `origin/feat/148-strip-median` FZ-91 (parked on his look).
- Milestone for the batch: M5 was the lead's pick; a structure milestone
  instead is his call. F1 and U1: file when D1 is Done, or now?
- To file on his 11-05 13:50 answers (log): ssh README, `fleetz doctor`,
  `info` contributors, `AGENTS`/`RELAY` boxes; FZ-77 r2. Wall-clock tests
  flaky under load (#121, #152 review comments): a ticket? Help `y` row
  still "copy hash" (#116).

## Next step

FZ-103: scope check and review when the worker reports, then commit,
CI, combined checks, gate, merge. Then the O3 ruling and the architect's
branch check before FZ-104.
