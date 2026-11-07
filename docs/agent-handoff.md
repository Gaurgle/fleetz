# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Session `4d31e22a` (`claude-fable-5-1`, RoosMacbookA) took over 2026-11-07
19:27 CET; the previous lead (`3f0d864a`) released the baton 11-06 16:50.
Workers: FZ-60 merge-round `fz-worker` running (dispatched 20:03).

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |
| Today's order (11-07 19:20, log) | 1 commit+push FZ-60, FZ-63 as-is; 2 FZ-63 README row, re-review, merge; 3 FZ-60 switcher option 1, review, his look, merge; 4 L1; 5 architect ticket check of Ready, then FZ-97 | SMBP trees (FZ-83, 65, 38, and 95, 87 behind FZ-83) untouched; no other ticket touching `main.rs`, `app.rs`, `ui/detail.rs` before the SMBP trees merge (11-07 19:30) | this order done |

Astra is the preferred lead. No `cargo install`, tag or release. His FZ-45
look rules (10-27, log) apply.

## In flight

- FZ-63 (#119): Done, merged `b342d30` 20:01, record in the log.
- FZ-60 (#116): branch at `d1a367f` (snapshot, switcher rounds 1 and 2,
  accepted diff `175d4e0cb8a4`). Worker: merge `main` `b342d30`
  (6 conflicts) plus the root-compare fix and the architecture.md event
  rows (#116 rulings), staged not committed. Then re-review vs `main`,
  his look at the switcher (pending), CI, verify, gate, merge.
- FZ-83 (#140), FZ-65 (#121), FZ-38 (#88): on at-a, untouched today.
  FZ-95 (#152, `c3c6830`, no priority) and FZ-87 (#144) wait for FZ-83.
- FZ-97 (#154, P1): step 5, after the architect's Ready check.
- L1 (module-split-design.md): step 4, needs an issue (his yes) and one
  line in `main.rs`; see open questions.
- Ready: FZ-78, 79, 58, 84, 89, 90, 92 (84, 89, 92 wait for FZ-83).

## Open questions

- L1 adds `#[cfg(test)] mod architecture;` to `main.rs`: does his 19:30
  rule (no other ticket touching `main.rs`) exclude it, or is one line ok?
- To file on his 11-05 13:50 answers (log): ssh README paragraph,
  `fleetz doctor`, `info` contributors, `AGENTS`/`RELAY` boxes; FZ-77 r2.
- #88 worktree: stays or goes; FZ-38 with font-checked glyphs? #81 and
  #45 trees here (test edits, CI): his yes. Flaky `clipboard::tests`.

## Next step

Stop condition (his word, 11-07 about 20:10: "we can stop after the
current tickets are finished"): FZ-63 and FZ-60 through merge, then the
lead stops. Steps 4 and 5 (L1, architect check, FZ-97) wait for his
word in a later session. Suggest `cargo install` after the merges.
