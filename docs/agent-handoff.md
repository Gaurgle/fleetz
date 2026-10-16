# Fleetz coordinator handoff

Last written 2026-10-16 16:35 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `29766f98-d471-41a5-998a-292c8326b5f7`), when
Andreas asked to find a good place to stop. **Nothing is running: no worker,
no reviewer, no build, no background job. One ticket is between dispatch and
merge: #72, the worktrees section, at Ready to integrate in its worktree,
waiting for Andreas's look** (the board's other Ready to integrate item is
#45, parked). **M0 and M1 are checkpointed; batch 1 of the post-M1 order
landed #70 and #71 today.** Read this file,
the repository instructions, [agent-workflow.md](agent-workflow.md) and
[release-plan.md](release-plan.md) before acting. GitHub (the issues and the
[release Project](https://github.com/users/Gaurgle/projects/1)) is
authoritative for status; this file says why things are where they are and
what to do next.

Keep it short: finished tickets and earlier leads go to
[agent-log.md](agent-log.md).

## Baton

**Released** 2026-10-16 16:35 CEST by `claude-fable-5-1` (`claude --agent
fz-coordinator`, session `29766f98-d471-41a5-998a-292c8326b5f7`), which held
it from 15:21 CEST. **No worker, reviewer, build or background job is
running.** One ticket is between dispatch and merge: #72 at Ready to
integrate, committed and pushed, waiting for Andreas's look (details under
"Next step"). The next lead records its own session and takeover time here,
after confirming with Andreas that this session has ended.

- **How it ended (2026-10-16).** At about 16:28 Andreas wrote: "find a good
  place to stop. finish the running tickets/tasks, and find a good place to
  stop". Nothing was running: #72's second review had been accepted, the
  diff committed (`6fbacce`), pushed, branch CI green, the coordinator's own
  fmt/clippy/tests green (1083). #72 changes the screen and so stops at
  Ready to integrate for his look; it was not merged. Nothing was dispatched
  after his message. Usage was not read.
- **Taking over does not resume work.** #72's merge needs Andreas's
  acceptance of the look first; batch 2 needs his decisions; filler tickets
  need his OK for issues (two were offered at about 16:14, unanswered: one
  key for "same remote", and the stale `CLAUDE.md` lines).

**Held before that** from 2026-10-16 15:21 CEST by the same session. Andreas
opened the session with "how is the status here? give me a short report, and
prioritize tasks", plus two UI ideas (the Info tab's worktree line, a worktree
section). Evidence that no lead was active: the baton below was released on
2026-10-10 13:43; `main` equals `origin/main` at `f981642`, clean, docs CI
green; the only worktree is the parked `issue-45`; the board shows nothing
In progress or in Review; the only `claude --agent fz-coordinator` process is
this session (pid 72479, started 15:17). Codex processes seen: the app-server
daemon of 2026-10-13 and an app-server started 14:29 today, neither a lead in
this repository.

- **Authorized on 2026-10-16.** After the first report Andreas wrote (about
  15:40): "i think all tickets should be completed, but prioritize. polish on
  polish on polish can be deferred, or pickued up in parallel when it fits.
  But the release is not in a hurry at all, this is and will still be a
  project i use, so lets prioritize that. I want it too look sleek (still
  Rust/TUI style) but with passion, but the functionality and reliability
  must come first, ofcourse! so all optimizations ARE good, we just need to
  think of what and when they are being worked on. A blocker is a blocker."
  and "lets get to building, I guess?". The coordinator proposed the order
  in [release-plan.md](release-plan.md) ("Dispatch order after M1") and a
  first batch of three: A, the ssh fetch hang; B, a worktrees section in
  the Info view (spec by the coordinator first); C, the Info header's
  worktree count in the `WT` colour. It asked for an OK to create the three
  issues and for "run batch 1". Andreas answered "sopunds good. lets get
  started" (about 15:48). Under that: the three issues may be created, A
  and C are dispatched now, B after its spec. Three slots are in force (two
  `fz-worker` plus one `fz-small`, `f981642`). Pace and dispatch, "small
  look changes no longer wait" and the existing-test rules hold as recorded
  below; B changes the screen, so it stops at Ready to integrate for his
  look.

Records of earlier leads (2026-10-02 to 2026-10-10) are in
[agent-log.md](agent-log.md), "Earlier leads". Their authorization
paragraphs are below.

## Authorized by the owner

Owner authority found inside records that moved to the log, copied here
verbatim. Authority records stay in this file.

From the lead record of 2026-10-10:

- **Authorized on 2026-10-10.** At 12:46 Andreas chose option (c) of the
  first report ("go with c"): triage and dispatch #66 now, one worker and
  one reviewer, while the lead writes the M1 checkpoint; #69 and #67 are
  not named and are not dispatched. At 12:52 ("all looks good") he
  approved the second existing-test edit on #66
  (`no_day_is_marked_until_the_user_moves`, recorded on the issue) and
  confirmed the M1 checkpoint in [release-plan.md](release-plan.md). #66
  changes the screen, so it stops at Ready to integrate for his look.

From the lead record of 2026-10-03:

- **Confirmed (2026-10-03, about 18:00).** The session had told Andreas how
  it read his opening message and listed three open questions (try FZ-13d,
  one test arm in FZ-13u, the standing instructions). He answered: "good!
  all ok from me."

From the lead record of 2026-10-02:

- **How that session took it.** Andreas answered "Retired, take over" about two older
  sessions still open in this repository (a `claude` started 2026-10-01 13:54
  and a `codex` started 2026-09-30 23:25), and "Both hold" to the standing
  instructions below.

Rulings recorded with FZ-09d, FZ-10c, FZ-10b and FZ-09b (2026-10-09; the
FZ-09d record itself is in the log):

**Rulings on #60** (all recorded there): Andreas allowed the fixtures of two
FZ-10b tests to land a poll that returns the `Pending` row (option A,
19:37); the coordinator allowed `src/ui/compact.rs` for the mark and
`README.md` for one paragraph, kept the kind glyph undimmed, and accepted
"Rebuilt locally" as the account rebuild's provisional footer word.
**Ruling of 19:37** (recorded on #60): Andreas allowed the fixtures of two
FZ-10b tests, `s3_a_healthy_budget_keeps_the_pending_floor` and
`s2c_a_known_budget_past_its_reset_is_unknown_and_the_floor_returns`, to
land a poll that returns the `Pending` row, every assertion kept ("go with
A"), because under design 4.7 a row the poll did not return no longer holds
the 15 s floor. The coordinator accepted "Rebuilt locally" as the account
rebuild's provisional footer word and the window-apply race as pre-existing. **Coordinator ruling on #49** (the
header needs the poll cost and the configured interval, which only `Config`
holds): both are copied onto `App` at startup beside `contrib_author`, and
the three predicates (`github_pace`, `budget_slowed`, `github_limit_until`)
are `App` methods taking them as parameters, so the scheduler and the header
share one definition and `ui::render`'s signature stays. **Rulings of 17:59 on FZ-10b**
(recorded on #48): Andreas allowed one match arm,
`LoadKind::FromMemory => LoadKind::FromMemory,`, in the existing test
`the_ambiguity_line_wins_over_every_loads_own_message` ("B1, go for A");
the coordinator allowed `src/github.rs` for the four-line `expect(dead_code)`
removal on `poll_cost` only, and a compile-forced field on the `LoadError`
literal in `a_launch_with_no_usable_cache_records_the_failed_attempt` under
the standing exception. **CI's toolchain is ahead of the machine's:** CI runs
`stable` (rustc 1.99.0 on 2026-10-09), the machine 1.96.1, and clippy's
`result_large_err` fired only on CI (`LoadError` at 128 bytes; boxed to 80, as
`FetchError.meta` is). A clean local clippy is weaker than CI's; wait for the
branch run before merging, as the rule already says.

**FZ-09b decision (Andreas, 17:08: "for fz-09b go with option A").** The
worker stopped before writing code because the design (4.1, C2) carries
coverage in `FetchMeta` and three tests from FZ-10a and FZ-09c assert a
cache-served load's whole meta equals `FetchMeta::default()`. Option A:
narrow those three assertions (`a_cache_served_load_carries_no_reading`,
`ck4_signed_out_serves_the_newest_keyed_file_and_the_poll_is_auth`,
`ck8_a_pinned_cache_is_served_without_an_attempt` in `src/github.rs`) to
`budget` and `account` being `None`, nothing else moved. A one-time release
for this ticket, recorded on #58. The body's forced-edit list also misses the
`land_load` helper (a seventh `LoadResult` literal, compile-forced).

## Authority and routing

Andreas owns priorities, acceptance, what he installs and runs, tags and
releases. The coordinator commits the reviewed diff on the ticket branch,
pushes it and merges into `main` under the safe-merge rule in
[agent-workflow.md](agent-workflow.md), and commits coordination docs straight
to `main`. When a condition of that rule fails or is unclear, stop at Ready to
integrate and ask. Workers and reviewers never commit. No agent tags,
releases, runs `cargo install`, force-pushes, rewrites pushed history or
deletes remote branches.

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
  going from the handover" and "sounds good to me" on 2026-10-09, under
  which the coordinator dispatched the `src/main.rs` line in plan order
  without asking per ticket; the stop of 2026-10-09 19:49 ends that day's
  run, not the standing scope). "keep me updated. backend is still
  vital, but its more important that we move forward in a good pace. But keep
  me updated, and ask me about decisions that are bigger". Under it the
  coordinator dispatched in plan order without waiting, ruled on small
  questions, reported at each merge, and went to him for design decisions,
  contract changes and test edits beyond the exception below.
- **More in parallel** (2026-10-03, about 17:35). "we are only running one
  agent right now, i think? Are'nt there any more tickets we can get working
  on in parallell?" Under it the coordinator filled the second worker slot
  with tickets that share a file with the first by symbol or region rather
  than waiting for whole-file disjointness, recorded each overlap in the
  assignment comment, merged one ticket at a time and reran the combined
  check against the new `main` for the next. Never more than two workers at
  once; he was offered a third and did not ask for it.
- **Where his testing matters** (2026-10-01; confirmed with the above). No
  "try the build" for tickets with nothing to see; a ticket that changes the
  screen or a key goes to him with the branch build before it merges, except
  as the next item says.
- **Small look changes no longer wait for him** (2026-10-02, about 21:28).
  "i dont think i need to review these small things either, lets keep going
  rather than stopping for minor things at this point". A minor on-screen
  change (a character, a color, a label, a status line, a small row or badge)
  merges under the safe-merge rule once reviewed and is reported at the merge
  with the command to look at `main`. Still his before they are built or
  merged: a new screen or a redesign (FZ-13s, the FZ-13 wave 4 questions), a
  new key or changed interaction, contract and owner items, a new config key,
  and any test edit beyond the standing exception. When in doubt, ask.
- **Existing tests** (in [agent-workflow.md](agent-workflow.md), "Validation
  contract"): editing an existing test is his decision, except a
  compile-forced new field in existing test code with no assertion changed.
  Two releases he gave for single tickets: the assertion on `1wt` (FZ-13t,
  done) and the positional calls to `SettingsOverlay::new` (FZ-13u, done: one
  argument added with a neutral value, no assertion changed), with one
  compile-forced arm in an exhaustive match of an existing test, which he
  allowed when asked ("all ok from me", 2026-10-03). An exhaustive match
  that needs an arm for a new variant is not a struct field: ask each time.
- **Writing files** (in `agent-workflow.md` and
  `.claude/agents/fz-coordinator.md`): Write or Edit, never heredocs or
  redirection; command text goes through a scratchpad file and `--body-file`.
- **Cleanup is part of Done** (same two files): after the merge and green CI
  on `main`, remove the ticket's worktree and local branch, clean, merged and
  pushed first, never forced; remote branches stay.
- **Issues:** ask before creating one. He has allowed: #68 (the mouse
  regression, 2026-10-09 "sounds good to me"), FZ-10a to c, the M3
  issues of 2026-10-01, and on 2026-10-02 FZ-13s #56, FZ-09a to f #57 to #62,
  FZ-13t #63, the redraw bug #64 and FZ-13u #65.
- **CI changes** were approved ticket by ticket, never in general.

## State of `main`

`main` is at the merge of #70, `5af75ea`, equal to `origin/main`, 1069
tests passing and 1 ignored, CI green. On 2026-10-16 it gained #71 (the
Info header's worktree count in lavender, a look change merged under the
standing instruction and not yet seen by Andreas on `main`) and #70 (ssh
fetches bounded at 10 s). The installed binary is still the `main` of
2026-10-10 (`677e8cc`). Before that: on 2026-10-10 it gained
#66 (the strip opens on today, total on the bottom border, look accepted)
and #69 (archived repos filtered server-side). Since 2026-10-03 it had
gained #68 (mouse capture reclaimed after a child process), FZ-09b (the
coverage note), FZ-10b (pacing and backoff), FZ-10c (the header's polling
states) and FZ-09d (dimmed not-in-last-poll rows, the account rebuild), all
with Andreas's words and look accepted. The local toolchain is rustc 1.99.0
since 2026-10-09 20:05 (`rustup update`), the same as CI's. **The first
launch of a build from this `main` changes the cache on disk** (FZ-09c): it
paints from the old `github-50.json`, and the first successful poll writes
`github-v2-github.com-<fingerprint>-50.json` beside it. The installed binary
keeps using the old file, which the new build deletes once it has gone 30
days unwritten. **The no-install
span of the identity series is closed** (FZ-05b opened it at `f37c6b9`,
FZ-05e closed it at `40a2c73`): `main` shows every checkout again. Andreas was
told; installing and tagging are his. To look at `main` without installing:
`cargo run --release --manifest-path ~/repos/fleetz/Cargo.toml`.

## Worktrees

Two exist besides the main checkout, under `.claude/worktrees/`. Every
ticket worktree of a merged ticket (on 2026-10-16: `ssh-fetch`,
`info-wt-colour`; on 2026-10-10: `issue-66`, `issue-69`;
on 2026-10-09: `mouse`, `fz-09b`, `fz-10b`, `fz-10c`, `fz-09d`) is removed
with its local branch; remote branches stay on GitHub.

- **`worktrees-section`, branch `feat/72-worktrees-section`:** #72, clean,
  committed `6fbacce` and pushed, CI green, Ready to integrate, waiting for
  Andreas's look; a `target/release` build of it exists there. Kept until
  the ticket is merged or withdrawn.
- **`issue-45`, branch `ci/45-pull-request-trigger`:** uncommitted `ci.yml`
  and `docs.yml`, reviewer verdict blocked on a repository setting. Parked by
  Andreas until the repository goes public.

## Next step, and the order after it

**Next step (written 2026-10-16 16:35 CEST at the day's end):** batch 1's A
(#70) and C (#71) are Done. B, the worktrees section (**#72**), is at
**Ready to integrate**: spec accepted by Andreas ("yes, looks perfect"),
worker `claude-opus-5-5` medium, two review rounds (round 1 blocker: a
worktree row wider than the pane wrapped and shifted the commit list's row
tops; fixed by bounding the row to the pane width with the ellipsis, plus a
narrow-width test), round 2 accepted revision `4661605` (saved diff sha1
`466160516547885a6998a00ad45bd8d661909d8b`), committed as `6fbacce` on
`feat/72-worktrees-section`, pushed, branch CI green, coordinator's
fmt/clippy/tests green on the branch (main differs from the base `99aed78`
only in docs), 1083 tests. Board: Ready to integrate. **It waits for his
look** (a new section on screen): `cargo run --release --manifest-path
~/repos/fleetz/.claude/worktrees/worktrees-section/Cargo.toml`, select
`fleetz` (it has linked worktrees) and read the Info view's `worktrees`
section. When he accepts: merge with `--no-ff` under the safe-merge rule
(all other conditions hold), close #72 with the merge sha, confirm CI on
`main`, then remove the worktree and local branch. If he wants changes, they
go back through the worker and a re-review. Coordinator rulings given during
the ticket, recorded on the issue thread by this file: detached worktrees
count in the `WT` badge and the header count; counter colours and text come
from the new counts-based helpers `badges::counter_color` and
`model::counters_text`, which `status_color` and `short_status` delegate to.
Review follow-ups, not ticketed: `git::worktree_state` on a worktree that
lost its `.git` file reports the enclosing repo's state (guard on
`<wt>/.git`); the `branch_row_lines` doc comment about `shown` is stale and
Info's `shown_worktrees` set is always empty; launch now pays two git
processes per linked worktree (note it when the performance baseline is
rerun); the section label itself is not clipped below 9 columns, like the
other labels.
The order after that is the "Dispatch order after M1" table in
[release-plan.md](release-plan.md): batch 2 needs his decisions (fetch rule
6 for refetch on recovery; the FZ-13 wave 4 questions on #26) and the
list-row half of the worktree work follows #72. The second worker slot and
the `fz-small` slot are free; filler candidates are the follow-up list below,
each needing his OK for an issue. The earlier text follows for the record.

**Next step (written 2026-10-10 at the day's end):** nothing is dispatched,
Ready or in a worktree. Every ticket Andreas has named is Done (#66, #69).
#67 (the CI pane) is a planning issue he marked not authorized to start;
the standing scope's `src/main.rs` line is complete; FZ-13s #56 and the
FZ-13 wave 4 questions wait for him. The next lead's work, with Andreas:

1. **The release scope decision** the plan asks for after M1 and the
   baseline: which of M2 to M5 goes into the first public release. Not yet
   raised with him. Draft a proposal from the release plan's tables and the
   follow-ups below; it is his call.
2. **Follow-ups he may want ticketed** (ask before creating issues): the
   settings-save cursor reset (from #66's review), the `o` shell mouse
   capture (from #68's review, offered 2026-10-09, unanswered), the
   `start_account_rebuild` test (from #60's review), and the list under
   "Follow-ups found and not ticketed".
3. **Installed.** Andreas ran `cargo install --path ~/repos/fleetz` at
   about 13:45 on 2026-10-10 ("i installed"), so the installed `fleetz` is
   the `main` of `677e8cc` as version `0.5.1`. He chose not to bump the
   version yet; `main` is 190 commits past `v0.5.1` with a cache format
   change (FZ-09c), so the next release is at least `0.6.0` and is prepared
   only when he asks ("prepare 0.6.0").

The earlier list of 2026-10-09 is in [agent-log.md](agent-log.md), "Day
notes". Its items 1, 3 and 4 are done; item 2 stands: M3 wave 3 and the
rendering tickets in the split on #26 have no issues yet, and their bodies
are written before asking Andreas.

Keep to two workers, plus their reviewers. Ownership may be by symbol or
region when the assignment comment says so and the merges go one at a time.

## What waits on Andreas

- **One yes/no item offered on 2026-10-09, explained to him, unanswered:** an
  issue for the follow-up where mouse capture stays on while the `o` shell
  runs (junk mouse input in that shell; from #68's review). The `rustup
  update` question is settled: he ran it.
- **`cargo install --path .`** when he wants the day's work in the installed
  binary (his; he said "i can install later").
- Nothing blocks the next step.

Answered on 2026-10-03:

- "good! all ok from me." (about 18:00): FZ-13d may merge; the
  compile-forced match arm in FZ-13u's existing test may stand; the three
  standing instructions hold as the coordinator read them.
- "Merge as it is" (about 18:15), on **`R` while signed out of `gh`**.
  Before FZ-09c it rebuilt from the shared cache file, rediscovered local
  repos and said "Reloaded from cache". Now the rows stay on screen, the
  status line says "Load failed: not authenticated with GitHub. Run: gh auth
  login", and local repos are not rediscovered. It follows design 4.5 (a
  fallback reads only this credential's file); O12 as he accepted it speaks
  of "an account with no saved copy", and signed out there is no account,
  so he was asked. A signed-out `R` that behaves like a signed-out launch
  (O7: the newest saved copy) is a follow-up he did not ask for.

Still open:

1. **FZ-13 wave 4** (maximize and restore, hints that follow focus,
   economical folds): each needs a decision from him before it is a ticket.
   The questions are in the split on #26. **FZ-13s (#56)**, the loading
   screen with a bar and status lines: its decisions are listed in the issue;
   he said it is not prioritized.
2. **Follow-ups below:** may they become issues? Not asked.
3. **Before the repository goes public:** `docs/ideas.md` names one of his
   private repositories in an older note (told to him on 2026-10-02).
4. **Should a recovered network clear fetch marks sooner?** Offered during
   the FZ-04 test; it would change fetch contract rule 6.
5. Not blocking: #46 (edits an existing test); whether setting Status through
   `gh` belongs in FZ-08's first version.

Decided, nothing needed: #45 waits for the repository to go public; FZ-10's
O1 to O5 accepted (O1's words and look are his on the FZ-10c build); FZ-08
#21 is not to be started.

## Decisions of 2026-10-03 that later tickets inherit

- **A signed-out `R` fails and says so** (Andreas, "Merge as it is"): see
  What waits on Andreas, and amendment 9 of the freshness contract.
- **One function spells a host** (coordinator's ruling on #59):
  `model::host_key`, lowercasing only, used by `RemoteRef::key` and the cache
  file name. The hashing of an unusual host stays in `cache::host_part` and
  must not move into the identity key.
- **`FetchError.meta` is boxed** (coordinator's ruling on #59), because
  clippy's `result_large_err` fired once the account key was added. New
  fields for FZ-09b go inside `FetchMeta`.
- **The untyped "rate limit" message is a primary limit** (coordinator, with
  the FZ-10a review): the row is in `api-budget-design.md` 4.3, so FZ-10b's
  not-before is written against what the code does.
- **A test seam is the smallest that reaches the production line**
  (coordinator's ruling on #64): one private function for the
  `LoadingFrame` arm, not an extraction of the event dispatch.

## Decisions of 2026-10-02 that later tickets inherit

- **FZ-09 owner items: accepted, all as recommended** ("yes to all. it looks
  good", about 21:43). Recorded on #22 and at the top of "Owner items" in
  [coverage-cache-design.md](coverage-cache-design.md): O1 as the token
  fingerprint, O2, O3, O4, O5, O7 as the most recent saved copy, O8, O10,
  O11, O12, O14 and the section 6 contract changes; O9 and O13 later. This
  approves the cache format change of FZ-09c (#59). The words and look of O3
  and O4 are judged on the builds that draw them.
- **The ambiguity status line stays red** (his answer). It lasts 1.5 s like
  every status message, replaces the "Reloaded from cache" failure message,
  and a narrow footer can drop it.
- **His fleet has four repo names that exist under two owners.** He has seen
  the result (`cargo run ... -- --all` shows the other owner's copies as not
  cloned) and finds it good. The names are deliberately not written into this
  repository, which is to become public. The `t` overlay has no "all": the
  `-a` flag is the way to see every repo.
- **Rendering settings are ambient** (coordinator's ruling on #51). The active
  icon set is one thread-local in `src/ui/badges.rs`, read by the glyph choice
  functions, because existing tests pin the signatures of the functions that
  reach them and the palette tickets (FZ-13j, FZ-13k) will need an ambient
  setting too. It is sound because `#[tokio::main]` polls the main future on
  the calling thread and every draw happens in the event loop inside it.
  **Rendering must stay on the thread that set it.**
- **The owner rule is strict** (coordinator's ruling on #34, from design 3.5
  and D1): a row with an owner matches only an entry of that owner; a row or
  payload entry without an owner matches by name (a cache from before the
  field was asked for). One exception, found by the reviewer: the
  previous-entry lookup in `remotes_that_moved` matches an owner-less previous
  entry by name, or a push after a launch from such a cache is never fetched.
- **Notes commits do not count** in the local contribution strip, as stash
  commits do not (coordinator's ruling on #43).

## Facts later tickets need

- **Baseline (FZ-01).** First paint 6 to 7 ms; first useful row with a warm
  cache 257 ms for 14 rows and 1148 ms for 70; a cold cache costs about 6 s.
  Compare a base and a candidate binary interleaved in one session, never
  against the table. The harness is in the document's appendix. FZ-13a
  measured with it at 120x40: first legible row 1811 ms before, 1218 ms
  after; bytes to a settled screen 2.28 MB before, 0.96 MB after.
- **Freshness (FZ-03, amended).** `App::github_freshness` holds data time,
  last attempt and outcome. A cached fallback is an `Err` that carries data.
  [freshness-contract.md](freshness-contract.md) has six amendments from
  FZ-10, each in force when its ticket lands (3 and 4 are, with FZ-10a), and
  three from FZ-09c (7 to 9, in force): a fallback reads only this
  credential's file, cleanup is not a failure path, and an `R` that fails
  with no keyed file for the credential, or signed out, leaves the rows and
  says the load failed.
- **Fetch outcomes (FZ-04).** `f` fetches the selected repo only; `F` and the
  watch sweep fetch every clone. A failed fetch stays marked until that repo
  is fetched again. Commit `3469f67` has no recorded author session: it
  appeared between two sessions, Andreas says it was not his, and its content
  matched the reviewed change.
- **Identity (FZ-05a to f, all merged).** GitHub-owned fields live on one
  carrier row per remote identity, marked by the row's `owner`; the poll
  follows the mark through `carries` and `names_remote`. Discovery's
  `CheckoutRank` orders several clones of one repo and assembly reuses it for
  local carriers. `merge_sources` dedupes by `GithubRepo::identity_key`. Pass
  3 dedupes by canonical path. Load-side scenarios run through
  `assemble_view_in` with a temp home.
- **Cache keys (FZ-06a to e).** `WorkKey` is a hash of every (ref name,
  object id) pair plus the shallow file, with the HEAD symref and reflog size
  beside it; a failed ref query gives no key. Worktrees are read every sweep
  when `<common>/worktrees` exists. The histogram has its own key.
- **API budget (FZ-10).** Decisions A1 to A7 and the rulings on them are in
  the design. Backoff step 1 is exactly the interval; jitter from step 2 and
  on the cap (longest wait about twelve minutes); `Offline` and `Auth` are
  transparent to the ladder. FZ-10a is in: every answer with the rate-limit
  headers yields a `BudgetReading` in `FetchMeta.budget`, on `Ok` and `Err`
  alike; `Failure` carries `limit: Option<RateLimit>` (primary, or secondary
  with its `retry-after`) and `rate_limited` is true exactly when it is set;
  `FailureKind` has `Server` and `Refused`; `client()` is the one reused
  client (10 s connect, 30 s total); a connect failure is `Offline`, a later
  timeout `TimedOut`. Nothing consumes the reading yet: `apply_github_poll`
  binds `meta: _`, and `poll_cost` waits for FZ-10b with an
  `expect(dead_code)` that FZ-10b must remove.
- **Coverage and cache (FZ-09).** The account has 95 repos and 50 are
  fetched; `totalCount` and `hasNextPage` are free. Decisions C1 to C9 are in
  the design. `repo_limit` 100 fails with a 502 near 11 s. The cache is now
  written atomically (FZ-09a): temporary files are named `.` + target file
  name + `.tmp-<pid>-<counter>`, and startup removes only those, older than
  one hour, never from `ApiCache::new`. FZ-09c is in: `cache::KeyedCache`
  names the file `github-v2-{host}-{fingerprint}-{repo_limit}.json`
  (`FORMAT_VERSION` 2; the host from `model::host_key`, hashed to `h_` plus
  16 hex digits when it is not plain); production enters through
  `github::fetch_github` with an injectable token source, and `fetch_repos`,
  `request` and `with_budget` are test-only wrappers over the same code;
  `FetchMeta.account` holds the fingerprint, unknown for a load served from
  the legacy file or signed out; `FetchError.meta` is a `Box<FetchMeta>`.
  Launch waits on one `gh` lookup, at most `LAUNCH_LOOKUP_BOUND` (2 s).
  Superseded-file cleanup runs only inside a launch that has data and a
  known account.
- **UI (FZ-13).** The inventory behind the split is in the first comment on
  #26. Done from it: the wave removed, plain icons (also switchable in the
  `,` settings), the help, the header and footer widths, the focus fixes
  (`Shift-Tab` reverses `Tab` through `App::toggle_panel_focus_back`), the
  hyphen in the header. Both columns of the help are exactly full at 80x24: a new
  key row needs one merged or removed, and its `,` row is tied by test to the
  labels in `SETTING_FIELDS` (eleven rows now; the settings popup is 18 rows
  tall). A help description may be at most 26 columns wide without widening
  its column. Letters FZ-13a to FZ-13u are taken.

## Tickets that exist and wait

- **Coverage and cache:** FZ-09a, b, c, d and f are done; FZ-09e #61 stays
  not promoted.
- **API budget:** FZ-10a, b and c are done.
- **M3 regression:** #68 (mouse) done.
- **M3:** FZ-13s #56, which waits for Andreas. FZ-13a to f, t and u and #64
  are done. All of these issues sit in Backlog on the board; none is Ready.
- **Follow-ups (P2):** #46 (needs Andreas's OK).
- **#45** (M5), parked.
- **FZ-08 #21** (M4): a read-only GitHub Project board view. He said not to
  start it.
- Planning issues: #20, #24 to #30, and #2 to #5, #7.

## Follow-ups found and not ticketed

- **One key for "same remote".** Five sites lowercase an owner and a name:
  `main.rs::remote_key`, `main.rs::names_remote`, discovery's `repo_key`,
  `GithubRepo::identity_key` and `RemoteRef::key`. `remote_key` should
  delegate to `identity_key`; the rest should become one key and one match.
- **`unclaimed_local`** keeps an unused `checked_names` parameter for its
  signature: rename it `_checked_names`, or drop it when the existing test's
  call may be edited. The doc comment on the existing test
  `an_extra_checkout_named_like_its_remote_takes_nothing_from_the_poll` still
  says its load half waits for FZ-05f.
- **Same-named rows look identical** in the list (two owners, or extra
  checkouts) until FZ-05g, which is optional in the design and has no issue.
- **One measure for width.** The list, detail pane, header and footer count
  characters; the help uses ratatui's width. Wide user text (a CJK group
  name, an emoji) is measured narrower than it draws. One ticket should move
  the whole UI to ratatui's width.
- **Tests that could be sharper:** the header test covers the right-hand sum
  only through the Nerd Font watch mark; no test notices the cache rename
  being replaced by a copy (a hard-link test would); the PR section's
  equal-case test can be satisfied by the header row; no test pins that a
  branch or tag merely named `stash` or `notes` still counts; the startup
  copies of config onto `App` in `main` have no test.
- **From the help review:** the `J/K` row no longer says what the key does in
  the graph and compact views; the footer and the branches overlay spell
  `enter` in lowercase where the help says `Enter`; the footer keeps white
  keys where the help's are sapphire.
- **`window_exceeds_dataset`** reads the merged list, so org repos and
  same-named repos can hide or raise its warning; FZ-09b replaces it.
- **Refetch on recovery:** refetch repos whose fetch failed as unreachable or
  timed out when the GitHub poll recovers. Changes fetch contract rule 6.
- **Ref query cost** (FZ-06b review, medium): the widened `for-each-ref`
  computes `%(refname:short)` for every ref each sweep, about 74 ms per call
  at 5,000 refs and 300 ms at 20,000, against about 10 ms before. The largest
  repo in this fleet has 50 refs. Matters for a public release.
- **From the FZ-04 review:** the fetch tally's expiry is not extended by a
  later batch; one failing repo widens the compact grid's badge column; old
  fetch facts stay on a path that now holds a different repo; bidi characters
  are not stripped from the failure detail; a deleted checkout's row keeps
  its marks until a reload.
- **From the FZ-06 reviews, small:** `parse_other_refs` keeps a line with a
  missing id; `dirty_mode_status` and `local_only_status` start with no key;
  no deterministic E13 test; `handle_branch_input` takes the repo index from
  the selection, not the overlay; test scratch directories are not removed.
- **From the FZ-05b review:** a `file://` origin and a renamed GitHub account
  are refused where the old code claimed by folder; a clone with no origin
  ranks as off github.com.
- **`CLAUDE.md` text the code contradicts:** the tick is 100 ms, not 250 ms;
  `r` does not rebuild from cache; it lists only two of the four Nerd Font
  uses and does not mention the `icons` key or its row in the settings.
  `cache_ttl` decides nothing observable. (The stale names in the launch
  invariant were corrected on 2026-10-03.)
- **From the reviews of 2026-10-03:**
  - FZ-10a: two loopback tests use a 300 ms total deadline for answers
    expected in full and could fail on an overloaded runner; a test helper
    binds and drops a port before connecting (also `nowhere()` in FZ-09c's
    tests); `refusal_message` shows a JSON body without a `message` key raw
    (the wording is FZ-10c's); no test in `main.rs` pins that
    `apply_github_poll` releases the in-flight lock on `Server` and
    `Refused`.
  - #64: a test that runs the event loop itself needs the dispatch and the
    paint decision extracted from `run_event_loop` (with FZ-11 or FZ-13s);
    a test for a key press that dismisses the field with a frame queued.
  - FZ-13d: `README.md` lists `Tab` but not `Shift-Tab`; no test exercises
    the `Tab` or `BackTab` arm itself; the `FocusedPanel::Contributions` doc
    in `src/app.rs` is inaccurate; a resize or a settings change into
    compact view leaves focus on a pane compact does not draw.
  - FZ-13u: the doc on `set_active_icons` in `src/ui/badges.rs` still says
    "once at startup"; the settings popup clips its save-status line on a
    terminal shorter than 18 rows.
  - FZ-09c: a signed-out `R` could behave like a signed-out launch; the
    `KeyedCache::cleanable` wiring and the production use of
    `LAUNCH_LOOKUP_BOUND` have no test; CK9 checks the helper
    `ApiCache::new` calls, not `ApiCache::new`.
  - FZ-13f: the doc on the test `every_view_marks_its_selection_the_same_way`
    in `src/ui/detail.rs` says "all four views" and the test loops over five.
- **Tests:** the real-repo tests in `src/git.rs` inherit the machine's global
  git config; the parity tests assume the system temp directory is not inside
  a repository.
- **From #45's review:** a skipped job reports as Success, and both workflows
  name their job `check`.

## Practical notes for the next lead

Lessons from earlier days (worker briefs, reviews, clock times, tool output
that looks like an instruction) are in [agent-log.md](agent-log.md), "Day
notes".


- Use absolute paths and `git -C`; do not `cd` into a worktree: a `cd` in a
  compound command moved the session's working directory once.
- Split long commands: a compound commit, push and merge was refused by the
  permission check; one step at a time went through. A foreground `sleep` is
  refused too: wait with `gh run watch <id> --exit-status`.
- `git rev-parse` with two revisions fails under the `rtk` hook; ask for one.
  Use `rtk proxy rg` when the full output of a search is needed.
- `scripts/check-docs.sh` only scans tracked files: check a new document's
  dashes, links and anchors by hand before it is committed.
- Combined checks run in a throwaway clone under the session scratchpad, with
  the merge committed there and one shared `CARGO_TARGET_DIR`. Compare the
  merged tree on `main` with the tested clone's tree before pushing. Two
  accepted tickets can be checked in one clone by merging them in the order
  they will be merged. If `main` moved in code since the check, rerun it.
- A ticket branch with no commit of its own can be fast-forwarded to `main`
  while its uncommitted change stays (`git merge --ff-only main` in the
  worktree), when `main` did not touch the same files.
- `.claude/` is globally ignored but `.claude/agents/` is tracked: stage it
  with `git add -f`.
- Do not use `Closes #N` in a commit message: issues are closed by hand with
  the evidence.
- The REST `rate_limit` endpoint reports a different GraphQL bucket; read the
  real budget with `gh api graphql -f query='{ rateLimit { remaining } }'`.
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

From this repository, run:

```sh
claude --agent fz-coordinator
```

The agent definition pins the model and effort and carries the takeover
instructions. Use normal permissions with no bypass, on the Claude
subscription, with no API-key or third-party provider override. Stop if
Fable is unavailable or limited and ask Andreas for routing.
