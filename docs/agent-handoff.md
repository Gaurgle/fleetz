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

- Running, records on the issues: FZ-68 round 2 (#124, the flush
  divider on his accepted sketch, `fz-worker`, branch at `7694c29` plus
  an in-progress merge of `main`; stops for his look), FZ-74 (#130,
  `yy`/`yp`, `fz-worker` from `8f92063`; his look), FZ-76 (#133, icons
  repaint live, `fz-small`, in review, diff `c4b7c008e590`; no look).
  Ready: FZ-77 (#134, worktrees view), FZ-78 (#132, `M` all files,
  after FZ-68). FZ-75 (#131) is another session's research ticket.
- FZ-65 (#121): built, diff `9f4018d7160d` unreviewed, held for his wheel
  decision (select as built, or scroll clamped) and a wrapped-row ticket.
- #88 (FZ-38): parked, uncommitted on at-a. FZ-63 (#119), FZ-60 (#116):
  unreviewed trees lost with their worktrees 11-03; redispatch on his word.

## Open questions

- His yes wanted (asked 12:23, log): a pane 2 `worktrees` view (none
  filed), the FZ-66 band on every list, an ssh README paragraph. Code
  files highlighted after FZ-71; the CI YAML needs a tenth grammar.
- #88 worktree: stays or goes; FZ-38 with font-checked glyphs? #81 and
  #79 test edits; pane 1 wheel coalescing, flaky `clipboard::tests`: his yes.

## Next step

Merged 11-04: FZ-67 `2e71935`, FZ-70 `f968c79`, FZ-69 `01b51cb`, FZ-73
`14f44e3`, FZ-71 `9130491`, FZ-72 `8f92063`. Next: review and merge
FZ-76; FZ-68 round 2 and FZ-74 to his look; FZ-77 in the next worker
slot, FZ-78 after FZ-68; the `info` boxes ticket on his sketch yes
(ideas.md); then FZ-65 on his wheel answer, the `{`/`}` ticket on his
yes, FZ-41 (#97). FZ-63, FZ-60 on his word, FZ-58 after FZ-63.

Start a lead at the root: `claude --agent fz-coordinator`; Codex: `AGENTS.md`.
