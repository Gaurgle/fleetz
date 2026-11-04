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
choose tickets as you see fit best", log). Astra is the preferred lead,
Fable when Astra is unavailable. Never `cargo install`, tag or release.
UI rules from his FZ-45 look (2026-10-27) apply to every UI ticket (log).

## In flight

- Running since 11-04 11:40, base `fe014f5`, not yet reported: FZ-68
  (#124, divided preview, `fz-worker`, stops for his look), FZ-69 (#125,
  `<`/`>` step from the drawn width, `fz-worker`), FZ-70 (#126, notez's
  markdown colours, `fz-small`). FZ-71 (#127, tree-sitter fenced code,
  deps approved in its body) after FZ-70. Records on the issues.
- FZ-65 (#121): built, diff `9f4018d7160d` unreviewed, held for his
  wheel decision (selects as built, or scrolls with the highlight
  clamped) and his yes to a wrapped-row follow ticket.
- #88 (FZ-38): parked, uncommitted on at-a. FZ-63 (#119), FZ-60 (#116):
  unreviewed trees lost with their worktrees 11-03; redispatch on his word.

## Open questions

- Code files highlighted in the preview: his "later on" (11-04, log), a
  ticket on his yes after FZ-71. The CI YAML needs a tenth grammar.
- #88 worktree: stays or goes; FZ-38 resume with font-checked glyphs?
  #81 and #79: his answer on the existing footer and header test edits.
- Pane 1 wheel coalescing ticket, flaky `clipboard::tests` issue: his yes.

## Next step

Merged 11-04: FZ-67 `2e71935`. Next: review and integrate FZ-69 and
FZ-70 as they report (FZ-70 under the small-look authority, FZ-69
without a look); FZ-68 to Ready to integrate for his look; FZ-71 once
FZ-70 is on `main`; then FZ-65 on his wheel answer (merge-resolution
cycle), the `{`/`}` ticket on his yes, FZ-41 (#97). FZ-63, FZ-60 on
his word, FZ-58 (#114) after FZ-63. FZ-52 not before 0.8.0 and his word.

Start a lead from the repo root: `claude --agent fz-coordinator`
(subscription, no provider override); Codex from `AGENTS.md`.
