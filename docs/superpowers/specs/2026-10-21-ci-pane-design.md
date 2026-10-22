# CI pane: watch workflow runs, jobs and steps inside fleetz (FZ-30)

Written 2026-10-21 by the coordinator (`claude-fable-5-1`) for Andreas's
approval. Planning issue: [#67](https://github.com/Gaurgle/fleetz/issues/67)
(FZ-30, M4). Status: **accepted 2026-10-21**, then **amended the same
afternoon** after he used the band (see "Amendment 2026-10-21: a pane 2
page, not a pane"): CI becomes a view in pane 2 with the inspector
detailing the highlighted run, and the band goes. Sections A, B and E
below describe the band as merged in #89 and #91 and are kept for the
record; the amendment supersedes them. Tickets #89 to #93 plus FZ-30f.
The issue's two decisions stand: draw natively, never embed `gh`; its own
pane, pane 4, with the contributions strip renumbered to pane 5.

## What the pane is for

A workflow run can be watched without leaving fleetz: the selected repo's
recent runs on every branch, a selected run's jobs and steps live while it
runs, and the failing step with the tail of its log when it fails. Rerun,
cancel and the full log hand off to `gh` through the existing
suspend-and-run path; fleetz never writes to GitHub.

## Measured facts the design rests on

Measured 2026-10-21 against `Gaurgle/fleetz` with `gh api -i`:

1. **REST Actions calls bill the `core` resource, separate from GraphQL.**
   `X-Ratelimit-Limit: 5000`, `X-Ratelimit-Resource: core`. The fleet poll's
   GraphQL budget (`ApiBudget`, FZ-10) is untouched by anything here.
2. **A conditional request answered `304 Not Modified` is free.**
   `X-Ratelimit-Used` stayed at 19 across the 304 and rose to 20 on the next
   200. GitHub's docs say the same for authorized requests. Every endpoint
   below returns a weak `ETag`.
3. **Payload sizes.** `GET /repos/{o}/{r}/actions/runs?per_page=20` is
   237 KB (each run embeds two repository objects); `per_page=10` is about
   half. `GET /repos/{o}/{r}/actions/runs/{id}/jobs` is 1.7 KB for a
   one-job run. The list is the expensive call, so it is the one the ETag
   must protect.
4. **`gh run list --json` and `gh run view --json jobs` read these same
   endpoints.** fleetz calls them directly with reqwest and the token it
   already gets from `gh auth token` (`github::get_gh_token`), as the
   GraphQL fetch does, rather than spawning `gh` on every poll.
5. **No workflow-runs connection in GraphQL** lists a repo's runs across
   branches by recency; `checkSuites` hangs off a commit. REST is the only
   fit, as the issue assumed.

## Design

### A. Layout: a full-width band above the strip (Option A, accepted)

Window layout today: `body` (pane 1 beside window 2, which splits into
pane 2 and the inspector 3) over the contributions strip, which
`contributions::split_main_area` docks off the bottom of the whole area.

**Option A (recommended):** the CI pane is a second docked band, between
`body` and the strip. `split_main_area` becomes a bottom-dock stack that
takes both panes' fold states and returns `(body, ci, strip)`; `panel_rects`
and `render` both call it, so the first domain invariant holds with no
second copy of the arithmetic. Height `CI_HEIGHT = 10` rows (frame plus
eight lines). On a short terminal the CI pane is the first band to give way
(it drops to its folded row before the strip does, under the existing
`MAIN_MIN_HEIGHT` rule). Compact view docks it the same way.

**Option B:** the pane lives in window 2's column under the inspector,
sharing `split_inspector`. It keeps pane 1 tall but squeezes the commit
views, which he said he wants to keep watching beside CI. Not recommended.

Folded, it is one row with its title on a rule, exactly like the inspector
and the strip (`render_folded`). The title is `[4] ci` plus the freshness
mark from section D, for example `[4] ci  as of 12:02` or
`[4] ci  ⚠ 12:05`. **Decided 2026-10-21** ("as per config, but first time
users have all panes open"): a config key `ci_pane = true` (default) starts
it unfolded, `false` folded, beside `contributions` in the README block; a
missing config opens every pane.

### B. The renumbering (lands first, alone)

- `CONTRIBUTIONS_NUM` becomes 5 and `'5'` takes the strip in FZ-30a, with
  `'4'` left unbound; `CI_NUM = 4` and `FocusedPanel::Ci` arrive with the
  pane in FZ-30c (an unused variant would trip `dead_code` under
  `-D warnings`). Then the `'4'` key is the CI pane's;
  `press_panel_key`, `fold_flag`, `toggle_fold`, `fold_tag_at`, the focus
  cycle (list, detail, inspector, ci, strip; Compact toggles list, ci,
  strip) and `scroll_next_panel` (the CI pane is never a `J`/`K` target,
  like the strip) all learn the new pane.
- README's key table (`1`/`2`/`3`/`4` row, the mouse row, the `4` row, the
  `c` comment in the config block) and `help.rs` ("[3]/[4] folds") change
  with it.
- **Existing tests.** Thirteen existing tests pin the strip as pane 4 or
  the `4` key: in `ui/mod.rs` `the_fold_tag_is_where_the_title_draws_it`,
  `a_folded_row_is_a_rule_carrying_its_tag_in_focus_colors`; in
  `ui/contributions.rs` `the_strip_draws_a_framed_pane_with_one_row_per_weekday`,
  `the_github_source_draws_a_frame_but_no_grid_before_the_first_poll`,
  `the_4_key_unfolds_a_never_opened_strip_onto_today`,
  `a_travelled_strip_keeps_its_day_through_a_fold`; in `app.rs`
  `tab_walks_all_four_panes`, `shift_tab_walks_all_four_panes_in_reverse`,
  `tab_in_compact_toggles_between_the_list_and_the_pane`,
  `shift_tab_in_compact_toggles_between_the_list_and_the_pane`, the
  `FOLDABLE` block's number-key tests, `shift_scroll_skips_a_folded_inspector`
  and `saving_a_source_setting_unfolds_the_strip`. Each needs `4` to read
  `5` or `[4]` to read `[5]`, and the Tab walks gain one pane. **This is
  the existing-test edit the issue foresaw; it is his decision (see "Open
  with Andreas").** The renumbering ticket ships the strip-as-5 change with
  no CI pane content yet, so the layout and key changes are tested alone.

### C. Data: REST with conditional requests, in memory only

- **Scope.** Only the selected repo (the pane 1 cursor), only while the
  pane is unfolded, only for a row with a GitHub identity (owner and name
  from its carrier; a local-only row shows `no GitHub remote`).
- **Calls.** `GET /repos/{owner}/{repo}/actions/runs?per_page=10` for the
  list; `GET /repos/{owner}/{repo}/actions/runs/{id}/jobs` for the
  expanded run only; `GET /repos/{owner}/{repo}/actions/jobs/{job_id}/logs`
  (a redirect to a download) only when a failed run is expanded, once per
  job id, kept for the session. Each URL remembers its `ETag` and sends
  `If-None-Match`; a 304 costs nothing and changes nothing.
- **Parsing.** A `ci` module (`src/ci.rs`) owns `WorkflowRun { id, attempt,
  workflow, title, branch, event, status, conclusion, created, started,
  updated, url }`, `Job { id, name, status, conclusion, started, completed,
  steps }` and `Step { number, name, status, conclusion, started,
  completed }`, parsed from the JSON with serde as `github.rs` parses the
  GraphQL answer. The failing log is the job log cut to the failing step's
  `started..completed` window, last 12 lines, timestamps stripped.
- **Cadence.** A poll runs on selection dwell (300 ms after the cursor
  settles on a new repo, so a `j` run through the list sends nothing),
  then every 10 s while any shown run is `in_progress` or `queued`, every
  60 s when every shown run is finished, never while folded or while the
  row has no identity. Worst case is 2 calls per 10 s, 720 of the 5000
  `core` requests per hour, and almost all of them 304s. The rate-limit
  headers are read into a second `ApiBudget` for the `core` resource and
  the same `Pace` ladder stretches the cadence when the allowance is low.
- **Failures.** A `GithubBackoff` of its own doubles the interval after a
  failure, as the fleet poll does; a rate limit holds until `reset`. A
  failed CI poll never touches the fleet poll, the rows or the cache file.
- **Memory, not disk.** Runs and jobs are held per repo for the session in
  `App` (`ci: HashMap<RemoteKey, CiState>`). Nothing is written to the
  keyed cache: a new file format is a contract change and the data is
  seconds old by design. The pane on a repo not yet polled shows the
  frame with `loading` on the spinner.

### D. Freshness: the same three facts, per repo

Each `CiState` carries a `GithubFreshness` (data time, last attempt,
outcome), the type FZ-03 introduced, so the transitions are the ones
`docs/freshness-contract.md` already tests: a 304 is a successful attempt
(the data is confirmed current, data time advances), a failure leaves the
data and marks the title `⚠ <time>` with the contract's wording in the
pane's status line (`offline`, `rate limited until 12:40`, `not signed
in`), and the first success clears it. The fleet header's marker is not
involved: a CI fetch failure is the pane's business only.

### E. What the pane draws

Eight lines of content, fleetz's own style, no `gh` output.

```
 [4] ci  as of 12:02 ────────────────────────────────────────────────────
  ● ci       main   push   fix: keep the carrier mark       1m 12s  3m ago
  ▾ ● docs   main   push   docs(handoff): take the baton    8s      5m ago
      ● check                                               8s
        ✓ Set up job  ✓ Run actions/checkout@v4  ● Run scripts/check-docs.sh
  ✗ ci       feat/88 push   feat: pane 1 icons               2m 03s  1d ago
  ✓ release  v0.6.0  tag    chore(release): bump version     3m 40s  1d ago
```

- **Run row:** lamp (the CI colours from `badges::ci_display`, pulsing
  while in progress under the existing `dim_pending` convention),
  workflow, branch (or the tag), event, title, duration (ticking while it
  runs), age (`ago`, always, as he asked for the header). Columns size as
  the list's do; the title takes the slack.
- **Expanded run:** `Enter` on a run toggles its jobs under it, each job a
  row with its lamp and duration, and its steps on one wrapped line with
  `✓`, `✗`, `●` (running) and `·` (queued or skipped). Steps are the live
  part: the 10 s poll of `/jobs` moves the lamps.
- **Failed run expanded:** the failing step's line is followed by the last
  12 lines of its log, dimmed, with a `l` hint for the full log.
- **Cursor and scroll:** `j`/`k` move among run rows (an expanded run's
  jobs are not cursor stops in the first version); the band scrolls to
  keep the cursor visible. Mouse: click a run row to select it, click the
  expanded marker to toggle, wheel scrolls.
- **Empty states, in the status line of the pane:** `no GitHub remote`,
  `no workflow runs`, `not signed in` (the header's auth wording), and
  `run gone` on a row whose `/jobs` answered 404 (removed at the next list
  poll). A re-run arrives as a higher `attempt` on the same run id and
  replaces the row.

### F. Handoffs (keys are his choice; proposal)

All through `suspend_and_run`, like Enter for lazygit, so the terminal is
released, the command runs on the user's screen and the TUI is reclaimed:

- `l`: `gh run view <id> --log` (the full log; `--log-failed` on a failed
  run).
- `a`: `gh run rerun <id>` (`--failed` when the run failed; "again").
- `x`: `gh run cancel <id>` on a run in progress.
- `O`: opens the run's `url` with the existing opener, as `O` opens a PR.

`gh` prompts or fails on its own screen; fleetz polls again on return.

## Tickets (not created; ids from `scripts/relay-board next`)

1. **FZ-30a, the renumbering.** Strip to pane 5, `CI_NUM = 4` reserved
   and a `FocusedPanel::Ci` with no pane drawn yet, keys, README, help,
   the listed existing tests edited as "Open with Andreas" decides.
   `fz-worker` plus reviewer; a look change he sees at once, so it stops
   at Ready to integrate for his look at the branch build.
2. **FZ-30b, the data layer.** `src/ci.rs`: REST client over the existing
   reqwest client and token, conditional requests, parser, per-repo state,
   freshness, cadence and backoff, `AppEvent::Ci`. No drawing. Tests drive
   the parser on recorded payloads and the cadence as pure functions. The
   measurement in "Measured facts" is repeated on the ticket.
3. **FZ-30c, the band.** `split_main_area` as the bottom-dock stack,
   `panel_rects` parity test, fold and focus, the run list with lamps,
   durations and ages, the empty states. Stops for his look.
4. **FZ-30d, jobs, steps and the failing log.** Expansion, the live step
   line, the log tail. Stops for his look.
5. **FZ-30e, handoffs.** The four keys above. Stops for his look, since
   keys are an interaction choice.

Order: a, then b and c in the two worker slots (they share no file: b is
`ci.rs` and the event plumbing in `main.rs` by region, c is `ui/` and
`app.rs`), then d, then e. Each ticket's acceptance criteria are the
bullets of its section here.

## Rejected

- **Embedding `gh run watch`** in a pseudo-terminal: his decision on the
  issue; needs a terminal emulator dependency and cannot be restyled.
- **`gh` child processes per poll** (`gh run list --json`): a process
  spawn every 10 s, no conditional requests (the `gh` CLI does not send
  `If-None-Match`), and a second code path for auth and errors beside the
  reqwest one the app already has.
- **A disk cache for runs**: a new cache format for data that is stale in
  seconds; the in-memory map plus the first poll on unfold is enough.
- **Polling the pane while folded** so it is warm on unfold: the issue
  excludes it, and a free 304 is still a request against the allowance.
- **A view inside window 2** instead of a pane: his decision on the issue.

## Open with Andreas

1. **Layout:** **Option A, decided 2026-10-21 13:05** ("think option a
   for workflow pane").
2. **The thirteen existing tests** in section B: **"ok with me"
   (2026-10-21)**; `4` to `5`, `[4]` to `[5]` in FZ-30a, the Tab walks
   gaining a pane in FZ-30c, no assertion weakened.
3. **Keys:** `Enter` expand, `l` log, `a` rerun, `x` cancel, `O` open, and
   `5` for the strip: **"fine as proposed", 2026-10-21 13:05**, after
   the keys were explained in chat (all apply with the CI pane focused,
   to the run under its cursor).
4. **Start folded:** decided, see section A.
5. Creating the five issues FZ-30a to e as P2 in M4, and the word to run
   FZ-30a: **"sure" (2026-10-21)**; created and FZ-30a dispatched.

## Amendment 2026-10-21: a pane 2 page, not a pane

Andreas, after trying #91 ("this looks great!"), 15:07 to 15:12: "can we
use the inspector for inspecting the CI's ? so say im highlighting a
branch in pane 2, details for that is viewed in inspector. and if i
switch focus to pane 4/ci, and highlight a certain CI there, the
inspector can detail that instead?"; "why are we not having CI as it's
own page in pane 2 instead of its own pane?"; "id even want to be able
to scroll thru the ci script/code somehow"; and the decision: "i think
we can have a small addition for CI in the 'INFO' page, that's there to
shed light on the general status, but then i think CI should be a pane 2
page, and not it's own pane. use inspector to inspect CI details, and or
it's script?" This replaces his 2026-10-03 decision on #67 for an own
pane: that pane was built (#91), used, and found to cost ten rows and a
fourth focus for what pane 2 and the inspector already do.

### What changes

1. **`ci` is the sixth view of pane 2** (`info`, `commits`, `reflog`,
   `graph`, `branches`, `ci`), cycled with `[` and `]` as the others,
   titled `[2] ci`. Its rows are the band's run rows unchanged (lamp,
   workflow, branch, event, title, live duration, age), `j`/`k` move the
   cursor, the view scrolls, the empty states and the freshness mark
   move from the band's title and status line into the view's first
   line (`as of 12:02`, `⚠ 12:05 offline`, `loading`, `no GitHub
   remote`, `no workflow runs`, `not signed in`, `GitHub off under -d`).
2. **The band goes.** `FocusedPanel::Ci`, `CI_NUM`, `ci::split_docks`
   and the `ci_pane` config key are removed (unreleased, so no contract
   is broken); the strip returns to pane 4 and the `4` key, with the
   README, help, footer segment and the tests of #89 and #91 following
   it back. `contributions::dock_bottom` stays as the one carve the
   strip uses. The data layer (#90) and `ui::ci`'s row rendering stay
   and are reused by the view.
3. **Polling is lazy: only the `ci` view polls.** His 15:20: "having CI
   on its own pane 2 page also lets us lazyily load that when actually
   reaching for CI info". The data layer polls the selected repo only
   while pane 2 shows `ci`, with the dwell, the 10 s / 60 s cadence and
   the budget as before; never otherwise, and never for the `info`
   section. The `ci_folded` gate becomes "pane 2 is not showing `ci`".
   Runs held from an earlier visit stay for the session, so coming back
   paints at once and a 304 confirms them for free.
4. **A small CI section in `info`, from what is already in hand.**
   Between `branches` and `recent commits`: a `ci` section label and one
   line for the default branch from the fleet poll's check state (the
   same fact the pane 1 lamp draws, free: `● main passing`, `✗ main
   failing`, `● main running`, `no checks`), then up to three run rows
   (the latest run of each of the three most recent branches) when runs
   are held from a visit to `ci`, else a dim `] for runs` hint. Nothing
   polls for it; nothing is selectable; the full list is one `]` away.
5. **The inspector details the highlighted run** while pane 2 shows
   `ci`, exactly as it details a commit or a branch in the other views,
   with three bodies cycled by `[` and `]` while the inspector is
   focused (his to change at the look): **jobs** (each job with its
   lamp and duration, its steps on one wrapped line with `✓`, `✗`, `●`,
   `·`, and under a failed job the failing step's name and the last 12
   lines of its log); **log** (the selected job's full log, scrollable,
   fetched once per job id, ANSI and timestamps stripped); **script**
   (the workflow file that produced the run, from the run's `path` at
   its `head_sha`: `git show <sha>:<path>` when the commit is in the
   clone, else `GET /repos/{o}/{r}/contents/{path}?ref=<sha>`, fetched
   once per sha and path; the failing step's `run:` or `uses:` block
   highlighted when the run failed). `J`/`K` from pane 2 scroll the
   inspector as today; the inspector title names the body:
   `[3] inspector  jobs` (`log`, `script`).
6. **Handoffs** (section F) act on the highlighted run while pane 2
   shows `ci`, keys as proposed.

### Tickets, amended

- **FZ-30d (#92):** the `ci` view in pane 2, the band's removal and the
  strip's return to 4, the polling gate, the `info` section. Look change
  for his look.
- **FZ-30f (new):** the inspector's three bodies. Look change for his
  look.
- **FZ-30e (#93):** the handoffs, unchanged in substance, "with pane 2
  showing `ci`" instead of "with the CI pane focused".

### Rejected in the amendment

- Keeping the band beside the pane 2 view "to decide from use": two
  places for the same list; he decided from use already.
- Jobs expanded inline under the run row: the band is gone and pane 2
  has the inspector for exactly this.

## Amendment 2026-10-22: his look at FZ-30e and FZ-30f

From his look at the branch builds (11:30 to 11:45, screenshots on
#95), "yes to all" to the coordinator's proposal at 11:45.

### What changes

- **Jobs are a list, not a wrapped line** (FZ-30f, replaces the wrapped
  steps line of section E): the inspector's `jobs` body is one row per
  job (lamp, name, duration), `j`/`k` move a job cursor, and only the
  selected job shows its steps (one per line, `✓ ✗ ● ·`) and, when it
  failed, the failing step's log tail. The wrapped line "doesn't read
  linearly, just looks like text" and gave no way to say which job you
  look at. Option (b), the job list in pane 2 under the run list, was
  not taken: pane 2 stays one list, the inspector is the detail of the
  highlighted thing.
- **The log tail is lazy** (FZ-30f): the jobs body paints at once from
  `/jobs`; a job's log is fetched after the dwell, only for the selected
  job (first the failed one), cached per job id. Highlighting a run must
  never show `loading log` for the jobs body.
- **The inspector's CI bodies scroll** (FZ-30f bug): `j`/`k` and the
  wheel scroll the shown body as the other inspector bodies do.
- **`]` from a focused inspector in `branches`** enters `ci` and then
  cycles pane 2's views; the bodies cycle only once the inspector holds a
  run (FZ-30f bug, his item 8).
- **No suspend** (FZ-30e): `l` no longer suspends into `gh run view
  --log`; it focuses the inspector on the highlighted run's `log` body
  (FZ-30f's). `a`, `x` and `O` stay as built: background `gh`, no
  suspend. "it'd be better to always stay in this tool."
- **Pane 2 `ci` view width** (new, FZ-40): a headline row above the
  runs naming the columns, as the `branches` view has; columns lamp,
  workflow, branch, event, title, duration, age; the title takes the
  slack; at narrow widths drop event first, then title, then truncate
  the workflow name, branch last; duration and age always. His screenshot
  at 170 columns showed the workflow cut at 18 characters beside 60
  empty columns.
- **The run's own facts** (name, branch, event, commit, when, how long)
  stay the inspector's header; not repeated in pane 2.

### Tickets, amended

- **FZ-30f (#95):** fix cycle 3 under the amendment; still a look change
  for his look.
- **FZ-30e (#93):** `l` retargeted to the inspector's log body; after
  #95 merges, since it uses its body state.
- **FZ-40 (new):** the pane 2 `ci` view's headline and width budget.
  After #95 merges (same file region in `src/ui/ci.rs`).

## Amendment 2026-10-22, second: what a run is inspected for

After his look at the merged FZ-30f (12:30, "good!" with "it feels
incomplete. have a good thinker") and "i trudst you, do research and
think a little, what is useful for the CI workflows to inspect?"
(13:05). The coordinator's reading, presented in chat at 13:20;
tickets FZ-41 to FZ-44 wait for his word to create.

### The questions a run is asked, in order

1. Is something red that I care about: pane 1's lamp, pane 2's list.
2. Which job, which step, what did it say: the jobs list, the failing
   step, the tail (FZ-30f). The three bodies hide each other, which the
   split below fixes.
3. Is this new or has it failed for a week: a strip of the last eight
   runs of the same workflow on the same branch (`✓✓✓✗✗`), from the run
   list already in hand, no request.
4. Is it stuck or just slow: elapsed against the median of that
   workflow's completed runs ("2m 05s, usually 1m 20s") and a queued
   time while no runner has it; same data.
5. Where in my code: the job's check-run annotations (file, line,
   message; GitHub caps them at 10 errors and 10 warnings per step),
   one request per failed job, cached per job id, budget-gated like the
   log. Drawn under the failing step above the tail.
6. Rerun, cancel, open: FZ-30e.

Not built: artifacts, matrix management, dispatching workflows. "stay
in this tool" is for looking, not operating CI. Research: the terminal
tools for Actions (lazyactions, GAMA) stop at `gh run view --log`;
GitHub's own advice for a failure is to grep the log for `##[error]`.

### The split inspector (FZ-41)

```
[3] inspector  ci  fix: keep the carrier mark  main  push  ✓✓✓✗✗  2m 05s (usually 1m 20s)
 ● check     1m 02s │ steps                                       [[ ]] log script
 ✓ docs          8s │  ✓ Set up job
 ✗ release   2m 03s │  ✗ Run cargo clippy -- -D warnings
                    │     src/app.rs:3042  no method named `foo` found
                    │     error[E0599]: no method named `foo` ...
                    │     ...tail...
```

- Header: the run's facts, the history strip (FZ-40 draws the same
  strip in pane 2's row) and the duration against the usual (FZ-44).
- Left: the jobs list, one row per job (lamp, name, duration), fixed
  width from the longest name, clamped to about a quarter of the
  pane. The cursor lives here: `j`/`k` in the inspector, `J`/`K` from
  pane 2 (today `scroll_next_panel` routes to `scroll_inspector`, which
  the following jobs body ignores; it routes to `inspector_nav`). A
  click on a job row selects it.
- Right: the selected job's detail, three bodies cycled by `[`/`]`:
  `steps` (with the tail, and FZ-43's annotations), `log` (that job's
  full log), `script` (the workflow file, failing step highlighted).
  `J`/`K` scroll the right side while the inspector is focused; PgUp,
  PgDn and the wheel scroll the column under the mouse. `l` (FZ-30e)
  jumps the right side to `log`.
- Under about 100 columns the list collapses to one column: the
  selected job expanded in `steps`, as FZ-30f draws it today. No new
  config key.
- Geometry through one function the renderer and `panel_rects` share
  (the house invariant), so the job rows are click targets.

### Tickets (ids from `scripts/relay-board next`; not created)

- **FZ-41** the split inspector and its nav: the `J`/`K` routing, the
  follow, click to select. Geometry and keys only, no new data. Look
  change.
- **FZ-42** log navigation: `##[error]` and `##[warning]` lines in the
  failing colour, `n`/`N` jump between them, `##[group]` headers folded
  by default and opened with `Enter`. Small.
- **FZ-43** annotations under the failing step: `GET
  /repos/{o}/{r}/check-runs/{job_id}/annotations`, once per failed job,
  cached per job id, gated on the `core` budget as the log is; `path:line
  message` rows, dimmed when the run passed.
- **FZ-44** "usually 1m 20s" and the queued time in the header: the
  median of the same workflow's completed runs in the list; nothing
  fetched.

Order: FZ-30e rework, FZ-41 beside FZ-40, then FZ-42, FZ-43, FZ-44.
