# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Baton released 2026-11-19 18:00 CET by session `0dbccbeb`
(`claude-fable-5-1`, at-a): the app-split batch and its survey are
done, the refactor authority's scope is spent. No worker, reviewer
or architect running. `main` at `9edeea5` (code `5d2e7f6`), CI green,
no worktrees.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |
| Refactor batch (ended) | 2026-11-09 15:14 to 11-19 about 18:45, "refactoring can probably stop here now" (log) | FZ-103 to FZ-124 all Done; R2 (FZ-125) and the survey's seams need a new word | the refactor lands or revoked by word |

Astra is the preferred lead. No `cargo install`, tag or release. His
FZ-45 look rules (10-27, log) apply; subjects end with `(#n)`.

## In flight

- Lead ruling for his review: F1's `include_str!` test change (#166).
  The FZ-113, O-E and `refresh_interval` rulings he accepted 11-18
  about 13:33 (log). FZ-103 to FZ-113 and FZ-116 to FZ-124 are Done and logged.
- App-split batch, filed 11-18, architect-checked, strictly in order,
  one worker at a time: all moves Done (FZ-116 to FZ-124, the last
  merged `5d2e7f6` 11-19 14:01; `app.rs` 685 production lines). The
  post-batch survey landed `9edeea5` (log, "Survey after the app-split
  batch": next seams G1 `git.rs`, `ui/header.rs`, S1, A1j, an engine
  test ticket, a doc-comment ticket; all need his word). FZ-125 #182
  (R2) needs his ruling on Q1 (refreshed on the issue). His 13:15:
  "continue with tickets ... im sick and will stay in bed" (log).
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

Nothing is authorized. On his word: rule FZ-125 Q1 and rewrite its
body, or file the survey's seams (G1 first) as tickets with an
architect check, or the waiting feature tickets. Suggest `cargo
install` (eleven refactors since his last build).
