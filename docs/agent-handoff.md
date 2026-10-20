# Fleetz coordinator handoff

Last written 2026-10-17 17:35 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `d63c4af4-55f8-4f4f-ab06-d156d9582c03`, pid 12225)
at the stop Andreas asked for. **Nothing is running: no worker, no
reviewer, no build, no background job. #78, #80 and #82 are Done today;
#81 and #79 are parked, each waiting on one owner answer (see "In
flight"); nothing new starts today.** Read this file, the repository instructions, [agent-workflow.md](agent-workflow.md) and
[release-plan.md](release-plan.md) before acting. GitHub (the issues and the
[release Project](https://github.com/users/Gaurgle/projects/1)) is
authoritative for status; this file says why things are where they are and
what to do next. Finished tickets, earlier leads, spent rulings, inherited
decisions, facts, the follow-up list and the longer practical notes are in
[agent-log.md](agent-log.md).

## Baton

**Held** since 2026-10-20 11:26 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `08f5d4fe-31ad-4813-8535-0abfc0844250`). Takeover
evidence: the 2026-10-17 record below says released with nothing running;
on start no ticket worktree existed under `.claude/worktrees/` (the
directory was gone), no local or remote branch of #79, #81 or #45 exists,
`main` is `edfe449` (docs commits only since 17 October), `relay-check`
up to date, board check clean. Andreas opened the session with FZ-34 (#83),
a P0 he created: "needs to be taken care of asap". That is the only ticket
dispatched this session; #81, #79 and batch 3 stay parked until he says
otherwise.

- **FZ-34 (#83) Done**, merged `dda9105` at 12:22 CEST, `main` CI green,
  worktree and local branch swept; the record is in the log ("Merged on
  2026-10-20"). Nothing is running.
- **Clock slip, corrected here.** The coordinator's "about HH:MM" stamps
  in its comments on #83, #84 and #85 ran up to 80 minutes ahead of the
  wall clock (the same slip the log notes for 2026-10-17). The true
  times, from git and GitHub: rulings 11:28, `libc` decision 11:33, R5
  11:45, R5 scope 11:49, #84 and #85 created 11:52, spec committed 11:54,
  review 1 11:59, spec approved 12:09, ticket commit 12:12, review 2
  12:16, merge 12:22. A correction comment is on #83.
- **FZ-35a (#84) and FZ-35b (#85) Ready**, spec approved (its
  "Decisions" section). Not dispatched: Andreas has not given the word
  to run them; asked at about 12:10. On his yes: FZ-35a first (branch
  `feat/84-fetch-progress`, worktree `.claude/worktrees/issue-84`, base
  current `main`), FZ-35b after it lands. Reviewer follow-ups from #83
  that belong to FZ-35a: claim at permit, a claim token, the unclaimed
  load fetches, and the temp packs left by a cancel inside the 2 s group
  wait.
- **Lost state to tell Andreas:** the worktrees of #81 (its uncommitted
  implementation, revision `686f7e92` in the worker's report) and #45 (the
  uncommitted `ci.yml` and `docs.yml`) are gone from disk and were never
  pushed. Both tickets need a fresh worker if resumed. The board still
  shows #81 In progress, #79 Ready and #45 Ready to integrate.

Previous record: **released** 2026-10-17 17:35 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `d63c4af4-55f8-4f4f-ab06-d156d9582c03`, pid 12225),
which held it from 16:38 CEST. Andreas at about 17:32: "you can start
looking for a place to finish up for today. finsih the started tickets".
**No worker, reviewer, build or background job is running.** The started
tickets: #82 finished (Done); #81 and #79 could not be finished without an
owner answer each and are parked with their state recorded under "In
flight". Nothing new starts today. Next lead records its own session and
takeover time here after confirming with Andreas.

- **How the day ran.** Takeover 16:38 (Andreas: "continue work"). #78
  (FZ-13y, reported by him at 16:40 and ticketed under "lets go with 2"),
  #80 (FZ-13o) and #82 (FZ-16a) Done, each one worker and one reviewer,
  one fix cycle in total (#78, test coverage). #79 created and twice
  stopped by its worker at the existing-test rule; #81 implemented twice
  (the second time under his "invert the order") and stopped at two
  existing tests. He answered the fourteen-question list at 16:58
  (recorded under "Authorized by the owner"), dropped FZ-13m ("we dont
  need a focus mode"), and had `scripts/relay-sweep` fixed after it
  removed #79's parked worktree (`d2adb8d`). The handoff was trimmed
  twice; #70 to #79 were retitled under their epics. Usage was not read.
- **Look not yet seen on `main`:** #75 (the `WT` badge in the dirty colour),
  #71 and #72 (Info header and worktrees section), #80 (one-row folds).
  Run `cargo run --release --manifest-path ~/repos/fleetz/Cargo.toml`.

Earlier lead records, including both 2026-10-16 sessions: the log, "Earlier
leads". Takeover evidence of this session is there too.

## Authorized by the owner

Owner authority, verbatim, newest first. A handoff preserves it; no agent
can create or expand it.

- **2026-10-20, about 12:05.** "no safety question, F keeps the cap, 200
  is fine": the FZ-35 spec is approved with its three points closed
  (recorded in the spec's "Decisions" and on #84 and #85, both Ready).
  Creation and the spec were his "1. yes" of about 11:50 ("Create FZ-35
  (a and b) as P1 in M1, spec first?"), "200mb is good" and "yes apply it
  everywhere" (the cap on every automatic fetch, launch included). Not
  yet given: the word to run FZ-35a; it follows FZ-34's integration.
- **2026-10-20, about 12:25.** "let me know when all is done so i can
  install": he installs himself once told; the coordinator never runs
  `cargo install`.
- **2026-10-20, about 11:32.** "ok, well lets go with libc option then?":
  `libc` approved as a direct dependency for FZ-34 (#83).
- **2026-10-20, about 11:20.** Session opened with: "i added a priority 0
  ticket that needs to be taken care of asap; Ticket: FZ-34 (#83)". Read
  as dispatch of #83 now, one worker and one reviewer, merge under the
  safe-merge rule (no screen change). Nothing else is authorized by it.

- **2026-10-17, about 16:58.** Answers to the coordinator's numbered list
  (given in chat; the list is reproduced in the log, "Questions of
  2026-10-17"): "1. yes" (retitle #70 to #79 under their epics and keep
  the FZ ids going; done at 17:00: FZ-04a #70, FZ-13v #71, FZ-13w #72,
  FZ-05h #73, FZ-13x #75, FZ-04b #76, FZ-09g #77, FZ-13y #78, FZ-03a #79;
  #74 stays plain as a one-off); "2 i think look at thr branch build
  first? always use "ago" even if 0 minutes", then a paste of option (a)
  alone, then "yes" to the coordinator's reading: #79 takes form (a)
  `⚠ github offline at 12:02, as of 11:02`, shown always with no
  threshold, and stops at Ready to integrate for his look; "3. yes, you
  decide order" (batch 3 may start: #67, FZ-14, FZ-15, FZ-16, order the
  coordinator's); "4. the last saved config would be restored, if
  possible." (FZ-13m restore target); "5. lets try it out, yes." (FZ-13n
  hints follow focus); "6. yes, economical" (FZ-13o); "7-10. ok with me,
  i trust." (issues may be created for the `o` shell mouse capture, one
  measure for width, the ref query cost, and the small review follow-ups;
  how to cut them is the coordinator's); "11. close it, but note it if
  something would break later on." (#46 closed 17:03 with the note);
  "12. yes" (#45 parked, #56 unprioritized, #61 unpromoted); "13. ok"
  (the #77 ruling stands); "14. lets do this later." (release scope).
- **2026-10-17, about 16:47.** "ok to create the header issue too", then
  "give me all questions that needs to be answered". The coordinator reads
  the first as create and run, like #78 ("too"): issue #79 created and
  dispatched in the second worker slot. The header wording is still his:
  #79 stops at Ready to integrate unless he accepts the example
  (`⚠ github offline 2h ago, as of 3h`) in chat first. He may correct the
  reading; nothing merges without the wording answer.
- **2026-10-17, about 16:45.** Andreas reported the GitHub contribution
  strip not adding the new day's square across midnight, then "i was wrong,
  it did update" (a poll had run). The coordinator offered (1) leave it,
  (2) ticket it as a filler for a free slot, not ahead of batch 3, and asked
  whether to create the issue. Andreas: "yeah, lets go with 2." The
  coordinator reads it as: create the issue (#78) and run it in a free
  slot; all slots were free, so it was dispatched at once. A small look
  change under the 2026-10-02 standing instruction.
- **2026-10-16, about 17:33 and 17:12.** Spent: they covered #72 to #77,
  all Done. Verbatim in the log, "Owner authority before 2026-10-16".
- **2026-10-16, about 15:40 and 15:48.** "i think all tickets should be
  completed, but prioritize. polish on polish on polish can be deferred, or
  pickued up in parallel when it fits. But the release is not in a hurry at
  all, this is and will still be a project i use, so lets prioritize that.
  I want it too look sleek (still Rust/TUI style) but with passion, but the
  functionality and reliability must come first, ofcourse! so all
  optimizations ARE good, we just need to think of what and when they are
  being worked on. A blocker is a blocker." and "lets get to building, I
  guess?". The coordinator proposed the order in
  [release-plan.md](release-plan.md) ("Dispatch order after M1") and a
  first batch of three (A #70, B #72 spec first, C #71), asking for an OK
  to create the three issues and for "run batch 1". Andreas: "sopunds
  good. lets get started". Three slots are in force (two `fz-worker` plus
  one `fz-small`). A ticket that changes the screen stops at Ready to
  integrate for his look, except as "small look changes" below says.
- **2026-10-01.** Commits, pushes and merges delegated to the coordinator
  under the safe-merge rule in [agent-workflow.md](agent-workflow.md).

Spent entries (2026-10-02 to 2026-10-10) and the one-time releases on
finished tickets are in the log, "Owner authority before 2026-10-16" and
"Rulings and one-time releases".

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
`claude-opus-5-5`, effort `medium`; small work Sonnet or `gpt-6-luna`. Record
the resolved model; never substitute silently. Stop at a usage limit and ask
Andreas for routing; no API billing or paid credits.

## What Andreas told the leads, and still holds

A new lead confirms the first three once before relying on them; the rest are
written into the repository.

- **Pace and dispatch** (2026-10-01; confirmed "Both hold" on 2026-10-02
  18:34; "keep going" at about 21:49; "continue the work from handoff" and
  "good! all ok from me." on 2026-10-03; "can you pick up the baton and keep
  going from the handover" and "sounds good to me" on 2026-10-09; "lets get
  started" on 2026-10-16). "keep me updated. backend is still vital, but
  its more important that we move forward in a good pace. But keep me
  updated, and ask me about decisions that are bigger". Under it the
  coordinator dispatches in plan order without waiting, rules on small
  questions, reports at each merge, and goes to him for design decisions,
  contract changes and test edits beyond the exception below.
- **More in parallel** (2026-10-03, about 17:35). "we are only running one
  agent right now, i think? Are'nt there any more tickets we can get working
  on in parallell?" Under it the second worker slot takes tickets that share
  a file with the first by symbol or region, recorded in the assignment
  comment; merges one at a time, the combined check rerun against the new
  `main`. Never more than two workers at once.
- **Where his testing matters** (2026-10-01). No "try the build" for tickets
  with nothing to see; a ticket that changes the screen or a key goes to him
  with the branch build before it merges, except as the next item says.
- **Small look changes no longer wait for him** (2026-10-02, about 21:28).
  "i dont think i need to review these small things either, lets keep going
  rather than stopping for minor things at this point". A minor on-screen
  change (a character, a color, a label, a status line, a small row or badge)
  merges under the safe-merge rule once reviewed and is reported at the merge
  with the command to look at `main`. Still his before they are built or
  merged: a new screen or a redesign, a new key or changed interaction,
  contract and owner items, a new config key, and any test edit beyond the
  standing exception. When in doubt, ask.
- **Existing tests** (in [agent-workflow.md](agent-workflow.md), "Validation
  contract"): editing an existing test is his decision, except a
  compile-forced new field in existing test code with no assertion changed.
  An exhaustive match that needs an arm for a new variant is not a struct
  field: ask each time.
- **Writing files:** Write or Edit, never heredocs or redirection; command
  text goes through a scratchpad file and `--body-file`.
- **Cleanup is part of Done:** `scripts/relay-sweep main` after the merge
  and green CI on `main`; remote branches stay.
- **Issues:** ask before creating one. Allowed so far: #68, FZ-10a to c, the
  M3 issues of 2026-10-01, FZ-13s #56, FZ-09a to f #57 to #62, FZ-13t #63,
  #64, FZ-13u #65, #70 to #72, and the three of 2026-10-16 17:12 above.
- **CI changes** were approved ticket by ticket, never in general.

## State of `main`

`main` is at the merge of #77, `57b885c`, 1090 tests passing and 1 ignored,
CI green. On 2026-10-16 it gained #76 (refetch on recovery, a contract
amendment), #77 (`cache_ttl` removed), #75 (the `WT` badge in the dirty colour, a look
change merged under the standing instruction and not yet seen by Andreas
on `main`), #72 (the Info view's `worktrees` section, look
accepted), #73 (one `model::identity_key` for "same remote", a pure
refactor), #74 (the `CLAUDE.md` Nerd Font sentence), #71 (the Info header's worktree count in
lavender, also not yet seen on `main`) and #70 (ssh fetches bounded at 10 s). The installed
binary is the `main` of 2026-10-10 (`677e8cc`, version `0.5.1`); `main` is
far past `v0.5.1` with a cache format change (FZ-09c), so the next release
is at least `0.6.0`, prepared only when he asks. Local toolchain rustc
1.99.0, same as CI's. To look at `main` without installing:
`cargo run --release --manifest-path ~/repos/fleetz/Cargo.toml`.

## Worktrees

Under `.claude/worktrees/`, besides the main checkout:

- **`issue-45`, branch `ci/45-pull-request-trigger`:** uncommitted `ci.yml`
  and `docs.yml`, reviewer verdict blocked on a repository setting. Parked by
  Andreas until the repository goes public.

The worktrees of #72 (`worktrees-section`), #73 (`one-remote-key`) and #74
(`claude-md-stale`) were removed by the sweep after their merges.

## In flight

- **#79 (FZ-03a), the header dates the failed attempt.** Ready, stopped
  at the existing-test rule, 2026-10-17 about 17:05 CEST. Form (a) shown
  always (owner entry 16:58; the amendment comment on the issue replaces
  the body's examples and criteria). The second worker pass found that
  `GithubFreshness::failed` always sets `last_attempt`, so four existing
  header tests in `src/ui/mod.rs` (lines 1873, 1890, 1950, 2001) would
  show the stamp and need their expected strings changed; the stop
  comment on the issue lists them and asks Andreas for (A) update the four
  strings only, (B) make the stamp conditional, (C) other. The coordinator
  recommends (A). On his answer: re-dispatch into the worktree
  `.claude/worktrees/issue-79`, branch `feat/79-header-attempt-age`,
  **base `84a06b7`** (recreated after the sweep removed the unstarted
  worktree, see the log's follow-ups), allowed files plus `src/app.rs` for
  the `last_attempt` doc comment only. Stops at Ready to integrate for
  his look at the branch build.

- **#81 (FZ-13n), footer hints follow focus.** In progress since 17:08
  under the 16:58 owner entry ("lets try it out, yes."). Base `84a06b7`,
  branch `feat/81-focus-hints`, worktree `.claude/worktrees/issue-81`,
  `fz-worker` (`claude-opus-5-5`, medium). Andreas chose "81: invert the
  order" (about 17:15): the segment keeps its words and the global row
  goes bare first; the five-step order is in the decision comment on the
  issue. Worker revision `686f7e92` implements it (1102 passed, 2
  failed) with the one authorized existing-test edit; two more existing
  footer tests encode the old order and fail: `the_hints_end_at_the_right_
  edge_beside_a_glyph_summary` (run it at 220 columns) and
  `the_footer_drops_descriptions_before_the_summary` (start the walk at
  260). Asked Andreas for those two edits (chat and the stop comment on
  the issue). On his yes: the worker makes exactly those two changes,
  then `fz-reviewer`, commit, push, CI, combined check, Ready to
  integrate for his look at the branch build. The worktree holds the
  uncommitted diff; the sweep keeps it.

Both parked tickets resume under the 16:58 owner entry once he answers;
no new authorization is needed, only the answer. A lead that resumes one
re-reads the issue's last two comments first.

#78, #80 and #82 are Done (merged `84a06b7`, `3c4dcf0`, `94bec02`; records
in the log).

#75, #76 and #77 are Done; one owner-visible ruling on #77 (the deleted
precondition assertion in
`startup_serves_an_expired_payload_rather_than_waiting_on_the_network`,
under "do what you need for cache_ttl") stands unless Andreas overrides it.
The records are in the log.

## Next step, and the order after it

All under the 2026-10-17 16:58 owner entry unless noted. Nothing starts
until Andreas opens the next session; "finish the started tickets" of
17:32 covers #81 and #79 only.

1. **#81** on his yes/no for the two existing footer tests: the worker
   (or a fresh `fz-worker` from the worktree's diff) makes exactly those
   two edits, then `fz-reviewer`, commit, push, CI, combined check, Ready
   to integrate for his look at the branch build.
2. **#79** on his (A)/(B)/(C): re-dispatch into the clean worktree, review,
   commit, push, CI, combined check, Ready to integrate for his look.
3. **Batch 3, order the coordinator's:** #67 CI pane first (an issue
   exists; design-heavy, so a spec under `docs/superpowers/specs/` goes to
   Andreas before any worker; its own body says the FZ-13 pane work lands
   first, which it now has), then FZ-14 attention view, FZ-15 changes
   since seen, FZ-16 handoffs, each spec first. Two `fz-worker` slots plus
   one `fz-small`. Remembered folds (deferred from FZ-13o to FZ-13m) have
   no home now that FZ-13m is dropped; ask him if he wants them at all.
4. **Follow-up issues** (7 to 10 of the list, approved): one measure for
   width, the ref query cost at 20,000 refs (batch 4 material, measured),
   the small review follow-ups cut into reviewable tickets (the
   fetch-outcome items as one; the settings-save cursor reset as one; test
   sharpening as one `fz-small`). Fillers for a free slot. The `o` shell
   one is Done (#82).
5. Batch 4 (FZ-12, FZ-11, the ref query cost), each measured against the
   FZ-01 baseline. Release scope: later, when Andreas asks.

## Open with Andreas

- #81: yes or no to the two existing footer test edits (the stop comment
  on the issue); then his look at the branch build.
- #79: (A), (B) or (C) on the four existing tests (the stop comment on
  the issue); then his look at the branch build.
- The Relay template's copy of `relay-sweep` (the `relay` skill in
  `~/claude-config`) has the hole fixed here in `d2adb8d`; his repo.
- The look of #71, #72, #75, #78 and #80 on `main` whenever he next runs it
  (`cargo run --release --manifest-path ~/repos/fleetz/Cargo.toml`); the
  installed binary is still the `main` of 2026-10-10.
- Before the repository goes public: `docs/ideas.md` names one of his
  private repositories in an older note (told to him 2026-10-02).
- The two config changes of 2026-10-01 that are his to make (the stale
  `autoMode.environment` block; the allow rules in
  `.claude/settings.local.json`); reminded 2026-10-17, no answer yet.
- Not blocking, confirmed 2026-10-17: #45 parked until public, #56 not
  prioritized, #61 not promoted, FZ-08 #21 not to be started. Release
  scope: later.

## Practical notes

The longer list is in the log, "Practical notes (longer list)". The ones
needed every session:

- Absolute paths and `git -C`; never `cd` into a worktree. One command per
  step for commit, push and merge. Wait for CI with a background
  `gh run watch <id> --exit-status`, never a foreground `sleep`.
- Combined checks run in a throwaway clone under the session scratchpad with
  one shared `CARGO_TARGET_DIR`; rerun if `main` moved in code.
- `.claude/agents/` is tracked: stage it with `git add -f`. No `Closes #N`
  in commit messages; issues are closed by hand with the evidence.
- Board edits: project `PVT_kwHOCU842c4BlQ1e`. Status field
  `PVTSSF_lAHOCU842c4BlQ1ezhj-XiE`: Backlog `34f9165b`, Ready `79ac8034`,
  In progress `a2e70ebb`, Review `40798e53`, Ready to integrate `d28a1321`,
  Done `5114f2e5`. Priority field `PVTSSF_lAHOCU842c4BlQ1ezhj-ZPA`: P0
  `64e82877`, P1 `63d976f2`, P2 `be37b935`. Worker field
  `PVTF_lAHOCU842c4BlQ1ezhj-ZPE`, Ticket reviewer field
  `PVTF_lAHOCU842c4BlQ1ezhj-ZPI`. Read item ids with
  `gh project item-list 1 --owner Gaurgle --limit 100 --format json --jq ...`;
  a new issue is added with `gh project item-add 1 --owner Gaurgle --url ...`.

## Start Fable manually

From this repository, run `claude --agent fz-coordinator`. The agent
definition pins the model and effort and carries the takeover instructions.
Use normal permissions with no bypass, on the Claude subscription, with no
API-key or third-party provider override. Stop if Fable is unavailable or
limited and ask Andreas for routing.
