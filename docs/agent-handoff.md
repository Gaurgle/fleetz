# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Baton released 2026-11-06 16:50 CET by session `3f0d864a`
(`claude-opus-5-5`, RoosMacbookA, since 15:48); he wrapped up for the day.
No worker or reviewer running. at-a holds FZ-83's uncommitted tree.
Worktrees here: `issue-116`, `119` (uncommitted, 10-30 base), `45`, `79`,
`81`, `144` (clean), `147` (clean), `148` (FZ-91, parked).

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

- FZ-83 (#140): uncommitted on at-a, diff `5f4c44d114ce` vs `334e9a6`;
  next a fresh full review, commit, merge with `main`, his look.
- FZ-95 (#152): `c3c6830`, fix list plus his 11-06 header criteria
  (stacked width-dependent summary, subtitles, the long-note banner) on
  #152. Waits for FZ-83 (both rewrite `header_lines`).
- FZ-87 (#144): his rulings on #144 (A + V1 + O2, key `S`); worktree
  `issue-144` clean; build after FZ-83 (shares model.rs, detail.rs).
- FZ-90 (#147): re-scoped on his screenshot to the inspector's changed-
  files paths (#147); Ready, next session. FZ-91 (#148) parked, Backlog.
- Filed 11-06: FZ-97 #154 slow launch (P1, first next), FZ-99 #156 PR
  view (design). Ready: FZ-78, 84, 89, 92 (last three wait for FZ-83).
- FZ-65 (#121): built, `9f4018d7160d` unreviewed, his wheel decision.
  #88 (FZ-38) parked, uncommitted. FZ-63 (#119), FZ-60 (#116): trees
  found uncommitted on RoosMacbookA (10-30 base); his word.

## Open questions

- To file on his 11-05 13:50 answers (log): ssh README paragraph,
  `fleetz doctor`, `info` contributors, `AGENTS`/`RELAY` boxes; FZ-77 r2.
- #88 worktree: stays or goes; FZ-38 with font-checked glyphs? #81 and
  #79 test edits; pane 1 wheel coalescing, flaky `clipboard::tests`: his yes.

## Next step

Stopped for the day: no dispatch until he says so. Next: FZ-97 (launch
time), FZ-90; on at-a, FZ-83 then FZ-95, FZ-87. FZ-95 priority unset
(his). Suggest `cargo install` (`main` has FZ-85, 86, 81, 98).

