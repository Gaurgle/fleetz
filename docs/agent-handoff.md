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
| Refactor batch from `docs/module-split-design.md` | 2026-11-09 15:14, "refactor only ... File L1, T1, E1, E2 and D1 and run them in order; F1 and U1 after"; about 19:20, "We can keep on refactoring as we go", "just keep on refactoring tonight. I'll be at dinner then sleep" | FZ-103 to FZ-107 (done), then FZ-108 (U1) and F1 (file and run), unattended tonight; the architect's survey and the A1 and E3 consults (reads); A1 and E2b code only after he reads their designs; no feature tickets | the refactor lands or revoked by word |

Astra is the preferred lead. No `cargo install`, tag or release. His
FZ-45 look rules (10-27, log) apply. Commit subjects end with the ticket,
`(#n)` (11-07, `00f4ab7`).

## In flight

- Done today, logged: FZ-91 `8ac7ff1`, FZ-103 `62aeb8f`, FZ-104
  `e831dd1`, FZ-105 `8f3a57d`, FZ-106 `cc1ecf5`, FZ-107 `270bd71`.
- FZ-108 (#165, U1) Done, merged `74d527d`; FZ-109 (#166, F1) Done,
  merged `f56ea6a` 21:08, logged; `main` CI 42690023117 watched, the
  `fz-109` worktree sweeps when green. Nothing running. One lead ruling
  for his review: F1 changed an existing test's `include_str!("main.rs")`
  to `"input.rs"` (the arm it reads moved; option A on #166).
- `docs/app-split-design.md` (`a0b95ab`) is proposed for his reading:
  A1, E3, E2b as eight tickets, owner items O-A to O-D; the survey is
  on `main` (`565b974`).
- Open from the 11-09 reviews (log): FZ-83 and FZ-95 follow-ups.
  FZ-100 (#157), FZ-102 (#159), the build label and FZ-97 (#154) wait
  on his word after the refactor.

## Open questions

- O5: `cli` as a source (ruling on #162, reversible); O6: E2b as its
  own ticket; `scripts/measure-modules` from the architect's scratch
  script? Older: his 11-05 answers to file (log); wall-clock tests flaky
  under load; help `y` row (#116).

## Next step

The unattended scope is complete: eight merges today, all green. Nothing
starts without his read of `docs/app-split-design.md` (E3 first, per
its order) and his word on O-A to O-D, O5, O6. Suggest `cargo install`
(the shading is the one visible change; the refactors should show
nothing). The lead stays up for his return or hands back on request.
