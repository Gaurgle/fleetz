# First-wave ticket drafts

Published as GitHub issues #14 through #19. Stable FZ IDs remain planning
references. See [release-plan.md](release-plan.md) for links to the live board. Symbol names are navigation hints;
verify the implementation at the assigned base commit before editing.

## FZ-01: Establish reproducible performance measurements

Type: measurement. Milestone: M0. Role: measurement worker, then reviewer.
Dependencies: none. Parallel-safe with FZ-02 if benchmarks run without competing
builds. Output: `docs/performance-baseline.md`; optional harness only after
coordinator agreement on its scope.

### Problem and outcome

An immediate first frame does not establish time to usable data. We need a
repeatable baseline before choosing concurrency, caching or rendering changes.

### Scope

Inspect `load_repos`, `assemble_view`, `build_repo_status`, `spawn_sweep`,
`run_event_loop`, `ui::render`, and intro composition. Use existing tooling.
Measure a release build; do not use debug timings to promise release behavior.

### Acceptance criteria

- Record commit, build profile, OS, terminal, dimensions, repo counts, fixture
  characteristics, intro setting and cache conditions.
- Distinguish first paint, first useful row and completed initial summary.
- Record warm startup, cold-cache startup and offline behavior separately.
- Measure representative small and larger fleets, idle behavior, local refresh,
  input-to-paint latency and rendering/output volume where instrumentation allows.
- Use repeat runs and report spread and sample size. Mark unavailable measures
  explicitly rather than estimating them as measured facts.
- Include repeatable commands, measurement limitations and ranked bottlenecks.
- Prefer synthetic repositories. Do not clear the user's cache, change their
  Git configuration, install tools, or repeatedly poll GitHub to generate load.

### Exclusions and validation

No optimization, new dependency, production telemetry or CI change. A reviewer
repeats at least one case. Recommend performance targets only after measuring;
do not invent a passing threshold from the result.

## FZ-02: Preserve newer GitHub state during local refresh

Type: implementation pilot. Milestone: M0/M1. Role: Rust implementation worker
and independent reviewer. Dependencies: none.

### Problem and outcome

A local sweep captures a repo while CI is pending. A GitHub response changes
CI to passed. The local sweep then replaces the full repo with its old snapshot,
restoring pending. Local completion must preserve current GitHub-owned fields.

### Scope and ownership

Start with `AppEvent::RepoUpdated`, its event handler, `SweepTarget`,
`rebuild_repo_status` and `spawn_sweep` in `src/main.rs`. Inspect `RepoStatus`
in `src/model.rs`. Prefer the smallest explicit field-ownership fix. Request a
coordinator decision before widening into a shared scheduler or major refactor.

### Acceptance criteria

- A deterministic regression test reproduces local-start, GitHub-update,
  local-finish and fails against the old behavior.
- The latest PR count/details, CI status/workflow/duration and other
  GitHub-owned fields survive local completion. Document the ownership split.
- Local branch, dirty, ahead/behind, history, stash and worktree results still
  update as appropriate; selection/group state is not accidentally reset.
- Test both completion orders and a failed GitHub poll using controlled events,
  without live network calls or sleep-based races.
- The single-repo and full-sweep paths obey the same merge behavior.
- A late result for a removed repo does not create an unrelated row.
- Existing tests remain intact. Add focused regressions, then pass fmt, clippy
  with warnings denied, and the full test suite.

### Exclusions and handoff

No polling cadence, UI, cache format, identity migration or general task
cancellation changes. Report any separate local-versus-local ordering issue as
a follow-up. Deliver the exact base, diff, tests and ownership explanation.

## FZ-03: Keep cached GitHub fallback visibly stale

Type: implementation. Milestone: M1. Dependencies: integrated FZ-02, then
coordinator agreement on freshness state. Do not run beside FZ-04.

### Problem and outcome

`fetch_repos` can return cached data after a transport or HTTP failure, while
the background event handler discards `from_cache`. The UI must distinguish a
poll attempt, a successful network response and usable cached data.

### Scope

Inspect `GithubFetch`, `fetch_repos`, `parse_cached` in `src/github.rs`,
`ApiCache` in `src/cache.rs`, GitHub events and startup handling in `src/main.rs`,
and freshness/error display state in `src/app.rs` and `src/ui/mod.rs`.
Coordinator assigns final file ownership before Ready.

### Acceptance criteria

- A cached fallback updates the attempt timestamp but never claims a new
  successful network refresh.
- Retain last-known data and identify cached/stale/offline state in the UI.
  Unknown cache age must remain unknown, not become the launch time.
- A real successful response clears the stale/error state and advances network
  freshness. Preserve the existing behavior of protecting good cache data from
  GraphQL error bodies.
- Test startup with cached data, background fallback, no usable cache, GraphQL
  error, and recovery with controlled responses or extracted pure transitions.
- Freshness handling does not trigger an immediate retry loop or blank good data.
- Document visible behavior and pass the standard checks.

### Exclusions

No full budget controller, adaptive polling redesign or account/cache-key
migration. Broader retry and cancellation design belongs to follow-on work.

## FZ-04: Report fetch outcomes accurately

Type: implementation. Milestone: M1. Dependencies: integrated FZ-03 and an
accepted per-repo fetch state contract. Serial with refresh-state work.

### Problem and outcome

Sweeps ignore the boolean from `git::fetch`, and the event's `fetched` value
means requested rather than successful. A failure must settle the operation
without implying that remote refs were refreshed.

### Scope

Inspect `git::fetch` and `run_git`, `spawn_sweep`, `RepoUpdated`,
`settle_refresh_fetch`, refresh expiry and associated status display.
Likely files: `src/git.rs`, `src/main.rs`, `src/app.rs`, relevant UI status code.

### Acceptance criteria

- Distinguish not requested, success, failure and timeout where the transport
  provides that distinction. Never report unknown as successful.
- Failed fetches stop their spinner and retain usable local status and refs.
- Only success advances last-successful-fetch information. Partial failure in
  a fleet sweep identifies affected repos without calling every fetch successful.
- Provide a concise actionable error without logging credentials or raw tokens.
- Test single fetch, mixed-outcome sweep, timeout and recovery. Use local test
  remotes or controlled process results; no GitHub credentials are required.
- Manual retry remains available; no automatic retry storm is introduced.
- Pass standard checks and document user-visible failure behavior.

### Exclusions

No new authentication mechanism, automatic repair, fetch cadence change or
global scheduler. Broader subprocess error-model changes need coordinator review.

## FZ-05: Define repository identity and collision behavior

Type: design and test-scenario inventory. Milestone: M1.
Dependencies: none for investigation; implementation requires accepted design.
Output: `docs/repository-identity-design.md`. Production code is read-only.

### Problem and outcome

Some paths use owner-aware remote matching while others deduplicate or update
by bare name. Distinct repos such as `alice/api` and `company/api` must remain
distinct without breaking local-only repositories or multiple checkouts.

### Scope

Trace `merge_sources`, discovery's `Wanted`, `assemble_view`,
`unclaimed_local`, `remotes_that_moved`, GitHub updates, groups and selection.
Read existing discovery invariants and tests before proposing changes.

### Acceptance criteria

- Map identity use from API through discovery, model, UI and persisted state.
- Define remote repository identity separately from local checkout identity.
- Address owner/name collisions, multiple clones, linked worktrees, local repos
  with no remote, renamed repositories, SSH/HTTPS URLs and non-GitHub remotes.
- Explain host scoping and case normalization without pretending Enterprise
  support already exists.
- State when name fallback is safe, when it is ambiguous, and what the user sees.
- Include concrete fixture scenarios, compatibility implications and the
  smallest ordered implementation tickets with file ownership.
- Reviewer traces both a collision and a local-only path through the proposal.

### Exclusions

No production migration, cache schema change or provider expansion. Identify
changes needing explicit approval before promoting implementation tickets.

## FZ-06: Specify correct cache invalidation

Type: investigation and test-scenario inventory. Milestone: M1/M2.
Dependencies: none for investigation; coordinate identity assumptions with FZ-05.
Output: `docs/cache-invalidation-design.md`. Production code is read-only.

### Problem and outcome

One `WorkKey` gates several data products with different inputs. Establish when
each can safely be reused and reproduce suspected gaps before choosing a fix.

### Scope

Inspect `WorkKey`, `refs_fingerprint`, `work_key`, `rebuild_repo_status` and
local contributions. Use temporary repositories for mutation experiments.

### Acceptance criteria

- Map commits, reflog, graph variants, stashes, worktrees and contribution data
  to their actual inputs and current invalidation behavior.
- Exercise another branch moving while HEAD stays fixed; remote ref changes;
  flat and nested tags added, moved or deleted; worktrees added/removed;
  stash changes; author/window changes; and a calendar date rollover.
- Consider packed refs, linked-worktree common directories, missing metadata
  and coarse timestamp resolution. Clearly separate confirmed gaps from risks.
- Never infer a clean working tree from unchanged HEAD/index metadata alone;
  unstaged and untracked changes still need a valid observation mechanism.
- Compare explicit generation tracking, Git-derived signatures and filesystem
  metadata by correctness and cost. Avoid assuming extra subprocesses are free.
- Produce bounded fix tickets with regression scenarios and dependencies.
- Reviewer repeats one confirmed gap using the documented fixture steps.

### Exclusions

No watcher dependency, persistent local-history cache, broad cache rewrite or
changes to the user's real repositories. Implementation follows accepted design.
