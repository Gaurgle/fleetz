# Fleetz coordinator handoff

Last written 2026-10-21 11:05 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `6c0bef32-e39d-4d66-98b4-edcb6d119773`). Read
this file, the repository instructions, [agent-workflow.md](agent-workflow.md)
and [release-plan.md](release-plan.md) before acting. GitHub (the issues and
the [release Project](https://github.com/users/Gaurgle/projects/1)) is
authoritative for status; this file says why things are where they are and
what to do next. Finished tickets, earlier leads, spent authority, rulings,
inherited decisions, facts, the follow-up list and the longer practical
notes are in [agent-log.md](agent-log.md).

## Baton

**Held** since 2026-10-21 10:50 CEST by `claude-fable-5-1` (session
`6c0bef32-e39d-4d66-98b4-edcb6d119773`), taken on Andreas's "continue"
after the file showed the baton released (2026-10-20 14:40 CEST, same
model, session `08f5d4fe-31ad-4813-8535-0abfc0844250`) and nothing
running. **Nothing is running: no worker, no reviewer, no build, no
background job.** The next lead records its own session and takeover
time here after confirming with Andreas.

- **Installed binary:** `fleetz 0.6.0`, the tagged build, installed
  2026-10-20 under his one-time release.
- **#88 FZ-38 parked** (his "skip the glyphs"): `.claude/worktrees/issue-88`
  keeps the uncommitted implementation with the temporary `I` cycle key
  on `feat/88-pane1-icons` (base `8414319`), nothing pushed; both
  worktree candidates (U+EC7D, U+EC7E) are missing from his Nerd Fonts
  (`fc-query` evidence on the issue). Resuming needs new candidates
  checked with `fc-query`, then his pick, then the hard-wiring. Removing
  the worktree is his call.
- **#79** Ready and **#81** Ready, both waiting on his answers (their
  stop comments); each needs a fresh worker from a new worktree.
- **Clock slip** on 2026-10-20: issue comments ran up to 80 minutes
  ahead of the wall clock (correction on #83). Read the clock before
  every stamp.

## Authorized by the owner

Owner authority, verbatim, newest first. A handoff preserves it; no agent
can create or expand it. Spent entries are in the log ("Owner authority
of 2026-10-17 to 2026-10-20" and earlier).

- **2026-10-21, 11:05.** To the spec's five open points: "1. show me the
  examples (ascii is fine)" (layout still open); "2. ok with me" (the
  thirteen existing tests pinning the strip as pane 4 may change `4` to
  `5` and `[4]` to `[5]`, and the Tab walks gain a pane in FZ-30c, no
  assertion weakened); "3. i dont know what these mean, what
  circumstance?" (keys still open, explained in chat); "4. as per config,
  but first time users have all panes open" (config key `ci_pane`,
  default open; recorded in the spec); "5. sure" (create FZ-30a to e, run
  FZ-30a: #89 to #93 created, #89 dispatched). Also "#79 and #81 you have
  to give me more info about" (explained in chat; no answer yet).
- **2026-10-21, 10:58.** "doesnt matter to me", to the takeover report's
  five options and the coordinator's suggestion (5: trim the handoff; 4:
  the #67 CI pane spec first). Read as: do those two in that order; the
  spec goes to him before any worker. Not read as an answer for #79,
  #81 or #88, nor as a yes to create the follow-up issues.
- **2026-10-20, about 13:20.** "remember to bump versions in a regulated
  and logical way": the versioning rule in `CLAUDE.md` ("Working
  conventions") is the coordinator's reading, applied until he corrects
  it. The next release is `0.7.0` for a feature, `0.6.1` for fixes only;
  bump last before the tag, on his word.
- **2026-10-17, about 16:58.** Still live from his numbered answers (the
  list and all answers are in the log, "Questions of 2026-10-17"): #79
  takes form (a) `⚠ github offline at 12:02, as of 11:02`, shown always,
  and stops at Ready to integrate for his look; "3. yes, you decide
  order": batch 3 (#67, FZ-14, FZ-15, FZ-16) may start, order the
  coordinator's, each spec first; "7-10. ok with me, i trust.": issues
  may be created for one measure for width, the ref query cost and the
  small review follow-ups, cut as the coordinator sees fit; "14. lets do
  this later." (release scope).
- **2026-10-16, about 15:40 and 15:48.** "i think all tickets should be
  completed, but prioritize. polish on polish on polish can be deferred,
  or pickued up in parallel when it fits. But the release is not in a
  hurry at all, this is and will still be a project i use, so lets
  prioritize that. I want it too look sleek (still Rust/TUI style) but
  with passion, but the functionality and reliability must come first,
  ofcourse! so all optimizations ARE good, we just need to think of what
  and when they are being worked on. A blocker is a blocker." and "lets
  get to building, I guess?", then "sopunds good. lets get started" to
  the order in [release-plan.md](release-plan.md) ("Dispatch order after
  M1"). Three slots are in force (two `fz-worker` plus one `fz-small`).
  A ticket that changes the screen stops at Ready to integrate for his
  look, except as "small look changes" below says.
- **2026-10-01.** Commits, pushes and merges delegated to the coordinator
  under the safe-merge rule in [agent-workflow.md](agent-workflow.md).

## Authority and routing

Andreas owns priorities, acceptance, what he installs and runs, tags and
releases. The coordinator commits the reviewed diff on the ticket branch,
pushes it and merges into `main` under the safe-merge rule, and commits
coordination docs straight to `main`. When a condition of that rule fails
or is unclear, stop at Ready to integrate and ask. Workers and reviewers
never commit. No agent tags, releases, runs `cargo install`, force-pushes,
rewrites pushed history or deletes remote branches.

Astra is the preferred lead. Fable 5.1 may take over when Astra is
unavailable. Routing: lead `gpt-6-astra` or `claude-fable-5-1` (effort
`high`); Codex workhorse `gpt-6.1-sol`; Claude worker or reviewer
`claude-opus-5-5`, effort `medium`; small work Sonnet or `gpt-6-luna`.
Record the resolved model; never substitute silently. Stop at a usage
limit and ask Andreas for routing; no API billing or paid credits.

## What Andreas told the leads, and still holds

A new lead confirms the first three once before relying on them; the rest
are written into the repository.

- **Pace and dispatch** (2026-10-01, reconfirmed through 2026-10-16 "lets
  get started"). "keep me updated. backend is still vital, but its more
  important that we move forward in a good pace. But keep me updated, and
  ask me about decisions that are bigger". The coordinator dispatches in
  plan order without waiting, rules on small questions, reports at each
  merge, and goes to him for design decisions, contract changes and test
  edits beyond the exception below.
- **More in parallel** (2026-10-03). Two workers may share a file by
  symbol or region, recorded in the assignment comment; merges one at a
  time with the combined check rerun. Never more than two workers.
- **Where his testing matters** (2026-10-01). No "try the build" for
  tickets with nothing to see; a ticket that changes the screen or a key
  goes to him with the branch build before it merges, except:
- **Small look changes no longer wait for him** (2026-10-02). A minor
  on-screen change (a character, a color, a label, a status line, a small
  row or badge) merges under the safe-merge rule once reviewed and is
  reported at the merge. Still his first: a new screen or redesign, a new
  key or changed interaction, contract and owner items, a new config key,
  any test edit beyond the standing exception. When in doubt, ask.
- **Existing tests** ([agent-workflow.md](agent-workflow.md), "Validation
  contract"): editing an existing test is his decision, except a
  compile-forced new field in existing test code with no assertion
  changed. An exhaustive match needing an arm for a new variant: ask.
- **Writing files:** Write or Edit, never heredocs or redirection; command
  text goes through a scratchpad file and `--body-file`.
- **Cleanup is part of Done:** `scripts/relay-sweep main` after the merge
  and green CI on `main`; remote branches stay.
- **Issues:** ask before creating one, except the 2026-10-17 items 7-10.
- **CI changes** were approved ticket by ticket, never in general.

## State of `main`

`main` is `v0.6.0` (`192a838`) plus docs commits, 1179 tests passing and
1 ignored, CI green. 0.6.0 carries FZ-34 to FZ-37 and FZ-26 (details in
the log, "Merged on 2026-10-20"). `libc` is a direct dependency since
#83 (approved). Local toolchain rustc 1.99.0, same as CI's.

## Worktrees

Under `.claude/worktrees/`: only `issue-88` (FZ-38, parked, uncommitted,
see "Baton"). The sweep keeps it.

## In flight

- **#89 FZ-30a** (strip to pane 5): `fz-worker` (`claude-opus-5-5`,
  medium) dispatched 11:15 in `.claude/worktrees/issue-89`, branch
  `feat/89-strip-to-pane-5`, base `5cce618`. Reviewer next, then Ready
  to integrate for his look (a tag and a key). Authority: his "sure"
  (11:05) to creating FZ-30a to e (#89 to #93) and running FZ-30a; his
  "ok with me" to the existing-test edits. #90 to #93 are Backlog and
  wait on the spec's open points (layout, keys) and his word.

## Next step, and the order after it

1. **#67 CI pane spec drafted** (2026-10-21 11:30,
   `docs/superpowers/specs/2026-10-21-ci-pane-design.md`, `1e522f3`,
   comment on #67). Waits on his answers to its "Open with Andreas":
   layout A or B, the thirteen existing-test edits for the renumbering,
   keys, start folded, and the yes to create FZ-30a to e. No worker on
   #67 before that. The handoff trim is done.
2. **Batch 3 after #67:** FZ-14 attention view, FZ-15 changes since seen,
   FZ-16 handoffs, each spec first. Remembered folds (deferred from
   FZ-13o to the dropped FZ-13m) have no home; ask him if he wants them.
3. **#88 FZ-38** if he wants it after all (see "Baton").
4. **#81 and #79** on his answers, each with a fresh worker.
5. **Follow-up issues** (items 7-10 of 2026-10-17, approved): one measure
   for width, the ref query cost at 20,000 refs, the small review
   follow-ups cut into reviewable tickets. Fillers for a free slot. The
   2026-10-20 follow-ups (log, "Follow-ups found and not ticketed") need
   his yes.
6. Batch 4 (FZ-12, FZ-11, the ref query cost), each measured against the
   FZ-01 baseline. Release scope: later, when Andreas asks.

## Open with Andreas

- Whether the parked #88 worktree stays or goes, and whether FZ-38
  resumes with font-checked worktree glyphs.
- #81: yes or no to the two existing footer test edits; #79: (A), (B) or
  (C) on the four existing header tests (stop comments on the issues).
- The pane 1 wheel: his terminal delivers several events per notch; a
  coalescing ticket needs his yes.
- The look changes merged 2026-10-20, now installed: `fetching N MB` and
  hold labels, the `y` status line, the reworded `/` help row.
- The Relay template's `relay-sweep` (`~/claude-config`) has the hole
  fixed here in `d2adb8d`; his repo.
- Before the repository goes public: `docs/ideas.md` names one of his
  private repositories in an older note (told 2026-10-02).
- The two config changes of 2026-10-01 that are his (the stale
  `autoMode.environment` block; the allow rules in
  `.claude/settings.local.json`); reminded 2026-10-17.
- Not blocking, confirmed 2026-10-17: #45 parked until public, #56 not
  prioritized, #61 not promoted, FZ-08 #21 not to be started.

## Practical notes

The longer list is in the log, "Practical notes (longer list)". Needed
every session:

- Absolute paths and `git -C`; never `cd` into a worktree. One command per
  step for commit, push and merge. Wait for CI with a background
  `gh run watch <id> --exit-status`, never a foreground `sleep`.
- Combined checks run in a throwaway clone under the session scratchpad
  with one shared `CARGO_TARGET_DIR`; rerun if `main` moved in code.
- `.claude/agents/` is tracked: stage it with `git add -f`. No `Closes #N`
  in commit messages; issues are closed by hand with the evidence.
- Board edits: project `PVT_kwHOCU842c4BlQ1e`. Status field
  `PVTSSF_lAHOCU842c4BlQ1ezhj-XiE`: Backlog `34f9165b`, Ready `79ac8034`,
  In progress `a2e70ebb`, Review `40798e53`, Ready to integrate
  `d28a1321`, Done `5114f2e5`. Priority field
  `PVTSSF_lAHOCU842c4BlQ1ezhj-ZPA`: P0 `64e82877`, P1 `63d976f2`, P2
  `be37b935`. Worker field `PVTF_lAHOCU842c4BlQ1ezhj-ZPE`, Ticket reviewer
  field `PVTF_lAHOCU842c4BlQ1ezhj-ZPI`. Item ids:
  `gh project item-list 1 --owner Gaurgle --limit 100 --format json`; a
  new issue is added with `gh project item-add 1 --owner Gaurgle --url`.

## Start Fable manually

From this repository, run `claude --agent fz-coordinator`. The agent
definition pins the model and effort and carries the takeover
instructions. Use normal permissions with no bypass, on the Claude
subscription, with no API-key or third-party provider override. Stop if
Fable is unavailable or limited and ask Andreas for routing.
