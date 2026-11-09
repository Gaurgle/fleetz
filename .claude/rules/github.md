---
paths:
  - "src/github.rs"
  - "src/github/**"
  - "src/cache.rs"
  - "src/cache/**"
  - "src/contributions.rs"
  - "src/app.rs"
  - "src/app/**"
  - "src/main.rs"
  - "src/tests.rs"
  - "src/digest_handler_tests.rs"
  - "src/engine/**"
---

# GitHub fetch, budget and cache invariants

Binding wherever the session runs. Loaded when a matching file is read or
edited; a loader without path scoping reads this file before changing the
modules it names. Moved from the root `CLAUDE.md` on 2026-10-30, text
unchanged.

**Batch on GraphQL, target on git.** Opposite cost shapes, and the code leans
on both. One GraphQL call returns every repo's state, so never split it into
per-repo queries: the rate limit has a floor of one point per call, so fifty
small queries cost more than one big one and tell you less. `git fetch` is
priced per remote, so use the poll's `pushed_at` diff to find which remotes
moved (`remotes_that_moved`) and fetch only those.

**GraphQL bills the `first:` you ask for, not what comes back**, and nested
connections multiply. The org fallback asked 25 orgs x 50 repos to fetch 17
from 4: 83% of the query's nodes, pricing the poll at 26 points, or 3120/hour
at the 30s interval against a 5000/hour budget. One fleetz idled at 62% of the
account and four exhausted it; sized to what it retrieves (`MAX_ORGS`,
`ORG_REPO_LIMIT`) the identical result costs 3. Measure with `rateLimit { cost
nodeCount }` before and after: two comments in `github.rs` disagreed about this
for months and the optimistic one was wrong. `totalCount` and `pageInfo` on a
connection the query already pages are free (measured for FZ-09b: cost 3 and
1030 nodes with and without them), which is how the coverage note knows
"50 of 95" at no cost.

The first poll has no previous dataset to diff, and fetching every repo past
the cutoff cost ~0.8s each. It uses a disk baseline: `FETCH_HEAD`'s mtime is
when that clone last fetched, so a remote pushed before then is already in hand
(`stale_remotes`). Unknowns must fetch -- no `pushed_at` means no GitHub match,
no `FETCH_HEAD` means never fetched -- or the fleet looks more current than it
is.

**Launch paints from the cache, however stale.** Blocking on the GraphQL
dataset cost 6-9s before the first frame, so startup passes
`CacheMode::AnyCached` and the first tick's poll replaces it. That only works
if the poll is due: a cache-served load is not an attempt and must leave
`GithubFreshness::last_attempt` unset (`DatasetSource::Cache`), or launch
shows the last session's numbers for a whole interval. The file is keyed by
format version, host, account and `repo_limit` (`cache::KeyedCache`): launch
reads this credential's saved copy after one `gh` lookup bounded at 2 s
(`github::LAUNCH_LOOKUP_BOUND`), and a credential with no saved copy paints
nothing and waits for GitHub, never another account's data. The unkeyed
legacy file is read only while no account has a keyed file. Signed out, or
after a lookup that timed out, launch shows the newest saved copy. Never
re-add `-p` to the key (`cache.rs`), which decides whether PR data is
*drawn*, not whether it is fetched.

**Never widen the GitHub fetch without checking the cache.** Results are cached
on disk (`cache.rs`); `r` is a local sweep plus a poll, `R` a full reload that
bypasses the cache, and under a known rate limit both send nothing (`r` sweeps,
`R` rebuilds from the dataset in memory; FZ-10b, `docs/api-budget-design.md`
4.3). Polls pace themselves on the budget headers and back off on failures
(`ApiBudget`, `GithubBackoff` in `app.rs`); never add an unconditional retry.
A field with no `first:` is free, which is how the contribution calendars ride
along at no cost; one with a `first:` is not, and a request never is.

**GitHub caches `contributionsCollection` against the `from` date alone**, so
the `recent` window rotates its `from` and must keep doing so: a fixed one
re-reads its own stale answer all day, and the strip drew 3 against
github.com's 11. The merged range is fixed at seven days, not derived from the
window, because a cached payload was built under a rotation the parser cannot
recover. Measurements, and the undocumented behaviour this leans on, are at
`contributions.rs::ROTATION_HOLD_SECS`.

**Only a response carrying `data` may be cached** (`accept_response`). GraphQL
errors arrive as HTTP 200, so a rate limit is a plausible 95-byte body. Caching
it before checking `errors` overwrote the good payload and took the
`read_stale` fallback with it, which the offline and non-2xx paths both need:
the dashboard went blank until a poll succeeded.
