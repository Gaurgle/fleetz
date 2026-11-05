# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Baton released 2026-11-05 15:35 CET by session `ea56f412`
(`claude-opus-5-5` since 13:55) at his 95% usage warning. No worker or
reviewer running (FZ-83's review stopped unfinished). 0.6.1 is pushed
(`d5cf417`); tag `v0.6.1` local only, his. Worktrees on at-a: `issue-121`,
`issue-137`, `issue-140`, `issue-88`, `status-bar` (FZ-93, merged: sweep).

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |

The 10-30 stop was lifted 2026-11-02 20:18 ("lets keep working then. you
choose tickets as you see fit best", log). Astra is the preferred lead. No
`cargo install`, tag or release. His FZ-45 look rules (10-27, log) apply.

## In flight

- FZ-81 (#137, yank flash, `35f17cc`, diff `7900ef870c0e`): his look
  accepted 14:25; merging with `main` conflicts in README's `yy`/`yp`
  row: a resolution cycle, review, gate, merge. FZ-83 (#140, CI lamp,
  Vercel rows, source column; uncommitted, diff `5f4c44d114ce` vs
  `334e9a6`, 1631 green): his design yes 15:32; next a fresh full
  review, commit, a merge with `main` (behind FZ-82/68/77), his look. Ready: FZ-84 to
  FZ-87, FZ-89 to FZ-92, FZ-78, FZ-79. FZ-80 Backlog.
- FZ-93 (#150) merged `b8c6115` 11-05 (log), 0.6.2 on `main`, untagged.
  Filed on his look 18:48: FZ-95 (#152, fixed header sections, owner/name)
  and FZ-96 (#153, digest fleet column) Ready; FZ-94 (#151) Backlog.
- FZ-65 (#121): built, `9f4018d7160d` unreviewed, his wheel decision.
  #88 (FZ-38) parked, uncommitted. FZ-63 (#119), FZ-60 (#116): trees
  lost 11-03; redispatch on his word.

## Open questions

- To file on his 11-05 13:50 answers (log): an ssh README paragraph,
  `fleetz doctor` (M5), contributors in the `info` inspector; the day's
  work, `AGENTS` and `RELAY` boxes after FZ-79 (his "what you think
  makes sense": lead's order in `ideas.md`). FZ-77 round 2: facts and
  commits; uncommitted files belong to `changes` (FZ-60).
- #88 worktree: stays or goes; FZ-38 with font-checked glyphs? #81 and
  #79 test edits; pane 1 wheel coalescing, flaky `clipboard::tests`: his yes.

## Next step

He stopped 11-05 18:55. Merged 11-05: FZ-74, 88, 82, 68, 77, 93 (log).
Next: FZ-81 against FZ-93, merge; FZ-95, 96; FZ-83's review, his look;
FZ-85, 86, 84, 87, 89, 90 to 92, 77 round 2, 78, 79. Worktrees pruned
at merge (log). Suggest `cargo install` before a screenshot.

