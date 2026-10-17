# Fleetz coordinator handoff

Last written 2026-10-17 16:38 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `d63c4af4-55f8-4f4f-ab06-d156d9582c03`, pid 12225)
on takeover. **Nothing is running: no worker, no reviewer, no build, no
background job. Nothing is in flight; the next tickets need Andreas's
answers or his OK for issues before dispatch.** Read this file, the repository instructions, [agent-workflow.md](agent-workflow.md) and
[release-plan.md](release-plan.md) before acting. GitHub (the issues and the
[release Project](https://github.com/users/Gaurgle/projects/1)) is
authoritative for status; this file says why things are where they are and
what to do next. Finished tickets, earlier leads, spent rulings, inherited
decisions, facts, the follow-up list and the longer practical notes are in
[agent-log.md](agent-log.md).

## Baton

**Held** since 2026-10-17 16:38 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `d63c4af4-55f8-4f4f-ab06-d156d9582c03`, pid 12225).
Andreas opened with "continue work". Evidence that no lead was active: the
previous session released the baton on 2026-10-16 at about 18:40 and no
other `claude --agent fz-coordinator` process existed; `main` equalled
`origin/main` at `c41602f`, clean, CI green (`ci` run `38931350486` on
`57b885c`, the `docs` run on `c41602f`); `relay-check` reported the template
and skill both at 0.5.2, up to date, the handoff at 299 lines; the sweep
kept only `issue-45`; the board showed nothing In progress, in Review or at
Ready to integrate except the parked #45. **No worker, reviewer, build or
background job is running.** Nothing was dispatched on takeover: every next
ticket waits on an owner answer or an OK for an issue (see "Next step").

- **Look not yet seen on `main`:** #75 (the `WT` badge in the dirty colour),
  #71 and #72 (Info header and worktrees section). Run
  `cargo run --release --manifest-path ~/repos/fleetz/Cargo.toml`.

Earlier lead records, including both 2026-10-16 sessions: the log, "Earlier
leads".

## Authorized by the owner

Owner authority, verbatim, newest first. A handoff preserves it; no agent
can create or expand it.

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
- **2026-10-16, about 17:33.** After the baton had been released and the
  coordinator had asked for #72's look, rule 6, the wave 4 questions and
  `cache_ttl`, Andreas wrote: "72 is good. Rule 6 yes for refetch (is good,
  yes?) give me fz-13 questions on 26 do w`hat you need for cache_ttl",
  then "let me know when this is good and i can restart this session".
  The coordinator reads it as: #72 accepted, merge it; the refetch-on-
  recovery change to fetch contract rule 6 is approved and its ticket
  (#76) may be created and run; `cache_ttl` is the coordinator's call, and
  its ticket (#77, remove the key and the dead path, two tests of the
  removed branch go with it) may be created and run; the wave 4 questions
  are answered in chat, no decision yet. Because he is restarting the
  session, #75, #76 and #77 were created and set Ready but **not
  dispatched**; the next lead dispatches them under this entry without
  asking again (two `fz-worker` plus one `fz-small`).
- **2026-10-16, about 17:12.** After the takeover report (which listed: #72
  waits for his look; batch 2's three open decisions, rule 6 refetch "yes or
  no?", the dirty-worktree list row "OK to create the issue and dispatch
  once #72 is merged?", the FZ-13 wave 4 questions; the two filler issues
  "OK to create them?"; and "trim the handoff ... then dispatch whatever
  you answer above") Andreas wrote: "sounds good. off you go". The
  coordinator reads it as: trim and commit the handoff; the three offered
  issues may be created (dirty worktree in the list row, one key for "same
  remote", the stale `CLAUDE.md` lines) and the two fillers dispatched now
  in the free slots; the list-row ticket after #72 merges. It is not an
  answer to rule 6 or the wave 4 questions, which stay parked, and not a
  look at #72, which stays at Ready to integrate.
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

- **#80 (FZ-13o), one-row folds.** In progress since 2026-10-17 17:08
  CEST under the 16:58 owner entry ("yes, economical"). Base `84a06b7`,
  branch `feat/80-one-row-folds`, worktree `.claude/worktrees/issue-80`,
  `fz-worker` (`claude-opus-5-5`, medium). Worker report 1 at about
  17:20: done but red on one unnamed assertion in the owner-approved test
  `a_folded_pane_keeps_only_its_frame` (the `╭` top corner); coordinator
  ruling on the issue: covered by the same authorization, change it to
  `─`. Worker is applying that (fix cycle 1); then `fz-reviewer`, then
  commit, push, CI, combined check and merge (small look change). Three
  assertions in that one existing test are the only existing-test edits.
- **#81 (FZ-13n), footer hints follow focus.** In progress since 17:08
  under the 16:58 owner entry ("lets try it out, yes."). Base `84a06b7`,
  branch `feat/81-focus-hints`, worktree `.claude/worktrees/issue-81`,
  `fz-worker` (`claude-opus-5-5`, medium). Worker done (revision
  `d8ef6e7c`, 1104 tests, no existing test touched) but the ticket's
  degradation order leaves the segment invisible below about 143 to 217
  columns. Asked Andreas (chat and the issue): (1) try as is, (2) invert
  the order so the segment keeps its words and the global row goes bare
  first (changes one existing footer test, his), (3) drop it. Review
  waits for the answer. Stops at Ready to integrate for his look.

#78 is Done (merged `84a06b7`, record in the log).

#75, #76 and #77 are Done; one owner-visible ruling on #77 (the deleted
precondition assertion in
`startup_serves_an_expired_payload_rather_than_waiting_on_the_network`,
under "do what you need for cache_ttl") stands unless Andreas overrides it.
The records are in the log.

## Next step, and the order after it

All under the 2026-10-17 16:58 owner entry unless noted.

1. **#79** on Andreas's (A)/(B)/(C) answer: re-dispatch, review, commit,
   push, CI, combined check, then Ready to integrate for his look.
2. **Batch 3, order the coordinator's:** #67 CI pane first (an issue
   exists; it is design-heavy, so a spec under `docs/superpowers/specs/`
   goes to Andreas before any worker), then FZ-14 attention view, FZ-15
   changes since seen, FZ-16 handoffs, each spec first. Two `fz-worker`
   slots plus one `fz-small`.
3. **FZ-13 wave 4,** answers in hand: FZ-13m maximize and restore (restore
   returns to the last saved layout if possible; the key and the panes are
   the coordinator's to propose in the spec), FZ-13n hints that follow
   focus (try it), FZ-13o economical folds (one row instead of three;
   remembered folds ride with FZ-13m's saved layout). Issues to create
   with the FZ-13 letters after `y`.
4. **Follow-up issues** (7 to 10 of the list): the `o` shell mouse
   capture, one measure for width, the ref query cost at 20,000 refs
   (batch 4 material, measured), the small review follow-ups cut into
   reviewable tickets (the fetch-outcome items as one; the settings-save
   cursor reset as one; test sharpening as one `fz-small`). Fillers for a
   free slot.
5. Batch 4 (FZ-12, FZ-11, the ref query cost), each measured against the
   FZ-01 baseline. Release scope: later, when Andreas asks.

## Open with Andreas

- #79: (A), (B) or (C) on the four existing tests (the stop comment on
  the issue); then his look at the branch build.
- The look of #71, #72, #75 and #78 on `main` whenever he next runs it
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
