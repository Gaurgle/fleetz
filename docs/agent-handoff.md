# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each ticket's issue holds its record and rulings, `docs/agent-log.md` holds
finished work, the owner's dated words and the practical notes. Also read
`release-plan.md`.

## Current lead

Session `018zBNkN`, took over 2026-10-30 19:35 CET on `claude-fable-5-1`;
the client switched it to `claude-opus-5-5` at 19:38 (not the lead's
choice). Two `fz-worker` running since 19:40 (FZ-63, FZ-60).
Leads run on the `at-a` machine as well as this one: worktree state is
per machine, reconcile with `git worktree list` before trusting any list.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |

His stop of 2026-10-29 15:35 lifted 2026-10-30 19:38 for tickets that
need no input from him ("Dispatch the ones my input is not needed").
Astra is the preferred lead, Fable when Astra is unavailable. Never `cargo
install`, tag or release. UI rules from his FZ-45 look (2026-10-27) apply
to every UI ticket; see the archive in the log.

## In flight

- FZ-59 (#115): Ready to integrate, gate owner look. `b5b4168` on
  `feat/115-markdown-reader`, branch CI 40876859246 green, verified in a
  scratch merge with `main`. Merge with `Closes #115` on his word.
- #96: Ready to integrate, gate owner look. #88 (FZ-38): parked, uncommitted.
- FZ-63 (#119), FZ-60 (#116): In progress from `41ecd76`, worktrees
  `issue-119`, `issue-116` here; records on the issues.

## Open questions

- Syntax highlighting route: tree-sitter as notez (eleven crates), markdown
  only, or own. A dependency decision, then a ticket on his yes.
- #88 worktree: stays or goes; FZ-38 resume with font-checked glyphs?
- #81 and #79: his answer on the existing footer and header test edits.
- Pane 1 wheel coalescing ticket and the flaky `clipboard::tests` issue
  need his yes.

## Next step

Review FZ-63 and FZ-60 as they report; both stop at Ready to integrate
for his look. FZ-58 (#114) after FZ-63 merges (same tree). On his word:
merge FZ-59 and #96, then FZ-41. FZ-52 not before 0.8.0 and his word.
Try FZ-59: `cargo run --release --manifest-path
.claude/worktrees/issue-115/Cargo.toml` on the machine that holds it.

Start a lead from the repo root: `claude --agent fz-coordinator` (normal
permissions, subscription, no provider override); Codex from `AGENTS.md`.
