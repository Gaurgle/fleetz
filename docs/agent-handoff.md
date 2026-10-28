# Fleetz coordinator handoff

Last written 2026-10-21 20:42 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `6c0bef32-e39d-4d66-98b4-edcb6d119773`) at the
release of the baton (the stamps of this session ran up to ten minutes
ahead of the clock at times; commit dates are authoritative). Read
this file, the repository instructions, [agent-workflow.md](agent-workflow.md)
and [release-plan.md](release-plan.md) before acting. GitHub (the issues and
the [release Project](https://github.com/users/Gaurgle/projects/1)) is
authoritative for status; this file says why things are where they are and
what to do next. Finished tickets, earlier leads, spent authority, rulings,
inherited decisions, facts, the follow-up list and the longer practical
notes are in [agent-log.md](agent-log.md).

## Baton

**Released** 2026-10-28 16:55 CET by `claude-fable-5-1` (`claude
--agent fz-coordinator`, session `2883bc20-7992-4566-8140-9e302e7b1d25`)
on his "ok. make that change, ship, and then stop for today?". Held
from 16:04 ("yes, lets go"). The stamps this session wrote on the
issues between the first dispatch and the FZ-61 merge ran up to an
hour ahead of the clock; commit dates are authoritative. **Nothing
is running: no worker, no reviewer, no build, no background job.**
Everything is pushed. Done this session, all merged to `main` on his
looks and words (records in the log, "Merged on 2026-10-28"): FZ-56
(#112) `3353adc`, FZ-55 (#111) `9643cd8`, FZ-61 (#117) `30a9883`;
`main` CI 40719616971 green, all three worktrees swept.
Filed: FZ-60 (#116, the `changes` view, Ready, P2, M4, design in
`docs/ideas.md` "Changed files in pane 2") and FZ-61 (#117, Done).
His stop of 16:28 ("stop after these worker/tickets are done"),
amended at 16:45 to include FZ-61, ended the session: **the standing
dispatch authority of 2026-10-27 11:05 is paused by that stop**; the
next lead needs his word to resume the order under "Next step"
(FZ-57, then FZ-60, FZ-58, FZ-59; his words on FZ-60: "its a go only
if we dont have any other, more important tickets to complete first").
His looks today: "111: scroll margin feels is great.", "112: is
great!", "author it is" (FZ-61's word).

Previous record: **Released** 2026-10-27 15:58 CET by `claude-fable-5-1`
(session `c47a5217-1276-4a26-b7a2-bceecc3c81fb`) on his "wait for
reviewers, then we can close session for today? if hand over is
complete and all is shipped". Held 14:35 to 15:58 (second stretch of
the day; the stamps written in this stretch ran up to 45 minutes
ahead of the clock again, commit dates are authoritative). **Nothing
is running: no worker, no reviewer, no build, no background job.**
Everything is pushed: `main`, and the three open ticket branches at
their recorded tips. Done this stretch: FZ-45 (#101) `7a2a020`, #93
`cb72465`, FZ-53 (#109) `1d4e8b0` with `main` CI 40525825654 green
and the worktree swept. Shipped on branches, reviewed and accepted,
parked: FZ-55 (#111) `84be091`, FZ-56 (#112) `fb0c46b`. Filed:
FZ-57 to FZ-59 (#113 to #115). His decisions this stretch are on the
issues (#109, #111, #93, #96, #115). Incident: a reviewer lost its
scratch copy and wrote into the main checkout; caught by the
pre-merge `git status` guard (see "In flight", #112). The standing
dispatch authority of 11:05 stands for the next lead.

Previous record: **Held** again since 2026-10-27 14:35 CET by the
same session on his "allright. we can continue now".

Previous record: **Released** 2026-10-27 13:20 CET by `claude-fable-5-1` (`claude
--agent fz-coordinator`, session
`c47a5217-1276-4a26-b7a2-bceecc3c81fb`) on his "find a place to stop
for today" and "if anything should be commited or pushed, you should
ship it". Held from 10:57 ("can we continue from where we stopped?").
Nothing was running at that release. Stamps this session ran ahead of the clock again (the log's
clock note for 2026-10-27); commit dates are authoritative. Done
today: FZ-51 (#107) Done `579f28e`; FZ-53 (#109) Ready to integrate
for his look (`0afd18b`); FZ-45 (#101) reviewed and accepted at
`f0c130c`, then his look asked for pane 1's row grammar and a
full-width inspector, delivered and **pushed unreviewed** as
`2cb7cfc` on his "ship it"; FZ-55 (#111) filed, built to a decision
point and **pushed unreviewed and red** as `b926726` (see "In
flight"). His standing dispatch authority of 11:05 stands for the
next lead (see "Authorized by the owner"). FZ-19 (#2) was passed
over: its body is stale (`epoz -d`) and needs a rewrite before it is
Ready. FZ-41 waits for #96 to merge (same `ci` view code). Two UI
rules from his look are under "What Andreas told the leads"; a
CLAUDE.md line for them waits on his yes.

Previous record: **Held** since 2026-10-27 10:57 CET by the same
session on his "can we continue from where we stopped?", resumed
FZ-53 at its review and, from 11:05, dispatched under the standing
authority.

Previous record: **Released** 2026-10-26 16:12 CET by `claude-fable-5-1` (`claude
--agent fz-coordinator`, session
`c47a5217-1276-4a26-b7a2-bceecc3c81fb`) on his "ok on the tests,
then close after FZ-50" and "id rather close it now". Held from
15:11 ("you can dispatch, choose what tickets to start on for now").
**Nothing is running: no worker, no reviewer, no build, no
background job.** The FZ-53 reviewer was stopped mid-review at his
word (its partial work is in this session's transcript only; the
next lead re-runs the review from the handoff in "In flight"). Done
this session: FZ-53 (#109) agreed and filed as an advisor before the
takeover (`27d750c`); FZ-49 (#105) Done `0db0d32`; FZ-50 (#106) Done
`4632ac5` (records in the log, "Merged on 2026-10-26", with the clock
note: this session's issue and chat stamps ran up to 70 minutes ahead
of the wall clock; commit dates are authoritative). One lesson,
recorded on #109: the lead ruled seven existing-test assertion edits
in under the standing exception and the worker correctly refused;
that exception covers only a new struct field, every other test edit
is his. He gave the word at 16:00 ("ok on the tests"). His standing
scope ("choose what tickets to start on for now") was given to this
session; the next lead asks before dispatching rather than inheriting
it.

Previous record: **Released** 2026-10-23 18:35 CEST by `claude-fable-5-1` (`claude
--agent fz-coordinator`, session
`d55ed60b-66ae-4198-a152-2c5eb3c46433`) on his "i want to start
closing down for tonight" and "start nothing new". Held from 15:00
("you can be the lead."). **Nothing is running: no worker, no
reviewer, no build, no background job.** Done this session: the repoz
convergence plan written and agreed, the design accepted with the name
decided (the binary becomes `repoz`; print mode is a flag of it),
FZ-46 to FZ-52 filed (#102 to #108, milestone `repoz convergence`), P1
committed in `~/repos/repoz` (`3fab42a`, `fix/fleetz-lessons`, pushed),
and three tickets Done: FZ-46 `36d1f20`, FZ-47 `289a1bf`, FZ-48
`06e5026` (records in the log, "Merged on 2026-10-23"; the clock note
there applies to this session's stamps). He tried `--print` on the
real fleet: "worked fine, and the print is perfect". His 7-day usage
was at 83% at about 17:30; the next lead should expect the limit. His
ruling 16:05: the convergence is a
step toward the public release, not its trigger; M5 still needs its
own tickets (worktree page in pane 2 and more); recorded in the plan.
His words: 15:18 "you may dispatch" (FZ-46 only); "#93, #96 and 0.7.0 can
wait, until all is in place. if needed, go to 0.6.1 instead" (a patch
release if only fixes land first); 15:38 "you may commit and push as
you need" (read as: the P1 commit in repoz and the coordination docs;
not as dispatch of FZ-47, which is asked below).

Previous record: **Released** 2026-10-22 17:58 CEST by `claude-fable-5-1` (`claude
--agent fz-coordinator`, session
`2e4524bf-8172-4d7c-8bd5-966212f13452`) on his "can i close this
session?": yes. **Nothing is running: no worker, no reviewer, no
build, no background job.** Held 10:50 to 17:58 (his "give me a brief
status, then lets continue"; the baton was first released at 16:20,
then held again for his dotfiles and claude-config shipping, outside
fleetz, and for the FZ-41 to FZ-45 tickets). Done today: the #92 UTC
hotfix (`bd0e4ee`) and #95 FZ-30f (`db04b56`); #93 and #96 at Ready to
integrate for his look; FZ-41 to FZ-45 (#97 to #101) created in
Backlog with their specs, not dispatched. The next lead records its own session and takeover time
here after confirming with Andreas. Chore for the next quiet moment:
this file is 420 lines against the 150 cap; move the 2026-10-21
records and the spent authority entries to the log. First finding at
the takeover: the
`ci` workflow was **red on `main` (`69c1b5b`) and on both ticket
branches**, contrary to yesterday's "branch CI green": three `ui::ci`
tests from #92 were timezone-dependent. Fixed and merged as `bd0e4ee`
on his word (log, "Merged on 2026-10-22"); `main` CI green again.
Lesson for every lead: "branch CI green" means the `ci` workflow, not
`docs`; confirm with `gh run list --branch <b> --workflow ci`.

Previous record: **Released** 2026-10-21 20:33 CEST by `claude-fable-5-1` (session
`6c0bef32-e39d-4d66-98b4-edcb6d119773`) at the end of the unattended
run he asked for at 18:52 ("keep going for 2 hours without me
interweining", stop at 21:30): the run ended early at its cap, with
#93 and #95 both at Ready to integrate for his look and the FZ-14
spec drafted. **Nothing is running: no worker, no reviewer, no build,
no background job.** Earlier today the same session held the baton
10:50 to 16:10 (seven tickets Done) and 18:55 to 20:33. The next lead
records its own session and takeover time here after confirming with
Andreas.
**Nothing is running: no worker, no reviewer, no build, no background
job.** Seven tickets went Done today (#89, #79, #81, #90, #94, #91,
#92; records in the log, "Merged on 2026-10-21"). The next lead
records its own session and takeover time here after confirming with
Andreas.

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

- **2026-10-27, 11:05 CET, standing dispatch authority.** "you, as
  the lead, have authority to decide what tickets to start on. if
  unsure, do a status check on the project", after "can we continue
  from where we stopped?". Read as: a standing scope for the lead to
  choose and dispatch tickets from the board, within the workflow's
  limits (two workers plus one `fz-small`, safe-merge rule, look
  items stop for him, FZ-52 and the `0.7.0` bump still need his
  explicit word, issues created only on his yes). Not time-boxed; it
  carries to the next lead as recorded here until he withdraws it.

- **2026-10-26, 15:10 CET, lead takeover and dispatch.** "you can
  dispatch, choose what tickets to start on for now." after "you may
  commit and push" (the FZ-53 docs, `27d750c`) and "sounds all good to
  me. add it? lets try a markdown / notez-only toggle? and leave the
  personal notes out for now. ready to put it as tickets?" (FZ-53,
  #109, filed). Read as: takeover; dispatch of the lead's choice from
  the recorded order and the new ticket, within the two-worker plus
  one `fz-small` limit; merges under the safe-merge rule. Chosen:
  FZ-49 (#105, next in the convergence order) and FZ-53 (#109).
  **Not** read as: merging #93 or #96 (they wait for his look), the
  `0.7.0` bump, FZ-52, or any ticket needing his look before merge.
  "for now" is a standing scope until he says stop.

- **2026-10-23, 14:58 to 15:00, lead takeover.** "i see. but i want the
  name for the TUI (fleetz) to actually be repoz, its just a better name.
  and (current) wont need it's own name, since it'll be a command version
  of fleetz"; "repoz-sh is a good name for it, if we want to keep it still
  as its own repo/"; then "you can be the lead. i agree on all these. as
  long as we dont touch the code of fleetz, except renaming it" to the
  plan `docs/superpowers/plans/2026-10-23-repoz-convergence-plan.md` and
  the four asks under it (takeover; design D1 to D9 with O2 to O5
  defaults; file FZ-46 to FZ-52 and the milestone, Backlog, not
  dispatched, and commit the docs; P1 is his). Read as: takeover; design
  accepted; O1, O6, O8 decided as the design now records; issues #102 to
  #108 and milestone 7 created; the docs committed to `main`. **Not**
  read as dispatch of any ticket, nor as the word for FZ-52 ("do NOT
  start on any name changing right now" stands). Standing constraint:
  fleetz's existing code is not changed except where print mode's wiring
  needs it, and by FZ-52 for the name.

- **2026-10-23, advisor session (not a lead; the baton stayed
  released).** "yes. but remember to use relay. set it up, and i can
  start a relay lead for it after.", answering the offer to write the
  repoz print mode design, after "exactly this. the actual command can
  be decided later. but i think this is the correct way to go." Spent
  on: `docs/repoz-print-design.md` (proposed, not accepted), this entry,
  "Next step" item 0, the CLAUDE.md index row and the release plan
  section. Read as: the effort runs under this Relay; **not** read as
  acceptance of the design, as the word to file FZ-46 to FZ-51, or to
  dispatch. Earlier the same day: "do NOT start on any name changing
  right now".

- **2026-10-22, 17:50 and 17:53.** "yes on all" to the slot digest
  proposal (headers as cursor stops, local-first numbers, spec and
  ticket), then "write them up and ship it. no need to work on this
  right now". Read as: create FZ-41 to FZ-45 (#97 to #101) with the
  spec `2026-10-22-slot-digest-design.md` and commit the docs; **no
  dispatch**. Queue after #93 and #96: FZ-41, FZ-45, then FZ-42 to
  FZ-44, each a look change for his look.
- **2026-10-22, 13:05.** "i trudst you, do research and think a
  little, what is useful for the CI workflows to inspect? what do i
  need? etc", after "good!" to the #95 build at 12:30 (read as the
  look OK; merged `db04b56`) with three fixes named: the pane 2 `ci`
  view's width and headlines (FZ-40), a split inspector (jobs list
  left, log or script right), and `j`/`k` plus shift `J`/`K` from pane
  2 for the jobs list. Read as: the design is the coordinator's
  (spec, "Amendment 2026-10-22, second"), FZ-40 runs now beside the
  #93 rework. Not read as the word to create FZ-41 to FZ-44 (asked at
  13:20).
- **2026-10-22, 11:45.** "ah, my bad. Capital O for that is good!
  yes to all." after his look at #93 and #95 (findings and screenshots
  on #95; neither accepted). Read as: #95 fix cycle 3 (jobs as a
  selectable list in the inspector, option (a); lazy log tail; the
  bodies scroll; `]` routing), #93's `l` retargeted to the inspector's
  log body with no suspend, after #95 merges; FZ-40 (#96) created for
  the pane 2 `ci` view's headline and width budget, after #95; the
  run's facts stay the inspector's header. All in the spec's
  "Amendment 2026-10-22". Both tickets and FZ-40 remain look changes
  for his look; nothing merges unattended.
- **2026-10-22, 10:53.** "yes, you know what to do, go", to the
  takeover report's proposal: fix the three UTC-dependent `ui::ci`
  tests from #92 by swapping the test helper to wall-clock components
  (an existing-test edit, assertions unchanged), through `fz-small`
  and `fz-reviewer` on `fix/92-ci-tests-utc` from `6a6de99`, merge
  under the safe-merge rule once CI is green, then re-verify #93 and
  #95 against the new `main`. Not read as the merge word for #93 or
  #95: both are look changes with new keys and still wait for his
  look (the report's question 2 had no explicit answer).
- **2026-10-21, 18:52.** "we can continue. I will be on a party for a
  while tho, can you keep going for 2 hours without me interweining?"
  Read as an unattended run under "Unattended runs" in
  agent-workflow.md. **Scope:** the standing batch 3 approval
  (2026-10-17) as amended today: run #95 FZ-30f and #93 FZ-30e in the
  two worker slots to Ready to integrate (both are look changes and
  wait for him; nothing merges without his look), then write the
  FZ-14 attention view spec under `docs/superpowers/specs/` for his
  approval (no worker on it). **Cap:** those two tickets and one spec.
  **Not in scope:** creating issues (the worktree disk-usage feature
  and the default-branch name wait for his yes), merging look changes,
  anything outside batch 3. **Stop conditions:** 21:30 (his "can you
  ruin autonomnously for two hours then stop? stop at 21:30", 18:58),
  the cap, a safe-merge or review blocker surviving two fix cycles, a
  usage limit, or his return; each ends with a report in chat and the
  handoff updated.
- **2026-10-21, 16:05.** "looks great. but yes, we need another color
  for the CI page, perhaps red? and CI page highlight is not showing
  anything in inspector yet". Read as: #92 accepted and merged
  (`69c1b5b`); the `ci` tab colour changes in FZ-30f (#95), the
  coordinator's reading of "perhaps red" being Catppuccin maroon, the
  red family without the failing lamp's red, for his pick at the look;
  FZ-30f (the inspector's bodies) dispatched under the standing
  approval.
- **2026-10-21, about 15:28 (clock read 15:30).** "sound sgood to me" to the #92 rulings of
  15:19: the `ci` tab in sky and the two further existing-test edits
  (the view-cycle wrap test; #90's `ci_folded` test renamed
  `the_ci_poll_waits_for_the_ci_view`) are his.
- **2026-10-21, 15:12 and 15:20.** "why are we not having CI as it's own
  page in pane 2 instead of its own pane?", "id even want to be able to
  scroll thru the ci script/code somehow", then the decision: "i think
  we can have a small addition for CI in the 'INFO' page, that's there
  to shed light on the general status, but then i think CI should be a
  pane 2 page, and not it's own pane. use inspector to inspect CI
  details, and or it's script?" and "having CI on its own pane 2 page
  also lets us lazyily load that when actually reaching for CI info".
  Read as: the spec's "Amendment 2026-10-21" (CI as the sixth pane 2
  view, the band removed and the strip back to 4, lazy polling only in
  the `ci` view, an `info` section from data in hand, the inspector's
  jobs/log/script bodies, handoffs on the highlighted run); #92
  rewritten as FZ-30d, FZ-30f created, #93 reworded; FZ-30d dispatched
  under the standing batch 3 approval. This replaces his 2026-10-03
  own-pane decision on #67.
- **2026-10-21, 15:07.** "this looks great! i feature, can we use the
  inspector for inspecting the CI's ? so say im highlighting a branch
  in pane 2, details for that is viewed in inspector. and if i switch
  focus to pane 4/ci, and highlight a certain CI there, the inspector
  can detail that instead?" Read as: #91 accepted and merged
  (`a0526eb`); FZ-30d (#92) redesigned so the inspector details the
  selected run (jobs, steps, the failing log) while pane 4 is focused,
  replacing the inline expansion of spec section E; the spec is amended
  and #92 updated before dispatch, under the standing batch 3 approval.
- **2026-10-21, about 14:15.** "great. lets continue? we'll could
  probably also look at a claude rule or something that cleans, i cant
  have the disk filling up like this", then "what do you think is good
  AND safe?" and, to the two-layer proposal, "ok. use an agent to set
  this up then? as we continue the work on fleetz in parallel?". Read
  as: create and run FZ-39 (#94, the Relay layer) now, and prepare the
  global `SessionStart` hook in `~/claude-config` with a general agent
  for his commit. Also "yes, delete those" (13:45) to the listed
  scratch builds and clones (done, 22 GB freed).
- **2026-10-21, 13:05.** "1, think option a for workflow pane. 3. fine as
  proposed. #79 go with your recommended option. #81 yes.": the CI pane
  spec is accepted on every point (layout A, keys as proposed); #79 takes
  option (A) (four expected strings updated) and #81 the two footer test
  edits. Both re-dispatched at 13:10 with fresh workers. Not given: the
  merge word for #89 ("i still need to check out #89 ?" answered: yes,
  or wave it through), and the word to run FZ-30b to e (the 2026-10-17
  batch 3 approval covers them once a slot is free; FZ-30c after #89
  merges).
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

- **UI rules from his FZ-45 look** (2026-10-27, 13:55 and 14:10).
  Indicators (CI, PRs, counters) are structured and have their place:
  a pane 2 view or digest draws them in pane 1's row grammar through
  the same badge functions and colors, never as prose. Information is
  not hidden behind a scroll except in a list, and a list that
  overflows says so with a count; non-list detail uses the full width
  of its pane and is visible without scrolling. Applies to every
  later UI ticket; FZ-45 fix cycle 2 is the first application. Worth
  a CLAUDE.md line on his yes.

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

`main` is `30a9883` (the merge of #117) as of 2026-10-28 16:50 CET,
1457 tests passing and 1 ignored, CI run 40719616971 green. Since `v0.6.0` it also carries the whole
print mode (FZ-46 to FZ-50), the `notes` view (#109), the slot digest
(#101), the scroll margin (#111), search by owner, organisation and
contributor (#112, mark `author` since #117). Lead's rule from today:
when a Cargo target dir was shared between a worker's
`--manifest-path` build, a reviewer's copy and the lead's run, `cargo
clean -p fleetz` before the final verification, or use a fresh dir;
stale incremental artifacts made 25 tests fail in a worktree whose
code was green everywhere else (#117).
Earlier record: `main` was `4632ac5` (the merge of #106) plus docs commits, 1348 tests
passing and 1 ignored, CI green (run 40346149642), as of 2026-10-26.
Earlier record: `main` was `db04b56` (the merge of #95) plus docs commits, 1296 tests
passing and 1 ignored, CI green (run 39763821838). Since `v0.6.0` it
carries #95 (the inspector's jobs, log and script bodies), #79 (header dates the failed
attempt), #81 (footer hints follow focus), #90 (CI data layer), #94
(relay-sweep prunes scratch builds), #89 and #91 (the band, later
undone) and #92 (CI as a pane 2 view with an `info` section). The next
release is `0.7.0`. Local toolchain rustc 1.99.0, same as CI's.

## Worktrees

Under `.claude/worktrees/` as of 2026-10-28 16:55: `issue-96`
(pushed, Ready to integrate for his look), `issue-88` (FZ-38, parked,
uncommitted, see the 2026-10-23 record). `issue-111`, `issue-112`
and `issue-117` were swept today after green `main` CI.

## In flight

Nothing running as of 2026-10-28 16:55 CET. Every ticket dispatched
since 2026-10-26 is Done and in the log (FZ-49, FZ-50, FZ-45, FZ-53,
FZ-51 on 2026-10-26 and 27; FZ-56, FZ-55, FZ-61 on 2026-10-28).

- **Next dispatch when he resumes the order (his word needed after
  the 2026-10-28 stop):** FZ-57 (#113, small), FZ-60 (#116, the
  `changes` view, Ready, conflicts by region with the notes
  follow-ups in `app.rs`, `detail.rs`, `help.rs`, `main.rs`: share by
  symbol as the workflow allows), FZ-58 (#114), FZ-59 (#115,
  `pulldown-cmark` approved by him 2026-10-27 16:15), FZ-41 after
  #96. His words on FZ-60: "its a go only if we dont have any other,
  more important tickets to complete first"; the lead placed it after
  FZ-57 and ahead of FZ-58 and FZ-59 (new capability before polish),
  and he can move it. #96 waits for his look ("not sure here"). Two
  lines on his yes: CLAUDE.md for his two UI rules;
  `.claude/agents/fz-reviewer.md` for scratch isolation (the
  2026-10-27 incident, in the log).
- **The merge-cycle pattern that worked three times (FZ-53, FZ-56,
  FZ-55):** the worker runs one `git merge --no-ff --no-commit
  origin/main` in the ticket worktree, resolves only the conflicts,
  stages, does not commit; the reviewer verifies the merged result
  against `main` equals the accepted diff plus the resolution hunks
  (`git merge-file --diff3` to redo it is a good check); the lead
  commits the merge, pushes, waits for CI, repeats if `main` moved
  (an automatic re-merge with no hand edits needs no second reviewer,
  only the lead's line-for-line comparison and checks). When the
  conflict lands in production code (FZ-56: FZ-45 had rewritten the
  function), the lead rules on the resolution and widens the scope;
  a missing-field build break in another file (`authors` in a test
  literal) is a blocker answered, not a fix cycle.
- **Known follow-ups not ticketed:** the `/` hint in `ui/mod.rs`
  says "fuzzy name"; the compact grid draws no search mark; the 9 ms
  debug timing assertion may flake under load (ruling: then make it a
  measurement); pane 1's fresh `ListState` scrolls on every step
  going up (notez's feel needs a persisted offset); the footer
  `j/k` label in `notes`; the "reading…" flash under watch mode.

Earlier record (2026-10-22, both merged since or waiting on his look,
see the items below): two workers running since 13:30 (both slots;
the `fz-small` slot was free):

- **#93 FZ-30e: Done**, merged `cb72465` 2026-10-27 on his "i trust
  you here" (record in the log, "Merged on 2026-10-27"); `#96` no
  longer needs re-verification against it beyond the normal scratch
  merge.
- **#96 FZ-40 (the `ci` view's headline row and width budget): Ready
  to integrate since 14:05, for his look.** Reviewed and accepted
  (diff `16bb0848`, seven mutations caught), committed `19a21d6` on
  `feat/96-ci-view-width` (base `91bba9f`), pushed; branch CI
  39775176164 green; combined with `main` 1303 tests under UTC, with
  #93 on top 1319 (README auto-merged), fmt and clippy clean. Rulings
  on #96 (14:00): the one existing-test index edit, OVERLAY headline,
  `TITLE_MIN` 20, all flagged for his look. Reviewer's low follow-ups:
  rows wrap below 23 pane columns (as before); three `Local::now()`
  reads in the `ci` block of `detail.rs` could share one `now`. Next:
  his look, merge, sweep, log; the second of #93/#96 to merge is
  re-verified against the new `main` (already checked together).

**Release:** he asked at 14:03 "remember to bump versions soon too, is
there a installable version soon". Since `v0.6.0` `main` carries
features, so the next release is `0.7.0`; the bump is one commit, last
before the tag, on his word, with What's Changed notes prepared by the
coordinator and the tag and `cargo install` his. Proposed: after #93
and #96 merge, prepare the `0.7.0` bump and notes and present the
commands.

Today's earlier trail (the seven merged tickets, the disk pause, the
rulings) is in the log: "Merged on 2026-10-21" and the lead record
for 2026-10-21 under "Earlier leads".

## Next step, and the order after it

Nothing starts until Andreas opens the next session.

0. **repoz convergence, accepted 2026-10-23** (authority entry of
   15:00). Design [repoz-print-design.md](repoz-print-design.md)
   accepted, plan
   [2026-10-23-repoz-convergence-plan.md](superpowers/plans/2026-10-23-repoz-convergence-plan.md)
   agreed, FZ-46 to FZ-52 filed (#102 to #108), Backlog, milestone
   `repoz convergence`. FZ-46 to FZ-50 Done; P1 landed
   (`~/repos/repoz` `3fab42a`). **Next: FZ-51 (#107) on his word**
   (measure print mode against bash repoz), then FZ-52 not before
   0.8.0 and his explicit word. Open follow-ups from the reviews, each
   an issue on his yes: the flaky `clipboard::tests` (FZ-48), the
   `target` skip in `check_option_case`'s no-PR check (FZ-49), the
   print-mode child without `--nocapture` (FZ-50), README has no
   `--print` flags yet (FZ-47 to FZ-50), the footer `j/k` label in
   `notes` (FZ-53). Ask before
   creating: an issue for the flaky `clipboard::tests`
   (`a_failing_command_is_reported`, `the_first_command_on_path_receives_the_hash`,
   a write-then-exec race seen once in five runs at the FZ-48 review).
   He can use print mode from `main`: `cargo run --release -- --print`. FZ-52 not before
   0.8.0 and his explicit word. Phase 0 check done: `repoz` is free on
   crates.io (API lookup, 2026-10-23 15:10).

1. **#93 and #96: his look, then merge** (see "In flight"). Try:
   `cargo run --release --manifest-path
   /Users/at-a/Repos/fleetz/.claude/worktrees/issue-96/Cargo.toml` and
   the same with `issue-93`. On "merge both": merge #96 first, then
   #93 re-verified against the new `main` (already checked together,
   1319 tests), `main` CI, sweep, log records. The `ci` view's
   selection wash only with focus (from the #92 review) is still his
   call.
2. **Prepare `0.7.0`** on his word after those merges: one bump commit
   (`Cargo.toml`, `Cargo.lock`), What's Changed notes from
   `scripts/release-notes.sh`, the release checks, commands presented;
   the tag and `cargo install` are his. Asked at 14:03 ("remember to
   bump versions soon too, is there a installable version soon").
3. **FZ-41 to FZ-45 created, Backlog** (#97 split inspector P1, #98
   log navigation P2, #99 annotations P2, #100 usual duration P2, #101
   slot digest P1; specs "Amendment 2026-10-22, second" and
   `2026-10-22-slot-digest-design.md`). Dispatch order on his word
   to run them: FZ-41, FZ-45, then FZ-42 to FZ-44. Not dispatched:
   "no need to work on this right now" (17:53).
4. **His feature of 20:38, not yet ticketed (ask before creating):**
   "per script, see the actual code (linting, lsp and highlight might
   be needed?) to watch the actual CI script code". Coordinator's
   reading, offered in chat: (a) syntax colour for the inspector's
   script body with a small YAML tokenizer of our own (keys, strings,
   comments, `run:` blocks), no crate, an easy FZ-30g; (b) linting via
   `actionlint`: a handoff through suspend-and-run, or run in the
   background when on PATH with its diagnostics drawn as marks in the
   body, a medium FZ-30h; (c) an LSP in the TUI: not recommended. His
   pick needed.
4. **FZ-14 attention view spec drafted** (19:00,
   `docs/superpowers/specs/2026-10-21-attention-view-design.md`, comment
   on #27): waits on his answers to its "Open with Andreas" (the reason
   table, section versus tag, the fetch two-failure rule, creating
   FZ-14a to c and the word to run FZ-14a).
4. **His feature of 16:12, not yet ticketed (ask before creating):**
   "how are we doing on the worktree page? it should also review how
   much space each worktree is taking up, as we ll as the collective
   amount, and their location on disk". The `info` worktrees section
   (#72) shows branch, state and path but no size. Ticket shape: per
   worktree size and the total, measured off the UI thread per sweep
   and cached (a `du` on a big tree is slow), the path shown as it is
   today; spec not needed, a ticket body with the measurement rule is.
4. **Follow-up from #92:** the default branch's name in the `info`
   line needs `name` on `defaultBranchRef` (free field, payload change,
   cached payloads must parse without it); ask before creating.
4. **Batch 3 after FZ-30:** FZ-14 attention view, FZ-15 changes since
   seen, FZ-16 handoffs, each spec first. Remembered folds (deferred
   from FZ-13o to the dropped FZ-13m) have no home; ask him.
5. **#88 FZ-38** if he wants it after all (see "Baton").
6. **Follow-up issues** (items 7-10 of 2026-10-17, approved): one
   measure for width, the ref query cost at 20,000 refs, the small
   review follow-ups cut into reviewable tickets. The 2026-10-20
   follow-ups (log, "Follow-ups found and not ticketed") need his yes.
7. Batch 4 (FZ-12, FZ-11, the ref query cost), each measured against
   the FZ-01 baseline. Release scope: later, when Andreas asks. The
   next release is `0.7.0` (the CI view and the hints are features).

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
