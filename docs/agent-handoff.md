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

- Done today, logged: FZ-91 `8ac7ff1`, FZ-103 `62aeb8f` (layer test;
  `super::` gap on #160), FZ-104 `e831dd1` (tests in sibling files).
- FZ-105 (#162, E1) Done, merged `8f3a57d` 18:26, logged; `main` CI
  42664646798 watched, worktree sweeps when green.
- FZ-106 (#163, E2a) Done, merged `cc1ecf5` 18:47, logged; `main` CI
  42667992050 watched, worktree sweeps when green.
- FZ-107 (#164, D1) next: `fz-architect` ticket check dispatched 18:48
  (the three edges' move sets, where `Icons`, `View`, the shared git and
  github types land); the worker dispatches on `fits`. Nothing running
  but the architect.
  E2b (planning functions under a fetch owner type) waits with E3 and
  A1 for the consults, filed on his yes (O6).
  Then FZ-105 to FZ-107 (E1, E2, D1), then FZ-108 (#165, U1) and F1
  (filed after D1). No worker or reviewer running.
- Open from the 11-09 reviews (in the log entries): FZ-83's branch-name
  and side-run follow-ups and `ci/tip.rs` split; FZ-95's held-fetch glyph
  (needs his yes on a `badges.rs` test rewrite), two-row header block,
  `detail.rs` seams. FZ-100 (#157), FZ-102 (#159), the build label and
  FZ-97 (#154) wait on his word after the batch.

## Open questions

- O5: `cli` as a source (lead's ruling on #162, reversible); O6: E2b
  as its own ticket. F1 after D1. Older: to file on his 11-05 answers
  (log); wall-clock tests flaky under load; help `y` row (#116).

## Next step

Run FZ-104 to merge, then FZ-105 (E1) in the same way; he is away
(about 17:05, "no prompts until completed"): stop only on a safe-merge
failure, a stuck worker or the usage limit. Suggest `cargo install`
(FZ-91's shading is on `main`).
