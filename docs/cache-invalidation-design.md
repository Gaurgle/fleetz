# Cache invalidation design (FZ-06)

Status: investigation output for [FZ-06](first-wave-tickets.md) (issue #19).
Base: `main` at `6dd260f`. Production code was read only; nothing under `src/`
changed. Fix tickets below are proposals for the coordinator.

## Summary

One `WorkKey` (`git::work_key`, src/git.rs:189-218) decides whether a sweep
reuses six expensive probes and the local contribution histogram. It is built
from HEAD's sha, the index's mtime and size, the stash reflog's size, and a
filesystem fingerprint of `packed-refs` plus the top level of `refs/tags`. It
does not see local branches other than HEAD's, remote-tracking refs, nested
tags, worktree metadata, the HEAD reflog, the author used for the histogram, or
the calendar. Fixture experiments confirmed thirteen distinct gaps (G1 to G13
in 3.5), each a class of mutation where a product's true answer changed and
the key did not, shown by 24 of the 50 experiment rows in 3.1. G12 and G13
(notes and the shallow boundary) were found by the reviewer. The most common
is the everyday push:
`git push` moves `refs/remotes/origin/main` onto the commit just made, the key
stays identical, and the graph keeps drawing `origin/main` one commit behind
until something else moves HEAD. Others: a branch moved or created while HEAD
stays put (including every commit made in a linked worktree), a fetch, nested
tags, `git worktree add/remove`, a week rollover that shifts the whole local
histogram one week, a histogram that goes stale after a sweep in GitHub mode,
git notes, `fetch --unshallow`, and the load path taking its key after its
probes. Working-tree state is not
affected: `git status --porcelain` runs on every sweep regardless of the key.
The adopted design is a ref-content signature built from the two
`for-each-ref` calls the sweep already makes (one widened to every ref
outside `refs/heads`), plus the content of `<common>/shallow`, filesystem
reads for the HEAD reflog and worktree metadata, a separate key stored with
the histogram, and every key input read before the gated probes. That closes
twelve of the thirteen gaps at zero extra
subprocesses per repo per sweep, except for repos with linked worktrees, which
pay one `git worktree list` per sweep. The remaining one, G8 (a histogram author
changed mid-session), is accepted as a limitation whose remedy is `R` (OD4).

## Decisions

Decided by the coordinator (`claude-fable-5-1`) on 2026-10-01, and amended the
same day by the coordinator's rulings on the FZ-06 review (OD1, OD2, OD3 and
the tickets). Not reviewed by Astra. None changes a persistence format, configuration or external contract,
so none needs the owner. The options considered are kept for the record.

| # | Question | Options considered | Decision |
|---|---|---|---|
| OD1 | How the key learns about refs | (a) signature from the cheap tier's existing `for-each-ref` output, one query widened to tags and `refs/stash`; (b) one extra `for-each-ref refs/` per repo per sweep (about 12 ms per repo per sweep, measured in 1); (c) recursive filesystem fingerprint of `refs/` (failed on a 1 s filesystem, E11a) | **(a)**, with the query widened to every ref outside `refs/heads` (review ruling 5), not only tags and `refs/stash`. The hash covers (ref name, full object id) pairs for every ref (branches, remote-tracking refs, tags, `refs/stash`, `refs/notes/*`, `refs/replace/*` and any other namespace), plus the HEAD symref (which branch HEAD names, or detached), plus the content of `<common>/shallow` when it exists. The full id comes from an added `%(objectname)` field; the existing `tip` fields stay abbreviated (6, FIX-A). HEAD's object id is not in the signature: `head_sha` stays in `WorkKey` beside it. Nothing derived goes in: no ahead or behind counts, no upstream, no short names. Scaling: the widened query's output grows with the tag count, as text only; the process count does not change |
| OD2 | Worktree list freshness | (a) run `git worktree list` every sweep when `<common>/worktrees` exists; (b) fingerprint `<common>/worktrees/*/{HEAD,gitdir}` from the filesystem; (c) run it every sweep everywhere | **(a).** Exact, and free for repos with no linked worktree. An absent `<common>/worktrees` means the linked list is empty, never "reuse the previous list": git deletes the directory when the last linked worktree is removed (review ruling 3) |
| OD3 | Histogram key | (a) store the inputs next to `daily` and reuse only on equality; (b) drop `daily` whenever a sweep runs without the probe; (c) keep today's behavior | **(a).** The stored inputs are `grid_start`, the configured author (`contributions_author`, `None` when unset), the OD1 ref signature and HEAD's object id (review ruling 1: `--all` walks HEAD, and the signature records only that HEAD is detached). Not the resolved email: see OD4 |
| OD4 | Author resolution when `contributions_author` is unset | (a) run `git config user.email` on every Local sweep (+1 process per repo per Local sweep); (b) stamp the repo and global config files by mtime and size; (c) accept staleness until the histogram is next re-probed, and re-resolve on every re-probe | **(c)**, against the worker's recommendation of (b). (b) is the filesystem-metadata mechanism rejected for refs in OD1: coarse timestamps, and the global config has several possible locations plus includes. A mid-session author change is rare and `R` fixes it once the next sweep runs (5.2). G8 (E6a) is therefore an accepted limitation |
| OD5 | Keep the index in the key | (a) keep it; (b) drop it once OD1 lands | **(a)** for now. The index is not an input of any gated product (2.3), so it only causes extra re-probes, but today it incidentally catches E1d and E5d. Revisit with measurements after FIX-A |
| OD6 | Ordering and ownership | sequence after FZ-03; fold FIX-E into FZ-12 or land it alone | **Accepted.** FIX-C (`src/git.rs` only) can land first. FIX-A, FIX-B and FIX-D land after FZ-03 is integrated. FIX-E lands separately as its own small ticket. FZ-04 will also own `src/git.rs` (its fetch result type) and `src/main.rs`, so neither FIX-C nor FIX-A may run beside FZ-04. The order between this series, FZ-04 and the FZ-05 series is set by the coordinator when the tickets are promoted |
| OD7 | `daily` counts the latest stash's WIP and index commits as contributions (found in passing, 3.4) | (a) a separate ticket to exclude `refs/stash`; (b) accept | **(a).** Kept out of this design's tickets; listed under follow-ups in 6 |

## 1. Method and evidence labels

Claims are labelled per section or per item with one of these.

- **Executed**: observed by running commands in temporary repositories under
  the session scratch directory. Git 2.39.3 (Apple Git-146), macOS, APFS
  (nanosecond mtimes) unless stated. Coarse timestamps were tested on an HFS+
  disk image created with `hdiutil` in the scratch directory (1 s mtimes,
  verified: a fresh file's mtime read `1790865583.000000000`). Fixture repos got
  their identity through `GIT_AUTHOR_*`/`GIT_COMMITTER_*` and
  `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`. No real repository and
  no global configuration was touched.
- **Read**: established by reading the code at the base commit.
- **Inferred**: a conclusion from the above that was not executed.

**How the key was observed.** The experiments use a shell **replica** of
`work_key` and `refs_fingerprint`, not the Rust function. It mirrors
src/git.rs:189-218 (`work_key`), 142-154 (`refs_fingerprint`) and 158-171
(`resolve_git_dir`) component by component: `git rev-parse HEAD`; `stat` of
`<gitdir>/index` (mtime, size); size of `<common>/logs/refs/stash`; mtime and
size of `<common>/packed-refs`; mtime of `<common>/refs/tags`; entry count of
`<common>/refs/tags`; `refs_mtime` as the `Option` max of the two mtimes.
`<common>` follows `<gitdir>/commondir` exactly as git.rs:198-201 does. The
gated products were computed with the exact argument lists that
`rebuild_repo_status` passes (src/main.rs:1994-2006, 2013-2030), with
`HISTORY_COUNT` = 200 (src/main.rs:43).

The replica was cross-checked against the real function where existing tests
already exercise it: `cargo test --manifest-path <worktree>/Cargo.toml
work_key` ran the four existing tests in `git::real_repo_tests`
(`a_new_tag_changes_the_work_key` at src/git.rs:1754,
`an_idle_repo_keeps_the_same_work_key` at 1768,
`deleting_a_tag_changes_the_work_key` at 1781,
`work_key_stable_then_changes_on_commit_and_stash` at 1982): 4 passed
(**Executed**). The replica agrees with all four (E3a, E3c, E5a, the idle
baseline). No existing test exercises a confirmed gap, and `rebuild_repo_status`
is private to the binary with no test seam, so the gaps themselves are shown
through the replica only.

**Process cost** (**Executed**; used in 5.1): 100 sequential runs each on a small
fixture repo measured `git rev-parse HEAD` at 12.1 ms, `git worktree list
--porcelain` at 11.5 ms and `git status --porcelain` at 12.5 ms per run,
including the `system()` shell spawn. This agrees with the finding the handoff
quotes from the performance baseline (about 11 git processes per local repo at
startup, several ms each); that document is not on this base and is not
linked.

## 2. Map

### 2.1 The key

`WorkKey` (src/git.rs:104-138), computed by `work_key` (src/git.rs:189-218):

| Component | Source | Notices |
|---|---|---|
| `head_sha` | `git rev-parse HEAD` (git.rs:190) | Any change of the commit HEAD resolves to. Not a symbolic change at the same sha |
| `index_mtime`, `index_size` | `stat <gitdir>/index` (git.rs:202, 211-212) | Any index write: `add`, `commit`, `checkout`, `stash push/pop`, `reset` |
| `stash_size` | size of `<common>/logs/refs/stash` (git.rs:203-205) | Stash entries added or removed, unless the size is unchanged |
| `refs_mtime` | max of `<common>/packed-refs` mtime and `<common>/refs/tags` directory mtime (git.rs:143-153) | Any `packed-refs` rewrite; any entry created, renamed or removed directly inside `refs/tags` |
| `packed_refs_size` | size of `<common>/packed-refs` (git.rs:144) | `packed-refs` rewrites that change its length |
| `loose_tag_count` | entries directly inside `<common>/refs/tags` (git.rs:151) | Flat tags added or deleted; a nested tag directory counts as one |

`<gitdir>` is `.git` or the target of a `gitdir:` file (`resolve_git_dir`,
git.rs:158-171). `<common>` is `<gitdir>/<contents of commondir>` when that file
exists, else `<gitdir>` (git.rs:198-201). (**Read**)

**When the key is `None`** (**Read**): `rev-parse HEAD` fails or times out
(unborn HEAD, broken repo, the 30 s `GIT_TIMEOUT` at git.rs:9), or
`resolve_git_dir` cannot resolve `.git`. `rebuild_repo_status` treats `None` as
changed (`key.is_some() && key == old.work_key`, main.rs:1981), so every
expensive probe runs again on every sweep. That is correct, and costs the full
probe set for each such repo each sweep. Rows built by `dirty_mode_status`
(main.rs:2139-2202), `local_only_status` (main.rs:2627-2687) and not-cloned rows
in `assemble_view` (main.rs:2572) start with `work_key: None`, so their first
sweep re-probes. **Executed** (E10b): an unborn HEAD gives `None`; the first
commit gives a key.

### 2.2 Where snapshots are compared, reused and dropped

(**Read**)

- **Snapshot taken**: `sweep_targets` (main.rs:1828-1842) clones each row into
  `SweepTarget::old` (`engine/fetch.rs`) when the sweep starts.
- **Compared and reused**: `rebuild_repo_status` (main.rs:1962-2054). The cheap
  tier runs first (1972-1978), then the key (1980), then either the six products
  are cloned from `old` (1983-1991) or re-probed (1992-2007). `daily` has its own
  three-way rule (2013-2030). The new key is stored with the result (2051).
- **Landed**: `apply_repo_updated` (main.rs:1665-1678) calls
  `RepoStatus::apply_local_sweep` (src/model.rs:480-530), which overwrites the
  row's sweep-owned fields, `work_key` and `daily` included, with the result.
  `repo_moved` (main.rs:1649-1656) compares `head_sha` only, to decide an early
  GitHub poll.
- **Built from scratch**: `build_repo_status` (main.rs:2691-2783) runs every
  probe at load and takes the key **last** (2780-2782). A reload replaces the
  whole list, which drops every snapshot.
- **Triggers**: watch timer (main.rs:652, fetch on), `r` (1508-1510, no fetch),
  `F` (1385-1388, fetch), `f` (1376-1384, single repo, fetch), branch switch
  (1061-1068, single repo, no fetch), `c` into Local when no row has a
  histogram (1317-1328), and moved remotes after a poll (1723-1726). Branch
  deletion from the overlay (main.rs:1015-1028) triggers no sweep.

### 2.3 Products

Tier: **always** means probed on every sweep; **gated** means reused from the
snapshot while the key is unchanged. Inputs are by reading; the last column is
by reading and confirmed by the experiment named.

| Product | Source | True inputs | Tier | Key component that moves when an input changes |
|---|---|---|---|---|
| `branches` (name, upstream, ahead, behind, `is_head`, tip) | `git::branch_statuses` (git.rs:230-281): `for-each-ref refs/heads/`; `git remote` when a branch lacks an upstream (284-288); `rev-list --count <b> --not --remotes` per qualifying branch (291-305) | Local branch refs, HEAD symref, upstream config, remote-tracking refs, remotes config | always (main.rs:1972) | n/a |
| `dirty` | `git::dirty_count` (git.rs:439-449): `status --porcelain` | Working tree, index, ignore rules, `status.*` config | always (main.rs:1973) | n/a |
| `ahead`, `behind` totals | fold of `branches` (main.rs:1976-1978) | as `branches` | always | n/a |
| `remote_branches` | `git::remote_branches` (git.rs:915-935): `for-each-ref refs/remotes/` | Remote-tracking refs | always (main.rs:1974) | n/a |
| `remote_only_branches` | `git::unfetched_branches` (git.rs:974-985), pure over the two above | as those two | always (main.rs:2034) | n/a |
| `commits` | `git::commits` (git.rs:481-526): `log --format=... --numstat --shortstat [--author=A] -200 <every local branch>` (main.rs:1994-2001) | Tips of **all** local branches (HEAD when there are none); the branch name list; `args.author` (fixed for the session, main.rs:1940 via `spawn_sweep`) | gated | HEAD's branch: `head_sha`. Another branch moved or created: **none** (E1a, E1b, E9a, E9d). Branch deleted: `refs_mtime`, `packed_refs_size`, only because git 2.39 rewrites `packed-refs` on every deletion (E1c) |
| `reflog` | `git::reflog` (git.rs:635-649): `reflog -n 200` | `<gitdir>/logs/HEAD` (per worktree) | gated | HEAD moves: `head_sha`. Checkout of another branch at the same sha: `index_mtime`, incidentally (E1d). `symbolic-ref`: **none** (E1e). `reflog expire`: **none** (E15a) |
| `graph` | `git::graph` (git.rs:680-717): `log --decorate=full --format=%h\|%p\|%D\|%aN\|%s -n 200 <every local branch>` | As `commits`, plus **decorations**: every branch, remote-tracking ref, tag and HEAD symref that points at a walked commit | gated | HEAD: `head_sha`. Flat tags: `refs_mtime`, `loose_tag_count`. Remote-tracking refs: **none** (E2b, E2c). Nested tags: **none** (E3d, E3e). Other branches: **none** (E1a, E1b, E8b). HEAD symref at the same sha: **none** (E1e) |
| `graph_expanded` | `git::graph_expanded` (git.rs:740-773): `log --graph --decorate --abbrev-commit --date=iso-strict -n 200 <every local branch>` | As `graph`; dates absolute, so no clock input (git.rs:750-755) | gated | As `graph` |
| `stash_entries`, `stash_count` | `git::stash_list` (git.rs:1034-1045): `stash list --format=%s` | `<common>/logs/refs/stash` | gated | `stash_size`, unless the size is unchanged; then `index_mtime`, incidentally, only in the checkout that ran the stash and only on a fine-grained filesystem (E5d vs E5f, E11b) |
| `worktrees` | `git::worktree_list` (git.rs:1048-1058), parsed to linked (path, branch) pairs (1092-1129) | `<common>/worktrees/*/gitdir` and `*/HEAD` | gated | **none** (E4a-E4d) |
| `daily` | `git::daily_commits` (git.rs:45-68): `log --all --date=short --pretty=%ad --since=<start> --author=<a>`; author from `contributions_author` (main.rs:322, fixed for the session) or `git config user.email` (git.rs:91-98) | **Every** ref (heads, remotes, tags, `refs/stash`); `start` = `contributions::grid_start(today)` (contributions.rs:68-71), which steps by 7 days each Sunday; the resolved author; whether the strip is in Local mode | gated twice (main.rs:2013-2030): kept as-is when the probe is off, reused when the key is unchanged and a histogram exists | HEAD: `head_sha`. Remote refs: **none** (E2c). Another branch: **none** (E1a). Author config: **none** (E6a). Week rollover: **none** (E7). A key that moved during a GitHub-mode sweep: consumed without refreshing `daily` (E14) |
| `last_local`, `remote_url`, `owner`, `name`, `path`, `location`, `groups` | load (`build_repo_status`, `local_only_status`, `assemble_view`) | various | load-owned; `apply_local_sweep` ignores the sweep's copy (model.rs:505-513) | n/a |

Inputs the table above leaves out, added after review (3.1, E16a and E16b):
every log-based product (`commits`, `graph`, `graph_expanded`, `daily`) also
depends on the shallow boundary (`<common>/shallow`) and on `refs/replace/*`;
`graph_expanded` also prints notes (`refs/notes/commits`) in its default
format; and `daily`'s `--all` also walks `refs/notes/*` and HEAD. Today's key
sees none of these.

## 3. Mutation experiments

### 3.1 Results

All **Executed** with the replica (section 1) unless marked. "Key moved" names
the components that changed; "Answer changed" names the products whose output
differed before and after. For `worktrees` the parsed form (linked entries only,
as `parse_worktree_output` keeps them) is what counts: the raw porcelain also
changes with the main checkout's HEAD, which fleetz drops.

| ID | Mutation | Key moved | Answer changed | Class |
|---|---|---|---|---|
| E1a | `git branch -f side <new commit>`, HEAD fixed | nothing | commits, graph, graph_expanded, daily | gap |
| E1b | `git branch feat HEAD~1` | nothing | commits, graph, graph_expanded | gap |
| E1c | `git branch -D feat` (loose) | `refs_mtime`, `packed_refs_size` (git rewrote `packed-refs`) | commits, graph, graph_expanded | non-gap (incidental) |
| E1d | `git branch same; git checkout same` (same sha) | `index_mtime` (checkout rewrote the index) | reflog, graph, graph_expanded | non-gap (incidental) |
| E1e | `git symbolic-ref HEAD refs/heads/same` (same sha) | nothing | reflog, graph, graph_expanded | gap (plumbing only) |
| E2a | local commit | `head_sha`, index | commits, reflog, graph, graph_expanded, daily | non-gap |
| E2b | `git push origin main` after E2a | nothing | graph, graph_expanded | **gap** |
| E2c | `git fetch --prune` of another clone's push | nothing | graph, graph_expanded, daily | gap |
| E2d | fetch of a new remote branch at a commit outside the walk | nothing | nothing | non-gap |
| E2e | `fetch --prune` deleting that remote branch | `refs_mtime`, `packed_refs_size` | nothing | non-gap (false invalidation) |
| E3a | `git tag v1` | `refs_mtime`, `loose_tag_count` | graph, graph_expanded | non-gap |
| E3b | `git tag -f v1 HEAD~1` | `refs_mtime` | graph, graph_expanded | non-gap |
| E3c | `git tag -d v1` | `refs_mtime`, `packed_refs_size`, `loose_tag_count` | graph, graph_expanded | non-gap |
| E3d | `git tag rc/2` with `rc/1` present | nothing | graph, graph_expanded | gap (documented limit, git.rs:131-134) |
| E3e | `git tag -f rc/2 HEAD~1` | nothing | graph, graph_expanded | gap |
| E3f | `git tag -d rc/2`, `rc/1` remains | `refs_mtime` (`packed-refs` rewrite) | graph, graph_expanded | non-gap (incidental) |
| E3g | annotated flat tag | `refs_mtime`, `loose_tag_count` | graph, graph_expanded | non-gap |
| E4a | `git worktree add -b wt1 <path>` | nothing | graph, graph_expanded, worktrees | gap |
| E4b | `git worktree add <path> wt2` (existing branch) | nothing | worktrees (and graph via the branch created just before) | gap |
| E4c | `git worktree remove <path>` | nothing | worktrees | gap |
| E4d | linked worktree switches to a new branch | nothing | worktrees (parsed), graph, graph_expanded | gap |
| E5a | `git stash push` | `index_mtime`, `stash_size` | reflog, stash, daily | non-gap |
| E5b | `git stash pop` | index, `stash_size` | stash, daily | non-gap |
| E5c | two pushes, then `stash drop stash@{1}` | index, `stash_size` | reflog, stash, daily | non-gap |
| E5d | `stash drop; stash push -m bbbb` replacing `aaaa` (same size) | `index_mtime` only | reflog, stash | non-gap (incidental) |
| E5e | `stash drop` | `stash_size` | stash, daily | non-gap |
| E5f | E5d done in the main checkout, observed from a linked worktree's row | nothing | stash | gap |
| E6a | repo-local `user.email` changes | nothing | daily | gap |
| E7 | calendar rolls Saturday 2026-10-03 to Sunday 2026-10-04 | nothing (no clock component) | daily: every slot shifts 7 days (3.2) | gap (replica arithmetic) |
| E8a | `git pack-refs --all` | `refs_mtime`, `packed_refs_size`, `loose_tag_count` | nothing | non-gap (false invalidation) |
| E8b | move a packed branch (writes a loose ref) | nothing | graph, graph_expanded | gap |
| E8c | delete a packed branch | `refs_mtime`, `packed_refs_size` | graph, graph_expanded | non-gap |
| E8d | `git gc` | `refs_mtime` | nothing | non-gap (false invalidation) |
| E8e | flat tag after `gc` | `refs_mtime`, `loose_tag_count` | graph, graph_expanded | non-gap |
| E9a | commit on `main` in the main checkout, observed from a linked worktree's row on `lw` | nothing | commits, graph, graph_expanded, daily | gap |
| E9b | stash in the main checkout, linked row | `stash_size` (common dir) | stash, daily | non-gap |
| E9c | tag in the main checkout, linked row | `refs_mtime`, `loose_tag_count` (common dir) | graph, graph_expanded | non-gap |
| E9d | commit on `lw` in the linked worktree, observed from the main row | nothing | commits, graph, graph_expanded, daily | gap |
| E10a | `rm .git/index` | index components | nothing | non-gap (false invalidation) |
| E10b | unborn HEAD, then first commit | `None`, then a key | all | non-gap |
| E10c | no `logs/refs/stash` | `stash_size` = 0 | n/a | non-gap |
| E11a | HFS+ (1 s): tag `v1` deleted and `v2` created in the same second as the previous key | nothing (3 of 3 runs) | graph decoration (3 of 3) | gap (coarse timestamps) |
| E11b | HFS+ (1 s): stash `aaaa` replaced by `bbbb` in the same second | nothing | stash | gap (coarse timestamps) |
| E13 | load path: probes, then a commit, then the key (the order of `build_repo_status`) | stored key equals the next sweep's | commits and graph stored are pre-commit | gap (order replayed by hand) |
| E14 | histogram in Local; commit; a sweep in GitHub mode; a sweep in Local | k0 != k1, then k1 == k1 | reused `daily` differs from the true one | gap (sequence replayed by hand) |
| E15a | `git reflog expire --expire=now --all` | nothing | reflog | gap (rare) |
| E15b | `git stash clear` | `stash_size` | stash, daily | non-gap |
| E16a | `git notes add -m hello HEAD` | nothing | graph_expanded, daily | gap |
| E16b | `git fetch --unshallow` in a `--depth 1` clone (**reviewer's run**) | nothing (key and the B1 signature) | graph rows 1 to 3 | gap |
| E17 | `checkout --detach`, then a commit | `head_sha`, index | reflog, graph, graph_expanded, daily | non-gap today; a gap in the first FIX-D draft (6) |

E16a and E17 are mine. The reviewer separately ran `git notes add` and saw
`daily` go from 2 to 3 with the key unchanged, which E16a agrees with; E16a
also shows `graph_expanded` changing, because `git log --graph` with its
default format prints notes. E16b is the reviewer's run only; I did not
repeat it. Not compared by anyone: `git replace --graft`, which changes `git
log` output (R6).

E13 and E14 replay the code's sequence of calls by hand with the replica, since
neither path can be driven from outside the binary. E7 is date arithmetic over
replicas of `grid_start` and `parse_daily`. They are as deterministic as the
others, but the Rust function was not executed.

### 3.2 The week rollover (E7)

`grid_start` with `YEARS` = 3 (contributions.rs:16, 68-71) gives 2023-10-01 for
Friday 2026-10-02 and Saturday 2026-10-03, and 2023-10-08 for Sunday 2026-10-04
(**Executed**, replica). `DailyCounts` stores slots as offsets from the start it
was built with (`parse_daily`, git.rs:79-83), and `Calendar::add_daily` adds
them positionally to a calendar built on today's start (contributions.rs:182-184,
299-303; `App::local_calendar`, app.rs:2112-2121). A Friday commit sits in slot
1097 under Saturday's start. Reused on Sunday, slot 1097 means 2026-10-09; its
true slot is 1090, and Sunday's `today_slot` is 1092. So after the rollover, the
whole reused histogram is drawn one week late, and the newest week falls past
today. Nothing in the key carries the date, so this lasts until each repo's HEAD
moves. **Read** plus replica arithmetic.

### 3.3 The histogram after a GitHub-mode sweep (E14)

With the probe off, `rebuild_repo_status` keeps `old.daily` but stores the
**new** key (main.rs:2014, 2051). A repo that moved while the strip showed
GitHub data therefore holds a fresh key next to a stale histogram. Switching back
to Local triggers no sweep while any row already has a histogram (main.rs:1322-1327),
and the next sweep finds the key unchanged and a histogram present, so it reuses
it (main.rs:2015). **Read**; the sequence replayed by hand confirmed the reused
histogram differs from a fresh one.

### 3.4 Found in passing: `daily` counts stash commits

`daily_commits` walks `--all` (git.rs:52-61), which includes `refs/stash`. The
latest stash's WIP commit and index commit are authored by you, so each stash
push adds contributions to today, and a pop removes them (E5a, E5b, E15b: the
`daily` answer changed on stash operations alone). **Executed.** This is a
counting question, not invalidation, so it is OD7 rather than a fix ticket here.

### 3.5 Confirmed gaps, non-gaps and unconfirmed risks

**Confirmed gaps** (answer changed, key did not). Counted as thirteen classes
of mutation, G1 to G13; together they cover the 24 rows classed "gap" in 3.1
(E17 is not counted: it is a non-gap today).
The closing ticket follows each.

- G1. Remote-tracking ref moves: push (E2b) and fetch (E2c). Graph decorations,
  and `daily` for fetched commits by you. FIX-A; FIX-D for `daily`.
- G2. A local branch other than HEAD's moves, is created, or is moved from
  another worktree (E1a, E1b, E8b, E9a, E9d). Commits, graph, `daily`. FIX-A;
  FIX-D for `daily`.
- G3. Nested tags added or moved (E3d, E3e). FIX-A.
- G4. Worktrees added, removed or switched (E4a-E4d). FIX-B.
- G5. Stash replaced at equal size where the index does not move: from another
  worktree (E5f) or within one filesystem time granule (E11b). FIX-A (stash
  tip).
- G6. Flat tag replaced within one time granule on a coarse filesystem (E11a).
  FIX-A.
- G7. HEAD repointed without an index write (E1e) and reflog expiry (E15a).
  Reflog and decorations. Rare. FIX-A.
- G8. Histogram author change (E6a). **Accepted limitation** (OD4): the author
  is re-resolved whenever the histogram is re-probed, and `R` forces that.
  No ticket closes it.
- G9. Histogram after a week rollover (E7). FIX-D.
- G10. Histogram after a sweep in GitHub mode (E14). FIX-D.
- G11. Load path takes its key after its probes (E13): a change during the
  probes is recorded in the key but not in the products, and the next sweep
  reuses the stale products. The comment at main.rs:2780-2781 claims the
  opposite. **Timing-dependent**: a real ordering defect that shows only when
  a mutation lands while the probes run. FIX-E, together with FIX-A's
  ordering rule (6, Interactions).
- G12. Git notes added, changed or removed (E16a; reviewer's run agrees).
  `daily` (a notes commit is yours and `--all` reaches it) and
  `graph_expanded` (default format prints notes). FIX-A (`refs/notes/*` in
  the widened query).
- G13. Shallow boundary moved: `fetch --unshallow` or `--deepen` (E16b,
  reviewer's run). Graph and commits gain rows; `daily` can too. No ref moves,
  so B1 alone misses it. FIX-A (content of `<common>/shallow`).

**Confirmed non-gaps**: commit on HEAD (E2a), flat tag add, move, delete and
annotated (E3a-E3c, E3g), stash push, pop, drop and clear in the same checkout
(E5a-E5c, E5e, E15b), stash and tags made in the main checkout seen from a
linked row (E9b, E9c), branch deletion (E1c, E8c), index loss (E10a), unborn
HEAD (E10b), missing stash log (E10c), a commit on a detached HEAD (E17),
working-tree edits (section 4). Some
are caught only incidentally: E1c, E3f and E8c depend on git rewriting
`packed-refs` on deletion, E1d and E5d on an index write. False invalidations
(cost, not correctness): E2e, E8a, E8d, E10a, and every `git add`.

**Unconfirmed risks**:

- R1. **reftable** (`git init --ref-format=reftable`, git 2.45 and later). Local
  git is 2.39.3 and cannot create one. By reading, such a repo has no
  `packed-refs`, no loose `refs/tags` and no `logs/refs/stash`, so the key
  reduces to `head_sha` and the index: every tag, stash drop and ref change
  would be missed. Not executed.
- R2. **Deletions are caught only because git 2.39.3 rewrites `packed-refs`** on
  every ref deletion (E1c, E3c, E2e). If a newer git skips that rewrite for a
  ref that exists only as a loose file, branch and nested-tag deletions become
  gaps. Not executed on another git version.
- R3. **Clock skew**: `refs_mtime` is a max, so a `packed-refs` mtime in the
  future would hide `refs/tags` changes until the clock catches up. Not
  executed.
- R4. **Filesystems coarser than 1 s** (FAT's 2 s, some network mounts) widen
  G5 and G6. Extrapolated from E11; not executed.
- R5. **Overlapping sweeps of one repo** (a full sweep and `f`, or a moved-remote
  sweep) land in completion order. Each result carries a key and products from
  the same rebuild, so a late older result is re-probed on the next sweep
  (inferred). Not executed; FZ-02 already owns completion order.
- R6. **`refs/replace`** (`git replace --graft` and friends) changes what `git
  log` prints for every gated product and `daily`. Not compared by anyone.
  By reading, FIX-A covers it: replace refs are in the widened query.

## 4. Working-tree state

How dirty state is observed (**Read**): `dirty_count` runs `git status
--porcelain` in the cheap tier on every sweep (main.rs:1970-1973), outside the
key. `rebuild_repo_status`, `build_repo_status`, `dirty_mode_status` and
`local_only_status` all call it unconditionally. Nothing in the code infers a
clean tree from the key or from HEAD or index metadata; the comments at
git.rs:186-188 and main.rs:1970-1971 state the rule. The `--no-optional-locks`
flag (git.rs:17) stops status refreshing the index's stat data but does not
change its answer.

**Executed** (E12a): an unstaged edit plus an untracked file left the key
identical, and `status --porcelain` reported both (2 lines). So the ticket's
rule holds today: unstaged and untracked changes are observed on every sweep
whatever the key says.

Paths that hide working-tree changes from that observation, by configuration
rather than by the cache (**Executed**): repo config
`status.showUntrackedFiles=no` hides untracked files (E12b, 1 line), and
`update-index --assume-unchanged` hides an edit (E12c). Both are the user
telling git to look away; fleetz inherits it, as repoz would. Not a fix
ticket; listed so nobody mistakes them for an invalidation bug.

Constraint for every fix below: `dirty_count` stays in the cheap tier, and no
key may gate it. FIX-A's regression set includes E12a for that reason.

## 5. Options

### 5.1 Today's cost (**Read**, per repo per sweep)

- Cheap tier: `for-each-ref refs/heads/` (1), `git remote` (0 or 1),
  `rev-list --count` (0 to n, one per branch with ahead > 0 or no upstream),
  `status --porcelain` (1), `for-each-ref refs/remotes/` (1), `rev-parse HEAD`
  (1). At least 4 on an idle sweep.
- Gated tier: `log` for commits, `reflog`, `log` for graph, `log --graph`,
  `stash list`, `worktree list`: 6.
- Histogram: `config user.email` (0 or 1) and `log --all` (1), only in Local
  mode and only when re-probed.
- Fetch: 1 when the sweep fetches.

At about 12 ms per process (1, **Executed**), each extra process per repo per
sweep adds about 25% to an idle sweep's 4.

### 5.2 Comparison

| Option | Correctness | Added processes per repo per sweep | Verdict |
|---|---|---|---|
| A. Explicit generation tracking (fleetz bumps a counter on its own `switch`, `delete`, `fetch`) | Sees only fleetz's own actions. Misses every terminal `git` command: G1 to G13 all stay | 0 | Rejected as the mechanism. Branch switch already forces a single-repo sweep; branch delete could do the same (FIX-B), which needs no counter |
| B1. Ref signature from data already fetched: hash (name, full oid) of `branch_statuses` and the widened remotes query plus the HEAD marker; widen that query to every ref and split by prefix; read `<common>/shallow`; stat `<gitdir>/logs/HEAD` and keep `stash_size` | Content-based: closes G1, G2, G3, G6, G7, G12, G13, and with the stash tip G5. The ref part reads through git, so it is immune to timestamp resolution, nesting, packing and the ref backend; the two `stat` parts (`logs/HEAD`, `stash_size`) are not, and on reftable (R1) they read as absent, leaving the stash tip and HEAD to carry those cases | 0 | **Adopted** (OD1) |
| B2. B1, but with a separate `for-each-ref refs/` call | Same as B1 | 1 (about 12 ms) | Not adopted (OD1) |
| B3. Per-product keys (history, decorations, stash, worktrees, daily) | Same as B1, and avoids re-running the `--numstat` log when only a remote ref moved | 0 beyond B1 | Deferred: optimisation, to measure after FIX-A |
| C. Filesystem metadata, extended (recursive walk of `refs/heads`, `refs/remotes`, `refs/tags`, plus `logs/HEAD`, `worktrees/`) | Fails at coarse timestamps (E11a and E11b confirm the mechanism), fails on reftable (R1), and depends on git's lock-and-rename touching the parent directory | 0, but a `readdir` per ref directory | Rejected |

What B1 hashes (OD1): (ref name, full object id) pairs for every ref under
`refs/` (local branches from the branch query; everything else, including
remote-tracking refs, tags, `refs/stash`, `refs/notes/*` and `refs/replace/*`,
from the widened query), plus the HEAD symref (the branch HEAD names, or
detached), plus the bytes of `<common>/shallow` when it exists (a small file
read, content-based, no timestamp; `shallow` lives in the common directory,
**Executed**: a `--depth 1` clone wrote `.git/shallow`). Nothing derived: no
ahead or behind counts, no upstream, no abbreviated names. `head_sha`, the
index components (OD5), `stash_size` and the size of `<gitdir>/logs/HEAD`
stay beside it in the key.

The widened query cannot exclude `refs/heads` on the git here:
`for-each-ref --exclude` is not in git 2.39.3 (**Executed**: the option is
rejected). So it runs with no pattern, lists every ref, and the parser splits
by prefix. Local branches are then listed twice, which costs text, not a
process.

HEAD's object id in the detached case (review ruling 1): the signature does
not need it, because `head_sha` stays in `WorkKey` and already covers every
gated product (**Read**). `commits`, `graph` and `graph_expanded` walk the
local branches, so a detached commit is not in them, and the only thing that
changes for them is the `HEAD` decoration, which moves with `head_sha`.
`reflog` changes only when HEAD moves or is repointed, which `head_sha` and
the `logs/HEAD` size cover. E17 confirms the key moves today. The histogram is
different, because its key (OD3) is separate from `WorkKey`, and `--all`
walks HEAD; so HEAD's object id goes into the histogram key.

Worktrees are not refs, so B1 alone does not close G4. Decided (OD2): run
`worktree list` in the cheap tier only when `<common>/worktrees` exists (one
`stat`, then 1 process for the minority of repos that have linked worktrees).
When the directory is absent the linked list is empty; it is never reused
from the previous snapshot. Git deletes `<common>/worktrees` when the last
linked worktree is removed (**Executed** by the reviewer, and by me: after
`git worktree remove` of the only linked worktree the directory was gone), so
reuse there would leave a stale row forever.
The filesystem fingerprint of `<common>/worktrees/*/{HEAD,gitdir}` was not
adopted.

The histogram gets its own key (OD3), closing G9 and G10: `grid_start`, the
configured author (`contributions_author`, `None` when unset), the B1
signature and HEAD's object id, stored beside `daily` when it is computed and compared before
reuse. The resolved email is not part of it (OD4): it is re-resolved on every
re-probe, as `rebuild_repo_status` already does (main.rs:2017-2020), so G8
remains an accepted limitation until the next re-probe. `R` is the remedy, in
two steps (**Read**): the reload replaces every row (`apply_load_result`,
main.rs:526) with rows built with `daily: None` (`build_repo_status`,
main.rs:2753), and the next Local sweep then re-probes and re-resolves. That
sweep comes from the watch timer or from `r`; `R` alone does not start one,
so until it runs the local strip is empty rather than stale.

**Adopted design**: B1 + worktree list when linked worktrees exist + the
histogram key + every key input read before the gated probes. Cost: zero added processes on an idle
sweep for a repo without linked worktrees; one for a repo with them. The one
scaling property: the widened query's output grows with the number of refs
(mostly tags, plus any other namespace a repo carries), as text only, with no
extra process. The key gains a hash and a few `stat`
calls. Re-probe frequency rises only where the answer actually changed, and
false invalidations from `pack-refs`, `gc` and `fetch --prune` (E8a, E8d, E2e)
disappear because packing changes no ref content.

## 6. Fix tickets

Each ticket keeps `dirty_count` ungated, changes no persistence or cache
format (`WorkKey` is in memory only; it derives `Debug, Clone, PartialEq, Eq`
and no serde, git.rs:103), and adds regression tests in `#[cfg(test)]` beside
the code. All fixture steps use a temp repo with per-repo identity.

### FIX-A. Ref-content signature in `WorkKey`

- Outcome: the key changes whenever any local branch tip, remote-tracking tip,
  tag (any depth), `refs/stash` tip or the HEAD symref changes, or the HEAD
  reflog grows or shrinks; it no longer changes on `pack-refs` or `gc` alone.
  It also changes on notes, replace refs and any other ref namespace, and on
  a change to `<common>/shallow`. The hash covers (ref name, full object id)
  pairs, the HEAD symref and the shallow file's content, nothing derived
  (OD1). Replaces `refs_fingerprint`.
- Outcome, failure: a cheap-tier query that fails (process error, timeout,
  non-zero exit) yields a `None` key, so two failed sweeps in a row cannot
  compare equal and reuse. Today `branch_statuses` (git.rs:243-248) and
  `remote_branches` (git.rs:929-934) turn failures into `Vec::new()`, which
  is indistinguishable from "no refs"; FIX-A needs a fallible form of both
  for the key while the callers keep their current empty-list behavior.
- Outcome, full ids: the queries add `%(objectname)` as an extra field.
  `BranchStatus::tip` and `RemoteBranch::tip` stay abbreviated
  (`%(objectname:short)`, git.rs:238, 923), because the branches view matches
  them against `%h` hashes in the graph (`App::inspected_hash`,
  app.rs:1831-1832).
- Outcome, callers: `remote_branches` has four callers (main.rs:1974, 2169,
  2652, 2748). After widening, its parser still returns only `refs/remotes/`
  entries to them; the other refs go only to the signature. Otherwise tags,
  notes and the stash would appear in the branches view, which lists
  `remote_branches` after the locals (`App::selected_commit_count`, app.rs:1747;
  ui/detail.rs:995).
- Outcome, ordering: every key input, the widened `for-each-ref` and the
  shallow read included, is read before the gated probes, on both the sweep
  path (`rebuild_repo_status`, where the cheap tier already runs first,
  main.rs:1972-1980) and the load path (`build_repo_status`, where
  `remote_branches` runs after the gated probes today, main.rs:2748). See
  Interactions for the overlap with FIX-E. Corrects the comments
  this change would otherwise falsify: the `WorkKey` doc (git.rs:100-102,
  including the "Known limit" note at 131-134), the `work_key` doc
  (git.rs:186-188) and the `HISTORY_COUNT` doc (main.rs:38-40).
- Files: `src/git.rs` (`WorkKey`, `work_key`, the widened query and its
  parser, fallible variants for the key), `src/main.rs` (`rebuild_repo_status`
  and `build_repo_status` pass the cheap-tier output into the key and read it
  before the gated probes).
- Depends on: FZ-03 integrated (owns `src/main.rs`); must not run beside FZ-04,
  which also owns `src/git.rs` and `src/main.rs` (OD6). Uses FIX-C if the
  helper has landed, otherwise keeps today's inline common-dir code.
- Regression scenarios (each: key before, mutate, key after, assert not equal):
  E2b (commit, push to a local bare remote), E2c (fetch another clone's push),
  E1a (`branch -f side <commit>`), E1b, E8b (`pack-refs --all` then move a
  branch), E3d and E3e (nested tags), E9a and E9d (commit in the other
  checkout of a linked pair, key of this one), E1e, E15a, E5f, E16a (`git
  notes add`), E16b (`fetch --unshallow` from a local bare remote into a
  `--depth 1` clone made with a `file://` URL). Plus: E8a and E8d leave the key
  equal; a failed query yields `None` (for example, a path whose `.git` is
  unreadable, or a repo where `for-each-ref` exits non-zero), and two such
  keys never compare as unchanged; beside
  `remote_branches_drop_the_default_branch_symref` (git.rs:1318), a test that
  `remote_branches` on a repo with a tag, a stash and a note still returns only
  `refs/remotes/` entries; `tip` stays abbreviated; the four existing
  `work_key` tests still pass; E12a still reports dirty with an unchanged key.
- Approval: none expected. It changes an internal type only.

### FIX-B. Worktree list freshness

- Outcome: `worktrees` reflects `worktree add`, `remove` and a linked
  worktree's branch switch on the next sweep.
  Corrects the comments this falsifies: "worktree" in the `WorkKey` and
  `work_key` docs (git.rs:100-102, 186-188, as rewritten by FIX-A) and in the
  `RepoStatus::work_key` doc (model.rs:211-215).
- Files: `src/main.rs` (`rebuild_repo_status`), `src/git.rs` (a cheap
  "has linked worktrees" check via the common dir, and the doc comments),
  `src/model.rs` (doc comment only).
- Depends on: FIX-C (common dir), FZ-03.
- Rule: an absent `<common>/worktrees` means an empty linked list, never
  reuse of the previous one (OD2).
- Regression: E4a, E4b, E4c, E4d (worktree list before and after the sweep),
  and "remove the only linked worktree": add one, sweep, `git worktree remove`
  it, sweep; the row's `worktrees` must be empty, and `<common>/worktrees`
  must be gone, which proves the absent-directory path was taken.
  Also: a repo with no `<common>/worktrees` runs no `worktree list` in an
  unchanged-key sweep (assert through the probe count, or leave to review if
  no seam exists).
- Decided: OD2 (a). No further approval.
- Also here: run a single-repo sweep after the branch-delete overlay
  (main.rs:1015-1028), as the switch path already does (1066-1068), so the
  branches view does not rely on R2.

### FIX-C. Common-directory helper

- Outcome: one function, for example `git::repo_dirs(path) -> Option<RepoDirs
  { git_dir, common_dir }>`, used everywhere fleetz reads git metadata by path.
  It must: resolve `.git` as a directory or a `gitdir:` file, relative or
  absolute (today's `resolve_git_dir`); resolve `<git_dir>/commondir`, relative
  or absolute (today inline at git.rs:198-201); return `common_dir == git_dir`
  when there is no `commondir`, which covers submodule checkouts; and say which
  files live where: per worktree are `HEAD`, `index`, `logs/HEAD` and
  `FETCH_HEAD` (**Executed**: a fetch in a linked worktree wrote
  `.git/worktrees/lw/FETCH_HEAD`); in the common dir are `refs/`,
  `packed-refs`, `logs/refs/`, `config` and `worktrees/`.
- Consumers: `work_key` (git.rs:196-201), `last_fetch_at` (git.rs:178-184, keeps
  the per-worktree `FETCH_HEAD`), and, for FZ-05 decision D10, discovery's
  origin read (`discovery::origin_slug`, src/discovery.rs:175-179), which today
  reads only a `.git` directory's `config`. With the helper, a linked
  worktree's origin is readable from `<common_dir>/config`. Whether discovery
  should use it is D10's question, not this helper's: the doc comment at
  src/discovery.rs:171-174 keeps worktrees on the folder-name match on purpose,
  so a worktree cannot race its main clone for the row. **Executed**: `git
  remote get-url origin` in a linked worktree returned the main clone's origin,
  which is the item
  [repository-identity-design.md](repository-identity-design.md) lists to
  confirm for D10.
- Files: `src/git.rs` only. Switching discovery to it is a matching-rule change
  under the FZ-05 series and needs its own ticket and review.
- Depends on: nothing in this series; can land first (OD6). Must not run
  beside FZ-04, which also owns `src/git.rs` (its fetch result type).
- Regression: a linked worktree, a relative and an absolute `commondir`, a
  submodule-style `gitdir:` file with no `commondir`, and a plain clone; assert
  both directories.
- Note for FZ-04 and FZ-11: because `FETCH_HEAD` is per worktree, a fetch in
  the main checkout leaves a linked worktree's `last_fetch_at` older, so
  `stale_remotes` (main.rs:2241-2263) fetches it again. Extra cost, not wrong
  data (**Inferred**).

### FIX-D. Histogram key

- Outcome: `daily` is reused only when it was computed for the same
  `grid_start`, the same configured author (`contributions_author`), the
  same ref signature (FIX-A) and the same HEAD object id (OD3). HEAD's id is
  needed because `--all` walks HEAD and the signature records only that HEAD
  is detached; the value is `WorkKey::head_sha` from the same sweep. A sweep without the probe keeps `daily`
  and the key it was computed under, not the new `WorkKey`. When
  `contributions_author` is unset, the email is resolved on every re-probe and
  is not compared on reuse (OD4); G8 stays an accepted limitation.
  Corrects the comment at main.rs:2010-2012, which says the histogram is
  re-run "only when the WorkKey says this repo moved".
- Files: `src/main.rs` (`rebuild_repo_status`), `src/model.rs` (a sweep-owned
  field next to `daily`, classified in `apply_local_sweep`'s exhaustive
  destructure), possibly `src/contributions.rs`.
- Depends on: FIX-A; FZ-03 (`src/main.rs`, `src/app.rs`). Decided by OD3 and
  OD4.
- Regressions that tell FIX-D apart: E7 (a sweep with probe start 2023-10-01,
  then one with 2023-10-08 on an unchanged repo: the second must re-probe);
  E14 (Local sweep, commit, sweep with no probe, Local sweep: the third must
  re-probe); detached HEAD (Local sweep, `git checkout --detach`, commit as the
  configured author, Local sweep: the count must rise by one). E7 and E14 fail
  today. The detached case passes today, because `head_sha` moves the
  `WorkKey` (E17), and fails against a histogram key without HEAD's id, which
  is the regression it guards. E2c (fetch of your own commit from another
  clone) is closed by FIX-A already; keep it as a check, not as FIX-D's
  evidence.
- Regression (passes today, pins OD4): with `contributions_author` unset, set
  repo-local `user.email` to A, commit as A, run a Local rebuild; change
  `user.email` to B, commit as B, and run a rebuild that re-probes (for
  example with `old.work_key: None`): the histogram counts B's commit, not
  A's. This is straightforward: `rebuild_repo_status` already calls
  `git::user_email` inside the re-probe arm (main.rs:2016-2020), and
  `main.rs`'s own `#[cfg(test)]` module can build a `SweepTarget` for a temp
  repo and call the private function directly. No new seam is needed. E6a is
  not a must-fail case: an unchanged key reusing A's histogram is the accepted
  behavior.
- Approval: none expected; in-memory only.

### FIX-E. Load-path key order

- Outcome: `build_repo_status` reads every key input before its gated
  probes, as `rebuild_repo_status` already does, and the comment at
  main.rs:2780-2781 is corrected. Today that means the `work_key` call; once
  FIX-A has landed it also means the widened `for-each-ref` (today
  `remote_branches`, main.rs:2748) and the shallow read.
- Files: `src/main.rs`.
- Depends on: FZ-03. Lands as its own small ticket (OD6). If FZ-12
  (progressive loading) restructures this function first, the order fix must
  carry over into it.
- Regression: E13 needs a mutation between the probes and the key. A
  deterministic test needs a seam (a probe hook or splitting the key read from
  the probes); without one, review the order by reading. Say which in the PR.

### Interactions

- **FZ-03, FZ-04**: they change refresh state in `src/main.rs` and `src/app.rs`,
  and FZ-04 also owns `src/git.rs` (its fetch result type). FIX-A, B, D and E
  touch `src/main.rs` and land after FZ-03 is integrated. FIX-C and FIX-A
  touch `src/git.rs` and must not run beside FZ-04. The order between this
  series, FZ-04 and the FZ-05 series is set by the coordinator when the
  tickets are promoted (OD6).
- **FIX-A and FIX-E**: both move key inputs ahead of the gated probes in
  `build_repo_status`. Moving only the `work_key` call (FIX-E alone) leaves
  the widened ref query after the probes once FIX-A exists, which recreates
  G11 for remote refs, tags, notes and the stash. Whichever of the two lands
  second finishes the job: every key input read before the gated probes on
  both the load and the sweep path.
- **FZ-05 series**: sweep-owned data is keyed by checkout path, which this design
  keeps: every key above is per checkout. D10 needs FIX-C (common-dir config
  read), then a discovery ticket.
- **FZ-11 (scheduler)**: if probes move off the sweep, the cheap tier that feeds
  FIX-A's signature must still run before any gated reuse, and `dirty_count`
  must keep its own cadence. The key must be taken before the probes it gates.
- **FZ-12 (progressive loading)**: if history loads on demand, each product
  needs the key it was computed under (B3). FIX-A's signature is the input to
  that.

### Follow-ups outside this design

- `daily` counts the latest stash's WIP and index commits as contributions
  (3.4). A separate ticket (OD7), not one of FIX-A to FIX-E.
- Per-product keys (B3), only if measurements after FIX-A justify them.
- Dropping the index from the key (OD5), after FIX-A, with measurements.

## 7. Reviewer reproduction (G1, the push)

Self-contained; needs only `git` and macOS `stat` (on Linux, use
`stat -c '%.9Y/%s'` for `stat -f '%Fm/%z'`, and `stat -c '%.9Y'` for
`stat -f '%Fm'`). It prints every key component separately, which is stricter
than the key itself (the key keeps only the max of the two mtimes), so
"identical" here implies an identical `WorkKey`.

```sh
set -eu
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid
export GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid
T=$(mktemp -d "${TMPDIR:-/tmp}/fz06-repro.XXXXXX")
git init -q --bare -b main "$T/origin.git"
git init -q -b main "$T/repo"
cd "$T/repo"
echo one > a && git add a && git commit -qm one
git remote add origin "$T/origin.git"
git push -q -u origin main
echo two > b && git add b && git commit -qm two
key() {
  printf 'head=%s ' "$(git rev-parse HEAD)"
  printf 'index=%s ' "$(stat -f '%Fm/%z' .git/index)"
  printf 'stash=%s ' "$(stat -f '%z' .git/logs/refs/stash 2>/dev/null || echo 0)"
  printf 'packed=%s ' "$(stat -f '%Fm/%z' .git/packed-refs 2>/dev/null || echo none/0)"
  printf 'tagsdir=%s ' "$(stat -f '%Fm' .git/refs/tags)"
  printf 'loose_tags=%s\n' "$(ls -A .git/refs/tags | wc -l | tr -d ' ')"
}
graph() { git log --color=never --decorate=full --format='%h|%p|%D|%aN|%s' -n 200 main; }
key > "$T/key.before"; graph > "$T/graph.before"
git push -q origin main
key > "$T/key.after"; graph > "$T/graph.after"
echo "--- key"; diff "$T/key.before" "$T/key.after" && echo "key: identical"
echo "--- graph"; diff "$T/graph.before" "$T/graph.after" || true
echo "fixture: $T"
```

Run it with `bash`. Expected, as observed (hashes differ per run):

```text
--- key
key: identical
--- graph
1,2c1,2
< 8f3ba92|33514a5|HEAD -> refs/heads/main|Fixture|two
< 33514a5||refs/remotes/origin/main|Fixture|one
---
> 8f3ba92|33514a5|HEAD -> refs/heads/main, refs/remotes/origin/main|Fixture|two
> 33514a5|||Fixture|one
```

The `graph` arguments are those `rebuild_repo_status` passes to `git::graph`
(main.rs:2003, git.rs:685-702) for a repo whose only local branch is `main`.
With every key component unchanged, `rebuild_repo_status` takes the reuse
branch (main.rs:1981-1991), so fleetz keeps drawing `origin/main` on commit
`one` after the push. Delete `$T` afterwards.

## 8. Code statements this contradicts

Each entry names the fix ticket that must correct it. Where two tickets touch
one comment, both are named, and the later one must leave it true for its own
change.

- git.rs:100-102 (`WorkKey` doc): "Equal keys across two sweeps mean log,
  reflog, graph, stash, and worktree output cannot have changed". Contradicted
  by G1 to G7. **FIX-A** rewrites it for the ref signature. **FIX-B** then
  removes "worktree" from it, since worktrees stop being gated by the key.
- git.rs:131-134 (`WorkKey::refs_mtime` doc, "Known limit" on nested tags).
  G3. **FIX-A**, which removes `refs_fingerprint` and this field.
- git.rs:186-188 (`work_key` doc): "if none of these moved, the expensive
  probes (log, reflog, graph, stash, worktree) cannot have new answers".
  Same gaps. **FIX-A**, and **FIX-B** for the word "worktree".
- model.rs:211-215 (`RepoStatus::work_key` doc): "reuses
  commits/reflog/graph/stash/worktrees". True today; **FIX-B** falsifies the
  "worktrees" part and must correct it.
- main.rs:2010-2012 (`rebuild_repo_status`, histogram): "only when the WorkKey
  says this repo moved". Contradicted by G9 and G10, and false once the
  histogram has its own key. **FIX-D**.
- main.rs:2780-2781 (`build_repo_status`): "Taken last, after every probe above,
  so the first sweep that finds it unchanged can safely reuse all of them". G11:
  taking it last is the unsafe order. **FIX-E**.
- main.rs:38-40 (`HISTORY_COUNT` doc) describes the key as HEAD, index and stash
  log; it also covers flat tags since the tag fix. Stale, harmless today.
  **FIX-A**, which changes what the key covers.

Nothing found contradicts `CLAUDE.md`, which does not describe `WorkKey`. One
related observation: the commit list and the graph walk the same refs, as
`CLAUDE.md` requires, and both go stale together under G2. Meanwhile the
always-probed `branches` list shows the new tip, so the branches view and the
graph can disagree for as long as the gap lasts.
