# Fleetz coordinator handoff

## Baton

**Held** by `claude-fable-5-1` (`claude --agent fz-coordinator`, session
`60a43b88-e448-4466-be9c-e93c226b8f44`), taken 2026-10-02 18:34 CEST.

- **Previous lead stopped.** Andreas answered "Retired, take over" in this
  session about the two older sessions still open in this repository (a
  `claude` started 2026-10-01 13:54 and a `codex` started 2026-09-30 23:25).
  Live state agreed: no build or worker process, no CI run and no issue
  comment since 2026-10-01 21:36, worktree files last written 21:18
  (`fz-05b`), 21:32 (`fz-13a`) and 18:34 (`issue-45`) that day.
- **Standing scope confirmed.** Andreas answered "Both hold" in this session
  on 2026-10-02 18:34 to the pace-and-dispatch and where-his-testing-matters
  instructions below. Their limits are unchanged.
- **State at 2026-10-02 21:47: two reviewers and one worker are
  running.** `fz-reviewer` on FZ-05f (#36, worktree `fz-05f`, uncommitted
  diff hash `95715991` against `82c75c5`); `fz-reviewer` on FZ-13e (#54,
  worktree `fz-13e`, uncommitted diff hash `622307cf` against `82c75c5`);
  `fz-worker` on FZ-09a (#57, worktree `fz-09a`, branch
  `fix/fz-09a-atomic-cache-writes`, base `841a2e2`, `src/cache.rs`),
  dispatched 21:47. Everything else dispatched today is merged and cleaned
  up: FZ-05b, FZ-05c, FZ-05d, FZ-05e, FZ-13a, FZ-13b, FZ-13c, FZ-13t and
  #43. Nothing waits at Ready to integrate. A lead that finds a later
  state recorded on an issue and not here trusts the issue, and checks
  #36, #54 and #57 before dispatching again.

> **The no-install span is closed.** FZ-05b opened it (`f37c6b9`,
> 2026-10-02 19:00) and FZ-05e closed it (`40a2c73`, 2026-10-02 21:31; CI
> on `main` green there and at the merge after it, `82c75c5`). `main` again
> shows every checkout. Andreas was told in the report after these
> merges. Installing and tagging remain his.

The rest of this file is the state as written at the end of 2026-10-01,
except "In flight", "What waits on Andreas", "Next steps" and the 2026-10-02
rows under "Done", which are kept current as tickets move.

Last full rewrite 2026-10-01 21:40 by `claude-fable-5-1`, when Andreas stopped
that session for the day with usage at 85%. Nothing was running then: no
worker, no reviewer, no build, no background job. Read this file, the repository
instructions, [agent-workflow.md](agent-workflow.md) and
[release-plan.md](release-plan.md) before acting. GitHub (the issues and the
[release Project](https://github.com/users/Gaurgle/projects/1)) is
authoritative for status; this file says why things are where they are and
what to do next.

**About times.** Between about 20:05 and 21:09 this session wrote estimated
clock times into issue comments, and they ran ahead, up to "00:50 on
2026-10-02". Everything in this session happened on 2026-10-01 between 19:34
and 21:40. Trust GitHub's own timestamp on a comment and `git log` for a
commit, not a time quoted inside a comment's text. Read the clock (`date`)
before writing a time.

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

## What Andreas told this session, and still holds

Said to the 19:34 session on 2026-10-01. A new lead confirms the first two
once before relying on them; the rest are written into the repository.
Confirmed to the 2026-10-02 18:34 session ("Both hold").

- **Pace and dispatch.** "keep me updated. backend is still vital, but its
  more important that we move forward in a good pace. But keep me updated,
  and ask me about decisions that are bigger". Earlier: "get along with the
  rest". Under it the coordinator dispatched in plan order without waiting,
  ruled on small questions, reported briefly at each merge, and went to him
  for design decisions, contract changes, anything on screen, and test edits
  beyond the exception below.
- **Where his testing matters.** "these 'small' tickets like these cache
  fixes, coverage etc, are not much for me to test in the actual software, i
  think you're tests tell a bigger and more true picture of the situation. I
  think my opinions will matter more when we start to look at the UI
  updates". So: no "try the build" for tickets with nothing to see; every
  ticket that changes the screen or a key goes to him with the branch build
  before it merges.
- **Small look changes no longer wait for him** (said to the 2026-10-02
  18:34 session at about 21:28, after he had tried five branch builds that
  day). On FZ-13b: "this i trust. i dont care to change that setting".
  Then: "i dont think i need to review these small things either, lets
  keep going rather than stopping for minor things at this point". So
  this narrows the item above: a minor on-screen change (a character, a
  color, a label, a status line, a small row or badge) merges under the
  safe-merge rule once reviewed, and the coordinator reports it at the
  merge with the command to look at `main`. Still his before they are
  built or merged: a new screen or a redesign (FZ-13s, the FZ-13 wave 4
  questions), a new key or changed interaction, contract and owner items
  (FZ-09), a new config key, and any test edit beyond the standing
  exception. When in doubt whether something is minor, ask. A new lead
  confirms this once before relying on it.
- **Existing tests** (in [agent-workflow.md](agent-workflow.md), "Validation
  contract"): editing an existing test is his decision, except a
  compile-forced new field in existing test code with no assertion changed,
  which goes ahead and is confirmed by the reviewer.
- **Writing files** (in `agent-workflow.md` and
  `.claude/agents/fz-coordinator.md`): Write or Edit, never heredocs or
  redirection; command text goes through a scratchpad file and `--body-file`.
- **Cleanup is part of Done** (same two files): after the merge and green CI
  on `main`, remove the ticket's worktree and local branch, clean, merged and
  pushed first, never forced; remote branches stay.
- **Issues:** he allowed creating the FZ-10a to c issues and the M3 issues
  his answers settled. Ask before creating others.
- **CI changes** were approved ticket by ticket, never in general.

## Done on 2026-10-01

All merged into `main` with `git merge --no-ff`, CI green on `main`, issues
closed with the evidence in a comment. `main` is at `5fbba56` plus this
file's commit.

| Ticket | Issue | Ticket commit | Merge | What landed |
|---|---|---|---|---|
| FZ-02 | #15 | `8027d3b` | `28ebba7` | Sweep and poll each write only the fields they own |
| Pin | #1 | `1c3701a` | `6dd260f` | `dtolnay/rust-toolchain` pinned to a commit |
| FZ-01 | #14 | `1cfd89c` | `b084d21` | [performance-baseline.md](performance-baseline.md) |
| FZ-05 | #18 | `100cb6e` | `ee65eb7` | [repository-identity-design.md](repository-identity-design.md) |
| FZ-06 | #19 | `f2f3aa7` | `ff56a0d` | [cache-invalidation-design.md](cache-invalidation-design.md) |
| FZ-05a | #31 | `5972d50` | `8c1546c` | `RemoteRef { host, owner, name }` in `model.rs` |
| Tests | #44 | `3b38c49` | `375bc7a` | `Config::load_from_candidates` seam and missing-config tests |
| FZ-06a | #37 | `520e77e` | `28fd17f` | `git::repo_dirs`, the git and common directories in one helper |
| FZ-03 | #16 | `5217d30`, `963f575` | `407aa49` | GitHub freshness as three facts; the header segment |
| Tests | #42 | `497ee62` | `d564fc7` | repoz parity tests run from outside any repository |
| FZ-04 | #17 | `4839891`, `3469f67` | `f8a54c0` | Fetch outcomes per checkout; `⚠ unreachable` and its kin on the row |
| FZ-06b | #38 | `e33914d` | `f3663e6` | `WorkKey` hashes every ref name and object id plus the shallow file |
| FZ-06e | #41 | `e33914d` (FZ-06b) | `f3663e6` | Closed without a dispatch: FZ-06b reordered the load path |
| FZ-10 | #23 | `9494938` | `f627a84` | [api-budget-design.md](api-budget-design.md); owner items accepted; tickets #47 to #49 |
| FZ-06c | #39 | `5c83fb0` | `24b2210` | The worktree list is read on every sweep |
| FZ-06d | #40 | `92cf932` | `e4e3f42` | The local histogram carries its own reuse key |
| FZ-09 | #22 | `ce557e1` | `d0f23af` | [coverage-cache-design.md](coverage-cache-design.md); owner items accepted on 2026-10-02 |

The first eleven rows are from earlier sessions that day; the last six, the
FZ-04 fix and merge, and the FZ-13 split are this session's.

## Done on 2026-10-02

| Ticket | Issue | Ticket commit | Merge | What landed |
|---|---|---|---|---|
| FZ-05b | #32 | `47ce339` | `f37c6b9` | Discovery matches by owner and name; a stable six-step primary checkout; ambiguities as data |
| FZ-05c | #33 | `24f2436` | `d6604cb` | `assemble_view_in` takes `home` and the scan roots; an S8a smoke test through the seam |
| FZ-13a | #50 | `62598b8` | `ed225d5` | The intro wave is deleted; the loading field stays; `intro` switches the loading screen |
| FZ-13t | #63 | `70e84c9` | `5b4d4c1` | The list row's worktree count is `WT` in `colors::WORKTREE_CLR` (lavender) |
| FZ-05d | #34 | `4eafc18` | `3daea80` | One carrier row per remote identity; the poll, moved-remote check, fetch baseline and sort keyed by owner and name; the ambiguity status line |
| FZ-13c | #52 | `b494248`, `b36d1f3` | `117514c` | A two-column help sized from its content, complete against the event loop, keys in sapphire |
| FZ-13b | #51 | `ffba73e` | `aec945c` | Config key `icons`: the Nerd Font set by default, six plain one-column characters with `plain` |
| FZ-05e | #35 | `27a8c70` | `40a2c73` | The GitHub dataset keeps one entry per owner and name; the moved-remote check returns each row once |
| Stash | #43 | `5604301` | `82c75c5` | The local contribution count excludes stash and notes commits |

FZ-13b: Andreas accepted the six characters unseen in detail ("this i
trust"). **Rule later tickets inherit, ruled by the coordinator on #51:**
the active icon set is an ambient thread-local in `src/ui/badges.rs`, the
single source read by the glyph choice functions, because existing tests
pin the signatures of the functions that reach them and the palette
tickets (FZ-13j, FZ-13k) will need an ambient render setting anyway. It is
sound because `#[tokio::main]` polls the main future on the calling thread
and every draw happens in the event loop inside it. **Rendering must stay
on the thread that set it.** His idea from that build, not ticketed and to
be asked: switching the icon set from inside the app (a settings row, which
needs existing calls to `SettingsOverlay::new` in tests edited).

FZ-05e: Andreas's own fleet has four repo names that exist under two
owners (found in his saved GitHub copy; the names are deliberately not
written into this repository, which is to become public). The coordinator
first held the ticket at Ready to integrate for his look; he tried the
build and saw nothing, because the coordinator had told him to choose
"all" in the `t` overlay, which has no such choice. The `-a` flag shows
those rows. He then gave the "small look changes" instruction and it
merged under the safe-merge rule. He has since confirmed the result on his
fleet and that the ambiguity status line stays red (FZ-05d's worker's
choice; it lasts 1.5 s, replaces the "Reloaded from cache" failure message,
and a narrow footer can drop it).

FZ-13c: Andreas asked for color after the first build and accepted the
second on 2026-10-02 ("the colored help section looks good").

FZ-05d took three fix cycles and the reviewer's acceptance covers the whole
diff (hash `453e8477` against `d6604cb`, on #34). Rulings by the coordinator,
all recorded on #34: `src/discovery.rs` opened for `#[cfg(test)]` on two
wrappers and for exposing `CheckoutRank` instead of a copied order; an owned
row matches only its own owner (design 3.5, D1); the previous-entry lookup
in `remotes_that_moved` keeps the legacy direction, after the reviewer
showed the strict form missed a push following a launch from a pre-owner
cache. `CLAUDE.md` carries invariant changes 3 and 4 and a paragraph on the
carrier rule.

FZ-13t: Andreas asked for it and accepted the branch build on 2026-10-02
("WT badge is good"). The detail pane's "N worktrees" line stays in
`SUBTEXT`, by the wording of his direction.

FZ-13a: Andreas tried the branch build on 2026-10-02 and accepted it as a
step ("lets go with A"): the wave is gone, and replacing the noise field
with `LOADING`, a bar and status lines is its own ticket, FZ-13s (#56,
Backlog, P2), created with his OK. His words: "loading screen animations
are not prioritized right now ... get to it at a later stage. well answer
the loading text's when we get to that ticket." Do not dispatch #56 until
he takes it up; its owner decisions are listed in the issue.

FZ-05b took two test-only fix cycles after the first review. The second was
the coordinator's ruling: the path tie-break had no test that failed without
it, and the S5 primary choice is an acceptance criterion. A fresh reviewer
killed all six step mutants and accepted the whole revision (diff hash
`c91ed0bf` against `e4e3f42`, recorded on #32). `CLAUDE.md` carries
invariant change 1 of the identity design's section 3.10.

## In flight

Worktrees are under `.claude/worktrees/`.

- **FZ-05f (#36), `fz-05f`, branch `fix/fz-05f-pass3-canonical-path`,
  base `82c75c5`.** Implemented (`src/main.rs` only, +268 -22, diff hash
  `95715991`, 877 tests by the worker's run, no existing test edited; the
  report is on #36), in review. Pass 3 dedupes by canonical
  path only; the `checked_names` parameter of `unclaimed_local` is kept
  for the signature and ignored. The veto in its body is settled (Andreas
  approved the change with the design on 2026-10-01). On acceptance:
  commit, push, CI, combined checks, merge under the safe-merge rule, edit
  `CLAUDE.md` invariant change 2, and tell him what rows are new (dirty
  extra checkouts, same-named local-only repos). The identity series is
  then complete.
- **FZ-13e (#54), `fz-13e`, branch `fix/fz-13e-measure-drawn-width`, base
  `82c75c5`.** Implemented (`src/ui/mod.rs` and `src/ui/footer.rs`, +238
  -3, diff hash `622307cf`, 878 tests by the worker's run; the report is
  on #54), in review. The header and footer count characters
  instead of bytes. The reviewer is asked whether these two lines should
  use ratatui's own width. On acceptance it merges under the "small look
  changes" instruction. FZ-13f (#55) waits for it: both edit the header in
  `src/ui/mod.rs`.
- **FZ-09a (#57), `fz-09a`, branch `fix/fz-09a-atomic-cache-writes`, base
  `841a2e2`.** Worker dispatched 2026-10-02 21:47: `ApiCache::write`
  through a temporary file and a rename, new files mode 0600; `src/cache.rs`
  only, standard library only. Then a reviewer, then the safe-merge rule
  (no format or key change).
- **`issue-45`, branch `ci/45-pull-request-trigger`:** uncommitted `ci.yml`
  and `docs.yml`, reviewer verdict blocked on a repository setting. Parked
  by Andreas until the repository goes public.

No other worktree or local ticket branch exists. Merged branches remain on
`origin`.

## What waits on Andreas

1. **Nothing about FZ-09: the owner items are answered.** On 2026-10-02 at
   about 21:43 he answered "yes to all. it looks good" to a message that
   offered "all as recommended" for them. Recorded on #22 and at the top
   of "Owner items" in
   [coverage-cache-design.md](coverage-cache-design.md): O1 as the token
   fingerprint, O2, O3, O4, O5, O7 as the most recent saved copy, O8, O10,
   O11, O12, O14 and the section 6 contract changes accepted; O9 and O13
   later. That approves the cache format change of FZ-09c (#59). The
   issues FZ-09a #57, FZ-09b #58, FZ-09c #59, FZ-09d #60, FZ-09e #61 (not
   promoted) and FZ-09f #62 (optional) each carry a comment saying so.
2. **Answered the same way, the same "yes to all":** the ambiguity status
   line stays red; the `LoadingFrame` redraw bug is issue #64 (Backlog,
   P2); switching the icon set from the settings overlay is FZ-13u, #65
   (Backlog, P2), and for that ticket only he released the mechanical
   edit of existing test calls to `SettingsOverlay::new` (one argument
   added with a neutral value, no assertion changed). He also confirmed
   the two-owner rows on his own fleet: he understands the other owner's
   copies now and finds it good (his words name that owner, so they are
   not quoted in this repository).
3. **FZ-13 wave 4** (maximize and restore, hints that follow focus,
   economical folds): each needs a decision from him before it is a ticket.
   The questions are in the split on #26.
4. **Follow-ups below:** may they become issues? Not asked.
5. **Should a recovered network clear fetch marks sooner?** Offered during
   the FZ-04 test; he merged without choosing it. It would change fetch
   contract rule 6.
6. Not blocking: whether Wi-Fi was off or changing state for the earlier
   `⚠ timeout` rows; #46 (edits an existing test); whether setting Status
   through `gh` belongs in FZ-08's first version.

Decided, nothing needed: #45 waits for the repository to go public; FZ-10's
O1 to O5 accepted as recommended (O1's words and look are still his on the
FZ-10c build); FZ-13: delete the intro wave and keep the loading screen, Nerd
Font icons stay the default with an `icons` setting for plain characters (a
setup wizard is an idea for FZ-17, not M3), a two-column help, a hyphen in
the header.

## Next steps, in order

1. **FZ-05f (#36) and FZ-13e (#54) are in flight** (see "In flight"):
   review each when its worker reports, then merge under the safe-merge
   rule. With FZ-05f the coordinator edits `CLAUDE.md` invariant change 2,
   and the identity series is complete.
2. **After FZ-05f, FZ-10a (#47)** takes the `main.rs` line; **after
   FZ-13e, FZ-13f (#55)** takes the header, then FZ-13d (#53).
3. **FZ-09:** the owner items are accepted. FZ-09a (#57) is in flight.
   FZ-09f (#62, the PR section label in `src/ui/detail.rs`) can run in any
   free slot. As each FZ-09 ticket lands, amend `CLAUDE.md` and
   [freshness-contract.md](freshness-contract.md) as the design's section
   6 says.
4. **The `main.rs` line after the identity series**, one worker at a time,
   in the order ruled for FZ-09 (C8): FZ-10a #47, FZ-09c #59, FZ-09b #58,
   FZ-10b #48, FZ-10c #49, FZ-09d #60. #47 carries a note from FZ-09:
   `token_id` is a pinned FNV-1a fingerprint and the new fetch field is one
   struct with a `Default`. FZ-10a owns `src/github.rs` and can start once
   FZ-05e is merged.
5. **M3 in the second slot:** FZ-13e #54 and FZ-13f #55 (both touch the
   header in `ui/mod.rs`, so one at a time, and not beside FZ-10c), FZ-13d
   #53 (one key arm in `main.rs`). Each stops at Ready to integrate for
   Andreas, so mind how many builds wait for him at once. Wave 3 and the
   rendering tickets in the split on #26 have no issues yet; write their
   bodies before asking. FZ-13s #56 waits for him.
6. #43 (`git.rs`, stash commits in the local count) fits between any two.

Keep to two workers with disjoint ownership, plus their reviewers.

## Facts later tickets need

- **Baseline (FZ-01).** First paint 6 to 7 ms; first useful row with a warm
  cache 257 ms for 14 rows and 1148 ms for 70; a cold cache costs about 6 s.
  Compare a base and a candidate binary interleaved in one session, never
  against the table. The harness is in the document's appendix; FZ-13a's
  worker extracted and ran it.
- **Freshness (FZ-03, amended).** `App::github_freshness` holds data time,
  last attempt and outcome. A cached fallback is an `Err` that carries data.
  [freshness-contract.md](freshness-contract.md) has six amendments from
  FZ-10, each in force when its ticket lands.
- **Fetch outcomes (FZ-04).** `f` fetches the selected repo only; `F` and the
  watch sweep fetch every clone. A failed fetch stays marked until that repo
  is fetched again. Commit `3469f67` has no recorded author session:
  it appeared between two sessions, Andreas says it was not his, and its
  content matched the reviewed change.
- **Identity (FZ-05).** GitHub-owned fields keyed by owner and name,
  sweep-owned fields by checkout path. Andreas signed off on the four
  `CLAUDE.md` invariant changes in section 3.10 and on FZ-05f.
- **Cache keys (FZ-06a to e).** `WorkKey` is a hash of every (ref name,
  object id) pair plus the shallow file, with the HEAD symref and reflog
  size beside it; a failed ref query gives no key. Worktrees are read every
  sweep when `<common>/worktrees` exists. The histogram has its own key.
- **API budget (FZ-10).** Decisions A1 to A7 and the rulings on them are in
  the design. Backoff step 1 is exactly the interval; jitter from step 2 and
  on the cap (longest wait about twelve minutes); `Offline` and `Auth` are
  transparent to the ladder.
- **Coverage and cache (FZ-09).** The account has 95 repos and 50 are
  fetched; `totalCount` and `hasNextPage` are free. Decisions C1 to C9 are
  in the design. `repo_limit` 100 fails with a 502 near 11 s.
- **UI (FZ-13).** The inventory behind the split is in the first comment on
  #26: no font fallback, 42 hard-coded RGB colors and no `NO_COLOR`, a help
  overlay that shows 13 of 30 rows at 80x24, `Shift-Tab` cycling forward,
  header and footer measuring bytes, about 150 tests pinning exact glyphs,
  strings and colors.

## Tickets that exist and wait

- **Identity:** FZ-05e #35 (next), FZ-05f #36. Serial.
- **API budget:** FZ-10a #47, FZ-10b #48, FZ-10c #49. Serial, after the
  identity series.
- **Coverage and cache:** FZ-09a #57, FZ-09b #58, FZ-09c #59, FZ-09d #60,
  FZ-09e #61 (not promoted), FZ-09f #62 (optional). None is ready for
  dispatch until Andreas has answered the owner items its body names.
- **M3:** FZ-13s #56 (Backlog, waits for Andreas), FZ-13b #51, FZ-13c #52, FZ-13d #53,
  FZ-13e #54, FZ-13f #55.
- **Follow-ups (P2):** #43; #46 (needs Andreas's OK).
- **#45** (M5), parked.
- **FZ-08 #21** (M4): a read-only GitHub Project board view. Reshaped at his
  request; he said not to start it.
- Planning issues: #20, #24 to #30, and #2 to #5, #7.

## Follow-ups found and not ticketed

- **ssh for fetches:** an unreachable SSH remote hangs to the 30 s limit and
  leaves an ssh child running, and ssh can prompt on the TUI's terminal.
  Candidate: `-o ConnectTimeout=10 -o BatchMode=yes`, only when none of
  `GIT_SSH_COMMAND`, `GIT_SSH` or `core.sshCommand` is set.
- **Refetch on recovery:** refetch repos whose fetch failed as unreachable or
  timed out when the GitHub poll recovers. Changes fetch contract rule 6.
- **Ref query cost** (FZ-06b review, medium): the widened `for-each-ref`
  computes `%(refname:short)` for every ref each sweep, about 74 ms per call
  at 5,000 refs and 300 ms at 20,000, against about 10 ms before. The
  largest repo in this fleet has 50 refs. Matters for a public release.
- **From the FZ-04 review:** the fetch tally's expiry is not extended by a
  later batch; one failing repo widens the compact grid's badge column; old
  fetch facts stay on a path that now holds a different repo; bidi
  characters are not stripped from the failure detail; a deleted checkout's
  row keeps its marks until a reload.
- **From the FZ-06 reviews, small:** `parse_other_refs` keeps a line with a
  missing id; `dirty_mode_status` and `local_only_status` start with no key;
  no deterministic E13 test; `handle_branch_input` takes the repo index from
  the selection, not the overlay; test scratch directories are not removed.
- **From the FZ-05b review:** a `file://` origin and a renamed GitHub
  account are now refused where the base claimed by folder; a clone with no
  origin ranks as off github.com.
  `s5_of_two_equally_deep_clones_the_first_path_is_primary` passes only by
  the filesystem's listing order and is redundant with
  `s5_the_first_path_is_primary_whichever_clone_is_offered_first`.
- **From the FZ-13a review (medium, older than the ticket):** in
  `run_event_loop` the `AppEvent::LoadingFrame` arm assigns
  `app.needs_redraw = app.loading_field.is_some()`. A frame queued while
  the `Loaded` arm runs resets the flag after the dismiss, the held first
  paint is lost and the loading field stays up until a spinner tick,
  `RepoUpdated` or a key. Candidate: OR the value in, with an event-order
  test (`Loaded` then `LoadingFrame`). Also a stale doc comment on
  `an_animation_frame_is_never_held` ("made the intro stutter").
- **`CLAUDE.md` text the code contradicts:** the tick is 100 ms, not 250 ms;
  `r` does not rebuild from cache; it names `last_github_poll` and
  `GithubFetch::from_cache`, which no longer exist; it lists only two of the
  four Nerd Font uses. `cache_ttl` decides nothing observable.
- **Tests:** the real-repo tests in `src/git.rs` inherit the machine's
  global git config; the parity tests assume the system temp directory is
  not inside a repository.
- **From #45's review:** a skipped job reports as Success, and both
  workflows name their job `check`.
- **Nits in merged designs:** in the API budget design, O5 calls two test
  helpers "tests" and one line in FZ-10c's acceptance is over-long.

## How the day ran

- The usage limit was hit once in the afternoon with five agents in flight.
  This session kept to two or three and ended at 85% by Andreas's call.
- Every substantive ticket changed in review. Reviewers ran mutants and
  found untested rules (FZ-05b), a temporary-file collision and a rule that
  voided cache isolation (FZ-09), and tests that would have broken (FZ-10).
- Three tickets stopped on the existing-test rule before the standing
  exception existed. Tell every worker to look for forced test edits before
  writing the fix.
- A reviewer or worker can be continued for a narrow follow-up through its
  agent id within a session; across sessions, dispatch a fresh one with the
  issue's last comment.

## Practical notes for the next lead

- Use absolute paths and `git -C`; do not `cd` between checkouts.
- Split long commands: a compound commit, push and merge was refused by the
  permission check; one step at a time went through. A foreground `sleep` is
  refused too: wait with `gh run watch <id> --exit-status`.
- `git rev-parse` with two revisions fails under the `rtk` hook; ask for one.
- `scripts/check-docs.sh` only scans tracked files: check a new document's
  dashes, links and anchors by hand before it is committed.
- Combined checks run in a throwaway clone under the session scratchpad,
  with the merge committed there and one shared `CARGO_TARGET_DIR`. Compare
  the merged tree on `main` with the tested clone's tree before pushing.
- `.claude/` is globally ignored but `.claude/agents/` is tracked: stage it
  with `git add -f`.
- The REST `rate_limit` endpoint reports a different GraphQL bucket; read
  the real budget with `gh api graphql -f query='{ rateLimit { remaining } }'`.
- Board edits: project `PVT_kwHOCU842c4BlQ1e`. Status field
  `PVTSSF_lAHOCU842c4BlQ1ezhj-XiE`: Backlog `34f9165b`, Ready `79ac8034`,
  In progress `a2e70ebb`, Review `40798e53`, Ready to integrate `d28a1321`,
  Done `5114f2e5`. Priority field `PVTSSF_lAHOCU842c4BlQ1ezhj-ZPA`: P0
  `64e82877`, P1 `63d976f2`, P2 `be37b935`. Worker field
  `PVTF_lAHOCU842c4BlQ1ezhj-ZPE`, Ticket reviewer field
  `PVTF_lAHOCU842c4BlQ1ezhj-ZPI`. Read item ids with
  `gh project item-list 1 --owner Gaurgle --limit 100 --format json --jq ...`.

## Start Fable manually

From this repository, run:

```sh
claude --agent fz-coordinator
```

The agent definition pins the model and effort and carries the takeover
instructions. Use normal permissions with no bypass, on the Claude
subscription, with no API-key or third-party provider override. Stop if
Fable is unavailable or limited and ask Andreas for routing.
