# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Session `0dbccbeb` (`claude-fable-5-1`, at-a) took the baton
2026-11-18 12:20 CET; the previous lead `19b5ad57` released it 11-10
10:40 after his "stop after those" (FZ-110, FZ-111). No worker or
reviewer running. `main` at `1a5ce9a` (`7f7275b` plus docs), CI green.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |
| Refactor batch from `docs/module-split-design.md` | 2026-11-09 15:14, "refactor only ... File L1, T1, E1, E2 and D1 and run them in order; F1 and U1 after"; about 19:20, "We can keep on refactoring as we go", "just keep on refactoring tonight. I'll be at dinner then sleep" | FZ-103 to FZ-112 (done), then `docs/app-split-design.md`'s eight tickets in its order (accepted 21:56, "I accept all changes in the design plan"); renewed 11-18 12:22, "keep refactoring and let me know when that's done then"; 13:05 (sick, resting) yes to filing the remaining app-split issues and running unattended; no feature tickets; stop at a safe-merge failure, red `main`, two failed fix cycles or the usage limit; report when the batch lands | the refactor lands or revoked by word |

Astra is the preferred lead. No `cargo install`, tag or release. His
FZ-45 look rules (10-27, log) apply; subjects end with `(#n)`.

## In flight

- Done 11-09, logged: FZ-91, FZ-103 to FZ-109, FZ-112. One lead ruling for
  his review: F1 changed a test's `include_str!("main.rs")` to `"input.rs"` (#166).
- FZ-111 (#168) merged `500de91`, FZ-110 (#167) merged `7f7275b` on
  11-10, both logged; `main` CI green, worktrees swept, none open.
- FZ-113 (#170, E3) In progress since 11-18 12:30, base `1a5ce9a`,
  branch `refactor/fz-113-engine-github-sync`, worktree
  `.claude/worktrees/fz-113`; record comment 6977104134 (edit by id,
  not `--edit-last`); in Review, diff `be64536a87e8`, hard tiers.
  Then E2b, A1a, A1c to A1i, R2: issues to file from FZ-116 (his yes
  13:05), architect drafting the move sets. Survey note for `fz-architect`: section
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

Finish FZ-113 (worker running), then the next app-split ticket in
order under the renewed standing authority; report to him when the
batch lands. Suggest `cargo install`.
