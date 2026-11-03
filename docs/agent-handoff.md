# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each ticket's issue holds its record and rulings, `docs/agent-log.md` holds
finished work, the owner's dated words and the practical notes. Also read
`release-plan.md`.

## Current lead

Session `11cc93e2`, `claude-fable-5-1`, `at-a` (smbp), took over 2026-11-03
12:00 CET from `018zBNkN` on `jean-1` (stopped, confirmed by Andreas).
Nothing running. Worktrees are per machine: `issue-115`, `issue-96` and
`issue-88` are here; `issue-119` and `issue-116` appear on neither
machine's `git worktree list` so far (ssh to `jean-1` refused, 11-03).

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |

The 10-30 stop was lifted 2026-11-02 20:18 ("lets keep working then. you
choose tickets as you see fit best", log). Astra is the preferred lead,
Fable when Astra is unavailable. Never `cargo install`, tag or release.
UI rules from his FZ-45 look (2026-10-27) apply to every UI ticket (log).

## In flight

- FZ-66 (`{`/`}` from pane 2 step the CI bodies): proposed 11-03 on his
  ask, filed and dispatched on his yes. FZ-65 (#121): built, diff
  `9f4018d7160d` unreviewed, held for his wheel decision (wheel selects
  as built, or scrolls with the highlight clamped) and his yes to a
  ticket for the wrapped-row follow mismatch. Records on the issues.
- #88 (FZ-38): parked, uncommitted on at-a.
- FZ-63 (#119), FZ-60 (#116): their unreviewed trees from `41ecd76` went
  with worktrees found on neither machine (11-03); redispatch on his word.

## Open questions

- Highlighting route (fenced code and the CI YAML): `syntect` proposed
  11-03, tree-sitter as notez, or own. His decision, then a ticket.
- #88 worktree: stays or goes; FZ-38 resume with font-checked glyphs?
- #81 and #79: his answer on the existing footer and header test edits.
- Pane 1 wheel coalescing ticket, flaky `clipboard::tests` issue: his yes.

## Next step

Merged 11-03: FZ-59 `4545a1f`, #96 `98b63e5`, FZ-64 `cbb6cfb`. Next:
FZ-65 on his wheel answer, FZ-66 on his yes, FZ-41 (#97) after them
(keeps the tab line). Script and log highlighting in the inspector is
filed once he picks the YAML route (`syntect` or own). FZ-63 and FZ-60:
redispatch from `main` on his word, FZ-58 (#114) after FZ-63 merges.
FZ-52 not before 0.8.0 and his word.

Start a lead from the repo root: `claude --agent fz-coordinator` (normal
permissions, subscription, no provider override); Codex from `AGENTS.md`.
