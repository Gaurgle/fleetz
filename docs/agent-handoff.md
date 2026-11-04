# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Session `ea56f412` (`claude-fable-5-1`, `at-a`) took the baton
2026-11-04 09:15 CET on his "continue"; `11cc93e2` released it 11-03.
Three workers running (In flight). Worktrees on at-a: `issue-121`,
`issue-124`, `issue-125`, `issue-126`, `issue-88`; none on `jean-1`.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |

The 10-30 stop was lifted 2026-11-02 20:18 ("lets keep working then. you
choose tickets as you see fit best", log). Astra is the preferred lead.
Never `cargo install`, tag or release. His FZ-45 look rules (10-27, log)
apply to every UI ticket.

## In flight

- FZ-68 (#124, divided preview): Ready to integrate at `7694c29` (merge
  with `main` reviewed, CI green); waits for his look and his tab-strip
  answer (`[/] NOTES` alone in `notes`, or an elided strip later).
  FZ-71 (#127, tree-sitter, deps approved in its body): `fz-worker`
  from `01b51cb`, 12:30. FZ-72 (#128, `ssh` label, Nerd warning glyph):
  Ready on his pasted yes 12:20, stops for his look. Records on issues.
- FZ-65 (#121): built, diff `9f4018d7160d` unreviewed, held for his wheel
  decision (select as built, or scroll clamped) and a wrapped-row ticket.
- #88 (FZ-38): parked, uncommitted on at-a. FZ-63 (#119), FZ-60 (#116):
  unreviewed trees lost with their worktrees 11-03; redispatch on his word.

## Open questions

- His yes wanted (asked 12:23, log): a pane 2 `worktrees` view (none
  filed), the FZ-66 band on every list, an ssh README paragraph in
  FZ-72, the `yy`/`yp` yank design (lead's table in the log). Code
  files highlighted after FZ-71; the CI YAML needs a tenth grammar.
- #88 worktree: stays or goes; FZ-38 resume with font-checked glyphs?
  #81 and #79: his answer on the existing footer and header test edits.
- Pane 1 wheel coalescing ticket, flaky `clipboard::tests` issue: his yes.

## Next step

Merged 11-04: FZ-67 `2e71935`, FZ-70 `f968c79`, FZ-69 `01b51cb`. Next:
merge FZ-68 on his look (fresh gate); FZ-71 through review; FZ-72 in
the second worker slot; then FZ-65 on his wheel answer (a merge-
resolution cycle), the `{`/`}` ticket on his yes, FZ-41 (#97). FZ-63,
FZ-60 on his word, FZ-58 (#114) after FZ-63. FZ-52 after 0.8.0, his word.

Start a lead at the root: `claude --agent fz-coordinator`; Codex: `AGENTS.md`.
