# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Session `ea56f412` (`claude-fable-5-1`, `at-a`) holds the baton again
since 2026-11-05 12:26 CET, on his batch of feedback (it released it
11-04 16:15). No worker or reviewer running. Worktrees on at-a:
`issue-121`, `issue-124`, `issue-130`, `issue-134`, `issue-88`.

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

- FZ-81 (#137, yank flash, `35f17cc`, diff `7900ef870c0e`, CI green):
  Ready to integrate for his look. FZ-77 (#134, look accepted 11-05):
  merge-resolution worker running against `main` `1affbc7`. FZ-83 (#140,
  CI lamp, option C ruled): `fz-worker` running. Ready: FZ-84 to FZ-87,
  FZ-89 to FZ-92, FZ-78, FZ-79. FZ-80 Backlog. Stop after these.
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

Merged 11-05: FZ-74 `fb26449`, FZ-88 `334e9a6`, FZ-82 `fa3c5ec`, FZ-68
`1affbc7`. Next: finish FZ-77's merge (review, gate, merge) and FZ-83
(review, his look); FZ-81 on his look. Then, on his word: FZ-85, FZ-86,
FZ-84, FZ-87, FZ-89, FZ-90 to FZ-92, FZ-77 round 2, FZ-78, FZ-79. One
target dir per worktree, pruned at merge (log).

Start a lead at the root: `claude --agent fz-coordinator`; Codex: `AGENTS.md`.
