# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Session `6d8fd7f3` (`claude-fable-5-1`, at-a) took over 2026-11-09
09:49 CET on his "we need to finish ticket fz-83"; the previous lead
(`4d31e22a`, RoosMacbookA) released the baton 11-07 20:55. at-a holds the
FZ-83 (`issue-140`), FZ-65 (`issue-121`), FZ-38 (`issue-88`) and FZ-95
(`issue-152`) trees; `issue-137` and `status-bar` are merged, sweepable.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |
| No ticket touching `main.rs`, `app.rs` or `ui/detail.rs` | 2026-11-07 19:30 (log) | ended 11-09 13:10 with FZ-65's merge (FZ-83 merged, FZ-38 closed): the refactor window is open; FZ-95 runs on his 11-09 word | ended, row to the log at the next edit |

The 11-07 order (steps 1 to 3) is done; steps 4 and 5 (L1, the architect's
Ready check, FZ-97) were not reached and need his word again: he stopped
the session at "the current tickets" (20:10, log). Astra is the preferred
lead. No `cargo install`, tag or release. His FZ-45 look rules (10-27,
log) apply. Commit subjects end with the ticket, `(#n)` (11-07, `00f4ab7`).

## In flight

- `main` at `cb9987f` (FZ-83 merged `985b682` 10:50, Done; FZ-38 #88
  closed 11:25; both logged). Comment times before 12:30 drift ahead (log).
- FZ-65 (#121): merged `e2fa215` 13:10 on his second look, `main` CI
  green, `issue-121` swept, Done and logged with metrics.
- FZ-95 (#152, P1): `c3c6830` + merge `5f12f3b` (reviewed), pushed. Fix
  cycle reviewed, committed `cbff8e5` (both placements behind `W`), CI
  42609778666 watched. **Asked him 12:20:** may the `badges.rs` hold-text
  test be rewritten (glyph in pane 1, text in the banner)? Then his look
  (pick the placement), remove the loser, re-review, gate. FZ-100 (#157,
  P1) follows it, not started on his 12:40 word. Record: #152.
- FZ-102 (#159, P2, wrapped rows in the other views); FZ-101 (#158, P2);
  FZ-97 (#154, P1, docs-only, his word). Ready: 78, 79, 58, 84, 87, 89, 90, 92.

## Open questions

- L1's one line in `main.rs` (`mod architecture;`): now (a, lead's pick), after FZ-65 (b), or `tests/` (c)?
- To file on his 11-05 13:50 answers (log): ssh README, `fleetz doctor`,
  `info` contributors, `AGENTS`/`RELAY` boxes; FZ-77 r2.
- Wall-clock tests flaky under load (11-09, three named in #121 and #152
  review comments): a ticket? Help `y` row still "copy hash" (#116).

## Next step

His 12:40 word: finish FZ-65 and FZ-95 completely, start no new ticket
(FZ-100 included), then a summary of fleetz's state. Then his word again.
