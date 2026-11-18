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
| Refactor batch from `docs/module-split-design.md` | 2026-11-09 15:14, "refactor only ... File L1, T1, E1, E2 and D1 and run them in order; F1 and U1 after"; about 19:20, "We can keep on refactoring as we go", "just keep on refactoring tonight. I'll be at dinner then sleep" | FZ-103 to FZ-112 (done), then `docs/app-split-design.md`'s eight tickets in its order (accepted 21:56, "I accept all changes in the design plan"); renewed 11-18 12:22, "keep refactoring and let me know when that's done then"; about 12:50 (sick, resting) yes to filing the remaining app-split issues and running unattended; no feature tickets; stop at a safe-merge failure, red `main`, two failed fix cycles or the usage limit; report when the batch lands | the refactor lands or revoked by word |

Astra is the preferred lead. No `cargo install`, tag or release. His
FZ-45 look rules (10-27, log) apply; subjects end with `(#n)`.

## In flight

- Lead ruling for his review: F1's `include_str!` test change (#166).
  The FZ-113, O-E and `refresh_interval` rulings he accepted 11-18
  about 13:33 (log). FZ-103 to FZ-113 and FZ-116 to FZ-119 are Done and logged.
- App-split batch filed 11-18 (architect-checked, bodies on the issues):
  FZ-116 #173 (E2b), FZ-117 #174 (A1a), FZ-118 to FZ-123 #175 to #180
  (A1c to A1h), FZ-124 #181 (A1i), FZ-125 #182 (R2, needs his design
  ruling, not for the unattended run). Strictly in that order, one
  worker at a time (each shares `app.rs` with the next).
- FZ-120 (#177, A1e changes and worktrees) In progress since 17:52,
  base `2e5a291`, branch `refactor/fz-120-app-changes-worktrees`,
  worktree `.claude/worktrees/fz-120`; record on the issue (edit by
  comment id); worker light, reviewer standard. The usage limit paused
  the FZ-118 worker 15:00 to 17:10; the next reset is five hours on.
- Survey for `fz-architect` after the batch: `docs/architecture.md`
  section 3's `app.rs` lines and section 5's cell counts are stale,
  section 2's intro drifted; `src/ci.rs:11` names `App::ci_budget`.
- Wait on his word: FZ-100 (#157), FZ-102 (#159), the build label,
  FZ-97 (#154); FZ-114 (#171, a feature, not on the board, filed 11-10
  on his "put the contributions restore on a ticket"); FZ-83 and FZ-95
  follow-ups in the log.

## Open questions

- `scripts/measure-modules`? The flaky fixed-dir test in
  `poll/tests.rs:1282` (an existing test, his)? Older in the log.

## Next step

Finish FZ-113 (worker running), then the next app-split ticket in
order under the renewed standing authority; report to him when the
batch lands. Suggest `cargo install`.
