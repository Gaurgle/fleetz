# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Baton released 2026-11-07 20:55 CET by session `4d31e22a`
(`claude-fable-5-1`, RoosMacbookA, since 19:27); he stopped for the day
after FZ-63 and FZ-60 merged. No worker or reviewer running. at-a holds
FZ-83's, FZ-65's and FZ-38's uncommitted trees. Worktrees here: `45`
(CI edits) and `81` (test edits), both uncommitted and waiting on him.

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

- Nothing dispatched. `main` at the FZ-60 merge `3dd6112` plus the docs
  commit after it (CI run 42408060136; `issue-116` swept once green).
- FZ-83 (#140): uncommitted on at-a, diff `5f4c44d114ce` vs `334e9a6`;
  next a fresh full review, commit, merge with `main`, his look.
- FZ-95 (#152, `c3c6830`, no priority) and FZ-87 (#144) wait for FZ-83.
  FZ-65 (#121, `9f4018d7160d`, his wheel decision) and FZ-38 (#88) on at-a.
- FZ-100 (#157, P1, M3): his header bug in `notes`, filed 20:52; waits for
  the SMBP merges (touches detail.rs) and orders against FZ-95.
- FZ-97 (#154, P1): investigation, docs only; first candidate on his word.
- Ready: FZ-78, 79, 58, 84, 89, 90, 92 (84, 89, 92 wait for FZ-83).

## Open questions

- L1 adds one line to `main.rs` (`mod architecture;`): allowed under the
  19:30 rule, or after the SMBP merges? (a) one line ok, (b) wait, (c) a
  `tests/` file against the repo convention. Lead recommends (a).
- To file on his 11-05 13:50 answers (log): ssh README paragraph,
  `fleetz doctor`, `info` contributors, `AGENTS`/`RELAY` boxes; FZ-77 r2.
- #88 worktree: stays or goes; FZ-38 with font-checked glyphs? #81 and
  #45 trees here: his yes. Flaky `clipboard::tests`. The help `y` row
  on `main` still says "copy hash; changes: path" (#116 follow-up).

## Next step

On his word: FZ-97 (docs-only investigation) is the only Ready ticket
clear of the 19:30 rule; then the L1 answer. Suggest `cargo install`
(`main` now has FZ-63 and FZ-60).
