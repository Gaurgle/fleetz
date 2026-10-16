# Fleetz coordinator handoff

Last written 2026-10-16 17:40 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `80d0c941-d673-4c3a-bccd-ab7e3949c00d`), when
Andreas asked to stop so he can restart the session with new Claude rules.
**Nothing is running: no worker, no reviewer, no build, no background job.
#72, #73 and #74 are Done today; #75, #76 and #77 are Ready and authorized,
not dispatched.** Read this file, the repository instructions, [agent-workflow.md](agent-workflow.md) and
[release-plan.md](release-plan.md) before acting. GitHub (the issues and the
[release Project](https://github.com/users/Gaurgle/projects/1)) is
authoritative for status; this file says why things are where they are and
what to do next. Finished tickets, earlier leads, spent rulings, inherited
decisions, facts, the follow-up list and the longer practical notes are in
[agent-log.md](agent-log.md).

## Baton

**Released** 2026-10-16 17:40 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `80d0c941-d673-4c3a-bccd-ab7e3949c00d`, pid 13907),
which held it from 17:10 CEST (released once at 17:30, then taken up again
by the same session when Andreas answered at 17:33). **No worker, reviewer,
build or background job is running.** The next lead records its own
session and takeover time here, after confirming with Andreas that this
session has ended (he said he would restart it for new Claude rules).

- **How it ended (2026-10-16).** At about 17:22 Andreas wrote "stop at a
  good place soon. we have new claude rules", at 17:27 "let me know when im
  good to restart this session", and at 17:33 the answers recorded under
  "Authorized by the owner" plus "let me know when this is good and i can
  restart this session". The coordinator finished what was in hand: #73
  (merged `4b8d13f`) and #72 (look accepted, merged `338024e`, combined
  checks 1085 tests, `main` CI green), both closed and swept; created #76
  and #77 from his answers and set #75, #76, #77 Ready; gave him the wave
  4 questions in chat. Nothing was dispatched after 17:22. Usage was not
  read.
- **Session summary.** Takeover 17:10; handoff shortened to the 0.4.0 shape
  (`a3bd0b5`); issues #73, #74, #75 created and #73, #74 dispatched under
  the 17:12 authorization below; both accepted at first review and merged.
  One coordinator slip: the times in the coordinator's comments on #73
  (17:50) and #74 (18:00, 18:15 in the record) were written from memory;
  the clock read 17:20 to 17:25. An attempt to edit them was refused by the
  permission check; the closing comment on #73 says so.

**Held before that** from 17:10 CEST by the same session. Andreas opened
with "update Relay. continue the work! start with giving me a quick
status". Evidence that no lead was active: the previous session released
the baton at 16:35 and its process (pid 72479) was gone; the only
`claude --agent fz-coordinator` process was this one; `main` equalled
`origin/main` at `9e34da3`, clean, CI green; `relay-check` reported the
template and skill both at 0.4.0 with one drift line, the handoff at 649
lines. Earlier lead records: the log, "Earlier leads".

## Authorized by the owner

Owner authority, verbatim, newest first. A handoff preserves it; no agent
can create or expand it.

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
- **2026-10-10, 12:46 and 12:52.** "go with c": triage and dispatch #66,
  one worker and one reviewer; "all looks good": the second existing-test
  edit on #66 and the M1 checkpoint confirmed. Both Done.
- **2026-10-03, about 18:00.** "good! all ok from me." to three open
  questions (try FZ-13d, one test arm in FZ-13u, the standing instructions).
- **2026-10-02.** "Retired, take over" about two older sessions; "Both
  hold" to the standing instructions below.
- **2026-10-01.** Commits, pushes and merges delegated to the coordinator
  under the safe-merge rule in [agent-workflow.md](agent-workflow.md).

One-time releases on finished tickets (#60, #58, #48, FZ-13t, FZ-13u) are
spent; their text is in the log, "Rulings and one-time releases".

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

`main` is at the merge of #72, `338024e`, equal to `origin/main` plus the
docs commit that records this, 1085 tests passing and 1 ignored, CI green.
On 2026-10-16 it gained #72 (the Info view's `worktrees` section, look
accepted), #73 (one `model::identity_key` for "same remote", a pure
refactor), #74 (the `CLAUDE.md` Nerd Font sentence), #71 (the Info header's worktree count in
lavender, a look change merged under the standing instruction and not yet
seen by Andreas on `main`) and #70 (ssh fetches bounded at 10 s). The installed
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

Nothing dispatched. Three tickets are Ready, authorized by the 17:33 entry
and waiting only for a lead to dispatch them:

- **#75**, the dirty worktree colours the `WT` badge in the list row
  (P1, M3; coordinator's colour-not-count ruling on the issue; a minor look
  change, merges under the standing instruction). `fz-small` or
  `fz-worker`; files `src/ui/badges.rs`, maybe `src/ui/compact.rs`,
  `README.md`.
- **#76**, refetch `Unreachable` and `TimedOut` repos once when the GitHub
  poll recovers from `Offline` (fetch contract rule 6 amended; the wording
  is in the body and binding). `fz-worker`; files
  `docs/fetch-outcome-contract.md`, `src/main.rs` or `src/app.rs`.
- **#77**, remove the inert `cache_ttl` key and the dead TTL read path
  (config contract change; removes the two tests of the removed branch,
  `cache_expired_returns_none` and the second `from_secs(0)` fixture, by
  the owner's "do what you need"). `fz-worker`; files `src/config.rs`,
  `src/cache.rs`, `src/github.rs`, `src/main.rs`, `README.md`.

#76 and #77 both touch `src/main.rs` (#76 in `apply_github_poll`, #77 the
two `KeyedCache::new` calls) and `src/github.rs` is #77's only: record the
split by symbol in the assignment comments and merge one at a time.

## Next step, and the order after it

1. **Dispatch #75, #76 and #77** (above), each from current `main`, with
   its own reviewer; two `fz-worker` plus one `fz-small`.
2. **Wave 4 of FZ-13** waits for Andreas's answers to the three questions
   (given to him in chat on 2026-10-16; also in the first comment on #26):
   FZ-13m maximize and restore (the key, which panes, what restore returns
   to), FZ-13n hints that follow focus (whether the footer changes with the
   focused pane, against the "a key sits beside what it changes" rule),
   FZ-13o economical folds (one row instead of three, folds remembered
   between launches, or both).
3. Then batches 3 and 4 of the "Dispatch order after M1" table in
   [release-plan.md](release-plan.md); filler candidates are in the log's
   "Follow-ups found and not ticketed", each needing his OK for an issue.

## Open with Andreas

- The FZ-13 wave 4 answers (above).
- #71's and #72's look on `main` whenever he next runs it
  (`cargo run --release --manifest-path ~/repos/fleetz/Cargo.toml`); the
  installed binary is still the `main` of 2026-10-10.
- FZ-13 wave 4 questions on #26; FZ-13s #56 (he said not prioritized).
- Follow-ups that may become issues (the log's "Follow-ups found and not
  ticketed"): the `o` shell mouse capture (offered 2026-10-09), the
  settings-save cursor reset, the `start_account_rebuild` test, and the
  rest of the list.
- Before the repository goes public: `docs/ideas.md` names one of his
  private repositories in an older note (told to him 2026-10-02).
- Not blocking: #46 (edits an existing test); #45 parked; FZ-08 #21 not to
  be started; #67 not authorized to start.

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
