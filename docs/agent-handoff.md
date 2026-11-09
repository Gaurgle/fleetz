# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Baton released 2026-11-09 22:27 CET by session `19b5ad57`
(`claude-fable-5-1`, at-a, since 15:15) on his "Round off asap, at a
good place" (context past 80 percent, the automatic handoff did not
fire). No worker or reviewer running. `main` at `2ad77fa`.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |
| Refactor batch from `docs/module-split-design.md` | 2026-11-09 15:14, "refactor only ... File L1, T1, E1, E2 and D1 and run them in order; F1 and U1 after"; about 19:20, "We can keep on refactoring as we go", "just keep on refactoring tonight. I'll be at dinner then sleep" | FZ-103 to FZ-109 (done), FZ-110 to FZ-112 (running), then `docs/app-split-design.md`'s eight tickets in its order (accepted 21:56, "I accept all changes in the design plan"); no feature tickets | the refactor lands or revoked by word |

Astra is the preferred lead. No `cargo install`, tag or release. His
FZ-45 look rules (10-27, log) apply; subjects end with `(#n)`.

## In flight

- Done today, logged: FZ-91, FZ-103 to FZ-109, FZ-112 (`bae1fca`
  22:22; `main` CI 42701963780 was running at handback; sweep `fz-112`
  when green). One lead ruling for his review: F1 changed an existing
  test's `include_str!("main.rs")` to `"input.rs"` (option A on #166).
- FZ-111 (#168) Ready to integrate: accepted on `39a6c5d05a3d`, merge
  commit `cb85fde` on the branch (main `bae1fca` merged in), pushed, CI
  42702243309 running at handback; combined checks done on `bae1fca`
  (main moved by docs only since: `2ad77fa`, this commit). Next: gate
  `scripts/relay-gate .claude/worktrees/fz-111 39a6c5d05a3d <origin/main>`,
  merge, sweep, log with the metrics on the issue.
- FZ-110 (#167) Ready to integrate: accepted on `2503a3ccfea7`, commit
  `13490e6`, pushed, branch CI 42701600753 green. Next: merge `main`
  into `.claude/worktrees/fz-110` (expect `docs/architecture.md` to
  conflict with FZ-111's and FZ-112's rows), resolve, scope, re-review
  the resolution, checks, commit, CI, gate, merge, sweep, log.
- FZ-113 (#170, E3 `GithubSync`) Ready: body and the architect's check
  on the issue; starts after FZ-110 merges (shares `app.rs`); hard tier.
  Then E2b, A1a, A1c to A1i, R2 per `docs/app-split-design.md`
  (accepted 21:56). No worker or reviewer running at handback.
- FZ-100 (#157), FZ-102 (#159), the build label and FZ-97 (#154) wait
  on his word; FZ-83 and FZ-95 follow-ups in the log.

## Open questions

- O5 (`cli` a source, #162) and O6 (E2b its own ticket) are settled by
  his design acceptance; `scripts/measure-modules`? the flaky fixed-dir
  test in `poll/tests.rs:1282` (an existing test, his)? Older in the log.

## Next step

The next lead: finish FZ-111 (gate, merge) and FZ-110 (merge of main,
re-review, gate, merge) as recorded above, both under the standing
authority; then FZ-113 on the same authority. Suggest `cargo install`.
