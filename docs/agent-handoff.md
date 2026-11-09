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
| No ticket touching `main.rs`, `app.rs` or `ui/detail.rs` | 2026-11-07 19:30 (log) | the refactor window opens after the SMBP trees (FZ-83, 65, 38) merge | those merges |

The 11-07 order (steps 1 to 3) is done; steps 4 and 5 (L1, the architect's
Ready check, FZ-97) were not reached and need his word again: he stopped
the session at "the current tickets" (20:10, log). Astra is the preferred
lead. No `cargo install`, tag or release. His FZ-45 look rules (10-27,
log) apply. Commit subjects end with the ticket, `(#n)` (11-07, `00f4ab7`).

## In flight

- `main` at the FZ-83 merge `985b682`, CI 42596419465 green; `issue-140`,
  `issue-137` and `status-bar` swept. FZ-83 Done 12:45 on his look; its
  record is in the log. Follow-ups named there: branch name from any held
  run at the tip, side-run by sha, culprit flicker, `ci/tip.rs` split.
- FZ-95 (#152, no priority) and FZ-87 (#144) were waiting for FZ-83 and
  are free now; FZ-65 (#121, `9f4018d7160d`, his wheel decision) and
  FZ-38 (#88) sit uncommitted on at-a.
- Filed 11-07, after the SMBP merges: FZ-100 (#157, P1, Ready, `notes`
  header, ordered against FZ-95); FZ-101 (#158, P2, Backlog, moved rows
  flash). FZ-97 (#154, P1): docs-only, his word.
- Ready: FZ-78, 79, 58, 84, 89, 90, 92 (84, 89, 92 were waiting for FZ-83).

## Open questions

- L1 adds one line to `main.rs` (`mod architecture;`): allowed under the
  19:30 rule (a, lead's pick), after the SMBP merges (b), or a `tests/` file (c)?
- To file on his 11-05 13:50 answers (log): ssh README paragraph,
  `fleetz doctor`, `info` contributors, `AGENTS`/`RELAY` boxes; FZ-77 r2.
- #88 worktree: stays or goes; FZ-38 with font-checked glyphs? Flaky
  `clipboard::tests`. Help `y` row still "copy hash; changes: path" (#116).

## Next step

The 19:30 rule still holds until FZ-65 and FZ-38 merge. On his word: FZ-97, the L1 answer, which of
the freed tickets (FZ-95, 87, 84, 89, 92) next. Suggest `cargo install`.
