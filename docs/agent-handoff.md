# Fleetz coordinator handoff

The baton between leads: at most 60 lines, read on every start. GitHub
(the issues and https://github.com/users/Gaurgle/projects/1) holds status,
each issue its record and rulings, `docs/agent-log.md` finished work, the
owner's dated words and the practical notes. Also read `release-plan.md`.

## Current lead

Baton released 2026-11-19 19:35 CET by session `0dbccbeb`
(`claude-fable-5-1`, at-a) on his "we can stop for today after the
worker and/or ticket is finished". No agent running. `main` at
`159af0c`: the 0.7.0 bump (one commit, the last before the tag) on
top of the version label merge `15acc3f`; the tag is his.

## Standing authority

| What | Granted | Scope | Until |
|---|---|---|---|
| Integrate under safe-merge; coordination docs straight to `main` | 2026-10-01 setup, `CLAUDE.md` Ship policy | the release effort | revoked by word |
| Choose and dispatch tickets from the board | 2026-10-27 11:05, "you, as the lead, have authority to decide what tickets to start on" | two workers plus one `fz-small`; look items stop for him; FZ-52 and the `0.7.0` bump need his explicit word; issues only on his yes | revoked by word |
| Small look changes merge without his try | 2026-10-02 | a character, color, label, status line, small row or badge; new screens, keys, config keys and test edits still his | revoked by word |
| Existing-test exception | validation contract in `agent-workflow.md` | a compile-forced new field with no assertion changed | revoked by word |
| Refactor batch (ended) | 2026-11-09 15:14 to 11-19 about 18:45, "refactoring can probably stop here now" (log) | FZ-103 to FZ-124 all Done; R2 (FZ-125) and the survey's seams need a new word | the refactor lands or revoked by word |

Astra is the preferred lead. No `cargo install`, tag or release. His
FZ-45 look rules (10-27, log) apply; subjects end with `(#n)`.

## In flight

- Lead ruling for his review: F1's `include_str!` test change (#166).
  FZ-103 to FZ-113 and FZ-116 to FZ-124 are Done and logged.
- App split complete (`app.rs` 685 lines); the survey landed `9edeea5`
  (log, "Survey after the app-split batch": next seams G1 `git.rs`,
  `ui/header.rs`, S1, A1j, an engine test ticket, a doc-comment
  ticket; all need his word). FZ-125 #182 (R2) in Backlog on his "lets
  wait" (11-19); the (c) shape recommended on the issue.
- Wait on his word: FZ-100 (#157), FZ-102 (#159), the build label,
  FZ-97 (#154); FZ-114 (#171, a feature, not on the board, filed 11-10
  on his "put the contributions restore on a ticket"); FZ-83 and FZ-95
  follow-ups in the log.

## Open questions

- `scripts/measure-modules`? The flaky fixed-dir test in
  `poll/tests.rs:1282` (an existing test, his)? Older in the log.

## Next step

Nothing is authorized. He tags `v0.7.0` on `159af0c` when he wants
the release (`release.yml` publishes; notes from
`scripts/release-notes.sh`). Owed to him: the waiting tickets
explained one by one (FZ-100, FZ-102, FZ-97, FZ-114; "the build
label" is untraceable, ask). Refactoring stopped on his word; FZ-125
in Backlog. `cargo install` again for the label and the version.
