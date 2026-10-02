# GitHub coverage and cache isolation design (FZ-09)

Status: **revision 2**, for
[FZ-09 / #22](https://github.com/Gaurgle/fleetz/issues/22), answering the
first review (findings 1 to 12 and its optional items). **Architectural
decisions C1 to C9 taken by the coordinator (`claude-fable-5-1`) on
2026-10-01**, as recommended in revision 1, with the review's conditions
([Decisions](#9-decisions)). Anything that changes a file on disk or what the
user sees or waits for is an **owner item**, presented to Andreas together
([Owner items](#owner-items)). **He accepted them all as recommended on
2026-10-02**, which includes adopting the keyed, versioned cache file below
(O1, O2) and the contract and invariant changes of section 6. The words and
look named in O3 and O4 are still judged on the builds that draw them.

Written against `main` at `769c1fd` and revised at `aa66577`; production code
was read, not changed, and nothing was built. `src/github.rs`, `src/cache.rs`
and `src/contributions.rs` are unchanged since `6794962`, the base of the
FZ-10 measurements. `src/main.rs` line numbers are at `aa66577` (FZ-06c moved
them); the rest are at `769c1fd`. They drift; follow the symbols.

Every claim carries one label, as in [api-budget-design.md](api-budget-design.md):

- **Measured**: from a request in [section 3](#3-measurements) (R1 to R6), from
  the FZ-10 requests (M1 to M9 in [api-budget-design.md](api-budget-design.md)),
  from the structure of the owner's cache file (counts and ages only, no
  names), or from the reviewer's local `gh` runs where said.
- **Read**: from the code at the base.
- **Documented**: published behaviour with a link, not verified here.
- **Inferred**: reasoning from the above, not observed.

## Summary

One GraphQL request fills one cache file, and that file holds every GitHub
product fleetz shows: the viewer's repo page, the org fallback pages, open PR
counts and the first five PRs, the CI rollup and one check suite per repo, and
four contribution calendar windows plus the rotating `recent` window (Read).
The file is `github-{repo_limit}.json` in the platform cache directory,
written with a plain `fs::write`, keyed by `repo_limit` alone, so every
account and every host share it (Read).

Coverage is incomplete on the owner's own fleet and nothing says so. GitHub
reports 95 repos on the viewer connection; fleetz fetches 50 (Measured, R3).
The header's only coverage warning, `window_exceeds_dataset`, counts the
merged list after the bare-name dedupe (55 entries, Measured) and takes its
oldest date from the union with the org pages, which reaches 2060 days back
while the viewer page stops at 237 (Measured). So at a one-year window 4 repos
pushed within that year are missing with no warning, at two years 35, and with
`all` (no cutoff) the warning can never fire (Measured counts, Read logic).

Coverage can be known for free: `totalCount` and `pageInfo { hasNextPage
endCursor }` on the three connections leave the price at cost 3 and 1030
nodes (Measured, R1 against R2, dry runs, not charged). Fetching the missing
45 is a second page at cost 1 and about 7 s (Measured, R4).

The design:

- **Coverage facts in the payload** (free), a header note that is right, rows
  a successful poll did not cover marked as such instead of silently keeping
  old values, and the `-p` PR section saying "5 of 8" when it lists five.
- **No pagination by default.** One request stays one request. Re-fetching the
  tail on demand is recorded, not promoted.
- **A keyed cache file**: format version, host (`github.com`, the same string
  as the identity design's `RemoteId` host) and an account key derived from
  the `gh` token (the same identity FZ-10 uses for its budget), plus
  `repo_limit` as today. What the account part of the name is made of is the
  owner's choice (O1).
- **Atomic writes** (a uniquely named temporary file and a rename), files
  readable only by the user, which needs nothing from anyone and can go first.
- **Migration** that keeps "launch paints from the cache" where it is safe: the
  old file is read only while no account has a keyed file yet.
- **A deadline fallback** (a smaller page after deadline-shaped failures),
  designed, not promoted.

Six bounded tickets, FZ-09a to f. FZ-09a (atomic write, `src/cache.rs` only)
and FZ-09f (`src/ui/detail.rs` only) are free to start now.

### Before the owner: what changes in accepted contracts

Detail in [section 6](#6-contract-and-invariant-changes). Written against the
freshness contract as amended on 2026-10-01 (its "Amendments" section).

| Item | Today | After FZ-09 |
|---|---|---|
| `CLAUDE.md` "Launch paints from the cache": key | Keyed by `repo_limit` alone | Keyed by format version, host, account and `repo_limit`; still never by `-p` |
| `CLAUDE.md` "Launch paints from the cache": what launch can paint | Any cached payload | The current credential's saved copy. A credential with **no saved copy paints nothing** and waits for GitHub; the old unkeyed file only while no account has a keyed file |
| `CLAUDE.md` "Launch paints from the cache": launch path | No subprocess before the cache read | One `gh auth token` lookup (about 0.05 s, FZ-10's measurement), bounded at 2 s, before the cache read |
| Freshness rule 9, amendment 2 | No `Pending` floor while failing or while the pace is stretched | Also: only rows covered by the last poll count toward the `Pending` floor |
| Freshness transitions | "Poll succeeds: GitHub-owned fields replaced" (in fact: of matched rows only); "`R` fails, cache exists: rebuilt from cache" | Matched rows replaced, carrier rows the poll did not return marked; "cache exists" means this credential's saved copy; a poll on another account rebuilds the rows |
| Freshness "What the user sees" | Header says fresh, cached or stale | Adds a coverage note (O3) and marked rows (O4) |
| FZ-10 4.1 "token id, never stored or logged" | In memory only | Stored as the cache file's account fingerprint if the owner keeps K1 (O1); never the token, never logged |

### Decisions

Architectural, decided by the coordinator on 2026-10-01 (detail in
[section 9](#9-decisions)).

| ID | Decision |
|---|---|
| C1 | Account key K1: a pinned FNV-1a 64 fingerprint of the token, shared with FZ-10's `token_id`, under the conditions of review findings 2, 6, 7 and 11. What sits on disk (K1, K2 or K4) is the owner's (O1) |
| C2 | FZ-10a's new fetch field is one struct with a `Default` (`FetchMeta`); FZ-09c adds the account key to it, FZ-09b the coverage |
| C3 | `LoadResult` gains one field, `Option<FetchMeta>`, in FZ-09b: `None` for a window apply and a rebuild from memory, stored only when `Some` |
| C4 | Superseded cache files deleted after 30 days unwritten, under the safety rules of 4.5 |
| C5 | No pagination by default |
| C6 | Smaller page after deadline failures (FZ-09e), not promoted |
| C7 | Partial responses stay failures until one is observed |
| C8 | Order: FZ-09a and FZ-09f now; FZ-10a, FZ-09c, FZ-09b, FZ-10b, FZ-10c, FZ-09d after the identity series |
| C9 | `gh auth token --hostname github.com` |

## 1. What is fetched and what is cached today

All Read unless marked.

### 1.1 The one payload

`github.rs::build_query` (431) builds one query; `accept_response` (744)
writes its raw body to the cache when it carries `data` and no `errors`;
`parse_cached` (569) and `split_viewer` (594) turn a body into a
`GithubFetch`. The body is stored exactly as GitHub sent it, so everything in
the table is one file.

| Product | Query field | Bound | Truncation | Today's coverage fact |
|---|---|---|---|---|
| Viewer repos (name, push time, privacy, owner) | `viewer.repositories(first: repo_limit, orderBy: PUSHED_AT DESC, affiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER])` | `repo_limit`, default 50 (`config.rs` 104) | Repos older than the 50th push are absent | None: only the node count |
| Org list | `viewer.organizations(first: MAX_ORGS)` | 8 (`MAX_ORGS`, 426) | A ninth org contributes nothing | None |
| Org repos | each org's `repositories(first: ORG_REPO_LIMIT, orderBy: PUSHED_AT DESC)` | 12 per org (427) | An org's 13th repo is absent | None. The `MAX_ORGS` doc says "overrunning either page drops repos silently" |
| PR count | `pullRequests(states: OPEN, first: 5) { totalCount }` | Complete | None | `pr_count` is `totalCount` |
| PR list | the same connection's `nodes` | 5 | The 6th and later open PRs | Derivable (`totalCount` against the node count), not used |
| CI state | `defaultBranchRef.target.statusCheckRollup.state` | Complete aggregate for the default branch head | None | n/a |
| CI workflow name and duration | `checkSuites(first: 1)` | 1 suite | Describes one suite of possibly many; which one is GitHub's default order (Inferred: not necessarily the latest) | None |
| Contribution calendars | `y0` to `y3`, `recent`: `contributionsCollection(from, to)` | Windows cover the grid by construction (`the_query_covers_every_year_of_the_grid`) | None | n/a |
| Issues | Not fetched | n/a | n/a | FZ-07 would add them; the coverage rule below applies from day one |

Not cached, for completeness: the budget reading FZ-10a adds (memory only,
[api-budget-design.md](api-budget-design.md) 4.1), the window state
(`window.toml`, local, in the same directory as the cache file), and
everything git-derived.

The calendars and the viewer repos are **per account** (the `viewer`), so the
whole file is an account's data, not only the repo rows (Read).

`Viewer` deserializes the calendar windows through a `#[serde(flatten)]` map
of `ContributionsCollection` (`github.rs` 159 to 160), so any new field at the
viewer level lands in that map and must be declared on `Viewer`, or the whole
payload fails to parse (Read; Inferred consequence). Fields on a connection
(`RepositoryConnection`, `OrganizationConnection`) have no such map and an
undeclared one is ignored (Read).

### 1.2 The owner's fleet

Counts from the owner's cache file `~/Library/Caches/fleetz/github-50.json`
(123683 bytes, mode 644, written 18:41:39Z on 2026-10-01) and from R3 and R4.
Read only; no names copied. The reviewer recounted the file and found the
same.

| Fact | Value | Label |
|---|---|---|
| Viewer page | 50 of 50 asked: full | Measured (file) |
| Viewer `totalCount` | 95; `hasNextPage` true | Measured (R3) |
| Orgs | 4 of `MAX_ORGS` 8; `totalCount` 4, `hasNextPage` false | Measured (file, R3) |
| Org pages | 6, 4, 4 and 3 of 12; each `totalCount` equal to its nodes | Measured (file, R3) |
| Org repos not on the viewer page | 9 of 17; none of the 17 is on page 2 | Measured (R3, R4) |
| Merged list after the bare-name dedupe | 55 entries; by owner and name, 59 (4 pairs of same-named repos with different owners on the viewer page) | Measured (file) |
| Viewer page boundary (oldest push on page 1) | 237 days ago | Measured (file) |
| Oldest entry in the merged list | 2060 days ago (5 org repos are older than the boundary) | Measured (file) |
| Page 2 | 45 repos, pushed 303 to 2823 days ago; 4 within 365 days, 35 within 730 | Measured (R4) |
| Open PRs | No repo above 2, so the 5-PR page truncates nothing today | Measured (file, R4) |

### 1.3 What the user sees when a bound is hit

- **Viewer page** (hit today). Repos older than the boundary are not rows at
  all, not even "not cloned". A local clone of one is not wanted in pass 1 or
  2 (`Wanted` is built from the window-filtered dataset, `main.rs` 2622), so it
  shows only in "local changes" when dirty, with no GitHub data. The header
  note `(window exceeds fetched repos, repo_limit=50)` appears only under the
  rule in 1.5, which misses most of the cases (Read; counts Measured).
- **Org list or an org page** (not hit today). The repos reachable only
  through the ninth org, or past an org's twelfth repo, are absent. Nothing on
  screen says so (Read).
- **PR list** (not hit today). The row badge and the detail header use
  `pr_count` (`totalCount`) and are right. With `-p`, the PR section's label
  is built from `repo.prs.len()` (`ui/detail.rs` 1047 to 1051) and says "5
  open PRs" when there are 8 (Read).
- **Check suites**. The workflow name and duration describe one suite; the CI
  lamp uses the rollup and is right (Read).

### 1.4 Rows a poll does not cover

`apply_github_poll` (`main.rs` 1749) writes GitHub fields only on rows that
find a same-named entry in the new dataset (1797 to 1804), then replaces
`app.dataset` (1820). A row whose repo is not in the poll keeps its old CI,
PRs and kind for the rest of the session, and nothing marks it (Read). It
happens when:

- the repo fell below the viewer page since the data the rows were built from
  (a cache-served launch from yesterday's file, or a long session in which
  newer pushes push it out);
- the repo was deleted, made inaccessible, or its org left the first 8;
- the token now belongs to another account (2.2).

Two side effects (Read): `github_poll_secs` (1946) holds the 15 s floor while
**any** row is `Pending`, so a row frozen at `Pending` keeps the floor for the
whole session; and a later window apply rebuilds from the new `app.dataset`,
so the same row then disappears or loses its match, an inconsistency between
the poll and the window path. Saying which rows were not covered needs the row
key from FZ-05d ([repository-identity-design.md](repository-identity-design.md)
5.4).

### 1.5 `window_exceeds_dataset`

`github.rs::window_exceeds_dataset` (907) returns true when the dataset has at
least `repo_limit` entries and the cutoff is older than the oldest entry. It
is called once, in `assemble_view` (`main.rs` 2821), only when there is a
cutoff (Read). The dataset it receives is the merged list (viewer plus org
pages, deduped by bare name). Three errors follow (Read; numbers Measured on
the owner's fleet):

1. **The oldest date is the union's, not the viewer page's.** The truncation
   point is the viewer boundary (237 days); org repos reach 2060 days. Any
   cutoff between the two gives no warning although page 2 has repos in it:
   4 missing at one year, 35 at two.
2. **No cutoff, no warning.** `all` (`Mode::All`) passes no cutoff, so it
   never warns, with 45 repos missing.
3. **The count is the wrong count.** The merged list can reach `repo_limit`
   when the viewer page is not full (40 viewer repos plus 17 org repos is 57),
   which warns on a complete fetch; and the bare-name dedupe can take a full
   viewer page below `repo_limit`, which suppresses the warning. FZ-05e moves
   the dedupe to owner and name, which narrows the second case but not the
   first.

### 1.6 Partial responses

A GraphQL response may carry both `data` and `errors`
([GraphQL spec, Response](https://spec.graphql.org/October2021/#sec-Response-Format),
Documented). `accept_response` checks `errors` first and turns any non-empty
`errors` into an `Api` failure, so such a body is neither applied nor cached
(Read). Whether GitHub answers this way for an org that enforces SAML SSO on a
token that is not authorized for it is not established here (Inferred, not
observed; R1 to R6 carried no `errors`). If it does, an affected user sees
`⚠ github` on every poll and the data never refreshes.

## 2. Cache identity today

### 2.1 Layout and key

- Directory: `dirs::cache_dir()/fleetz` (`cache.rs` 28), which is
  `~/Library/Caches/fleetz` on macOS (Measured: the owner's file is there).
  When `dirs::cache_dir()` is `None`, the code falls back to the literal
  relative path `~/.cache` (no tilde expansion), so the cache lands under the
  current directory (Read). The doc comment on `ApiCache` says
  `~/.cache/fleetz/github.json`, which is neither the directory on macOS nor
  the file name (Read).
- File: `github-{repo_limit}.json` (40), created with the process umask (644
  on the owner's machine, Measured). Content: the raw response body. Write
  time (mtime) is the data time (`read_stale`, 75; freshness contract,
  Vocabulary).
- `ApiCache::new` (27) also deletes three obsolete names on every call
  (`github.json`, `github-{n}-prs.json`, `github-{n}-noprs.json`); it is
  called on every poll (`spawn_github_poll`, `main.rs` 2245) and every load
  (2310) (Read).
- Reads: `read` (TTL) and `read_stale` (any age); `CacheMode::AnyCached` tries
  both, so `cache_ttl` decides nothing observable (Read; FZ-10 found the
  same).

### 2.2 Cases

| Case | What happens today | Label |
|---|---|---|
| Account switch in `gh` (`gh auth switch`, `gh auth login` as another user, `GH_TOKEN` set in one shell) | The next launch paints the previous account's repos and contribution calendar from the shared file. The first poll uses the new token and overwrites the file. Rows from the old account that the new dataset lacks keep their GitHub fields until `R` (1.4) | Read; `GH_TOKEN` precedence Documented ([gh environment](https://cli.github.com/manual/gh_help_environment)) and Measured by the reviewer on `gh` 2.98.0 |
| A second host | fleetz only talks to `api.github.com` (`request`, 696), so every payload is github.com data. `get_gh_token` runs `gh auth token` with no `--hostname`. With `GH_HOST` set to another host, that lookup fails and fleetz reports `Auth`; `gh auth token --hostname github.com` returns the github.com token and honours `GH_TOKEN` | Read (fleetz); Measured by the reviewer on `gh` 2.98.0 ([gh auth token](https://cli.github.com/manual/gh_auth_token)) |
| Corrupt or truncated file | `parse_cached` fails, so it counts as no cache: launch waits on the network (6 to 7.5 s), and an offline launch or an offline `R` gets the error panel because the fallback is empty. The file stays until the next successful write | Read |
| Torn write | `fs::write` truncates, then writes about 120 KB. A launch in another instance between the two reads a prefix, which is the corrupt case above. A process killed mid-write leaves the prefix on disk | Read; likelihood Inferred (small window) |
| Two or three instances | All on the same account and `repo_limit` share one file; each polls and writes it (A4 in FZ-10). A launch or `R` fallback reads whichever wrote last, which is harmless while they share an account and torn-write-prone otherwise. Instances on different accounts overwrite each other's data | Read |
| Different `repo_limit` | Separate files; never read across | Read |
| Format change between versions | The body is the query's shape. Additive fields parse through serde defaults, and `fetch_repos`'s `usable` filter rejects a payload without a calendar or PR counts. Nothing marks the format, so an incompatible change (a renamed alias, a moved field) would parse into wrong data silently, and two binaries with different query shapes (the installed release and a `cargo run` from a worktree, which the owner runs side by side) rewrite one file in turn | Read; side-by-side running Measured by FZ-10 (three instances during M1) |
| Tests and the real directory | `cache.rs::cache_path_varies_by_query_shape` calls `ApiCache::new`, so `cargo test` runs the three deletions above against the owner's real cache directory. Harmless today (the names are obsolete); it means **no new deletion may go into `ApiCache::new`** | Read |

## 3. Measurements

Six live GraphQL requests, each started by hand, none in a loop, on
2026-10-01, with the same token fleetz reads (`gh api graphql -i -F
query=@file`). The query was rebuilt with FZ-10's reconstruction of
`build_query` (scratch only, `src/` untouched); the variants change only what
the table says. `rateLimit` was added beside `viewer`, as in FZ-10.

| # | Time (UTC) | Query | Dry run | HTTP | cost | nodeCount | `remaining` after | Wall |
|---|---|---|---|---|---|---|---|---|
| R1 | 18:44:26 | Fleet query as sent, `repo_limit` 50 | yes | 200 | 3 | 1030 | 4401 | 0.47 s |
| R2 | 18:44:26 | R1 plus `viewer.login`, and `totalCount` and `pageInfo` on the viewer repos, the org list and each org's repos | yes | 200 | 3 | 1030 | 4401 | 0.46 s |
| R3 | 18:44:40 | R2 for real | no | 200 | 3 | 1030 | 4398 | 6.92 s |
| R4 | 18:44:56 | Viewer repos only, `first: 50, after:` R3's cursor, same repo fields, no calendars, no orgs | no | 200 | 1 | 350 | 4396 | 7.17 s |
| R5 | 18:45:09 | Calendar windows only (`y0` to `y3`, `recent`) | no | 200 | 1 | 0 | 4392 | 1.89 s |
| R6 | 18:45:20 | Viewer repos only, `first: 25`, with the coverage fields | no | 200 | 1 | 175 | 4390 | 3.30 s |

`remaining` before R1 was 4401 (R1 was a dry run and its own `used` read 599,
unchanged by R2). After R6 it was 4390. Charged to these requests: 3 + 1 + 1
+ 1 = 6 points; the other 5 came from other clients on the token during the
same minute (Measured: `used` moved by 2 across R4, whose cost was 1).

What they establish:

- **Coverage facts are free**: adding `totalCount` and `pageInfo` to the
  three existing connections, and `viewer.login`, leaves cost 3 and 1030
  nodes (R1 against R2), and the real query costs 3 (R3). Consistent with the
  invariant that a field without a new `first:` is free. `poll_cost` in
  FZ-10 does not change.
- **Dry runs were not charged**: `used` stayed at 599 across R1 and R2, a
  second observation after FZ-10's M6.
- **The owner's coverage**: 50 of 95 viewer repos; orgs complete (R3).
- **A second page costs 1 point and about 7 s** (R4).
- **The repo pages, not the calendars, take the time**: calendars alone 1.89
  s (R5); 25 repos 3.30 s (R6); 45 older repos 7.17 s (R4); the full query
  6.92 s (R3), against 6.26 s, 6.73 s and about 7.3 s in FZ-10. Server time
  per returned repo varied between about 0.07 and 0.16 s across these runs;
  one run each, so a range, not a rate (Inferred).

Not rerun: the price of `repo_limit` 20 and 100, the 502 at 100, the cost of
the org fallback and the header behaviour of the budget (FZ-10, M1 to M9).

The worker's own local check of `gh auth token` under `GH_HOST` was refused by
the session's permission policy (it would print a credential). The reviewer
ran the equivalent checks on `gh` 2.98.0; their results are the second-host
row in 2.2 and the basis of C9.

## 4. Options and decisions

Each subsection keeps the alternatives that were weighed and records the
coordinator's decision of 2026-10-01 (C1 to C9). Where a choice is the
owner's, it says so.

### 4.1 Coverage indicators

What is knowable for free (Measured, R2): per connection, `totalCount` and
`hasNextPage`. The boundary of a page is its last node's push time, already in
the payload (both connections are ordered by push time, Read). PR coverage is
already in the payload (`totalCount` against the nodes).

| Option | Verdict |
|---|---|
| **A. Ask `totalCount` and `pageInfo { hasNextPage }` on the viewer repos, the org list and each org's repos; derive each page's boundary; carry a coverage record on the fetch** | **Decided.** Free (R2). The payload describes its own coverage, so a cached payload, a degraded payload (4.6) and a fresh one are read the same way |
| B. Infer coverage from counts | Not as the main rule: "50 of 50" cannot tell a full page from an exact fit. Used only for a payload written before A (below) |
| C. A separate `totalCount` query | Rejected: a request for what a field gives free |

The coverage record holds, per page: returned, `totalCount`, `hasNextPage`,
and the boundary. It is built from the pages **before** `merge_sources`, so
FZ-05e's dedupe does not touch it. A payload without the fields has
`totalCount` and `hasNextPage` **unknown**, never complete. No viewer-level
field is added (1.1: it would have to be declared on `Viewer`), and
`checkSuites { totalCount }` and `endCursor` are left out (fewer query changes
to price).

The header rule, replacing the call in `assemble_view` in production. The
note shows when any of:

- the viewer page has `hasNextPage`, and there is no cutoff or the cutoff is
  older than the viewer boundary;
- an org page has `hasNextPage`, and there is no cutoff or the cutoff is older
  than that org page's own boundary;
- the org list has `hasNextPage` (always: nothing says how recent the missing
  orgs' repos are).

For a payload without coverage fields, the note follows
`window_exceeds_dataset`, unchanged, called on the **viewer page before the
merge** (its nodes, its boundary, `repo_limit`), not on the merged list. That
removes errors 1 and 3 of 1.5 for old payloads too; error 2 stays for them
(the function needs a cutoff), and goes once the payload carries the fields.
`window_exceeds_dataset` keeps its signature and its test. The words are the
owner's (O3); the information is "fetched N of M", and which kind (your
repos, org repos, orgs).

The `-p` PR section label uses `pr_count` with the listed count when they
differ ("5 of 8 open PRs"), an optional ticket (FZ-09f, O8).

### 4.2 Pagination (C5, decided)

The [ideas.md](ideas.md) section "The one real caveat: repo_limit" (under the
shipped "Interactive time scope") recommended shipping a cap plus a note, and
adding "re-fetch on widen" only if it bites. Nothing measured here overturns
that; what is new is that the note is wrong (1.5) and that the gap is 45 repos
on the owner's fleet.

| Option | Cost at the owner's fleet (Inferred from R3, R4) | Verdict |
|---|---|---|
| **A. No pagination; the coverage note** (4.1) | None | **Decided (C5)** |
| B. Raise `repo_limit` | 100 failed with 502 after about 11 s, twice (FZ-10, M3, M7) | Rejected: does not answer |
| C. Paginate the viewer connection on every poll | +1 point and about +7 s per page per poll: 4 points instead of 3, 480 points an hour per instance at 30 s instead of 360, and a poll that takes about 14 s | Rejected as the default: a third more budget for repos that by construction were pushed least recently |
| D. Fetch the tail on a slow cadence (for example every tenth poll) | +1 point per page per tenth poll | Not now: a second dataset with its own freshness, which the freshness contract does not have |
| E. Re-fetch on widen: when the window or `all` reaches past the boundary and `hasNextPage` is true, fetch the remaining pages once and keep them until `R` | +1 point and about 7 s per page, only when asked | **Recorded for later** (O9). It is the ideas.md option 2. Needs a second, page-keyed cache product and a decision on how the tail ages, so it is not in a ticket here |

Pagination by page is not the per-repo split the "batch on GraphQL" invariant
forbids: each page still batches every repo on it. It does add requests, so
any pagination ticket must price itself with FZ-10's procedure.

### 4.3 Cache key: account, host and format version (C1, C9, decided; O1 accepted)

The file name gains three parts. Example:
`github-v2-github.com-2a8d4ab865f3ca20-50.json`.

- **Format version** (`v2`; the current file is v1 by implication). Bumped
  only when an older payload would be misread, not when a field is added
  (serde defaults and `usable` already cover additions, Read). A bump means
  one cold launch (4.5).
- **Host**: the lowercased host string the identity design uses for
  `RemoteId` ([repository-identity-design.md](repository-identity-design.md)
  3.1, 3.4), `github.com` today, since the only endpoint is `api.github.com`.
  One function produces it for both. A host string that is not plain
  `[a-z0-9.-]` (none today) would be hashed rather than written raw (Inferred
  safety rule).
- **Account**: a fingerprint of the token, the same identity FZ-10's
  `token_id` keys the budget by (C1). Specified below.
- **`repo_limit`**: as today.

**One token lookup per attempt** (review finding 6). The lookup that picks the
file is the lookup whose token goes into the `Authorization` header of that
attempt's request, if one is made. Two lookups could straddle an account
switch and write one account's answer into the other's file. The token source
is injectable (a function or trait object passed down, defaulting to `gh`), so
tests drive it without a subprocess.

**The lookup before the launch read is bounded at 2 s** (finding 7). Launch
waits on it before it can pick a file; `gh` reading a keychain can stall. On
timeout or failure, the signed-out rule below applies. The background poll's
lookup is not bounded here; FZ-10's 30 s client deadline and the 60 s give-up
already cover it.

**The token host** (C9): `get_gh_token` asks `gh auth token --hostname
github.com`, so the token is the one for the host the request goes to.
Measured by the reviewer on `gh` 2.98.0: the flag works and honours
`GH_TOKEN`. Visible effect: with `GH_HOST` set to another host, fleetz gets
its github.com token instead of failing as `Auth`.

**The fingerprint** (K1, specified byte for byte). FNV-1a 64 over the raw
UTF-8 bytes of the prefix `fleetz-account-v1:` followed by the token with
leading and trailing whitespace trimmed; offset basis `0xcbf29ce484222325`,
prime `0x100000001b3`, wrapping multiplication; written as 16 lowercase hex
digits, zero-padded. Computed directly over the bytes, **not** through
`std::hash::Hash`/`Hasher` (whose `str` impl appends a terminator byte, Read
from std, and whose `DefaultHasher` is not stable across Rust releases,
[std docs](https://doc.rust-lang.org/std/collections/hash_map/struct.DefaultHasher.html),
Documented). Test vectors (Measured with a scratch implementation, checked
against the published FNV-1a 64 values for the empty string,
`cbf29ce484222325`, and `a`, `af63dc4c8601ec8c`): the dummy token
`gho_EXAMPLEexampleEXAMPLEexample0000` gives `2a8d4ab865f3ca20`, with or
without surrounding whitespace; an empty token gives `5fccb9a3d0a45785`.

What the fingerprint protects and what it does not (finding 11). FNV-1a is
not a cryptographic hash: anyone holding a candidate token can compute its
fingerprint and compare it with the file name. It cannot be turned back into
the token: 64 bits in a name against about 178 bits of randomness in a
current `gh` token (160 in an older 40-hex-digit one), so the name narrows
nothing an attacker could search (Inferred from the token formats). The files
are readable only by the user (4.4).

Options for the account part. C1 is K1; **what sits on disk is the owner's
choice** (O1):

| Option | Before the first paint | On a token rotation (same account) | After an account switch | On disk | Notes |
|---|---|---|---|---|---|
| **K1. Fingerprint of the token** (above) | One `gh` lookup, about 0.05 s, bounded at 2 s | One cold launch (6 to 7.5 s to the first useful row); offline, nothing to show until GitHub answers | Never shows the other account's data | 64 bits derived from the token, in a file name | **Coordinator's choice (C1).** Same identity as FZ-10's `token_id` |
| K2. The account login, from `viewer.login` (free, R2); the newest file for the host served at launch, the first poll checking the login | No subprocess | No cold launch | Shows the previous account's data until the first poll after the switch lands, or for as long as GitHub is unreachable; then FZ-09d's rebuild replaces it | Only the login, which is public | `viewer.login` must be declared on `Viewer` (1.1) |
| K4. K1 with SHA-256 instead of FNV-1a | As K1 | As K1 | As K1 | A cryptographic digest | A new dependency (owner approval). Same isolation as K1; differs only in being a standard one-way function |
| K3. The login read from gh's own config (`hosts.yml`) | A file read | No cost | Wrong under `GH_TOKEN` | The login | Rejected: gh's file format is private, and `GH_TOKEN` overrides the stored account |

**Signed out, or the lookup timed out, at launch**: no account is known. Serve
the newest current-version keyed file for the host and `repo_limit` (the
legacy file only under 4.5 step 2), as `DatasetSource::Cache` (no attempt was
made, so freshness rule 1 holds); the account key of the load is **unknown**
(4.7). The attempt that follows does its own token lookup. Signed out, it
gets no token and fails as `Auth`, as today. After a lookup that only timed
out, it may get a token and succeed, and then its known account key triggers
the rebuild of 4.7. On a machine where one person uses two accounts this can
show the other account's data until sign-in or that first poll (O7).

Rejected: an envelope in the file (a JSON wrapper holding account, host and
version beside the body). It changes the content every existing reader
parses, breaks the existing test `github.rs::a_payload_with_data_is_cached`
(it asserts the file equals the body byte for byte), and cannot be read
without reading the whole file, which a name can.

### 4.4 Atomic writes

Every write gets its own temporary file in the same directory, named from the
target, the process id and a process-wide counter (an `AtomicU64`), for
example `.github-v2-...-50.json.tmp-{pid}-{n}`, opened with `create_new`
(review finding 1). Within one process `R` and a background poll can write at
once; a shared name would let one writer's half-written file be renamed into
place by the other. Then `rename` over the target. A rename within one
filesystem replaces the name atomically
([POSIX rename](https://pubs.opengroup.org/onlinepubs/9699919799/functions/rename.html),
Documented), so a reader sees the old body or the new one, never a prefix.

On Unix the temporary file is created with mode `0600`, so the renamed cache
file is readable only by the user (today 644, Measured). The rename keeps it.

The renamed file's mtime is the temporary file's write time, so the data time
stays "when the answer was written" (Inferred from rename semantics; pinned by
the existing test `cache.rs::a_stale_read_reports_when_the_file_was_written`
through the same `write`). A failed write or rename leaves the old file and
removes the temporary one; errors stay ignored, as today, since a cache write
must never fail a poll. `fsync` is not proposed: the cache is a convenience,
and a power loss costs one cold launch (Inferred).

Two writers at once, in one process or two: the last rename wins and the file
is always one complete body. Its mtime can be a few seconds later than the
instant its data left GitHub when the slower writer's request started earlier
(Inferred); data time is at most that far off, and never moves into the
future.

Temporary files left by a crash between write and rename are removed at the
next launch when older than one hour and matching the temporary pattern, from
the production startup path in FZ-09c, never in `ApiCache::new` (2.2, last
row).

### 4.5 Versioning and migration (C4, decided)

One read rule at launch (`CacheMode::AnyCached`), in order:

1. The current-version file for this host, account and `repo_limit`.
2. Otherwise, the legacy unkeyed `github-{repo_limit}.json`, **only when no
   keyed file of any account exists for this host and `repo_limit`** (review
   finding 2). That is the first launch after the upgrade, and it is what the
   old version would have painted, so no regression. Once any account has
   saved a keyed file, the legacy file is never read again by the new
   version, whatever an old binary keeps writing into it.
3. Otherwise, no cache: the request, as today. A credential with no saved
   copy paints nothing at launch.

There is no step for an older keyed version (finding 3): a format bump exists
precisely because an older payload would be misread, and `usable` cannot
detect that. A bump costs one cold launch.

Step 2 serves launch only. A fallback after a failed attempt
(`settle_attempt`) and `R` read only step 1's file (finding 4): a failure on
this credential must not show another account's data. So an `R` that fails,
or a launch whose legacy file is unusable and whose request fails, with no
keyed file for this credential, has no fallback: the load fails and says so.
Today an offline `R` with any cache file says it reloaded from the cache
(O12). Writes go only to the current-version keyed file.

Cleanup (C4). Delete a file when all hold (finding 10):

- it matches the explicit pattern `github-{n}.json` (legacy) or
  `github-v{k}-{host}-{fingerprint}-{n}.json` with `k` older than current, or
  with the current `k` and another fingerprint; nothing else in the directory
  is touched (`window.toml` lives there);
- it has not been written for 30 days;
- a current-version keyed file for the **current** account and the same
  `repo_limit` exists (so cleanup does not run when the account is unknown:
  signed out, a timed-out lookup);
- it is not the file this launch just served;
- `dirs::cache_dir()` is `Some` (no cleanup under the relative fallback path
  of 2.1).

| Option | Effect | Verdict |
|---|---|---|
| **Delete under the rules above** | The installed old binary and a new `cargo run` coexist: the old one keeps its file alive by writing it. An account still in use keeps its file alive the same way. Disk cost: one 120 KB file per stale key for a month | **Decided (C4)** |
| Delete the legacy file after the first keyed write | The installed old binary launches cold every time after a new build ran, and rewrites the file, which the new build deletes again | Rejected |
| Never delete | Files accumulate per account and version, a few hundred KB | Rejected |

Downgrade: an older binary never reads keyed files and keeps reading its
legacy file while it exists (Read).

### 4.6 When the query no longer fits GitHub's deadline (C6, decided)

`repo_limit` 100 drew HTTP 502 after about 11 s, twice (FZ-10, M3, M7); the
current query takes 6.3 to 7.3 s; the repo pages, not the calendars, take the
time (R4 to R6). A query that no longer fits fails on every attempt, and
FZ-10's backoff only spaces the failures out.

| Option | Verdict |
|---|---|
| A. Nothing beyond FZ-10's backoff | Rejected: the dashboard stays stale with no way out |
| **B. Smaller page after deadline-shaped failures**: after two consecutive `Server` (5xx) or `TimedOut` failures that each took at least 8 s, the next attempts ask for half the page (50, 25, 12; floor 10). After 10 successes at the smaller size, one attempt at the configured size. The coverage record shows the smaller page, so the note says so (4.1). The payload goes to the same keyed file: it describes its own coverage | **Decided (C6), not promoted** (FZ-09e) until a deadline failure is seen at the configured size. Promoting it is the owner's (O13). Cost goes down, not up |
| C. Split into requests: calendars alone (1 point, 1.9 s, R5) plus repos in pages of 25 (1 point and about 3.3 s each, R6) | Fits with margin, at about +2 points per poll, and changes "one request" into several, which the "batch on GraphQL" invariant reads against. Kept as the step after B if B's floor is reached |
| D. Drop the org fallback into its own request | Rejected for time: without it the query took 6.73 s (FZ-10, M4); it is two thirds of the price, not of the time |

A fast 5xx (under 2 s) is an outage, not the deadline, and never shrinks the
page (Inferred rule). A `repo_limit` set above 50 in the config is the most
likely way to meet the deadline; FZ-09e also says in the README that 50 is
the size measured to fit.

### 4.7 Rows a poll did not cover, and an account change

After FZ-05d every carrier row has a remote key (`owner`, name; name alone for
a legacy row). On an applied successful poll:

- Carrier rows whose key is not in the new dataset go into a set on `App`
  ("not in the last poll"), keyed by the remote key. Their GitHub fields keep
  their values. A later poll that returns the key removes it. A failed poll
  changes nothing (freshness rule 4). No `RepoStatus` field is added, so no
  `RepoStatus` struct literal changes (Read: the identity design avoided a
  field for the same reason).
- **The set is cleared whenever rows are rebuilt from one dataset** (review
  finding 9): a launch, `R`, a window apply, FZ-10b's rebuild from memory and
  the account rebuild below. Rebuilt rows come from that one dataset, so none
  of them is "not in" it.
- Rows in the set do not hold the 15 s `Pending` floor (`github_poll_secs`).
- What the row shows is the owner's (O4).

**Account change while running.** The load records the dataset's
`FetchMeta`, and with it the account key, on `App`. `LoadResult` carries it
as `Option<FetchMeta>` (C3): `Some` for a load that fetched or read the cache,
`None` for a window apply and for a rebuild from memory, exactly as `dataset`
already is; `apply_load_result` stores it only when it is `Some`, so neither
of those resets coverage or the account key to unknown.

**The rebuild rule** (second review, finding 1). An applied successful poll
rebuilds the rows only when **the poll's account key is known** and either
differs from the load's key or the load's key is unknown. A poll carrying no
account key (a default `FetchMeta`, as every existing poll test builds)
never rebuilds. The rebuild goes **through FZ-10b's rebuild-from-memory
`LoadKind`** (review finding 8): discovery and statting from `app.dataset`,
no `git fetch`, no request, no change to `window.toml` or to the status line's
"Window:" message, which `spawn_window_apply` would make. So the previous
account's rows and calendar go at once (O5).

**An unknown account key on the load** (finding 9): a launch served from the
legacy file or under the signed-out rule has no known account, so the first
applied poll that carries a key rebuilds. A token rotation mid-session
changes the fingerprint without changing the account, which causes one
rebuild from data that is the same account's: harmless, and accepted (it
cannot be told apart without K2's login).

Rejected: clearing a not-covered row's GitHub fields. A repo that fell below
the page usually has a correct last value (it was pushed less recently than
50 others), and a blank lamp reads as "no CI", which is false.

### 4.8 Partial responses (C7, decided)

| Option | Verdict |
|---|---|
| **A. Unchanged: any `errors` is an `Api` failure, nothing applied or cached** | **Decided (C7)** until a partial answer is observed in use |
| B. Accept a body with `data` and `errors` when every error has a `path` under `organizations` and none is `RATE_LIMITED`: apply and cache it, and mark the affected org pages as not covered (4.1) | Designed, not promoted and not ticketed. Keeps "only a response carrying `data` is cached" literally, but changes what a cached payload may contain, so it is the owner's if ever promoted |

## 5. Every cached product: identity and coverage

The completion evidence for this ticket. "Identity" is what must match for
data to be shown; "coverage" is what tells complete from cut off.

| Product | Identity today | Identity after FZ-09 | Coverage today | Coverage after FZ-09 |
|---|---|---|---|---|
| Viewer repo page | File by `repo_limit`; rows by bare name, then `RemoteId` after FZ-05 | File by version, host, account, `repo_limit`; rows by `RemoteId` | Node count only; header rule wrong (1.5) | `totalCount`, `hasNextPage`, boundary; correct note |
| Org list | Same file | Same keyed file | None | `totalCount`, `hasNextPage` |
| Each org's repo page | Same file | Same keyed file | None | `totalCount`, `hasNextPage`, boundary per org |
| Repo metadata (push time, privacy, owner) | Per node, by name | Per node, by `RemoteId` (FZ-05) | Complete for returned nodes | Rows a poll did not return are marked (4.7) |
| PR count | Per node | Per node | Complete (`totalCount`) | Unchanged |
| PR list (5) | Per node | Per node | Derivable, unused; `-p` label wrong | "5 of 8" (FZ-09f) |
| CI rollup | Per node | Per node | Complete aggregate | Rows not covered marked, excluded from the `Pending` floor |
| CI workflow name and duration | Per node | Per node | One suite, order unspecified | Unchanged; recorded as a limitation |
| Contribution calendars (`y0` to `y3`, `recent`) | Same file, **any account's** | Keyed file: the account's own | Complete by construction | Unchanged |
| Budget reading (FZ-10) | Memory, `token_id` | Memory, the same fingerprint | n/a | n/a |
| Issues (FZ-07, not built) | n/a | Must ride the keyed file or key its own the same way | n/a | Must carry `totalCount` and `hasNextPage` from its first version |
| A tail of older repos (4.2 E, not built) | n/a | Its own product, keyed the same way plus a page key | n/a | Its own `hasNextPage` |

## 6. Contract and invariant changes

Each item changes text in an accepted document. The coordinator edits
`CLAUDE.md` and the contracts; no FZ-09 ticket does. All need the owner's
sign-off. Written against [freshness-contract.md](freshness-contract.md) with
its six FZ-10 amendments (accepted 2026-10-01).

**Freshness contract:**

| Rule | Change |
|---|---|
| Vocabulary, Attempt | Unchanged. The token lookup before a cache read is not an attempt (it is not a request) |
| Vocabulary, Data time | Unchanged. An atomic write keeps "the file's write time is the data time" |
| Vocabulary, failure kinds (amendment 3) | Unchanged by FZ-09 |
| Rule 1 (a cache read is not an attempt) | Unchanged, including a signed-out or timed-out launch served from the newest current-version keyed file (4.3) |
| Rule 2 (a fallback is a failure with data) | Unchanged in kind; the fallback reads only this credential's keyed file, so a credential with none has no fallback (4.5) |
| Rule 3 | Unchanged |
| Rule 4 (a failed poll changes no row) | Unchanged; a failed poll also changes no coverage mark |
| Rule 5 (a failure never touches the cache file) | Unchanged. Cleanup deletes only superseded files under the rules of 4.5, at launch, never on a failure path |
| Rule 6 and amendment 1 | Unchanged by FZ-09 |
| Rule 7, rule 8 | Unchanged |
| Rule 9 and amendment 2 | **Added to amendment 2**: the `Pending` floor counts only rows covered by the last poll (4.7) |
| Rule 10 and amendment 4 | Unchanged |
| Transitions, "Launch, cache served" | **Amended**: "from cache" means this credential's keyed file, or the legacy file while no account has a keyed file (4.5) |
| Transitions, "Launch, no usable cache" | **Amended**: includes a credential with no saved copy while other accounts' copies exist; nothing is painted from them |
| Transitions, "Poll succeeds" | **Amended**: "GitHub-owned fields of matched rows replaced; carrier rows the poll did not return marked as not in the last poll" |
| Transitions, new row | "Poll succeeds carrying a known account key that differs from the rows' key, or the rows' key is unknown": rows rebuilt from the poll's dataset (a poll with no account key never rebuilds), set of marks cleared, `data_time`, `last_attempt`, `outcome` and the cache file as for "Poll succeeds" (O5) |
| Transitions, "`R` fails, cache exists" | **Amended**: "cache exists" means this credential's keyed file. Without one, `R` fails with no rows rebuilt and says so (O12) |
| Transitions, amendment 5 | Unchanged; the rebuild from memory it adds also clears the marks (4.7) |
| What the user sees, and amendment 6 | **Added**: a coverage note (O3) and not-covered rows (O4) |
| Not in this contract | "Cache isolation by account and host (FZ-09)" and "partial coverage of the dataset (FZ-09)" point here once accepted |

**`CLAUDE.md` invariants:**

| Invariant | Change |
|---|---|
| "Launch paints from the cache, however stale" | **Amended**: "Keyed by `repo_limit` alone" becomes "keyed by format version, host, account and `repo_limit`". Launch reads **this credential's** saved copy after one `gh` lookup bounded at 2 s; a credential with no saved copy paints nothing and waits for GitHub; the unkeyed legacy file is read only while no account has a keyed file. "Never re-add `-p`" stays. (The stale names `last_github_poll` and `GithubFetch::from_cache` in the same paragraph are FZ-10's finding; the coordinator corrects them in the same edit) |
| "Never widen the GitHub fetch without checking the cache" | Kept. FZ-09b's fields were priced (R1, R2: no change) and follow FZ-10's procedure |
| "Only a response carrying `data` may be cached" | Kept; the write becomes atomic. Partial responses stay failures (4.8 A) |
| "GraphQL bills the `first:` you ask for" | Kept. Adds the measured fact that `totalCount` and `pageInfo` on an existing connection are free (R2) |
| "Batch on GraphQL, target on git" | Kept by FZ-09a to d and f. FZ-09e's step C would amend it if ever reached |
| "GitHub caches `contributionsCollection` against the `from` date" | Unchanged |
| "Status vocabulary is split by kind" | Kept: coverage is a counter ("50 of 95"), words are the owner's |

**Other designs:**

- [api-budget-design.md](api-budget-design.md) 4.1: "a `token_id` (a hash of
  the token ... never stored or logged)" becomes "the pinned fingerprint of
  4.3 here; never stored except as the cache file's account part, never
  logged". Issue #47 (FZ-10a) already carries this note. If the owner chooses
  K2 (O1), nothing derived from the token is stored and the amendment is not
  needed.
- [repository-identity-design.md](repository-identity-design.md) 3.8:
  "Per-account and per-host file keys are FZ-09" is answered here.

## 7. Regression scenarios

Every scenario runs without a network: planted payload bodies, planted cache
files in a temporary directory with planted mtimes, an injected token source,
and `App` states built through the existing helpers, as FZ-03's tests do.
"Boundary" is a page's oldest push. Ids are prefixed by area: CV coverage, CK
cache key, CW writes, CM migration and cleanup, CD deadline, CP partial
responses. **53 scenarios.**

| # | Given | Expected | Ticket |
|---|---|---|---|
| CV1 | Payload: viewer `totalCount` 95, 50 nodes, `hasNextPage` true, boundary 237 days ago; cutoff 300 days ago | Coverage note shown | b |
| CV2 | As CV1, plus 17 org repos, 5 older than the boundary (the owner's measured shape); cutoff 365 days ago | Note shown (today: not shown) | b |
| CV3 | As CV1, no cutoff (`all`) | Note shown (today: never) | b |
| CV4 | Viewer 40 of 40, `hasNextPage` false, plus 17 org repos (57 merged); cutoff older than every entry | No note (today: a false warning) | b |
| CV5 | Viewer complete; one org page 12 of 15, `hasNextPage` true, boundary 90 days ago; cutoffs 30 days, 120 days, none | No note at 30 days; note naming org repos at 120 days and with no cutoff | b |
| CV6 | Org list `totalCount` 10, 8 returned, every page complete; cutoff 1 day | Note naming incomplete orgs, whatever the cutoff | b |
| CV7 | Payload without coverage fields (today's file): 50 viewer nodes with boundary 237 days ago, 17 org repos reaching 2060 days; cutoffs 365 days and none | At 365 days the note shows: `window_exceeds_dataset` is called on the viewer page before the merge. With no cutoff, no note, as the function says. The coverage record's `totalCount` and `hasNextPage` stay unknown | b |
| CV8 | The query string | Has `totalCount` and `pageInfo` on the three connections and no new viewer-level field; existing query tests pass unchanged; price recorded in the ticket by FZ-10's procedure | b |
| CV9 | Rows for `alice/a` and `alice/b`; a successful poll returns `alice/a` only | `alice/b` is in the not-covered set and keeps its CI and PRs; a later poll returning it removes it | d |
| CV10 | As CV9, then a failed poll | The set unchanged (rule 4) | d |
| CV11 | `alice/b` frozen at `Pending` and not covered; every covered row `Passed` | Interval is the configured one, not 15 s | d |
| CV12 | Rows built from account A's dataset; an applied poll on account B | Rows rebuilt from B's dataset through the rebuild-from-memory `LoadKind`: no request, no `git fetch`, `window.toml` not written, no "Window:" status; no row carries A-only GitHub data; B's calendar shown | d |
| CV13 | A late answer on account A after B's applied (discarded by rule 10) | Triggers no rebuild | d |
| CV14 | `-p`, `pr_count` 8, five PRs listed | Section says 5 of 8 | f |
| CV15 | `alice/b` in the set; then each of: a launch, `R`, a window apply, a rebuild from memory, an account rebuild | The set is empty after each | d |
| CV16 | A launch served from the legacy file, or under the signed-out rule (account unknown); then an applied poll carrying account A's key | One rebuild from A's dataset | d |
| CV17 | Rows from account A under fingerprint F1; an applied poll on the same account under a rotated token, fingerprint F2 | One rebuild; the rows equal those the poll alone would give; no second rebuild on the next poll | d |
| CV18 | An `App` whose load's account key is unknown (or that never had a load); an applied successful poll whose `FetchMeta` is the default (no account key) | No rebuild; only the matched rows' GitHub fields change | d |
| CV19 | A load whose `FetchMeta` carries coverage (viewer 50 of 95, boundary 237 days ago); then a window apply to 365 days | The window apply's `LoadResult` carries `None`; `App` keeps the load's `FetchMeta`; the note shows for 365 days | b |
| CV20 | A load on account A; a window apply; then an applied poll on account A | No rebuild: the window apply left the account key A in place | d |
| CK1 | Tokens A and B, same `repo_limit` | Two file names; writing B leaves A's file byte-identical | c |
| CK2 | Injected token B at launch; only A's keyed file exists | A's data is not painted; behaves as no cache | c |
| CK3 | Same token, `repo_limit` 50 and 100 | Two files (today's rule kept) | c |
| CK4 | Injected token source fails; keyed files for A (older) and B (newer) exist | B's file served as `DatasetSource::Cache`; no attempt recorded; the account key of the load is unknown; the first poll records `Auth` | c |
| CK5 | The host part of the key | Equals the `RemoteId` host key for a `github.com` remote (one function) | c |
| CK6 | Tokens `gho_EXAMPLEexampleEXAMPLEexample0000` (bare and with surrounding whitespace) and the empty string | Fingerprints `2a8d4ab865f3ca20`, `2a8d4ab865f3ca20`, `5fccb9a3d0a45785` | c |
| CK7 | Any write, read or failure path | The token appears in no file name, file body, status line or log; only the fingerprint | c |
| CK8 | Existing tests with `ApiCache::at` and `fetch_repos(.., AnyCached)` | Unchanged and pass; no `gh` subprocess runs for a pinned cache | c |
| CK9 | `ApiCache::new` | Deletes nothing beyond today's three obsolete names; `cache_path_varies_by_query_shape` unchanged and passing | c |
| CK10 | Injected token source returning A on its first call and B on any later call; a launch with no keyed file, so a request follows | The source is called exactly once for the attempt; the request's `Authorization` carries A and the answer is written to A's file | c |
| CK11 | Injected token source that does not return within 2 s at launch; a keyed file for B exists | After 2 s, B's file served under the signed-out rule (as CK4) | c |
| CW1 | A write of body X over body W, the rename step replaced by an injected function returning an error (the write helper takes its rename step as a parameter for this) | The file still holds W; no temporary file left | a |
| CW2 | A planted temporary file 2 h old and one 10 min old | Startup cleanup removes the first, keeps the second | c |
| CW3 | Two writes in sequence | The file is exactly the second body; mtime is the second write's | a |
| CW4 | Existing `a_rate_limited_body_does_not_overwrite_the_cache`, `a_payload_with_data_is_cached`, `a_stale_read_reports_when_the_file_was_written` | Unchanged and pass | a |
| CW5 | A planted truncated file (first 1000 bytes of a valid body); `ApiCache::write` of a full body | The file is the full body, byte for byte | a |
| CW6 | `ApiCache::write` into a read-only directory holding a file W | No panic; W unchanged; no temporary file left | a |
| CW7 | Two writes in one process interleaved: writer 1 creates its temporary file, writer 2 creates its own and renames, writer 1 finishes and renames | Two distinct temporary names; the file is exactly writer 1's body, complete | a |
| CW8 | On Unix, a write into an empty directory | The cache file has mode `0600` | a |
| CM1 | Only legacy `github-50.json` exists; launch | Painted from it as `DatasetSource::Cache` with its mtime as data time; the first successful poll writes the keyed file | c |
| CM2 | Legacy and this credential's keyed file exist | The keyed file is served | c |
| CM3 | No keyed file for this credential; (a) `R` whose request fails, with a legacy file present; (b) a launch whose legacy file is unusable and whose request fails | No fallback in either: the load fails and says so; the legacy file is not used | c |
| CM4 | This credential's keyed file exists; legacy 31 days unwritten; legacy 2 days unwritten | First deleted at startup, second kept | c |
| CM5 | Only a `v1`-keyed or other older-version keyed file for this credential | Not served: cold launch | c |
| CM6 | Legacy exists; account A's keyed file exists; launch on token B | Legacy not served; B behaves as no cache | c |
| CM7 | Signed out (or the lookup timed out); superseded files 40 days old | Nothing deleted | c |
| CM8 | The cleanup function called directly with the path of the file just served among its candidates, that file otherwise matching every deletion rule | Not deleted. Defence in depth: under the other rules it is unreachable (a served file is this credential's current file, which never matches, or was served while the account is unknown, when cleanup does not run, CM7); this pins the rule against a later change of them | c |
| CM9 | `window.toml`, an unrelated `notes.json` and a `github-50.json.bak`, all 40 days old, beside a current keyed file | None deleted | c |
| CM10 | `dirs::cache_dir()` is `None` (the cleanup function given no directory) | Cleanup skipped | c |
| CD1 | Two consecutive `Server` failures, each 11 s, at 50 | The next attempt asks for 25; coverage shows 25 of the total | e |
| CD2 | Two `Server` failures, each under 2 s | Page unchanged | e |
| CD3 | Ten successes at 25 | The next attempt asks for the configured 50 | e |
| CP1 | Body with `data` and an `errors` entry whose `path` is under `organizations` | Today's behaviour: `Api` failure, cache untouched (4.8 A) | (pins today, any ticket touching `accept_response`) |

## 8. Implementation tickets

Each block can serve as an issue body. All keep the three standard checks.
"Forced test edits" lists every existing test the ticket would have to touch,
found by reading the tests at `769c1fd` and rechecked at `aa66577`. A
compile-forced new field in an existing test's struct literal, with no
assertion changed, is allowed under the owner's standing exception in
[agent-workflow.md](agent-workflow.md) ("Validation contract"); the worker
lists every test it touched and the reviewer confirms. Any other edit stops
the ticket.

**Order (C8, decided).** FZ-06c and FZ-06d are integrated; the `main.rs` line
is at the identity series, with FZ-05b in review, then FZ-05c to f (FZ-05d
reworks `apply_github_poll`, FZ-05e owns `merge_sources` in `src/github.rs`).
Then FZ-10a (#47, owns `src/github.rs`), **FZ-09c**, **FZ-09b**, FZ-10b
(#48), FZ-10c (#49), **FZ-09d**. FZ-09e is not promoted.

- **FZ-09a now**: it owns only `src/cache.rs`, which no in-flight or planned
  ticket touches. It changes no key and no format.
- **FZ-09f now**: it owns only the PR section label in `src/ui/detail.rs` and
  depends on nothing in this design. It is serial with FZ-05g only if FZ-05g
  is promoted, since that optional ticket also owns `src/ui/detail.rs`.
- FZ-09c after FZ-10a: both change the token lookup in `request`, and the
  account key is FZ-10a's `token_id`, already a pinned fingerprint per #47.
  FZ-09c before FZ-10b: no dependency either way (a rebuild from memory
  reads no cache). The order lands the on-disk change, which waits on O1 and
  O2, before FZ-10b's larger `main.rs` change, and gives FZ-10's A4 (later
  coordination through the cache) its keyed file early.
- FZ-09b after FZ-05e (both edit around `merge_sources`/`split_viewer`) and
  after FZ-10a (`FetchMeta` exists first, C2) and FZ-09c (the account key is
  in `FetchMeta` before FZ-09b carries it on `LoadResult`).
- FZ-09d after FZ-05d (carrier mark and remote key) and after FZ-10b (its
  rebuild-from-memory `LoadKind`, and both change `apply_github_poll` and
  `github_poll_secs`).

### FZ-09a: atomic cache writes

- **Owns**: `src/cache.rs` only.
- **Outcome**: 4.4. `ApiCache::write` writes a uniquely named temporary file
  (pid and a process-wide counter, `create_new`, mode `0600` on Unix) in the
  same directory and renames it over the target; the rename step is a
  parameter of a private helper so a test can make it fail; a failure leaves
  the old file and no temporary file; errors stay ignored. Fix the `ApiCache`
  doc comment (directory and file name, 2.1).
- **Acceptance**: CW1, CW3, CW4, CW5, CW6, CW7, CW8.
- **Forced test edits**: none. `ApiCache` gains no field, so the struct
  literals in `cache_write_and_read`, `cache_expired_returns_none` and
  `a_stale_read_reports_when_the_file_was_written` still compile.
- **Excludes**: the key, the file name, migration, temporary-file cleanup
  (FZ-09c, with CW2), `src/github.rs`, `src/main.rs`, any deletion inside
  `ApiCache::new`.
- **Approval**: none beyond the safe-merge rule: the format and the key do not
  change. Files becoming readable only by the user is listed for the owner
  (O14).

### FZ-09b: coverage facts and the header note

- **Owns**: `src/github.rs` (query fields, the coverage record built in
  `split_viewer` from the pages before the merge, the header rule as a pure
  function beside `window_exceeds_dataset`, coverage into `FetchMeta`);
  `src/main.rs` (`load_repos`, `spawn_window_apply` and `assemble_view`'s
  note, `apply_github_poll`, `LoadResult` gaining an `Option<FetchMeta>` field
  and `apply_load_result` storing it only when `Some`, C3); `src/app.rs` (the
  dataset's `FetchMeta` on `App`).
- **Outcome**: 4.1. `totalCount` and `pageInfo { hasNextPage }` on the three
  connections, declared on `RepositoryConnection` and `OrganizationConnection`
  with serde defaults, no viewer-level field; the coverage record, unknown
  for a payload without the fields; the note from it, with
  `window_exceeds_dataset` called unchanged on the viewer page for unknown
  coverage; the note kept current after a poll and a window apply (a window
  apply carries `None` and keeps the stored `FetchMeta`).
  Provisional words; the owner settles them (O3). The query change is priced
  and timed with FZ-10's procedure and recorded.
- **Acceptance**: CV1 to CV8, CV19. Existing query tests
  (`build_query_always_asks_for_full_pr_data`,
  `the_org_fallback_is_billed_at_its_own_page_size`,
  `the_pr_page_is_not_shrunk_for_cost`,
  `the_calendar_field_rides_the_existing_query`) and
  `window_exceeds_dataset_when_full_and_cutoff_older` pass unchanged (by
  reading: the added fields keep every asserted substring and the single
  `viewer`).
- **Forced test edits**, allowed under the standing exception (the new
  field as `None`, or a `FetchMeta::default()` inside a `GithubFetch`, no
  assertion touched):
  - `LoadResult` literals in `main.rs` tests: the helpers `launch` and
    `reload_lands`, and inline in
    `a_load_result_stores_its_calendar_on_the_app`,
    `a_cache_served_launch_leaves_the_poll_due_immediately`,
    `a_network_served_launch_starts_the_poll_clock` and
    `a_window_apply_does_not_blank_the_calendar`. Six.
  - `main.rs` `poll_says_passed` (a `GithubFetch` literal): none, because
    FZ-10a already added `FetchMeta` there and FZ-09b only adds inside it. If
    FZ-10a landed otherwise, this one literal gets the neutral value under the
    same exception.
- **Approval**: O3 (words) before merge; stops at Ready to integrate for the
  owner to see the note. No owner approval is needed for the test edits above.
- **Excludes**: pagination, rows not covered (FZ-09d), the cache key, the PR
  label (FZ-09f), any viewer-level query field.

### FZ-09c: keyed, versioned cache file and migration

- **Owns**: `src/cache.rs` (a keyed cache directory type beside `ApiCache`,
  the read order and the cleanup of 4.5, temporary-file cleanup);
  `src/github.rs` (a production entry point that takes an injectable token
  source, looks the token up **once** per attempt, derives the account key,
  picks the file and uses the same token for the request; the 2 s bound on
  the launch lookup; `gh auth token --hostname github.com`; the account key
  added to `FetchMeta`; the fingerprint, which is FZ-10a's `token_id`);
  `src/main.rs` (the two `ApiCache::new` call sites in `spawn_github_poll` and
  `load_repos`, and the startup cleanup call).
- **Outcome**: 4.3 and 4.5. `ApiCache`, `ApiCache::new` and `ApiCache::at`
  keep their fields and behaviour; production stops calling `ApiCache::new`
  except to name the legacy file. `fetch_repos(repo_limit, &ApiCache, mode)`
  keeps its signature and behaviour for the existing tests, as a
  `#[cfg(test)]` wrapper if production no longer calls it; the worker picks
  the form that leaves every existing test untouched. `README.md`: where the
  cache lives, that it is per account and readable only by the user.
- **Acceptance**: CK1 to CK11, CM1 to CM10, CW2.
- **Forced test edits**: none, by construction: `ApiCache` gains no field (the
  three `cache.rs` literals compile), `cache_path_varies_by_query_shape` keeps
  calling `ApiCache::new` with today's meaning, the `github.rs` tests use
  `ApiCache::at` and `fetch_repos`, which stay pinned and make no `gh` call
  (CK8), and the account key goes into `FetchMeta`, which test code builds
  through its `Default` (#47). If the worker finds otherwise, the standing
  exception covers only a compile-forced field; anything else stops it.
- **Approval**: O1 and O2 (the file name on disk and what it is derived from,
  the migration), O7, O10, O11, O12. The safe-merge rule excludes cache format
  changes without the owner's approval, so this ticket is not merged before
  O1 and O2.
- **Excludes**: coverage, an envelope or any change to the body, cross-instance
  coordination (FZ-10 A4 B), Enterprise support (no new endpoint), the
  account rebuild (FZ-09d).

### FZ-09d: rows the poll did not cover, and an account change

- **Owns**: `src/main.rs` (`apply_github_poll`, `github_poll_secs`,
  `apply_load_result` clearing the set, the rebuild trigger through FZ-10b's
  rebuild-from-memory `LoadKind`); `src/app.rs` (the set, the dataset's
  account key from `FetchMeta`); the row and detail drawing of the mark in
  `src/ui/` as the coordinator assigns it (`ui/badges.rs` or `ui/detail.rs`;
  not `ui/mod.rs`'s header, which FZ-10c owns).
- **Outcome**: 4.7, including the clearing rule and the rebuild rule (only a
  poll with a known account key that differs from the load's, or meets an
  unknown one, rebuilds).
  Provisional look; the owner settles it (O4).
- **Acceptance**: CV9 to CV13, CV15 to CV18, CV20. Every existing test passes
  unchanged: the `github_poll_secs` tests build `App` without a poll, so the
  set is empty (`poll_relaxes_when_nothing_is_pending`,
  `poll_tightens_while_a_run_is_in_flight`,
  `a_configured_interval_below_the_floor_is_raised`); the poll tests
  (`a_sweep_landing_after_a_poll_keeps_the_polls_github_fields`,
  `a_poll_landing_after_a_sweep_keeps_the_sweeps_local_fields`,
  `a_failed_poll_mid_sweep_leaves_github_fields_and_lands_the_sweep`,
  `a_failed_background_poll_with_a_cache_present_changes_no_row`,
  `a_successful_poll_after_a_failure_clears_it`) assert fields the mark does
  not change. None of them rebuilds: every existing poll test builds its
  `GithubFetch` with a default `FetchMeta`, which carries no account key, and
  a poll without a key never rebuilds (4.7, CV18).
- **Forced test edits**: none expected (`App` is built through helpers;
  `apply_github_poll` keeps its signature; the account key reaches `App`
  through the `LoadResult` field FZ-09b added).
- **Approval**: O4, O5; stops at Ready to integrate (what a row looks like).
- **Excludes**: clearing fields, pagination, the header note, a rebuild path
  of its own (it uses FZ-10b's).

### FZ-09e: smaller page after deadline failures (not promoted)

- **Owns**: `src/github.rs` (page size per attempt), `src/main.rs` or the
  FZ-11 scheduler (the failure count it reads from FZ-10b's state),
  `README.md` (`repo_limit`'s measured ceiling).
- **Outcome**: 4.6 B. **Acceptance**: CD1 to CD3. **Promote** when a deadline
  failure is seen at the configured size, or before raising `repo_limit`'s
  default; promoting it is the owner's (O13). **Excludes**: splitting into
  several requests (4.6 C).

### FZ-09f: the `-p` PR section count (optional)

- **Owns**: `src/ui/detail.rs` (the PR section label only).
- **Outcome**: "5 of 8 open PRs" when the listed count is below `pr_count`.
  **Acceptance**: CV14; existing `ui/detail.rs` tests pass unchanged.
  **Depends on**: nothing. **Approval**: O8; stops at Ready to integrate
  (wording).

## 9. Decisions

### Decided by the coordinator (2026-10-01)

As recommended in revision 1; the review found each sound under the
conditions stated.

- **C1 Account key.** K1: the pinned FNV-1a 64 fingerprint of 4.3, used as
  FZ-10's `token_id` and as the cache key, so the two cannot disagree; FZ-10
  4.1's "never stored" is amended (#47 carries it). Conditions, all applied:
  the legacy file read only while no account has a keyed file (finding 2),
  one token lookup per attempt with an injectable source (finding 6), the
  launch lookup bounded at 2 s (finding 7), and the owner told plainly what
  the name reveals and offered K2 and K4 (finding 11). What sits on disk is
  his choice (O1); if he picks K2 or K4, C1 follows it.
- **C2 `FetchMeta`.** FZ-10a's new fetch field is one struct with a
  `Default`; FZ-09c adds the account key to it, FZ-09b the coverage.
- **C3 `LoadResult`.** One new field, `Option<FetchMeta>`, added by FZ-09b,
  so the note and the account key are right from the first frame. `None` for
  a window apply and a rebuild from memory, as `dataset` already is;
  `apply_load_result` stores it only when it is `Some`, so neither resets the
  stored coverage or account key (second review, finding 2). Its six
  compile-forced test literals take `None` under the standing exception.
- **C4 Cleanup.** 30 days unwritten, with the safety rules of 4.5 (finding
  10).
- **C5 Pagination.** None by default; re-fetch on widen recorded (O9).
- **C6 Deadline.** 4.6 B as FZ-09e, not promoted; C as the next step.
- **C7 Partial responses.** Unchanged until observed (4.8 A).
- **C8 Order.** As in [section 8](#8-implementation-tickets).
- **C9 Token host.** `gh auth token --hostname github.com` in FZ-09c,
  confirmed by the reviewer on `gh` 2.98.0.

### Owner items

**Accepted by Andreas on 2026-10-02, all as recommended.** The items were
put to him that day with their recommendations and with "all as recommended"
offered as a complete answer; his reply was "yes to all". That settles: O1
as the fingerprint of the `gh` token (K1); O2; O3 and O4, whose words and
look he still sees on the builds that draw them; O5; O7 as the most recently
saved copy; O8; O10; O11; O12; O14; and the contract and invariant changes of
section 6. O9 and O13 stay "later", as recommended: neither is built until it
is taken up.

For Andreas, in plain language: everything you would find on disk, see on
screen or wait for. Each has a recommendation. O6 was withdrawn (a test-edit
question now covered by your standing rule on compile-forced fields); the
other numbers are kept so references on the issue stay valid.

- **O1 One saved GitHub copy per account, and what its file name is made
  of.** Today fleetz keeps one saved copy of your GitHub data and shows it at
  launch, whichever account saved it; after switching accounts in `gh`, launch
  shows the old account's repos and contribution graph until the first
  refresh. Proposed: one saved copy per account. Three ways to name it:
  - **From your `gh` token (recommended by the coordinator).** The name holds
    16 characters computed from the token. They cannot be turned back into
    the token, but someone who already had a token could check whether it is
    yours by computing the same 16 characters. Switching accounts never shows
    the other account's data. Costs: launch first asks `gh` for the token
    (about a twentieth of a second, see O11); when `gh` gives you a new token
    for the same account, the next launch shows no GitHub data until GitHub
    answers (about 7 s), and if you are offline at that moment it shows none
    at all.
  - **The same, with a standard cryptographic scramble.** Same behaviour; the
    name is computed with a well-known one-way function instead. Costs a new
    library in fleetz.
  - **From your GitHub username.** Only public information on disk, no `gh`
    lookup before the first paint, and no slow launch when the token changes.
    Cost: after switching accounts, launch shows the previous account's repos
    and graph until the first refresh lands (a few seconds), or for as long
    as GitHub cannot be reached.
- **O2 The first launch after upgrading.** The new version shows your
  existing saved copy at once, but only until any account has saved its own
  copy; from then on it never reads the old file. An older fleetz you still
  run keeps using the old file; it is deleted after 30 days without use.
  Recommended: yes.
- **O3 Saying when GitHub has more repos than fleetz fetched.** Your account
  has 95 repos; fleetz fetches the 50 most recently pushed. Today the warning
  almost never shows: with a one-year window 4 repos are missing silently,
  with two years 35, and with "all" it never shows. Proposed: a short header
  note such as "50 of 95 repos" whenever the window reaches past the oldest
  fetched repo, and the same for org repos or orgs if those are ever cut off.
  You settle the words and look on the branch build. Recommended: yes.
- **O4 Repos the last refresh did not include.** When a repo drops out of the
  fetched 50 during a session, it keeps showing its last CI result and PR
  count with nothing to say they are old. Proposed: keep the values but show
  them dimmed (a `Pending` light stops blinking), with the detail pane saying
  they are from an earlier refresh; such a repo no longer keeps fleetz polling
  every 15 seconds. You settle the look. Recommended: yes.
- **O5 Switching accounts while fleetz runs.** Proposed: the next refresh on
  the new account rebuilds the list from that account's data, without
  re-fetching your git remotes or changing your time window. The same happens
  once after a launch that could not tell which account it was showing. Today
  the old account's repos stay until you press `R`. Recommended: yes.
- **O6** Withdrawn.
- **O7 Launching while signed out of `gh`.** Today fleetz shows the saved
  copy and then says it is not signed in. Proposed: the same, using the most
  recently saved copy on this machine, which on a machine with two of your
  accounts may be the other one until you sign in. Alternative: show no
  GitHub data until you sign in. Recommended: the first.
- **O8 The PR list with `-p`.** It says "5 open PRs" when there are 8 and
  fleetz lists 5. Proposed: "5 of 8 open PRs". Recommended: yes, a small
  optional change.
- **O9 Later, not now: fetching the older repos.** Fleetz could fetch the
  other 45 repos when you widen the window past the newest 50: one more
  GitHub request, about 7 s and a small share of the hourly allowance, only
  when you ask for it. Recommended: not now; the note (O3) first, and this
  only if the missing repos bother you.
- **O10 Another account's saved copy is deleted after 30 days unused.** If
  you stop using an account (or `gh` gives you a new token), its saved copy
  is removed from disk once it has gone 30 days without being written. Your
  time window file and anything else in that folder are never touched.
  Recommended: yes.
- **O11 A short `gh` lookup before the first paint.** With the token-based
  names of O1, every launch first asks `gh` for your token (about a twentieth
  of a second). If `gh` has not answered within 2 seconds, fleetz stops
  waiting and treats you as signed out (O7). Also: if your shell sets
  `GH_HOST` to another GitHub server, fleetz now still uses your github.com
  login instead of reporting that you are not signed in. Recommended: yes.
- **O12 `R` while offline, for an account with no saved copy.** Today `R`
  with no network rebuilds from the saved copy, whichever account saved it,
  and says so. Proposed: it uses only the current account's saved copy; with
  none, it says the reload failed instead. Recommended: yes.
- **O13 Later: shrinking the request when GitHub times out.** If GitHub ever
  starts failing the request because it takes too long, fleetz would fetch
  fewer repos per refresh until it succeeds, and the header note (O3) would
  say so. Not built until that is seen; your yes is asked when it is.
  Recommended: build it then.
- **O14 Saved files readable only by you.** Today the saved copy can be read
  by other users of the machine (mode 644). Proposed: only you can read new
  saved copies (mode 600). The old file keeps its old permissions until it is
  deleted (O2, O10). Recommended: yes.
- **The contract and invariant changes** in
  [section 6](#6-contract-and-invariant-changes): the saved copy is per
  account and launch shows nothing from another account's copy; a repo the
  last refresh did not include no longer keeps the 15-second polling going; a
  refresh on another account rebuilds the list; the header can say how much
  was fetched. Recommended: accept, together with O1 to O5.

What does not change: no new settings, no change to how often fleetz asks
GitHub, no extra GitHub requests (unless O9 is taken later), and the saved
data itself stays GitHub's answer as sent.

## 10. Found on the way, and not established

| What | Where | Who |
|---|---|---|
| The `ApiCache` doc names `~/.cache/fleetz/github.json`; the file is `github-{repo_limit}.json` under `dirs::cache_dir()`, `~/Library/Caches` on macOS | `cache.rs` 11 | FZ-09a |
| The fallback when `dirs::cache_dir()` is `None` is the literal relative path `~/.cache` | `cache.rs` 29 | Recorded; FZ-09c skips cleanup there (CM10) |
| The `-p` PR section label counts the listed PRs, not the open ones | `ui/detail.rs` 1047 to 1051 | FZ-09f |
| `cache_ttl` decides nothing observable (FZ-10 found it too) | `fetch_repos` 649 to 652, `config.rs` | Coordinator; removing a config key is the owner's. Not proposed here |
| The check suite behind the workflow name and duration is GitHub's first, not necessarily the latest | `build_query` `checkSuites(first: 1)`, `parse_ci` | Not ticketed; a candidate for the CI work in M4 |
| `ApiCache::new` runs three deletions on every poll and every `cargo test` run | `cache.rs` 34 to 38 | Harmless; FZ-09c must not add to it (CK9) |

Not established:

- Whether GitHub returns `data` with `errors` for SAML-protected orgs (1.6).
- How time scales with returned repos beyond the single runs in section 3;
  whether a fixed server deadline exists (FZ-10: evidence near 11 s).
- Whether `pushedAt: null` repos sort first or last on the viewer connection;
  `parse_repos` drops them either way, but a null at the end of a page would
  move the boundary by one node.
- How long `gh auth token` can stall on a locked keychain; the 2 s bound is a
  margin over the measured 0.05 s, not a measured worst case.
