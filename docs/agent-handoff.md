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
| No ticket touching `main.rs`, `app.rs` or `ui/detail.rs` | 2026-11-07 19:30 (log) | the refactor window opens after the SMBP trees merge; FZ-83 merged, FZ-38 closed 11-09, FZ-65 left; FZ-95 and FZ-100 named by him 11-09 | FZ-65's merge |

The 11-07 order (steps 1 to 3) is done; steps 4 and 5 (L1, the architect's
Ready check, FZ-97) were not reached and need his word again: he stopped
the session at "the current tickets" (20:10, log). Astra is the preferred
lead. No `cargo install`, tag or release. His FZ-45 look rules (10-27,
log) apply. Commit subjects end with the ticket, `(#n)` (11-07, `00f4ab7`).

## In flight

- `main` at the FZ-83 merge `985b682`, CI green. FZ-83 Done 12:45; FZ-38
  (#88) closed 13:20, both in the log. FZ-83 follow-ups in its log entry.
- FZ-65 (#121): `af908db` (ticket + merge of `main` + yank-reset order),
  all reviewed, CI green, combined checks green, gate holds. **Ready to
  integrate, waiting for his look** (15:30; the wheel selects in the
  notes tree). On his word: merge `--no-ff`, `Closes #121`. Record: #121.
- FZ-95 (#152, P1): `c3c6830` + merge `5f12f3b` (reviewed), pushed. Fix
  cycle running (`fz-worker`, fable high, 14:35): B2, B3, N1 to N6, his
  11-06/11-07 amendments, both window placements behind a temporary `W`
  toggle for his pick; then review, his look, remove the loser, re-review,
  gate. FZ-100 (#157, P1) follows it. Record: #152.
- FZ-102 (#159, P2, wrapped-row follow, after FZ-65 and FZ-95); FZ-101
  (#158, P2); FZ-97 (#154, P1, docs-only, his word). Ready: FZ-78, 79,
  58, 84, 87, 89, 90, 92.

## Open questions

- L1's one line in `main.rs` (`mod architecture;`): now (a, lead's pick), after FZ-65 (b), or `tests/` (c)?
- To file on his 11-05 13:50 answers (log): ssh README paragraph,
  `fleetz doctor`, `info` contributors, `AGENTS`/`RELAY` boxes; FZ-77 r2.
- Wall-clock tests flaky under load (11-09, three named in #121 and #152
  review comments): a ticket? Help `y` row still "copy hash" (#116).

## Next step

FZ-65 and FZ-95 are running (his 13:15 word); FZ-100 after FZ-95. Then
on his word: FZ-97, the L1 answer, the next Ready ticket. `cargo install`.
