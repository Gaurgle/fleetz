# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each ticket's issue holds its record and rulings, `docs/agent-log.md` holds
finished work, the owner's dated words and the practical notes (the
pre-0.8.0 baton is archived there verbatim, moved 2026-10-30). Also read
`release-plan.md`.

## Current lead

None. Released 2026-10-29 14:00 CET by `claude-fable-5-1` (session
`1ce7d882`) on "we can paus for today when this is done". Nothing running.
Leads run on the `at-a` machine as well as this one: worktree state is
per machine, reconcile with `git worktree list` before trusting any list.
Migrated to Relay 0.8.0 on 2026-10-30 by the advisor session, no lead active.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |

Paused by his stop of 2026-10-29 15:35: nothing new starts until his word.
Astra is the preferred lead, Fable when Astra is unavailable. Never `cargo
install`, tag or release. UI rules from his FZ-45 look (2026-10-27) apply
to every UI ticket; see the archive in the log.

## In flight

- FZ-59 (#115): Ready to integrate, gate owner look. `b5b4168` on
  `feat/115-markdown-reader`, branch CI 40876859246 green, verified in a
  scratch merge with `main`. Merge with `Closes #115` on his word.
- #96: Ready to integrate, gate owner look. #88 (FZ-38): parked, uncommitted.

## Open questions

- Syntax highlighting route: tree-sitter as notez (eleven crates), markdown
  only, or own. A dependency decision, then a ticket on his yes.
- #88 worktree: stays or goes; FZ-38 resume with font-checked glyphs?
- #81 and #79: his answer on the existing footer and header test edits.
- Pane 1 wheel coalescing ticket needs his yes; the flaky `clipboard::tests`
  issue needs his yes.

## Next step

On his word: merge FZ-59, then FZ-63 (#119) and FZ-58 (#114) as the two
worker slots, FZ-60 (#116) after them, FZ-41 after #96. FZ-52 not before
0.8.0 and his explicit word. Try FZ-59: `cargo run --release
--manifest-path .claude/worktrees/issue-115/Cargo.toml` on the machine
that holds the worktree.

## Start the lead

From the repo root: `claude --agent fz-coordinator` (normal permissions,
Claude subscription, no provider override). A Codex lead starts from
`AGENTS.md`.
