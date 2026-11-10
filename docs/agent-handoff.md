# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Baton released 2026-11-10 10:40 CET by session `19b5ad57`
(`claude-fable-5-1`, at-a) after his "do Fz-110 and FZ-111 ... then
stop after those". No worker or reviewer running. `main` at `7f7275b`.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |
| Refactor batch from `docs/module-split-design.md` | 2026-11-09 15:14, "refactor only ... File L1, T1, E1, E2 and D1 and run them in order; F1 and U1 after"; about 19:20, "We can keep on refactoring as we go", "just keep on refactoring tonight. I'll be at dinner then sleep" | FZ-103 to FZ-112 (done), then `docs/app-split-design.md`'s eight tickets in its order (accepted 21:56, "I accept all changes in the design plan"); no feature tickets | the refactor lands or revoked by word |

Astra is the preferred lead. No `cargo install`, tag or release. His
FZ-45 look rules (10-27, log) apply; subjects end with `(#n)`.

## In flight

- Done 11-09, logged: FZ-91, FZ-103 to FZ-109, FZ-112. One lead ruling for
  his review: F1 changed a test's `include_str!("main.rs")` to `"input.rs"` (#166).
- FZ-111 (#168) merged `500de91`, FZ-110 (#167) merged `7f7275b` on
  11-10, both logged; `main` CI green, worktrees swept, none open.
- FZ-113 (#170, E3) Ready, body and check on the issue; hard tier.
  Then E2b, A1a, A1c to A1i, R2. Survey note for `fz-architect`: section
  2's intro and rows drifted (app.rs re-measured, github/cache rows).
- FZ-100 (#157), FZ-102 (#159), the build label and FZ-97 (#154) wait
  on his word; FZ-83 and FZ-95 follow-ups in the log.
- FZ-114 (#171), filed 11-10 by the resumed FZ-105 worker on his word
  ("put the contributions restore on a ticket"): reverses FZ-91's shading.
  Not on the board, no milestone or priority; a feature, needs his word.

## Open questions

- `scripts/measure-modules`? The flaky fixed-dir test in
  `poll/tests.rs:1282` (an existing test, his)? Older in the log.

## Next step

The next lead: FZ-113 under the
standing authority (he said "stop after those" of FZ-110 and FZ-111 on
11-10; confirm before dispatching). Suggest `cargo install`.
