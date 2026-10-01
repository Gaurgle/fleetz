# GitHub API budget and retry design (FZ-10)

Status: design for [FZ-10 / #23](https://github.com/Gaurgle/fleetz/issues/23),
**revision 3**, answering the first review (findings F1 to F7 and its smaller
list) and the re-review. **Architectural decisions A1 to A7 taken by the
coordinator (`claude-fable-5-1`) on 2026-10-01**, including its rulings of the
same day on A3, A5 and A6, which revision 2 had raised
([Decisions](#decisions)). Astra has not reviewed. The owner's items are
open and are presented to Andreas together ([Owner: open](#owner-open)).

Written against `main` at `f8a54c0`
(`f8a54c0bfabaf1161f042d08893121f1173f4469`), with FZ-03 and FZ-04
integrated; production code was read, not changed. The first draft was at
`6794962`. `src/github.rs` did not change between the two; line numbers in
`src/main.rs` and `src/ui/mod.rs` below are at `f8a54c0`. They drift; follow
the symbols.

Every claim carries one label:

- **Measured**: from a request listed in [Measured cost](#2-measured-cost)
  (M1 to M9), or a local timing, with its number.
- **Read**: from the code at the base.
- **Documented**: GitHub's published behaviour
  ([GraphQL rate limits](https://docs.github.com/en/graphql/overview/rate-limits-and-query-limits-for-the-graphql-api),
  [REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)),
  not verified here.
- **Inferred**: reasoning from the above, not observed.

## Summary

The fleet poll's price depends on `repo_limit`: 2, 3 and 4 points at 20, 50
and 100 (Measured, M2, M1, M6). At the owner's 50 it is 3 points and 1030
nodes, exactly what `CLAUDE.md` says. One instance spends 360 points an hour
at the default 30 s interval and 720 at the 15 s `Pending` floor; three
instances were running during the measurement and the account stood at 1436
points 38 minutes into its window (Measured, M1). Budget can be read at no
extra request: the response headers carry `x-ratelimit-remaining`, `-used`,
`-reset` and equal the in-query `rateLimit` numbers (Measured, M1, M2, M9),
while the REST `/rate_limit` endpoint reported a different, nearly unused
GraphQL bucket for the same token and is unusable for this (Measured).

Two findings change the picture. `repo_limit` 100 fails with HTTP 502 after
about 11 s, twice in a row (Measured, M3, M7): evidence of a server deadline
near that time, not proof of a fixed one. The real query at 50 answered in
6.3 s here and about 7.3 s in the reviewer's run, so it sits at roughly 57%
to 66% of that deadline. And `rateLimit(dryRun: true)` prices a query
without running it and was not observed to be charged (Measured, M6), which
gives every future query change a cheap way to obey "never widen the GitHub
fetch without checking".

Today fleetz reads no budget, retries a rate limit every 30 s until the window
resets, reports a 403 or 429 limit as `http error`, creates a client with no
timeout per request, and lets `R` stack full loads (Read). The design: read
the budget from headers, anchored to the local clock on arrival; pace
automatic polls by `remaining` and time to reset with a fixed reserve, and
poll at the configured interval while the budget is unknown; honour the reset
and `retry-after`; back off exponentially on server errors, timeouts, refused
credentials and malformed bodies; reuse one client with a 30 s deadline; no
new config key. Three serial tickets, FZ-10a to c, placed after the current
`main.rs` line.

### Before the owner: what changes in accepted contracts

Each needs Andreas's sign-off, since he accepted the freshness contract. Detail
in [Contract and invariant changes](#contract-and-invariant-changes).

| Item | Today | After FZ-10 |
|---|---|---|
| Rule 6 | The next automatic attempt comes one normal interval after any attempt | **At least** one normal interval later: longer under backoff, a rate limit or a low budget. Still never an immediate retry |
| Rule 9 | While failing, the poll interval is the configured one | While failing, **at least** the configured one: backoff stretches it up to about twelve minutes |
| A6 and O2 (rule 1 kept) | `r` and `R` always ask GitHub | Under a known rate limit, if the owner accepts O2, `r` refreshes local state only and `R` rebuilds from the data in memory, neither asking GitHub. No attempt is recorded, so rule 1 holds unchanged |
| Failure kinds | `Offline`, `Http`, `Api`, `Auth`, `Malformed`, `TimedOut` | Adds `Server` (a 5xx answer) and `Refused` (credentials or permissions refused); a 403 or 429 limit is reported as rate limited |
| Rule 10 and D4 | A client timeout is FZ-11's | The client timeout moves to FZ-10a; cancelling a hung request stays FZ-11's |

### Decisions

Architectural, decided by the coordinator on 2026-10-01 (detail in
[section 7](#7-decisions)). The A3, A5 and A6 refinements were ruled on by
the coordinator on 2026-10-01, after revision 2 raised them.

| ID | Decision |
|---|---|
| A1 | Budget read from response headers. No query change |
| A2 | Pacing plus a fixed reserve of 10% of `limit`. While the budget is unknown, the configured interval applies; nothing is slowed on a guess |
| A3 | Backoff doubles from the interval per consecutive failure, cap 600 s, jitter 0.8 to 1.2 from an injected source. Step 1 is exactly the interval, with no jitter; jitter applies from step 2 and on top of the cap (480 to 720 s there). `Offline` and `Auth` are transparent to the ladder: they keep the count and leave any not-before in place; `Auth` retries at the plain interval |
| A4 | Instances stay independent and pace on the shared `remaining`. Cache-based coordination is not designed here |
| A5 | One reused client, connect timeout 10 s, total 30 s. A timeout after connecting is `TimedOut`; a connect failure, including a connect timeout, is `Offline`. Token lookup stays per request |
| A6 | Under a known rate limit, `R` rebuilds from the dataset already in memory as `DatasetSource::Local`, recording no attempt, and only while no poll is in flight. Whether `r` and `R` skip the request at all is the owner's (O2) |
| A7 | `R` pressed while a load runs is ignored, in FZ-10b |

Open for the owner (detail in [Owner: open](#owner-open)):

| ID | Decision | Recommendation |
|---|---|---|
| O1 | What the header says when polling is slowed, paused or rate limited | Say which, and when the next automatic attempt is; no budget number in the header |
| O2 with A6 | Manual `r` and `R` while GitHub is rate limiting | No request until the limit lifts: `r` refreshes local state, `R` rediscovers from the data in memory, both say when GitHub resumes |
| O3 | Config keys | None |
| O4 | The contract changes in the table above (rules 6, 9 and 10, the new failure kinds) | Accept |
| O5 | FZ-10a edits four existing test builders, only to add the new empty field the compiler requires | Allow |

## 1. The request lifecycle today

All Read unless marked.

### The one request

There is one kind of GitHub API request in the program: the fleet GraphQL
query built by `github.rs::build_query` (431) and sent by `github.rs::request`
(680). Nothing else in `src/` talks to `api.github.com` (Read: `rg reqwest`
finds only `request`). The ignored live test
`github.rs::rotation_premise_still_holds` (1770) sends its own small query by
shelling out to `gh api graphql`; it runs only by hand.

| Step | Where | What happens |
|---|---|---|
| Token | `github.rs::get_gh_token` 923 | `gh auth token` runs as a subprocess **on every request**, never cached. About 0.05 s locally (Measured, three runs, no network). A failure is `FailureKind::Auth`, and no request is sent |
| Query | `build_query` 431 | Rebuilt per request: four year windows plus the rotating `recent` window (`contributions.rs::rotation_tick` 132), `repositories(first: repo_limit)`, and the org fallback `organizations(first: MAX_ORGS 8) { repositories(first: ORG_REPO_LIMIT 12) }` (426, 427) |
| Client | `request` 694 | `reqwest::Client::new()` **per request**: no connection reuse, no connect timeout, no total timeout |
| Transport error | `request` 706 | `FailureKind::Offline` |
| Non-2xx | `request` 714 | `FailureKind::Http`, message is the status only. The body and every header are dropped, so a 401, a 403 or 429 rate limit, a `retry-after` and a 502 are all the same `http error` |
| Body read | `request` 721 | A failed read is `Offline`, whatever the cause |
| Body | `accept_response` 744 | Unparseable: `Malformed`. `errors` present: `Api`, with `rate_limited` set by `is_rate_limited` 384 from GraphQL `type == RATE_LIMITED`. No `data`: `Malformed`. Only a body with `data` is written to the cache (`cache.rs::write` 89, a plain `std::fs::write`, not atomic) |
| Fallback | `settle_attempt` 668 | Any failure gets the cached payload attached as `FetchError::fallback` |

### What triggers it

| Trigger | Path | Bound on frequency | Leaves |
|---|---|---|---|
| Launch, usable cache | `spawn_load` 458 with `bypass_cache: false` -> `fetch_dataset` 2402 `CacheMode::AnyCached` | No request; the cache is served (`fetch_repos` 649) | `GithubFreshness::loaded` (`app.rs` 1142): data time from the file, `last_attempt` unset, so the first tick polls |
| Launch, no usable cache | same, falls through to `request` | Once | Success or `Failed`; error panel if no cache at all |
| Watch tick | `run_event_loop`, 711 to 714, every `TICK_INTERVAL` 100 ms (74): `github_poll_due` 1859 then `start_github_poll` 1956 | `retry_due(last_attempt, github_poll_secs)`; in-flight lock `claim_github_poll` 1968 | `apply_github_poll` 1741 |
| Default interval | `github_poll_secs` 1938 | `config.github_poll_interval` (default 30, `config.rs::default_github_poll_interval` 84), floored at `GITHUB_POLL_MIN` 15 (1668) | |
| `Pending` floor | `github_poll_secs` 1943 to 1951 | 15 s while any row's CI is `Pending` and polls succeed (rule 9, D3). No upper bound on how long: a run stuck `Pending` holds it indefinitely | |
| Push detected | `apply_repo_updated` 1714 sets `push_detected` via `repo_moved` 1693; `github_poll_due` 1874 | Polls on the next tick regardless of the interval, only while not failing; only one in flight | |
| Watch off | `github_poll_due` 1868 | Only the one attempt a cache-served launch is owed (rule 8) | |
| `r` | key handler 1550 | Local sweep plus `start_github_poll`; the in-flight lock stops stacking. **Ignores `retry_due`**, so `r` during a failure or a rate limit sends a request at once | |
| `R` | key handler 1562 | `spawn_load` with `bypass_cache: true`: discovery plus a request. **No guard**: neither the poll lock nor `app.loading` is checked, so each press starts another full load and another request | `loaded` supersedes every poll started before it |
| Give-up | `expire_stuck_github_poll` 1836 after `GITHUB_POLL_STUCK_AFTER` 60 s (1662) | Records `Failed(TimedOut)` and releases the lock; **the request keeps running** and its late answer is filtered by attempt id (`accept_answer`, `app.rs` 1198) | Next attempt one interval after the give-up |
| Push-driven fetches | `remotes_that_moved` 1895 after a successful poll | Not an API call; `git fetch` per moved remote (FZ-04's territory) | |

Not cancelled by anything: the poll task, the `R` load task, the token
subprocess. Nothing reads `x-ratelimit-*`, `retry-after`, `Date` or
`rateLimit` (Read: no match for `remaining`, `resetAt`, `retry-after` or
`x-ratelimit` in `src/` outside discovery).

`cache_ttl` (default 120) no longer decides anything observable: `AnyCached`
serves the TTL read and then the stale read of the same file
(`fetch_repos` 651, 652), and every other path is `Bypass` (Read).

## 2. Measured cost

### Method

The query was reconstructed by reading `build_query` into a Python script in
the scratch directory, not by changing `src/`. It reproduces the function's
text including whitespace. Checked two ways: for 2026-08-18, tick 0 it yields
the three year windows the test `the_query_covers_every_year_of_the_grid`
expects; for the day of measurement it yields the aliases `y0` to `y3` and
`recent` with the exact `from`/`to` dates found in the owner's live cache file
`github-50.json` (read only). The only change in a measured query is one
top-level field beside `viewer`: `rateLimit { cost nodeCount remaining resetAt
limit used }`. Sent with `gh api graphql -i -F query=@file`, the same token
fleetz reads. `repo_limit` 50 is the owner's value (`~/.config/fleetz/config.toml`
does not set it; default `config.rs` 104).

### Requests

Nine live GraphQL requests over two sessions, each started by hand, none in a
loop. Times UTC, 2026-10-01. M1 to M8 are the first draft's; M9 is this
revision's.

| # | Time | Query | HTTP | cost | nodeCount | `used` after | Wall |
|---|---|---|---|---|---|---|---|
| M1 | 17:00:43 | Fleet query, `repo_limit` 50, as sent | 200 | 3 | 1030 | 1436 | 6.26 s |
| M2 | 17:01:14 | Same, `repo_limit` 20 | 200 | 2 | 820 | 1445 | |
| M3 | 17:01:24 | Same, `repo_limit` 100 | **502**, HTML from nginx, no `x-ratelimit-*` headers | none | none | | about 11 s |
| M4 | 17:01:44 | `repo_limit` 50 without the org fallback | 200 | 1 | 350 | 1473 | 6.73 s |
| M5 | 17:01:57 | `{ rateLimit {...} }` alone | 200 | reports 1 | 0 | 1473 (unchanged) | |
| M6 | 17:02:03 | `repo_limit` 100 with `rateLimit(dryRun: true)` | 200, no `viewer` | 4 | 1380 | 1473 (unchanged) | 0.40 s |
| M7 | 17:02:11 | `repo_limit` 100 again, as M3 | **502** | none | none | | 11.15 s |
| M8 | 17:02:27 | `{ rateLimit {...} }` alone | 200 | reports 1 | 0 | 1493 | |
| M9 | 18:09:14 | `{ rateLimit { cost remaining used resetAt limit } }` alone, to read the headers | 200 | reports 1 | | 1070 | about 1 s |

The first review made three requests of its own: cost 3 and 1030 nodes at
`repo_limit` 50, the real query taking about 7.3 s.

What they establish:

- **The poll's price is a function of `repo_limit`**: 20 -> 2, 50 -> 3,
  100 -> 4 (M2, M1, M6). At 50 it is 3 points and 1030 nodes (M1, and the
  review), identical to the 2026-08-24 figure in `github.rs` at `MAX_ORGS` and
  in `CLAUDE.md`. The org fallback returned the same 17 repos from 4 orgs (6,
  4, 4, 3).
- **The org fallback is two of the three points** at 50: 680 of 1030 nodes
  (M1 against M4).
- **The answer time is a range, not a number**: 6.26 s (M1), 6.73 s for the
  query without the org fallback (M4), about 7.3 s (the review). The FZ-01
  baseline measured a cold launch at 6273 ms median, about 6.2 s of it this
  request. Call it 6 to 7.5 s on this fleet, from three runs.
- **`repo_limit` 100 did not answer**: HTTP 502 after about 11 s, twice
  within a minute (M3, M7), while its dry-run price is only 4 (M6). Two
  failures at the same length are evidence of a server-side deadline near
  11 s, consistent with GitHub's documented 10 s timeout; they do not prove
  the deadline is fixed, or that every 502 is one. Whether a 502 is charged
  could not be told apart from the other instances' polls (M4 to M8 window).
- **Reading the budget needs no extra request**: the `x-ratelimit-remaining`
  and `x-ratelimit-used` headers equal the body's `rateLimit` numbers on
  every 200 (M1, M2, M4, M5, M6, M9), so the field adds nothing the headers do
  not carry. A `rateLimit`-only query reports `cost: 1` but `used` did not
  move (M5).
- **The reset header is epoch seconds and a `Date` header comes with it**
  (M9): `X-Ratelimit-Reset: 1790878989`, which is 18:23:09Z, equal to the
  body's `resetAt`, and `Date: Thu, 01 Oct 2026 18:09:15 GMT`. The local clock
  agreed with `Date` within a second on that run. This is what
  [Clocks](#clocks) builds on.
- **A dry run was not observed to be charged**: M6 priced the 100 query
  without executing it in 0.4 s and `used` did not move. One observation, so
  the pricing procedure still reads `used` before and after.
- **The REST `/rate_limit` endpoint is not the same bucket.** Read before and
  after (not counted against the cap), it reported GraphQL `used` 11 to 15
  with reset 16:47 at 16:33, and `used` 3 with reset 17:50 at 17:00 and 17:02,
  while the in-band count over the same minutes ran 1436 to 1493 with reset
  17:23:06 (Measured). It did not move with M1 to M8. Why is unknown; the
  consequence is that fleetz must read the headers, never this endpoint.

Budget before and after M1 to M8, in-band: 3564 remaining right after M1 (so
about 3567 before it), 3507 after M8 at 17:02:27. Those requests cost 3 + 2 +
1 = 6 charged points, plus whatever the two 502s cost (at most 4 each if
charged). The rest of the 57-point drop in those 104 s came from other clients
(Measured: three `fleetz` processes were running; the owner's two installed
instances and a release build from another worktree). M9 reports cost 1;
whether it was charged was not checked, since M5 and M8 already say a
`rateLimit`-only query is not.

### Already measured elsewhere

| Fact | Source |
|---|---|
| Poll at cost 3 / 1030 at `MAX_ORGS` 8 x `ORG_REPO_LIMIT` 12; 26 / 9125 at 25 x 50 | `CLAUDE.md`, `github.rs` doc on `MAX_ORGS`, 2026-08-24; re-confirmed by M1 |
| Calendar windows are free fields | `build_query` comments; `contributions.rs::year_windows` doc |
| Board list `items(first: 100)` cost 1 / 100 nodes; one ticket in full cost 1; proposed 300 s board poll = 12 points an hour while the view is open | FZ-08 issue #21, measured 2026-10-01 |
| Cold-cache live launch: first useful row 6273 ms median (n=5), about 6.2 s of it the GraphQL fetch | `docs/performance-baseline.md`, case E |

### The server deadline

The fleet query answers in 6 to 7.5 s (Measured, three runs). The larger query
at `repo_limit` 100 drew HTTP 502 after about 11 s, twice, with an nginx HTML
page and no rate headers (Measured, M3, M7). Doubling the viewer page added
one point (M6) and stopped getting answers. The reading this design acts on
(Inferred): **query growth is a correctness risk before it is a budget
risk**. A query that no longer fits fails on every attempt, whatever the
budget, and the dashboard goes stale with nothing it can retry its way out
of. Two observations do not tell a fixed deadline from a busy minute, so the
rule below is a margin, not a threshold.

What could grow it (Inferred, from the code and the plans):

- Raising `repo_limit`, `MAX_ORGS` or `ORG_REPO_LIMIT` to cover more repos or
  orgs. The org fallback alone is two thirds of the nodes (M1 against M4).
- A user with more orgs and repos than the measured fleet (4 orgs, 17 org
  repos, 50 viewer repos), which fills pages the owner's fleet leaves partly
  empty. Billing counts the `first:` asked for; server time follows what
  actually exists, so another account can be slower at the same cost
  (Inferred, not measured).
- FZ-05e does not change the query: it changes how `merge_sources` dedupes
  nodes already returned (`docs/repository-identity-design.md`, "no query or
  payload change"). It can add pressure to raise the caps once same-named
  repos the name dedupe drops today become visible rows.
- FZ-09 adding coverage fields that carry a `first:` (pagination cursors,
  issue or PR pages), and FZ-07 or other features adding fields to this query
  rather than a request of their own.

Consequences (coordinator, 2026-10-01, worded per the review):

- Every query change is priced with the dry run and timed once for real before
  it merges ([Pricing a query change](#pricing-a-query-change)). Suggested, not
  decided: a change that takes the real answer past about 8 s goes to the
  coordinator.
- A 5xx answer is classified as its own kind, `Server`, not `http error`, and
  its message keeps the status code (FZ-10a). It is **not** labelled "query
  too slow": a 502 or 504 can be the deadline, a deploy or an outage, and one
  answer does not say which. It backs off like other server failures (A3).
  The header words are the owner's (O1).
- What to do when the query no longer fits (paginate, split it into requests,
  or ask for less) belongs to **FZ-09**, which owns coverage. Not designed
  here.

### Pricing a query change

Build the exact query the changed code will send (from a test that prints
`build_query`'s output, or a faithful reconstruction as in [Method](#method)).
Add one top-level field beside `viewer`:
`rateLimit(dryRun: true) { cost nodeCount remaining used }`, and send it once
with `gh api graphql -i -F query=@file`. The answer has `cost` and `nodeCount`
and no `viewer`; in M6 `used` did not move. Check `used` against the
`x-ratelimit-used` header of the previous answer and record both. Compare
with the same dry run of the query before the change and record both prices
in the ticket. Then send the real query once, with `rateLimit { cost
nodeCount }` in place of the dry run, and time it: **the dry run prices but
does not time**, and M6 priced at 4 the query that M3 and M7 could not get
answered. If the price at the configured `repo_limit` changed, update
`poll_cost` ([4.2](#42-spending-it-a2-decided)) in the same change. Never
loop, and read the budget from the response headers, not from `gh api
rate_limit`.

## 3. Budget arithmetic

At 3 points per poll (`repo_limit` 50, Measured, M1) and a 5000-point hourly
limit (Measured: `limit` 5000 in every response):

| Fleetz instances on one account | Default 30 s | `Pending` floor 15 s |
|---|---|---|
| 1 | 360 / h (7%) | 720 / h (14%) |
| 2 (the owner's usual) | 720 / h (14%) | 1440 / h (29%) |
| 3 (running during M1 to M8) | 1080 / h (22%) | 2160 / h (43%) |
| N at which the hour runs out with nothing else on the token | 14 | 7 |

All Inferred from the measured cost. At `repo_limit` 20 the figures fall by a
third; at 100 (if it answered) they rise by a third, and N becomes 10 and 5.
Add per instance: the FZ-08 board poll at 12 / h while open; each `r` one
poll; each `R` one poll, unbounded when pressed repeatedly today (Read,
section 1). Other clients on the same token (`gh` itself, editor
integrations, agents running `gh`, CI using a personal token) draw from the
same 5000 (Documented: the primary limit is per user). The account was at
1436 points 37.6 minutes into its window (Measured, M1): about 2300 an hour,
so roughly half the budget was already spoken for during the measurement,
more than three instances at 30 s account for (Inferred: some instance at the
`Pending` floor, plus other clients such as agents using `gh`).

### What a limit looks like and what fleetz does

| Limit | How it arrives | How long | Today | Where |
|---|---|---|---|---|
| Primary, GraphQL body | HTTP 200, `errors[].type == "RATE_LIMITED"` (`CLAUDE.md`, observed in this project) | Until `x-ratelimit-reset`, up to an hour (Documented; the window is fixed, measured reset 17:23:06 across all of M1 to M8) | `Api`, `rate_limited`, header `rate limited`; cache untouched; **retried every interval (30 s) for the rest of the window**, about 120 refused requests an hour per instance | `accept_response` 756, `is_rate_limited` 384, `github_poll_secs` 1940 |
| Primary, HTTP | 403 or 429 with `x-ratelimit-remaining: 0` (Documented) | Same | `Http`, label `http error`; header data dropped; retried every interval | `request` 714 |
| Secondary | 403 or 429, with `retry-after` or a "secondary rate limit" message (Documented; triggers include concurrency, more than 2000 points a minute, and server CPU time, which timeouts count against) | `retry-after` seconds; without it, at least a minute (Documented) | `Http`, `http error`; retried every interval, which GitHub documents as able to extend the limit | `request` 714 |
| Server error, deadline among them | 502 after about 11 s, HTML body, no rate headers (Measured, M3, M7); 504 and other 5xx (Documented as possible) | For a query too large to answer: every attempt (Inferred) | `Http`, `http error`; retried every interval, each retry more server time | `request` 714 |
| Error panel text | | | `render_load_error` matches the words `rate limit` in the message (`ui/mod.rs` 826), so only the body-typed case gets its "clears on its own" hint | |

The `Pending` floor is already off while failing (rule 9, D3). The push
shortcut is too (`github_poll_due` 1874).

## 4. Options and decisions

Each subsection keeps the alternatives that were weighed and records the
coordinator's decision of 2026-10-01 (A1 to A7). Where a choice is the
owner's, it is marked open.

### 4.1 Knowing the budget (A1, decided)

| Option | Correctness | Cost | Verdict |
|---|---|---|---|
| **A. Response headers** `x-ratelimit-{limit,remaining,used,reset,resource}` | Present on every 2xx (Measured) and documented on 403/429 limits; absent on a gateway 502 (Measured), where the last known value stands | No request, no query or cache change | **Decided (A1)** |
| B. `rateLimit { cost remaining resetAt }` in the poll | Same numbers (Measured); also gives the query's `cost`. Absent on any error body, which is exactly when it matters | No extra request (Measured), but changes the query and therefore the cached payload's content (not its format: `GraphQlData` has no `deny_unknown_fields`, Read) | Rejected: adds nothing headers lack except `cost`, which `poll_cost` covers |
| C. Separate `rateLimit` call | Correct but separate from the poll it describes | Not charged (M5), but a second request per poll, and secondary limits count requests | Rejected |
| D. REST `/rate_limit` | **Wrong bucket** (Measured) | Not charged | Rejected |

**The reading.** FZ-10a parses, from any response that carries them, success
or failure: `limit`, `remaining`, `used`, the raw `reset` epoch, the
response's `Date`, and a `token_id` (a hash of the token the request used,
computed in memory and never stored or logged). It travels out of
`fetch_repos` with the result. A response without the headers carries no
reading. A cache-served load carries none.

**The state.** FZ-10b keeps, in memory only, a new `app::ApiBudget` beside
`GithubFreshness`: the newest reading, its reset as a local instant (below),
and when it arrived. Not in the cache file: a budget is per account and per
moment, the cache is neither (FZ-09). It starts unknown and stays unknown
until a reading arrives. A reset instant in the past makes it unknown again,
since the window it described is over.

#### Clocks

`x-ratelimit-reset` is the server's epoch; fleetz's clock may disagree with
it. Comparing the two directly would pause too long or resume into a refused
window by the size of the skew. So the reset is turned into a delay when the
response arrives, in the server's own time, and anchored to the local clock:

- `delay = reset - Date`, both from the same response, clamped to 0..3600 s
  (the window is at most an hour, Documented). `reset_at = arrival + delay`,
  where `arrival` is the local wall clock when the response was read.
- No parseable `Date`: `delay = reset - local now`, clamped the same way. The
  skew then leaks in, bounded by the clamp.
- `retry-after` is already a delay and is anchored at `arrival` the same way.
- Every not-before and every reset is a local wall-clock instant
  (`DateTime<Local>`), as `retry_due` already uses. Not `Instant`: on macOS it
  does not advance while the machine sleeps (Inferred from `std`'s use of an
  uptime clock there), so a not-before kept in `Instant` would outlive a
  suspend by the length of the sleep.

#### Which reading wins

Readings can arrive out of order: an answer from a poll given up on, or one an
`R` superseded, lands after a newer one. Such an answer's outcome is discarded
(rule 10), but its headers are still true about the account at the moment the
server wrote them. Order readings by the server's own facts, not by arrival or
attempt id:

1. **Token.** A reading from an **applied** answer with a different
   `token_id` replaces the held one outright: it is a different credential,
   possibly a different account (see [Account switch](#account-switch)). A
   reading from a **discarded** answer applies only when its `token_id`
   equals the held reading's, and then under rules 2 to 4; otherwise, and
   when no reading is held, it is ignored. A discarded answer can be older
   than the switch it would undo, so it may never switch the token.
2. Same token, later raw `reset`: it is a newer window; it replaces.
3. Same token, same `reset`: `remaining` only falls within a window, so the
   reading with the lower `remaining` is the newer one and is kept.
4. Same token, earlier `reset`: an older window; ignored.

So a late reading never raises `remaining` within a window, never brings back
a credential that was replaced, and can still lower `remaining` on the
current one, which is the safe direction.

#### Account switch

The token is looked up per request (A5), so an account switch in `gh` takes
effect on the next request. When that request's answer is applied, its
reading carries a different `token_id` and replaces the budget (rule 1
above). A rate-limit not-before belonged to the old credential and is dropped
with it; the consecutive-failure count is kept, since it describes GitHub's
health, not the account. A late answer sent on the old token cannot undo this
(rule 1). A token rotation on the same account looks the same and costs
nothing: the new reading arrives with the response that noticed it. The cache
file is still keyed by `repo_limit` alone; isolating it by account is FZ-09.

A switch is noticed only by a request. With O2 accepted, nothing is sent
while a limit is known, so a token change in the middle of a limit is not
noticed until the not-before ends, or until the owner presses `r` or `R`
after it ends. Accepted: a limit lasts at most an hour, and the alternative
is a local token check on a timer, rejected in 4.3.

### 4.2 Spending it (A2, decided)

| Option | Behaviour | Verdict |
|---|---|---|
| A. Fixed interval (today) | Correct up to 6 instances at the floor (section 3), then exhausts and polls refused for the rest of the hour | Rejected as the only rule |
| B. Tiers (normal, slow, on-demand only) by `remaining` share | Simple to explain; steps are arbitrary and ignore how close the reset is | Rejected |
| **C. Pacing plus reserve** | Automatic interval = max(rule 6 interval, `cost * seconds_to_reset / (remaining - reserve)`); at or below the reserve no automatic poll until the reset | **Decided (A2)**, reserve 10% of `limit` |

**`cost`** is `github::poll_cost(repo_limit)`, the measured step function: up
to 20 -> 2, up to 50 -> 3, above -> 4 (M2, M1, M6). A value between two
measured points takes the higher one, so pacing errs slow. A pure function in
`github.rs` beside `build_query`, pinned by a test whose comment names M1, M2
and M6; a query change updates it under the pricing procedure. Not read from
the response: A1 keeps the query unchanged, and the headers carry no per-query
cost.

**Unknown budget**: before the first reading, while every response so far
carried no headers (only 5xx or transport failures since launch), and once the
known reset has passed, the configured interval applies, with the `Pending`
floor and the push shortcut as today. A headerless response after a known one
does not erase it: the last known value stands until its reset. Nothing is
slowed on a guess.

C in numbers (Inferred), `repo_limit` 50, reserve 500: at M1's state (3564
remaining, 22 minutes to reset) the pace is 3 * 1340 / 3064 = 1.3 s, so the
configured 30 s wins and nothing changes. At 600 remaining with 40 minutes
left it is 3 * 2400 / 100 = 72 s. It needs no knowledge of how many instances
exist, because `remaining` already counts all of them. The reserve is what is
left for `r`, `R`, the board view and other clients.

Under a **stretched** pace (pace above the configured interval): the
`Pending` floor and the push shortcut are suspended, as rule 9 already does
for failures, so CI lamps update at the stretched pace. A detected push is
remembered and served by the next due poll.

**Below the reserve** (paused): no automatic poll until `reset_at` plus a
jitter of 0 to 30 s from the injected source, so instances paused together do
not all resume in the same second. A push detected meanwhile is remembered and
served by the first poll after the resume. `r` and `R` still go (the reserve
exists for them) while `remaining >= cost`. With `remaining < cost` the next
request would be refused, so it is treated as a known primary limit until the
reset (4.3).

### 4.3 Retrying (A3 decided; manual refresh under a limit open, O2 with A6)

Rule 6 (every finished attempt advances the clock, no immediate retry) stays,
amended to "at least one interval". On top of it, each applied failure yields
a **not-before** instant, and the next automatic attempt is the latest of:
the rule 6 interval in force (the configured one while failing, rule 9), the
pace (4.2), and the not-before.

#### Classification (FZ-10a)

| Response | Kind | Limit |
|---|---|---|
| Transport failure before a response, including a connect timeout | `Offline` | |
| Total deadline reached after the connection was made, including while reading the body | `TimedOut` | |
| Body cut off for another reason | `Offline` (as today) | |
| 2xx, unparseable or no `data` | `Malformed` | |
| 2xx, `errors` with `type == RATE_LIMITED` | `Api` | primary |
| 2xx, other `errors` | `Api` | |
| 403 or 429 with `retry-after` | `Http` | secondary, with its delay |
| 403 or 429 with `x-ratelimit-remaining: 0` | `Http` | primary |
| 403 or 429 whose body says "secondary rate limit" | `Http` | secondary, no delay |
| 429 otherwise | `Http` | secondary, no delay |
| 401; 403 otherwise (scopes, SSO, permissions) | `Refused` (new) | |
| 5xx | `Server` (new) | |
| Other 4xx | `Http` | |
| `gh auth token` fails | `Auth` | |

The non-2xx body is read, within the 30 s total, only to look for the
secondary message, and kept in the failure message trimmed; an HTML body is
not shown. `rate_limited` becomes true for any failure with a limit, so the
header says `rate limited` for the 403 and 429 cases too. `Failure` gains the
limit and its delay beside `rate_limited`; it is built only through its
constructors outside `github.rs` (Read: no struct literal of `Failure`
elsewhere), so the field is local to FZ-10a.

#### Not-before per kind (FZ-10b)

`n` is the consecutive-failure count. Backoff (A3, as ruled by the
coordinator on 2026-10-01):

- **Step 1** (`n = 1`): exactly the configured interval, no jitter. This is
  today's behaviour, which the existing test
  `main.rs::no_attempt_sooner_than_one_interval_after_a_failure` pins: a 502
  at interval 90 is due again at exactly +90 s, not before and not later.
- **Step 2 on** (`n >= 2`): base = min(interval * 2^(n-1), 600 s), times a
  jitter factor in [0.8, 1.2] from an injected source, never less than the
  configured interval (which only matters for an interval above 480 s).
- **At the cap**, the jitter applies on top of it, so the wait there is 480
  to 720 s. Applying the cap after the jitter would put every long-failing
  instance at exactly 600 s, which is where jitter matters.

| Failure | Count | Not-before | Why |
|---|---|---|---|
| Primary limit, reset known | `n += 1` | `reset_at` plus 0 to 30 s of jitter | Every request before the reset is refused and documented as able to escalate |
| Primary limit, no reset header (or a reset in the past) | `n += 1` | Backoff | Nothing says when it lifts; the ladder reaches the cap at the sixth failure |
| Secondary limit | `n += 1` | `retry-after`; absent: the later of 60 s and the backoff | Documented minimum of a minute |
| `Server`, `TimedOut` (the client deadline or the 60 s give-up), `Malformed` | `n += 1` | Backoff | A 502 that is a query-size problem repeats; each retry costs server time that secondary limits count |
| `Refused` | `n += 1` | Backoff | A refused token rarely starts working on its own, but a transient 401 does happen; the ladder retries it within up to about twelve minutes instead of never. Documented: repeated failed authentication can make GitHub refuse the user for a while, which the cap keeps rare |
| `Http`, `Api` that are not limits | `n += 1` | Backoff | |
| `Offline` | unchanged | unchanged: a pending not-before stays; otherwise the rule 6 interval | Costs GitHub nothing; recovery is quick when the network returns (A3) |
| `Auth` (no token) | unchanged | unchanged: a pending not-before stays; otherwise the rule 6 interval | No request is sent; a later `gh auth login` is picked up within one interval without a keypress (A3) |

`Offline` and `Auth` are **transparent to the ladder** (coordinator,
2026-10-01): they neither change the count nor set or clear a not-before. They
say nothing about GitHub, so a limit or a backoff they interrupt is still in
force after them.

The first draft put `Auth` at the cap and paused polling after a 401 until the
token changed. Both are dropped: `Auth` sends nothing, so the cap bought
nothing and cost the user 8 to 12 minutes after signing in; and the 401
pause stopped automatic polling for good on a transient refusal (review F4).
Rejected for the 401: re-reading the token locally each interval to resume at
once on a new one. It shortens recovery after a re-login but adds a background
subprocess task to FZ-10b; `r` gives the same result on demand.

#### Transitions

Each is a rule and a scenario in [section 5](#5-controlled-failure-scenarios).

1. **Only an applied answer changes the count, the not-before or the
   token.** An answer discarded by `accept_answer` (rule 10) changes none of
   them; its budget reading applies only when its `token_id` equals the held
   one, under [Which reading wins](#which-reading-wins).
2. **Every applied attempt replaces the not-before**, except `Offline` and
   `Auth` (transition 4). The newest outcome decides it; an older one is
   never kept beside it.
3. **A success clears both**: `n = 0`, no not-before, even when the success
   came while a not-before was still pending (an `r` during backoff, or an
   `R` while a secondary limit's `retry-after` runs). The next automatic
   attempt is the rule 6 interval, or the pace, after it.
4. **`Offline` and `Auth` are transparent** (A3, coordinator, 2026-10-01):
   they keep the count and leave a pending not-before in place. A server
   failing behind a flapping network does not restart its ladder at every
   network blip, and an `r` that comes back `Offline` during a backoff or a
   limit does not cut it short. Example: 502, 502, `Offline`, 502 gives
   steps 1, 2, then the interval, then 3.
5. **A give-up is a `TimedOut` failure** (D4) and advances the ladder like a
   client timeout.
6. **An `R` that went to GitHub counts like any attempt**, success or
   failure.
7. **A token change**, seen only on an applied answer, drops the budget and
   any limit not-before ([Account switch](#account-switch)) and keeps the
   count. A discarded answer on another token triggers nothing.
8. **The reset passing** makes the budget unknown; a primary limit's
   not-before already sits at the reset plus jitter.

#### Manual refresh

- `r` and `R` ignore backoff and pace: a person asked. Their outcome counts
  like any attempt.
- Under a known limit (a primary or secondary limit whose not-before has not
  passed, or `remaining < cost`): open, for the owner as one decision (O2 with
  A6). Recommended: `r` does its local sweep and **skips the request**, with a
  status naming when GitHub resumes; `R` rebuilds without a request, as below.
  Alternative: let them through, which spends a refused request and risks
  extending a secondary limit.
- **A6, decided by the coordinator on 2026-10-01**: under a known limit, `R`
  rediscovers and re-sweeps from **the dataset already in memory**
  (`assemble_view` over `app.dataset`, as a window apply does, with discovery
  and fetches as `R` has), and lands as `DatasetSource::Local`, which leaves
  `GithubFreshness` untouched: data time, last attempt and outcome stay what
  the last real attempt left, and that outcome is the limit. Whether `r` and
  `R` skip the request at all stays the owner's (O2); this is how `R`
  rebuilds if he says yes.
  - Why not as first worded ("from the cache as a fallback carrying the limit
    failure"): through `GithubFreshness::loaded(DatasetSource::Fallback)`
    (`app.rs` 1150) that calls `failed(failure, now)`, which sets
    `last_attempt` and `outcome` for a request that was never made, against
    rule 1 and the definition of an attempt, and restarts the retry clock.
    Also rejected: reading the cache as `DatasetSource::Cache`, which respects
    rule 1 but needs a new cache-only mode in `github.rs` (`AnyCached` falls
    through to a request when the cache is unusable) and can move the data
    time back to the file's.
  - **Only while no poll is in flight** (`!github_poll_inflight`). A poll
    started before the limit became known can land newer CI data and a newer
    data time; a rebuild from the older memory copy landing after it would
    put older rows under that newer data time. With a poll in flight, `R`
    does nothing and shows the "a refresh is already running" status (one
    rule with A7, below).
  - The memory copy is at least as new as this instance's cache file (rule
    4, D5). With nothing in memory (the error panel), `R` only says when
    GitHub resumes. **Accepted cost**: that holds even if another instance
    sharing the cache file has since written a usable payload, which `R`
    then does not pick up until the limit lifts.
  - The status line after the rebuild names when GitHub resumes. Today a
    `LoadKind::Reload` with source `Local` says "Reloaded" (the `_ =>` arm of
    `apply_load_result`, `main.rs` 590); FZ-10b gives this case its own
    message, with provisional words that FZ-10c settles.
- `R` while a load is already running is ignored (A7, decided), as `r`
  already is by the poll lock. **One rule for both cases**: `R` ignored
  because a load is running (A7) or because a poll is in flight under a
  limit (A6) shows the same "a refresh is already running" status. The
  exact words are the owner's (O1).

### 4.4 Several instances and other clients (A4, decided)

One instance can know only what the headers say: `limit`, `remaining`, `used`,
`reset` for the whole account. It cannot see how many instances or which
clients spend.

| Option | Needs | Verdict |
|---|---|---|
| **A. Independent, adapting to `remaining`** (4.2 C) | Nothing shared | **Decided (A4)**. N instances converge on sharing what is left; each still pays its own polls, so N instances cost N times one when the budget is healthy |
| B. Coordinate through the disk cache: one instance holds a lease and polls, the others treat a fresh cache file as their poll | An atomic cache write (today a plain `fs::write`, so a reader can see a torn file, Read), a cache keyed by account and host (FZ-09), a lease file with expiry, and **a change to freshness contract rule 1 and decision D5**, which today say a cache read is not an attempt and a failed poll never applies the cache | Not designed here. A possible later ticket, after FZ-09's atomic, account-keyed cache; the rule 1 and D5 change is the owner's to approve |

Instances that hit the same limit or the same outage tend to fail together and
would retry together. Every resume point carries jitter from the injected
source: 0 to 30 s after a reset (a primary limit, or a pause below the
reserve), and the 0.8 to 1.2 factor on the backoff ladder from step 2 on.
Step 1 of the ladder and the first attempt after a launch (rule 8) have
none; step 1 is short, so instances failing together cost little there.

### 4.5 Deadlines and cancellation (A5, decided)

- **One reused client** (`reqwest::Client` built once in `github.rs`, for
  example behind a `OnceLock`, so no caller changes) with `connect_timeout`
  10 s and `timeout` 30 s. 30 s clears the measured 6 to 7.5 s answer and the
  11 s 502 (Measured) with margin, and fires well before the 60 s give-up,
  which stays as the backstop for a hung token subprocess. The total deadline
  covers the body read.
- **Classification of a timeout.** A timeout after the connection was made,
  including during the body read, is `TimedOut` and backs off. A connect
  failure, including a connect timeout, is `Offline` (A5, coordinator,
  2026-10-01). It means the path to GitHub is not up, which
  is what `Offline` describes and why A3 excludes it from backoff; as
  `TimedOut` it would put a dropped Wi-Fi on the server ladder. reqwest marks
  a connect timeout as both a connect and a timeout error, so the connect
  test goes first.
- **A pooled connection after a suspend.** A reused client keeps idle
  connections; after the laptop sleeps, the first request may go out on one
  the far end has dropped. It then fails once, as `Offline` (reset) or
  `TimedOut` (silence until the 30 s deadline), the dead connection is
  discarded, and the next attempt opens a new one: at most one failed poll.
  `Offline` leaves the ladder as it was; `TimedOut` from a healthy state is
  step 1, exactly the plain interval with no jitter (Inferred, not
  reproduced). The pool's idle timeout cannot prevent
  it, because it is measured on a clock that stops during sleep (Inferred).
  Accepted as a cost of reuse; if it shows up in use, `pool_max_idle_per_host(0)`
  keeps one configured client without pooling. Reuse saves a TLS handshake
  per poll (size not measured).
- **Token lookup stays per request**: 0.05 s (Measured), and it is how an
  account switch in `gh` is noticed. Rejected: caching it and re-reading only
  after a 401.
- **FZ-11's**: aborting the poll or `R` task on give-up or quit, a shared
  scheduler that owns `github_poll_due`, and the stale-result rejection (the
  attempt ids are already in place). FZ-10's policy is pure functions that
  scheduler calls.

### 4.6 What the user sees (O1, open)

The header's `rate limited` (FZ-03) stays. Information to add, recommended;
the choice, wording and look are the owner's (O1):

- Rate limited: when the next automatic attempt is (the reset time).
- Polling slowed by pace, or paused at the reserve: that it is, and why.
- Backing off after repeated failures: nothing new; the stale segment already
  says failed and how old the data is. The next attempt time is optional.
- Remaining budget as a number: not in the header (it says nothing actionable
  while healthy). Candidate home: the help overlay or a status line on `r`.
- 403 and 429 limits get the `rate limited` label instead of `http error`;
  `Server` and `Refused` get labels of their own. The classification is
  decided and is FZ-10a; the words are the owner's.
- `R` ignored because a load runs (A7) or a poll is in flight under a limit
  (A6): one status for both, such as "a refresh is already running", so the
  key does not look dead.

### 4.7 Configuration (O3, open)

Recommended: none. The reserve (10%), the cap (600 s), the jitter ranges and
the client deadlines are fixed rules with tests. A key such as a budget share
for someone whose CI uses the same token is the one plausible addition; not
proposed until someone asks (O3).

## Contract and invariant changes

Each item below names what it changes in an accepted document and who signs
it off.

- **Freshness contract rule 6, amended**: "the next automatic attempt comes
  one normal interval later" becomes "at least one normal interval later".
  Its two guarantees (every finished attempt advances the clock, no
  immediate retry) are kept, and backoff is floored at the interval so jitter
  cannot break them. Owner (O4).
- **Rule 9, amended**: "While `outcome` is `Failed`, the poll interval is the
  configured one" becomes "at least the configured one", since backoff
  stretches it to 8 to 12 minutes (480 to 720 s) at the cap. The first
  failure still waits exactly the configured interval. Its purpose, no `Pending` floor
  while failing, is kept and extended: a stretched or paused pace also
  suspends the floor and the push shortcut. Owner (O4).
- **Rule 1 and the definition of an attempt: unchanged** (A6 as ruled on
  2026-10-01). An `r` or `R` that sends nothing under a known limit records
  nothing (4.3).
- **Transitions table, one row added, under O2 with A6** (only if the owner
  accepts O2):

  | Event | Rows | `data_time` | `last_attempt` | `outcome` | Cache file |
  |---|---|---|---|---|---|
  | `r` or `R` under a known limit, no poll in flight | `r`: local state only. `R`: rebuilt from the dataset in memory | Unchanged | Unchanged | Unchanged | Untouched |

- **"What the user sees", one line added, under O2 with A6**: `r` or `R`
  under a known limit says in its status line that GitHub is rate limited and
  when it resumes, instead of `Refreshing…` or `Reloaded`. Wording is the
  owner's, as for the rest of that section.
- **Vocabulary, amended**: the failure kinds gain `Server` (a 5xx answer) and
  `Refused` (credentials or permissions refused: a 401, or a 403 that is not a
  limit). "A rate limit is never reported as offline" holds; a rate limit can
  now be an `Http` failure as well as an `Api` one, and `Failure::rate_limited`
  widens to match. Owner (O4), since the header words follow from the kinds.
- **Rule 10 and D4, amended**: "Cancelling the hung request and a client
  timeout are FZ-11" becomes "cancelling the hung request is FZ-11; the client
  timeout is FZ-10". The 60 s give-up stays as the backstop. Owner (O4).
- Existing wording gap, not changed here: the contract defines an attempt as
  a network request, while an `Auth` failure, which sends none, is recorded as
  an attempt today (FZ-03). This design keeps that and treats `Auth` like
  `Offline`.
- `CLAUDE.md` invariants "batch on GraphQL", "bills the `first:` you ask
  for", "never widen without checking the cache", "only a response carrying
  `data` may be cached" and the `recent` rotation are all kept. A slower poll
  does not break the rotation: each poll still uses the current hold's `from`
  (`rotation_tick`), polls simply skip holds.
- No cache format or key change, no config key, no dependency change
  (`reqwest` already has both timeouts).

## 5. Controlled failure scenarios

All without a network: pure functions over a status code, a header map and a
body, or over `GithubFreshness` and `ApiBudget` with a planted `now` and a
planted jitter source, as FZ-03's tests drive transitions. "Interval" is the
configured 30 s. Times are UTC as each clock reads them: header values on the
server's clock, everything else on fleetz's local clock. The two agree unless
a scenario says otherwise (S6a).

| # | Given | Expected | Ticket |
|---|---|---|---|
| S1 | 200 with `x-ratelimit-remaining: 3564`, `reset` 17:23:06, `Date` 17:00:43 | A reading with 3564 and that reset | a |
| S2 | 502, HTML, no rate headers, after S1 | No reading; kind `Server`, message carries 502; the held budget unchanged | a, b |
| S2a | 504, no rate headers | Kind `Server`, not `Http`; backoff step 1 | a, b |
| S2b | Launch, only 502s so far | Budget unknown; next attempts by the A3 backoff from the interval, no pacing | b |
| S2c | Budget known, `now` past its reset, healthy polls | Budget unknown; interval and `Pending` floor apply | b |
| S3 | Healthy budget, a `Pending` row, polls succeeding | Interval 15 s (today's behaviour kept) | b |
| S4 | 600 remaining, 40 min to reset, `repo_limit` 50, reserve 500 | Next automatic attempt 72 s after the last; `Pending` floor and push shortcut suspended | b |
| S4a | `poll_cost` at 20, 50, 100, and at 30 | 2, 3, 4, and 3 | a |
| S5 | 480 remaining (below reserve), reset 17:23:06, jitter 10 s | No automatic attempt before 17:23:16; `r` still sends | b |
| S5a | Below the reserve, a push detected at 17:10 | No poll at 17:10; the first poll after the resume serves it | b |
| S5b | 2 remaining, cost 3, `r` pressed | Treated as a known primary limit: no request if O2 is accepted, status names the reset | b |
| S6 | 200 body `RATE_LIMITED`, reset 17:23:06, jitter 10 s | `Failed(Api, rate_limited)`, cache file untouched, not-before 17:23:16; `retry_due` false at 17:23:15, true at 17:23:16 | a, b |
| S6a | As S6, local clock 10 min ahead of the server: server `Date` 17:00:00 and reset 17:23:06; the local clock reads 17:10:00 on arrival | On the local clock: `reset_at` 17:33:06, not-before 17:33:16 | b |
| S6b | Primary limit with no reset header | Not-before by the backoff, not "until the reset" | a, b |
| S7 | 403 with `x-ratelimit-remaining: 0` | `rate_limited` true, primary (today: `http error`) | a |
| S8 | 403 with `retry-after: 120` | Secondary; not-before arrival + 120 s | a, b |
| S9 | 429 without `retry-after` | Secondary; not-before the later of 60 s and the backoff | a, b |
| S9a | 403 with neither limit header nor the secondary message | Kind `Refused`, not rate limited | a |
| S10 | Six consecutive 502s, jitter 1.0 | Next attempts at +30, +60, +120, +240, +480, then +600 s (the cap) | b |
| S10a | First 502 with jitter draws 0.8 and 1.2; then a second 502 with draw 0.8 | Step 1 at exactly +30 s for either draw (no jitter at step 1); step 2 at +48 s | b |
| S11 | Backoff at step 3, then a success | Count reset; next attempt one interval later | b |
| S11a | Secondary limit not-before at 17:12, O2 rejected so `r` at 17:10 sends, and it succeeds | Not-before cleared, count 0; next automatic attempt one interval after 17:10 | b |
| S12 | Three consecutive `Offline` | Each next attempt one interval later (A3) | b |
| S12a | 502, 502, `Offline`, 502 | Steps 1, 2, interval, 3 (count kept across `Offline`) | b |
| S12b | Backoff at step 3 (not-before +120 s), `r` at +20 s comes back `Offline`; separately, the same with `Auth` | Count and not-before unchanged: the next automatic attempt is still at +120 s | b |
| S13 | `Auth`: `gh auth token` fails | Next attempt one interval later; a working token then polls | b |
| S14 | 401 three times, then a success | Backoff steps 1 to 3, then normal; automatic polling never stops | a, b |
| S15 | Rate limited until 17:23, user presses `r` at 17:10 | If O2 is accepted: local sweep runs, no request, status names 17:23; outcome and `last_attempt` unchanged | b |
| S16 | Rate limited until 17:23, no poll in flight, user presses `R` | If O2 is accepted: rows rediscovered from the dataset in memory as `DatasetSource::Local`; no request; `data_time`, `last_attempt` and `outcome` unchanged; the status line names 17:23, not `Reloaded` | b |
| S16b | Rate limited, a poll in flight, user presses `R` | If O2 is accepted: no rebuild and no request; the same "a refresh is already running" status as S18. After the poll lands, `R` behaves as S16 | b |
| S16a | Rate limited, nothing in memory (error panel), user presses `R` | No request; status names the reset | b |
| S17 | Backing off (not limited), user presses `r` | Request sent; a failure advances the ladder, a success resets it | b |
| S18 | `R` pressed while a load runs | No second load, no second request; the "a refresh is already running" status | b |
| S19 | A request that exceeds the client deadline after connecting, including during the body read | `Failed(TimedOut)` before the 60 s give-up; the in-flight lock released by `apply_github_poll` | a |
| S19a | A connect timeout | `Offline`, not `TimedOut` (A5) | a |
| S19b | Healthy, then the first request after a suspend fails on a dead pooled connection | One failure: `Offline` leaves the ladder as it was, `TimedOut` is step 1; either way the next attempt is exactly one interval later, no jitter, and is a normal request | b |
| S20 | Property, any sequence of outcomes and readings | No automatic attempt sooner than the rule 6 interval in force (the `Pending` floor while healthy and unstretched, else the configured interval) after the last finished attempt. The push shortcut is the only exception, and only while healthy and unstretched | b |
| S21 | A given-up poll's late answer with `remaining` 3000, after a newer reading of 2900 in the same window | Budget stays 2900; the late outcome is discarded | b |
| S21a | Late reading on the same token with a later raw `reset` | Replaces: it is a newer window | b |
| S22 | Applied answer whose reading has a different `token_id`, higher `remaining`, same reset | Replaces the budget; a pending primary limit not-before is dropped; the count is kept | b |
| S22a | Poll on token A in flight; the account is switched; `R` on token B lands and supersedes the poll, leaving B's budget and a B limit not-before; then A's answer arrives, with an A reading and a rate limit | A's answer is discarded: B's budget and B's not-before are unchanged, and the token held stays B | b |
| S23 | Cache-served launch with watch off | Exactly one attempt, at once, without pacing; none after it, whatever its outcome or not-before (rule 8) | b |
| S24 | Two instances paused below the reserve, jitter 3 s and 21 s | Resume at reset + 3 s and reset + 21 s | b |

## 6. Implementation tickets

Three tickets, serial with each other and with the other tickets that own the
same files. Each block is usable as an issue body.

**Order.** FZ-04 is integrated in the base. The `main.rs` line runs serially:
FZ-06b (#38, in progress), FZ-06e (#41), FZ-06c (#39), FZ-06d (#40), then the
identity series FZ-05b to FZ-05f (#32 to #36). FZ-05e (#35) also owns
`src/github.rs` (`merge_sources`), and FZ-05d (#34) reworks how
`apply_github_poll` keys GitHub fields. FZ-10a, b and c come after FZ-05f, in
that order, each branched from `main` after the previous one merged.

FZ-10b changes behaviour the user can notice (pacing, backoff, `r` and `R`
under a limit) with provisional status words; the README and the settled
user-facing wording land in FZ-10c. Between the two merges `main` behaves
newly with interim words, which matters only if Andreas installs from it in
that span.

**Landing notes.** The `CLAUDE.md` index row for this document is the
coordinator's to add when it lands; no ticket edits `CLAUDE.md`.

### FZ-10a: read the budget and classify failures

- **Owns**: `src/github.rs`.
- **Forced by the compiler, mechanical only**: `src/main.rs`. Carrying the
  reading out of `fetch_repos` adds a field to `GithubFetch` and to
  `FetchError`, and `main.rs` names their fields without `..`. In production
  code: the destructure of `GithubFetch` in `apply_github_poll` and the two
  `FetchError` patterns in `load_repos`. In existing tests (`main.rs`
  `mod tests`, verified at `f8a54c0`), four struct literals:
  - `poll_says_passed`, a `GithubFetch` literal;
  - `poll`, a `FetchError` literal;
  - `offline_with_cache`, a `FetchError` literal;
  - `a_cache_served_launch_with_watch_off_makes_exactly_one_attempt`, a
    `FetchError` literal.

  FZ-10a binds the new field as `_` or builds it as `None` at each of these
  and changes no behaviour in `main.rs`.
- **Needed**: one function in `src/ui/mod.rs`. `github_failure_label` (77)
  is an exhaustive `match` on `FailureKind` and keys `rate limited` to
  `FailureKind::Api`; the two new kinds do not compile without it, and an
  `Http` limit would not show as `rate limited`. Its words are provisional
  until O1 is settled in FZ-10c.
- **Outcome**: a budget reading (limit, remaining, used, raw reset, `Date`,
  token id) parsed from every response that carries the headers, success or
  failure, and carried out of `fetch_repos`; `poll_cost(repo_limit)`; the
  classification table of 4.3, with the limit and its `retry-after` on the
  failure and `rate_limited` true for every limit; one reused client with
  connect 10 s and total 30 s; a connect failure `Offline` (A5), a timeout
  after connecting `TimedOut`, including during the body read.
- **Acceptance**: S1, S2, S2a, S4a, S6 (classification part), S6b, S7, S8,
  S9, S9a, S14 (classification part), S19, S19a. Every existing test passes.
  No existing assertion is changed, removed or weakened. The only edits
  allowed to existing tests are the compile-forced new field, built as `None`
  or bound as `_`, in the four builders named above; any other edit to an
  existing test is out of scope.
- **Approval and constraints**: waits for the owner's O5 (the four test
  edits) before it is dispatched, as FZ-06b did for its test edit.
- **Excludes**: pacing, backoff, not-before, `ApiBudget` (FZ-10b); `src/app.rs`;
  header wording (FZ-10c); any query, cache or config change; cancellation
  (FZ-11).

### FZ-10b: pace and retry policy

- **Owns**: `src/app.rs`, `src/main.rs`.
- **Outcome**: `ApiBudget` with the clock anchoring and the reading order of
  4.1; pure pacing with the reserve, the resume jitter and the unknown-budget
  rule (A2); the not-before and count transitions of 4.3 with an injected
  jitter source; `github_poll_due` and `github_poll_secs` built on them; the
  `R` guard (A7); and, if the owner accepts O2, `r` and `R` under a known
  limit as in 4.3 (A6): the rebuild from memory only while no poll is in
  flight, and a status line naming when GitHub resumes in place of
  `Reloaded` (today's `_ =>` arm of `apply_load_result`, `main.rs` 590).
  Corrects the `GITHUB_POLL_MIN` and `start_github_poll` docs (see the table
  below).
- **Constraints that keep existing tests untouched** (checked at `f8a54c0`):
  - The rebuild from memory is marked by a new `LoadKind` variant, not a new
    `LoadResult` field: existing tests in `main.rs` build `LoadResult` as a
    struct literal, so a field would force edits there. A variant forces
    none: `LoadKind` is matched exhaustively only in `apply_load_result`,
    which is production code FZ-10b owns; tests only construct `Initial` and
    `Reload`.
  - `github_poll_due(&App, &Config, now)` and `github_poll_secs(&App,
    &Config)` keep their signatures, since existing tests call both. The new
    state is read from `App`. The same holds for
    `GithubFreshness::retry_due(now, every_secs)`, which `app.rs` tests call;
    the not-before and pace sit beside it rather than inside it. Every
    existing timing test applies a single failure or a success before its
    check, so step 1 being exactly the interval keeps them passing.
  - `ApiBudget` and the policy state live on `App` beside
    `GithubFreshness`; tests build `App` through helper functions, not
    struct literals, so a new field forces no test edit.
- **Acceptance**: S2b, S2c, S3, S4, S5, S5a, S5b, S6, S6a, S8, S9, S10,
  S10a, S11, S11a, S12, S12a, S12b, S13, S14, S15, S16, S16a, S16b, S17,
  S18, S19b, S20, S21, S21a, S22, S22a, S23, S24. Existing tests pass
  unchanged; none is edited. That includes
  `main.rs::no_attempt_sooner_than_one_interval_after_a_failure`, which pins
  step 1 at exactly one interval.
- **Gate**: the owner's O4 (rules 6, 9 and 10, vocabulary) before it merges,
  and O2 before the `r` and `R` part merges.
- **Excludes**: header and status wording beyond plain provisional text
  (FZ-10c); `README.md`; config keys (O3); cancellation and a shared
  scheduler (FZ-11); the board's own poll (FZ-08 adopts `ApiBudget` later);
  `src/github.rs`.

### FZ-10c: show it

- **Owns**: the header segment, `github_failure_label` and `render_load_error`
  in `src/ui/mod.rs`; `README.md`.
- **Outcome**: the information of 4.6 as the owner settles it (O1), final
  label words for `Server`, `Refused` and HTTP limits, the load-error panel
  showing its rate-limit hint when the typed `rate_limited` flag is set **as
  well as** on today's word match (not instead of it), and the panel's key
  hint corrected (it says `r` rebuilds from cache).
- **Acceptance**: the header shows each held-back state of 4.6 with a planted
  `App`; the panel's rate-limit hint appears for an `Http` limit; README
  describes the behaviour. Existing tests pass unchanged; none is edited.
  That includes `ui/mod.rs::a_rate_limited_load_says_it_clears_on_its_own`,
  which sets only the message text, so the word match must stay. Stops at Ready to integrate: wording and look are
  the owner's on the branch build.
- **Excludes**: policy changes; `src/app.rs` and `src/main.rs` except a
  status string FZ-10b left provisional, if the owner changes its words.

Every later query change (FZ-05 series, FZ-07, FZ-08, FZ-09) follows
[Pricing a query change](#pricing-a-query-change).

### Contradictions found, and who corrects them

| What | Where | Fix |
|---|---|---|
| `CLAUDE.md` says the tick is 250 ms; it is 100 ms | `CLAUDE.md` "Current state"; `main.rs::TICK_INTERVAL` 74 | Coordinator (`CLAUDE.md` text) |
| `CLAUDE.md` says "`r` rebuilds from cache"; `r` is a local sweep plus a poll that bypasses the cache | `CLAUDE.md` "Never widen the GitHub fetch"; key handler `main.rs` 1550 | Coordinator (`CLAUDE.md` text) |
| `CLAUDE.md` names `last_github_poll` and `GithubFetch::from_cache`, neither of which exists since FZ-03; the rule is now `GithubFreshness::loaded` leaving `last_attempt` unset for `DatasetSource::Cache` | `CLAUDE.md` "Launch paints from the cache" | Coordinator (`CLAUDE.md` text) |
| The load-error panel says "R retries against GitHub, r rebuilds from cache"; `r` does not rebuild from cache | `ui/mod.rs::render_load_error` 820 | FZ-10c |
| `GITHUB_POLL_MIN` doc calls the 15 s floor comfortable; true for one instance, not for several (section 3) | `main.rs` 1664 to 1668 | FZ-10b |
| `start_github_poll` doc says mashing cannot stack requests; true for `r`, not `R` | `main.rs` 1954, key handler 1562 | FZ-10b (with the A7 guard) |
| The load-error panel decides "rate limit" by matching words in the message, not the typed flag | `ui/mod.rs::render_load_error` 826 | FZ-10c |
| `cache_ttl` documented as a TTL, decides nothing observable | `fetch_repos` 651, 652; `config.rs` | Coordinator: not FZ-10's; candidate for FZ-09 |

### Interactions

- **FZ-04** (fetch outcomes, integrated): no API calls. A slower or paused
  poll means `remotes_that_moved` finds pushes later, so fewer targeted
  fetches; the watch sweep still fetches on `refresh_interval`.
- **FZ-05 series**: FZ-05e changes `merge_sources` dedupe only, no query
  change (`docs/repository-identity-design.md`), but owns `src/github.rs`, so
  FZ-10a follows it. `MAX_ORGS` and `ORG_REPO_LIMIT` carry two thirds of the
  cost (M1 against M4); any change goes through the dry-run procedure.
- **FZ-06 series**: all touch `src/main.rs`; FZ-10b follows them (Order,
  above).
- **FZ-09** (#22, coverage and cache isolation): the budget is per account;
  [Account switch](#account-switch) covers it in memory, the cache key is
  FZ-09's. 4.4 B needs FZ-09's atomic, account-keyed cache. Coverage cannot be
  bought by raising `repo_limit` past roughly 50 in one query: 100 drew a 502
  twice (M3, M7). More coverage means pagination, priced per page.
- **FZ-11** (#24, scheduler): owns cancellation and the loop that calls the
  policy; FZ-10b's functions are written to be moved under it unchanged.
- **FZ-12** (#25, progressive loading): a cold launch waits 6 to 7.5 s on the
  one request; progressive loading must not split it into several.
- **FZ-08** (#21, board): 12 points an hour per open board view at 300 s. It
  should obey the same not-before and reserve (one `ApiBudget`, since it is
  the same token); below the reserve it polls only on demand. Its own
  freshness state stays separate.

## 7. Decisions

### Architectural (decided by the coordinator, 2026-10-01)

- **A1 Budget source.** Response headers. No query change. Rejected: in-query
  `rateLimit`, a separate call, REST `/rate_limit` (4.1).
- **A2 Spending.** Pacing plus a fixed reserve of 10% of `limit`. While the
  budget is unknown, the configured interval applies; nothing is slowed on a
  guess (4.2).
- **A3 Backoff.** Doubling from the interval per consecutive failure, cap
  600 s, jitter 0.8 to 1.2 from an injected source. `Offline` excluded, so
  recovery is quick when the network returns (4.3). Ruled on 2026-10-01:
  step 1 is exactly the interval with no jitter; jitter applies from step 2
  and on top of the cap (480 to 720 s there); `Offline` and `Auth` are
  transparent to the ladder, keeping the count and any pending not-before;
  `Auth` retries at the plain interval.
- **A4 Several instances.** Independent, pacing on the shared `remaining`.
  Cache-based coordination is a possible later ticket, after FZ-09's atomic,
  account-keyed cache, and would change freshness rule 1 and decision D5,
  which only the owner can approve (4.4).
- **A5 Client.** One reused client, connect timeout 10 s, total 30 s. A
  timeout after connecting is `TimedOut`; ruled on 2026-10-01: a connect
  failure, including a connect timeout, is `Offline`. Token lookup stays per
  request (4.5).
- **A6 `R` under a known rate limit.** Ruled on 2026-10-01: `R` rebuilds from
  the dataset in memory as `DatasetSource::Local`, recording no attempt, and
  only while no poll is in flight. Not "from the cache as a fallback carrying
  the limit failure", which would record an attempt that was never made
  (4.3). Whether `r` and `R` skip the request at all is the owner's (O2).
- **A7 `R` while a load runs.** Ignored, in FZ-10b.

### Owner: open

Presented to Andreas together. Each says what he would see or wait for.

- **O1 What the header says when GitHub polling is held back.** Today the
  header says nothing while polls work, and `⚠ github rate limited, as of
  14:02` once GitHub refuses. Recommended: keep that, add when polling
  resumes (for example "until 15:00"), and say so in the same place when
  fleetz has slowed its polling because the account's hourly allowance is
  running low, or paused it because the allowance is nearly gone. No
  remaining-allowance number in the header. New labels for "GitHub had a
  server error" and "GitHub refused the sign-in". Pressing `R` while a reload
  or a GitHub refresh is already running does nothing; recommended, one
  short status for both, such as "a refresh is already running", so the key
  does not look dead. Wording and look are
  settled on the FZ-10c branch build.
- **O2 with A6: pressing `r` or `R` while GitHub is rate limiting.** Today
  both send a request that GitHub refuses, and repeated refused requests can
  make GitHub hold the limit longer. Recommended: until the limit lifts, `r`
  refreshes the local git state only and `R` re-scans your folders using the
  GitHub data already on screen; both say "GitHub rate limited until 15:00".
  The header keeps saying rate limited with the age of the data. Outside a
  rate limit nothing changes: `r` and `R` always ask GitHub at once, even
  while automatic polling is slowed or backing off. One side effect: since
  nothing is sent during the limit, switching GitHub accounts in `gh` in the
  middle of it is not noticed until the limit ends. Alternative: let them ask
  anyway.
- **O3 No new config key.** Recommended: none. Everything above is a fixed
  rule. A key (for example to leave room for CI using the same token) would be
  a config change; nothing asks for it yet.
- **O4 Changes to the freshness contract you accepted.** Recommended: accept
  all four.
  - Rule 6. Today: after any attempt, the next automatic one comes exactly
    one poll interval later. After: at least one interval later, so fleetz
    may wait longer while GitHub limits it, while the allowance runs low, or
    after repeated failures. It still never retries immediately.
  - Rule 9. Today: while GitHub polls fail, fleetz retries every configured
    interval (30 s). After: the first retry still comes after 30 s, then the
    wait doubles with each failure in a row, up to about twelve minutes (8
    to 12 at the cap, randomised so several fleetz windows do not retry
    together). A network outage is the exception: offline, it keeps
    retrying every 30 s, so it recovers quickly when the network returns.
  - Failure kinds. Two new ones the header can name: GitHub answered with a
    server error, and GitHub refused the sign-in or its permissions. A 403 or
    429 that is a rate limit says `rate limited` instead of `http error`.
  - Timeouts. A request that gets no answer is given up after 30 s instead of
    60 s, in FZ-10 rather than FZ-11. A connection that cannot even be opened
    within 10 s now reads `offline`, not `timed out`, since it means the
    network is down rather than GitHub being slow.
- **O5 Four existing tests get one mechanical edit in FZ-10a.** To carry
  the allowance numbers out of the GitHub fetch, FZ-10a adds one field to
  the two result types the fetch returns. Rust requires every place that
  builds one of those types by hand to name every field, and four existing
  tests in `src/main.rs` do (`poll_says_passed`, `poll`,
  `offline_with_cache`,
  `a_cache_served_launch_with_watch_off_makes_exactly_one_attempt`), so they
  stop compiling until the new field is added to them, set to "nothing". That
  is the whole edit: no check in any test is changed, removed or weakened,
  and every test must still pass. Recommended: allow it. FZ-10a is not
  dispatched before you say yes, as with FZ-06b.

What you will see or wait for, all of it, in plain language:

- **A low allowance slows the dashboard.** When the account's hourly GitHub
  allowance runs low (shared with every fleetz window, `gh`, agents and
  anything else using your login), automatic polling slows down; near the
  end it stops until the hour resets. While slowed, CI lamps update at the
  slower pace, a running CI job no longer refreshes every 15 s, and a push
  you just made waits for the next poll instead of triggering one. On the
  owner's usual load (two or three windows) this does not happen: the first
  measurement had about 3500 points left.
- **When GitHub rate limits**, fleetz stops asking until the limit lifts:
  for the hourly limit that is when the hour resets, and fleetz resumes
  within half a minute after it; for GitHub's short-term limit it is when
  GitHub says to come back, usually a minute or two. Today it keeps asking
  every 30 s. With O2 accepted, `r` and `R` also do not ask (above).
- **After repeated GitHub errors** (server errors, timeouts, odd answers)
  fleetz waits longer between tries, up to about twelve minutes. `r` always
  tries at once.
- **Signed out of `gh`**: fleetz notices a new `gh auth login` within one poll
  interval, without a keypress.
- **Sign-in refused by GitHub** (an expired or revoked token): fleetz retries
  on the same growing wait, up to about twelve minutes, so a brief refusal
  clears itself; after signing in again, press `r` to retry at once, or wait
  up to about twelve minutes.
- **`R` while a reload is running** is ignored, so mashing it no longer
  starts several reloads. With O2 accepted, `R` during a rate limit is also
  held off while a GitHub refresh is still out, and says so.
- **Network down**: a connection that cannot be opened now shows `offline`
  rather than `timed out`.
- **No new settings**, and nothing on disk changes.
