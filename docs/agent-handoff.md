# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Baton released 2026-11-09 13:26 CET by session `6d8fd7f3`
(`claude-fable-5-1`, at-a, since 09:49) on his "stop after fz-95 is
complete". No worker or reviewer running; no worktrees anywhere (all
six swept or removed today); `main` CI green at `63db73b`.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |

The 11-07 order (steps 1 to 3) is done; steps 4 and 5 (L1, the architect's
Ready check, FZ-97) were not reached and need his word again: he stopped
the session at "the current tickets" (20:10, log). Astra is the preferred
lead. No `cargo install`, tag or release. His FZ-45 look rules (10-27,
log) apply. Commit subjects end with the ticket, `(#n)` (11-07, `00f4ab7`).

## In flight

- Nothing dispatched. `main` at the FZ-95 merge `63db73b`, CI 42616557816
  green, `issue-152` swept.
  Done today, all logged with metrics: FZ-83 (`985b682`), FZ-65
  (`e2fa215`), FZ-95 (`63db73b`); FZ-38 (#88) closed unmerged. The 11-07
  19:30 rule ended with FZ-65's merge: the refactor window is open.
- Open from today's reviews (in the log entries): FZ-83's branch-name and
  side-run follow-ups and `ci/tip.rs` split; FZ-95's held-fetch glyph
  (needs his yes on a `badges.rs` test rewrite), two-row header block,
  his "more ideas for its placement", `detail.rs` seams; FZ-102 (#159,
  P2) wrapped rows in the other views. His build-label idea (ticket number
  in the top border for non-main builds): file as FZ-103 on his yes.
- FZ-100 (#157, P1) next after FZ-95 by his 11-09 order, not started on
  his word. FZ-101 (#158, P2); FZ-97 (#154, P1, docs-only, his word).
  Ready: 78, 79, 58, 84, 87, 89, 90, 92. Suggest `cargo install`.

## Open questions

- L1's one line in `main.rs` (`mod architecture;`): now (a, lead's pick), after FZ-65 (b), or `tests/` (c)?
- To file on his 11-05 13:50 answers (log): ssh README, `fleetz doctor`,
  `info` contributors, `AGENTS`/`RELAY` boxes; FZ-77 r2.
- Wall-clock tests flaky under load (11-09, three named in #121 and #152
  review comments): a ticket? Help `y` row still "copy hash" (#116).

## Next step

Stopped on his word ("stop after fz-95 is complete", 13:15). Nothing
starts without his word; FZ-100 is first in his stated order.
