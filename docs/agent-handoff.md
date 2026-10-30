# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each ticket's issue holds its record and rulings, `docs/agent-log.md` holds
finished work, the owner's dated words and the practical notes. Also read
`release-plan.md`.

## Current lead

None. Released 2026-10-30 20:17 CET by session `018zBNkN` (took over
19:35 on `claude-fable-5-1`, client switched it to `claude-opus-5-5` at
19:38). Nothing running: both workers reported and stopped.
Leads run on the `at-a` machine as well as this one: worktree state is
per machine, reconcile with `git worktree list` before trusting any list.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |

Stopped 2026-10-30 20:11 ("stop for today after this"): nothing new
until his word.
Astra is the preferred lead, Fable when Astra is unavailable. Never `cargo
install`, tag or release. UI rules from his FZ-45 look (2026-10-27) apply
to every UI ticket; see the archive in the log.

## In flight

- FZ-59 (#115): Ready to integrate, gate owner look. `b5b4168` on
  `feat/115-markdown-reader`, branch CI 40876859246 green, verified in a
  scratch merge with `main`. Merge with `Closes #115` on his word.
- #96: Ready to integrate, gate owner look. #88 (FZ-38): parked, uncommitted.
- FZ-63 (#119): round 1 fixed, diff `b68ed43af0f5` unreviewed; README
  `Enter` row still to fix. FZ-60 (#116): built, diff `47ed96878515`,
  unreviewed. Both from `41ecd76`, uncommitted in `issue-119`, `issue-116`.

## Open questions

- Syntax highlighting route: tree-sitter as notez (eleven crates), markdown
  only, or own. A dependency decision, then a ticket on his yes.
- #88 worktree: stays or goes; FZ-38 resume with font-checked glyphs?
- #81 and #79: his answer on the existing footer and header test edits.
- Pane 1 wheel coalescing ticket and the flaky `clipboard::tests` issue
  need his yes.

## Next step

On his word: FZ-63 README line (round 2), then re-review; review FZ-60.
Both stop at Ready to integrate for his look. FZ-58 (#114) after FZ-63 merges (same tree). On his word:
merge FZ-59 and #96, then FZ-41. FZ-52 not before 0.8.0 and his word.
Try FZ-59: `cargo run --release --manifest-path
.claude/worktrees/issue-115/Cargo.toml` on the machine that holds it.

Start a lead from the repo root: `claude --agent fz-coordinator` (normal
permissions, subscription, no provider override); Codex from `AGENTS.md`.
