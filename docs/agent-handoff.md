# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Taken 2026-11-06 15:48 CET by session `3f0d864a` (`claude-opus-5-5`) on
RoosMacbookA. at-a holds FZ-83's uncommitted tree (stuck there); his 15:45:
work "what makes sense ... that doesnt clash with the stuck 83". Here:
`issue-116`, `issue-119` (uncommitted, 10-30 base), `issue-45`, `79`, `81`.

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

- Running 11-06 (records on the issues): FZ-81 #137 resolution (`fz-small`),
  FZ-85 #142 (`fz-worker` opus:high), FZ-91 #148 (opus:medium), base `09f15f0`.
  FZ-95 held: its header rewrite clashes with FZ-83's `header_lines`.
- FZ-81 (#137, yank flash, `35f17cc`, diff `7900ef870c0e`): his look
  accepted 14:25; merging with `main` conflicts in README's `yy`/`yp`
  row: a resolution cycle, review, gate, merge. FZ-83 (#140, CI lamp,
  Vercel rows, source column; uncommitted, diff `5f4c44d114ce` vs
  `334e9a6`, 1631 green): his design yes 15:32; next a fresh full
  review, commit, a merge with `main` (behind FZ-82/68/77), his look. Ready: FZ-84 to
  FZ-87, FZ-89 to FZ-92, FZ-78, FZ-79. FZ-80 Backlog.
- FZ-95 (#152): `c3c6830` pushed, his look given, review 1 changes requested;
  fix list on #152. Next: one fix cycle, fresh review, gate, merge.
- FZ-65 (#121): built, `9f4018d7160d` unreviewed, his wheel decision.
  #88 (FZ-38) parked, uncommitted. FZ-63 (#119), FZ-60 (#116): trees
  found uncommitted on RoosMacbookA (10-30 base); his word.

## Open questions

- To file on his 11-05 13:50 answers (log): an ssh README paragraph,
  `fleetz doctor` (M5), contributors in the `info` inspector; the day's
  work, `AGENTS` and `RELAY` boxes after FZ-79 (his "what you think
  makes sense": lead's order in `ideas.md`). FZ-77 round 2: facts and
  commits; uncommitted files belong to `changes` (FZ-60).
- #88 worktree: stays or goes; FZ-38 with font-checked glyphs? #81 and
  #79 test edits; pane 1 wheel coalescing, flaky `clipboard::tests`: his yes.

## Next step

Finish the three running; then FZ-86, 90, 78 (clear of FZ-83). FZ-83,
FZ-95, 79, 84, 89, 92 wait for at-a. Suggest `cargo install` first.

