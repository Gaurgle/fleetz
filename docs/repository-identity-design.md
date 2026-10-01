# Repository identity and collision behavior (FZ-05)

Design for [FZ-05](https://github.com/Gaurgle/fleetz/issues/18).
Investigation only: no production code changed. Evidence is from reading the
code at base `6c72926` (main, FZ-02 integrated). Nothing was executed, so every
claim is labeled **confirmed by test** (a named existing test), **confirmed by
reading** (cited lines), or **unconfirmed**. Line numbers drift: each one is
paired with its symbol.

Decisions recorded below were made by the coordinator (claude-fable-5-1) on
2026-10-01, including the rulings on the first review. Astra has not reviewed
them. Andreas can veto any of them.

## Summary

fleetz has no single notion of "which repository". The GitHub dataset is
deduplicated by **bare name** before anything else sees it
(`github::merge_sources`, github.rs:460-461), so of `alice/api` and
`company/api` only the more recently pushed one survives, and the other is
absent from the dashboard entirely, not even "not cloned". Discovery's
owner-aware remote match (`discovery::Wanted`) then has nothing to
disambiguate, and its folder-name fallback lets any checkout whose folder has
the right name claim a GitHub row whatever its `origin` says. Every later
lookup (`build_repo_status`, `stale_remotes`, `sort_by_activity`,
`kind_by_name`, `apply_github_poll`, `remotes_that_moved`, groups) keys on the
bare name, case-insensitively, taking the first hit. Local data is already
keyed correctly by path (`apply_repo_updated`, `refresh_fetches`). The FZ-02
field-ownership split maps directly onto the fix. GitHub-owned fields are
keyed by a **remote identity** (`owner/name`, host carried, lowercased) and
live on exactly one row per remote identity, marked by the existing `owner`
field. Sweep-owned fields are keyed by a **checkout identity** (the path). The
chosen model (A) needs no new field and no cache or config format change. It
lands as six serial tickets (FZ-05a to f, one of them a pure test-seam
refactor) plus four optional ones. It changes three documented invariants in
`CLAUDE.md`, and the pass order wording adds a fourth (section 3.10).

## Decisions

| # | Question | Outcome | Where it lands |
|---|---|---|---|
| D1 | Core model | **Model A accepted**: rows keyed by remote identity, extra checkouts are local rows, keys derived from existing fields. Name-only matching exists only for legacy rows (m2). A LocalOnly row is never matched by name, at load or in the poll | 3.2, 3.3, 3.5 |
| D2 | Host scoping | **Accepted**: parse and carry the host, keep matching host-agnostic. Cost: a same-`owner/name` repo on another host claims the github.com row | 3.4, S11b |
| D3 | Folder fallback when `origin` parses but matches nothing wanted | **Allowed only when** the origin's owner equals the owner of the wanted repo the folder name points at, **and** the origin's `owner/name` is not in the full dataset. An owner-less (legacy) candidate keeps today's fallback. "Not in the full dataset" means "not known to fleetz", an accepted limitation | 3.3 rule 3, S9b to S9d, S12b |
| D4 | User view of an ambiguous name fallback | **Accepted**: claim none, one status line naming the first ambiguous path in sorted path order (o3). Ambiguity is judged on owner-distinct names in the **full** dataset (M1) | 3.3 rule 2, S3, S3b |
| D5 | Extra checkouts of one remote | **Accepted**: pass 3 dedupes by canonical path only (raw path when `canonicalize` fails); extra checkouts appear as local rows when dirty. Restores parity with repoz. Andreas may veto before FZ-05f is promoted. Accepted cost: the local contribution strip double-counts an extra checkout (M6) | 3.3, 3.10, FZ-05f |
| D6 | GitHub fields on LocalOnly rows | **One carrier per remote identity.** The load decides the carrier and marks it by setting `owner`; a row carries GitHub fields if and only if the load gave it an owner (legacy rows excepted, m2). A LocalOnly row carries only when its stored `remote_url` names a dataset repo that no Primary or Elsewhere row carries | 3.1, 3.5, S4, S5, S13b, S14, S17 |
| D7 | `owner/name` members in config groups | **Deferred**. FZ-05h is not promoted | 3.8, FZ-05h |
| D8 | Struct literals in existing test fixtures when a struct gains a field | **Allowed** as a compile-forced fixture update, under conditions in FZ-05b. Not extended to any other kind of test edit | FZ-05b, FZ-05f |
| D9 | Two visible rows with one name | **Default for FZ-05g**: `owner/name` on colliding rows only, compact columns sized from the drawn label (o1). Andreas accepts the look; FZ-05g stops at Ready to integrate | 3.9, FZ-05g |
| D10 | Linked worktree remote in passes 1 and 2 | **Accepted**: keep the folder-name fallback; revisit with FZ-06. A worktree row can still be the carrier through its stored `remote_url` (m1) | 3.3, 3.5, S6 |
| M3 | Ownership of the ambiguity status line | Discovery returns ambiguities as data (FZ-05b); FZ-05d carries the line to `App` | FZ-05b, FZ-05d |
| M4 | Interim states between tickets | **Assembly and poll land as one ticket (FZ-05d)**, chosen from the walk in section 5.1. The pass 3 bare-name filter and the LocalOnly name claim change only in FZ-05f. An interim gap remains from FZ-05b to FZ-05e for fleets with two same-named repos; nothing is tagged and Andreas does not install from `main` in that span (N1) | 5.1 |
| M5 | Pass order invariant | Recorded as the fourth `CLAUDE.md` invariant change, needs Andreas's sign-off | 3.10 |
| M7 | Load-side test seam | **Own ticket FZ-05c**, a pure refactor before assembly changes | FZ-05c |
| m3 | Clone whose `origin` is the upstream | **Accepted regression** (S12b). Optional follow-up FZ-05j, not promoted | S12b, FZ-05j |
| o2 | Primary checkout order | An origin on host `github.com` beats other hosts | 3.3 |
| Order | Ticket order | **a, b, c, d, e, f** (letters follow the order; see 5.2 for the mapping from the first draft) | Section 5 |

## 1. Identity map (facts)

"Owner-aware" means two repos with the same name and different owners are
kept apart. "Case" says how the key is compared. "On collision" is what
happens when two candidates share the key. All facts confirmed by reading the
cited lines unless noted.

### 1.1 GitHub API and dataset

| Site | Key | Owner-aware | Case | On collision |
|---|---|---|---|---|
| `github::build_query` repo fields, github.rs:360-379 | Selects `name` and `owner { __typename, login }`; no `nameWithOwner`, `url` or `id` | Data carries owner | n/a | n/a |
| `github::RepoNode` github.rs:211-218, `OwnerNode` github.rs:221-225 | `name: String`, `owner: Option<OwnerNode>`, `login: Option<String>` | Optional | n/a | n/a |
| `github::merge_sources` github.rs:451-463 | `name.to_lowercase()` in a `HashSet` after sorting newest push first | **No** | Lowercased | **Older-pushed repo of the same name is dropped from the dataset**. Applies to the viewer list and org lists alike |
| `github::parse_repos` github.rs:611-643 | Copies `name`, sets `owner` from `login` (line 628) | Carries it | Preserved | n/a |
| `github::GithubRepo` github.rs:9-21 | `name`, `owner: Option<String>` | Carries it | Preserved | n/a |
| `github::filter_since` / `filter_before` github.rs:724-739 | Time only | n/a | n/a | n/a |
| `github::window_exceeds_dataset` github.rs:745-758 | Count of dataset rows against `repo_limit` | n/a | n/a | Count is after name dedupe, so it can under-report |
| `cache::ApiCache` cache.rs:21-37 | File `github-{repo_limit}.json`; raw response body | Payload carries owner; file key has no account or host | n/a | One file per `repo_limit` for every account and host (FZ-09) |

### 1.2 Discovery

| Site | Key | Owner-aware | Case | On collision |
|---|---|---|---|---|
| `discovery::Wanted::new` discovery.rs:24-35 | `by_remote`: lowercased `owner/name` to GitHub name; `names`: lowercased names | `by_remote` yes; `names` no | Lowercased | Two owners of one name both map to the same bare name value; never happens today because `merge_sources` already dropped one |
| `Wanted::without` discovery.rs:38-48 | Filters both maps by **bare** found name | No | Lowercased | Finding `alice/api` removes `company/api` from pass 2's search |
| `Wanted::match_clone` discovery.rs:66-75 | `origin_slug` in `by_remote` first, then `matched_name(folder)` | Remote yes; fallback **no** | Lowercased | **Fallback is unconditional**: a checkout whose `origin` names a different owner still claims the row if its folder name matches |
| `discovery::origin_slug` discovery.rs:175-179 | `.git/config` `origin` URL parsed by `model::repo_from_remote_url`, lowercased `owner/name` | Yes | Lowercased | **Host discarded**. `insteadOf` rewrites not applied. A `.git` file (linked worktree, submodule) yields None |
| `discovery::origin_url` discovery.rs:182-195 | First `url` under `[remote "origin"]` only | n/a | Key name case-insensitive | `upstream` ignored. Confirmed by test `origin_url_reads_only_the_origin_remote` |
| `model::repo_from_remote_url` model.rs:383-401 | First two path segments after host; strips `.git` suffix | Yes | Preserved (callers lowercase) | Host never checked (doc comment model.rs:380-382). Confirmed by tests `owner_parsed_from_every_remote_url_form`, `repo_parsed_with_the_git_suffix_stripped` (includes the `github-work` alias) |
| `discovery::matched_name` discovery.rs:154-162 | Folder name, then folder without leading dot, against lowercased names | No | Lowercased compare, **returns the folder's spelling** | Any folder named like any wanted repo matches. Confirmed by tests `find_in_primary_is_case_insensitive` (returns `MyRepo` for GitHub `myrepo`), `find_in_primary_matches_a_dot_prefixed_clone` |
| `discovery::find_in_primary` discovery.rs:90-109 | Roots in configured order (`repo_dir` first, then `extra_dirs`); `seen` set of lowercased returned names | No | Lowercased | **First match wins**: an earlier root beats a later one; within one root, `read_dir` order decides. Confirmed by test `find_in_primary_dedupes_across_overlapping_roots` (same path via two roots only) |
| `discovery::find_elsewhere` discovery.rs:115-144 | No dedupe of its own | No | n/a | All matches returned; `assemble_view` keeps the first (walk order) |
| `discovery::repo_name_for_path` discovery.rs:200-206 | Folder basename, leading dot stripped | No | Preserved | n/a. Confirmed by test `repo_name_for_path_strips_a_leading_dot` |
| `discovery::find_all_local` discovery.rs:215-226 | `PathBuf` as given (not canonicalized), maxdepth 4 against pass 1's 3 | n/a | Byte-exact | Same directory through two root spellings (symlink) yields two candidates; the later name dedupe hides it |
| `discovery::FoundRepo` discovery.rs:6-9 | `name`, `path` | **No owner field** | n/a | n/a |

### 1.3 Assembly (`main.rs::assemble_view` and helpers)

| Site | Key | Owner-aware | Case | On collision |
|---|---|---|---|---|
| `kind_by_name` main.rs:2373-2376 | Lowercased name over the **full** dataset | No | Lowercased | `HashMap::collect` keeps the last entry |
| `Wanted::new` call main.rs:2404-2405 | `(owner, name)` of the window-filtered `active` | Yes | n/a | See 1.2 |
| `found_set` main.rs:2409-2412 | Lowercased found names | No | Lowercased | Feeds `Wanted::without` |
| home lookup main.rs:2415 | `dirs::home_dir()` read inside `assemble_view` | n/a | n/a | No test can run assembly without the real `$HOME` (M7) |
| pass 1/2 dedupe `seen` main.rs:2421-2432 | Lowercased found name | No | Lowercased | Primary beats Elsewhere (load-bearing, kept); within a pass, first wins |
| `sort_by_activity` main.rs:2318-2332 | Lowercased name to rank | No | Lowercased | Confirmed by test `activity_order_survives_a_case_mismatch` |
| `stale_remotes` main.rs:2241-2263 | Lowercased name to `pushed_at` | No | Lowercased | `HashMap::collect` keeps the last entry |
| `build_repo_status` `gh_match` main.rs:2728-2730 | First dataset entry whose name equals `found.name` | **No** | `eq_ignore_ascii_case` | Row takes that entry's CI, PRs, kind **and owner**, even when the checkout's `origin` names another owner |
| `unclaimed_local` main.rs:2341-2356 | Skips claimed paths, then names in `checked_names` | No | Lowercased | A second checkout whose folder has a claimed name is dropped, **even when dirty**. Confirmed by test `a_clone_passes_1_and_2_claimed_is_not_listed_again_as_local_only` (the path half) |
| pass 3 glyph main.rs:2508-2511 | `kind_by_name` | No | Lowercased | A local-only repo takes the glyph of any same-named dataset repo |
| pass 3 claim main.rs:2529-2535 | `checked_names.insert(name)` | No | Lowercased | Second dirty local-only repo with the same folder name is dropped; a pass 3 row also suppresses a same-named NotCloned row |
| not-cloned loop main.rs:2539-2574 | `checked_names.contains(name)` | No | Lowercased | Does **not** insert, so two same-named dataset entries would both become rows (unreachable today, reachable once `merge_sources` keeps both) |
| `local_only_status` main.rs:2627-2688 | `owner: None`, `remote_url` from `git remote get-url`; drops a clean repo (2637-2639) and one failing `local_repo_is_current` (2645-2648) | n/a | n/a | n/a |
| `dirty_mode_status` main.rs:2139-2203 (`-d`) | Folder name; no GitHub | n/a | n/a | No dedupe beyond path |

### 1.4 Live updates after FZ-02

| Site | Key | Owner-aware | Case | On collision |
|---|---|---|---|---|
| `apply_repo_updated` main.rs:1665-1678 | `RepoStatus.path` equal to the sweep's tilde display path | n/a (checkout key) | Byte-exact | Late result for a vanished path is dropped. Confirmed by tests `a_late_result_for_a_removed_repo_adds_no_row`, `a_sweep_result_keeps_identity_groups_and_selection` |
| `RepoStatus::apply_local_sweep` model.rs:480-530 | Takes sweep-owned fields only; `name`, `path`, `owner`, `remote_url`, `groups`, `location` are load-owned | n/a | n/a | n/a |
| `SweepTarget` main.rs:1602-1610, `sweep_targets` main.rs:1828-1842 | `display_path` (tilde form) | n/a | Byte-exact | n/a |
| `apply_github_poll` main.rs:1712-1719 | For each row, first new-dataset entry whose name equals `repo.name` | **No** | `eq_ignore_ascii_case` | Applies to **every** row with a matching name, including `LocalOnly` rows that the load gave no GitHub fields |
| `RepoStatus::apply_github` model.rs:445-466 | Ignores `name` and `owner` (the match key is the caller's) | n/a | n/a | n/a |
| `remotes_that_moved` main.rs:1760-1779 | Name match old vs new dataset, then `position` of first row with a path and that name | No | `eq_ignore_ascii_case` | Fetches the first same-named row with a path, which may be a local-only row |
| watch tick poll trigger main.rs:665-676 | `last_github_poll` None or older than the interval | n/a | n/a | A cache-served launch leaves it None (main.rs:539-541), so the first tick polls |
| `App::settle_refresh_fetch` app.rs:1444-1448, `refresh_fetches` app.rs:1257 | Display path | n/a | Byte-exact | n/a |

### 1.5 App state and UI

| Site | Key | Owner-aware | Case | On collision |
|---|---|---|---|---|
| `App::selected` and `clamp_after_reload` app.rs:1464-1472 | **Index** into `visible_repos`, clamped after a reload | No identity at all | n/a | A reload that reorders rows moves the cursor to another repo |
| `App::visible_repos` app.rs:1549-1590 | Group membership via `r.groups`; fuzzy on `r.name` (line 1569) | No | Fuzzy is case-insensitive | Same-named rows rank equally |
| `App::local_calendar` app.rs:2112-2121 | Sums `daily` over every visible row | No key | n/a | Two checkouts of one repo count the same commits twice. Happens today when a differently named copy is dirty (S5) |
| `section_rank` app.rs:302-308 | `location` | n/a | n/a | n/a |
| `apply_groups` main.rs:416-425 | Config member string against `repo.name` | No | `eq_ignore_ascii_case` | A member names every row with that name |
| branch overlay Enter main.rs:1048-1075 | Switches by the overlay's `repo_path`, then re-sweeps `selected_repo_index()` (line 1066) | n/a | n/a | If a reload landed while the overlay was open, the re-sweep can target a different row than the one switched |
| `f` key main.rs:1376-1384 | `selected_repo_index()` at keypress | n/a | n/a | n/a |
| repo list row `ui/detail.rs` line 344, compact cell `ui/compact.rs` line 159 | Draws `repo.name` only | No | As stored | Two rows read identically |
| `compact::column_count` ui/compact.rs:108-127 | Column width from the widest `r.name` | n/a | n/a | Any longer label drawn in a cell must be measured here too, or cells overflow |
| `owner_spans` / `name_spans` ui/detail.rs:393-430 | `RepoStatus::owner_display` (model.rs:432-437): API owner, else owner parsed from `remote_url` | Yes | As stored | API owner wins over the URL; with a wrong `gh_match` the pane shows one owner over a URL naming another. Confirmed by test `owner_display_prefers_the_api_over_the_remote_url` |
| remote line ui/detail.rs:711-720 | `repo.remote_url` verbatim | n/a | n/a | n/a |
| `badges::glyph_span` ui/badges.rs:25 | `repo.repo_kind` | n/a | n/a | Inherits whatever matching set it |

### 1.6 Persisted state

| Store | Repo identity in it | Notes |
|---|---|---|
| `config.groups` config.rs:36, `HashMap<String, Vec<String>>` | Bare repo names | Confirmed by test `parses_groups`. Matched case-insensitively by `apply_groups` |
| API cache `~/.cache/fleetz/github-{repo_limit}.json` cache.rs:21-37 | None of its own; raw GraphQL body, which includes `owner.login` | Any identity can be derived after parse without changing the file |
| Window state `~/.cache/fleetz/window.toml` window_state.rs:8-47 | None | Time window only |
| `config::save_settings` | None | Pane sizes and settings keys |

No other per-repo state is persisted (confirmed by reading: the only
`HashMap`/`HashSet` fields outside discovery and assembly are
`BranchOverlay::previews`, keyed by branch, and `App::refresh_fetches`, keyed
by display path).

## 2. Failure analysis

What the user sees today, walked through the code above.

**F1. `alice/api` next to `company/api`.** Setup: both in the dataset,
`alice/api` pushed later, clones at `~/repos/api` (origin `company/api`) and
`~/repos/alice-api` (origin `alice/api`). `merge_sources` keeps only
`alice/api`. `Wanted` holds `alice/api` and the name `api`. In pass 1,
`alice-api` matches by remote and `api` matches by folder name; whichever
`read_dir` yields first wins the row named `api` (`find_in_primary` `seen`).
That row takes `alice/api`'s CI, PRs and owner (`build_repo_status`
`gh_match`). If `~/repos/api` wins, the pane shows owner `alice` above a
remote URL naming `company`, with company's local state under alice's CI. The
loser shows up in pass 3 only if dirty, and only if its folder name is not
`api`. `company/api` never appears, not even as not cloned. **Confirmed by
reading** (github.rs:459-461, discovery.rs:66-75 and 92-104,
main.rs:2728-2738). Which clone wins is **unconfirmed**: both sit in one root,
so it depends on filesystem order.

**F2. Two clones of one repo.** `~/repos/fleetz` and `~/repos/old/fleetz`,
both origin `Gaurgle/fleetz`. Pass 1 keeps the first in walk order. The other
is hidden from pass 3 because its name is in `checked_names`, even when it
holds uncommitted work. A second clone in a folder with a different name
(`~/repos/fleetz-copy`) is instead shown as a local-only row when dirty,
because `unclaimed_local` checks the folder name. The same situation renders
two different ways depending on a folder name. **Confirmed by reading**
(main.rs:2499-2535). The walk-order winner within one root is
**unconfirmed**.

**F3. Linked worktree.** `origin_slug` returns None for a `.git` file, so a
worktree only matches by folder name. A sibling worktree `~/repos/site-feature`
of `~/repos/site` (origin `Gaurgle/portfolio-site`) does not match in pass 1:
**confirmed by tests** `a_linked_worktree_does_not_match_through_its_main_clones_remote`
and `find_all_local_includes_gitdir_pointer_worktrees`. It then shows as a
local-only row when dirty: **confirmed by reading** (main.rs:2498-2535). A
worktree whose folder is named like any wanted repo, its own or not, claims
that row: **confirmed by reading** (discovery.rs:70-74). Worktrees nested
inside a repo (such as `.claude/worktrees/`) are never found, because the
walker stops at a repo (discovery.rs:283-286).

**F4. Local repo with no remote.** If its folder name equals any wanted repo
name, it claims that GitHub row with that repo's CI and owner. Otherwise it
appears in pass 3 when dirty. Two dirty local-only repos with the same folder
name show as one row (main.rs:2531). Its glyph comes from any same-named
dataset repo (main.rs:2508). **Confirmed by reading.** The GitHub poll writes
CI, PRs and kind onto it whenever the full dataset has a repo of that name
(main.rs:1712-1719), which the load never did (main.rs:2667-2679). In watch
mode, on a cache-served launch, that happens on the first tick after the load
lands, because the load leaves `last_github_poll` unset (main.rs:539-541,
665-676). On a network-served launch it happens one poll interval later. So a
local-only row gains a stranger's CI badge almost at once on an ordinary
launch. **Confirmed by reading**, no test.

**F5. Repo renamed on GitHub, clone still on the old URL.** Dataset has
`Gaurgle/new-name`; `origin` says `Gaurgle/old-name`. No remote match. If the
folder is still `old-name`, the GitHub row is "not cloned" and the clone is a
local-only `old-name` row when dirty. If the folder was renamed to `new-name`,
the folder fallback matches and the row looks correct. **Confirmed by
reading**; the limitation is documented on `match_clone` (discovery.rs:63-65).

**F6. URL forms.** Scp-style with and without `.git`, HTTPS with and without
`.git` and with a trailing slash, `ssh://` with a port, and SSH host aliases
all parse to the same `owner/name`, and the slug is lowercased, so case
differences match. **Confirmed by tests** `repo_parsed_with_the_git_suffix_stripped`,
`owner_parsed_from_every_remote_url_form`, plus reading discovery.rs:178.
**Unconfirmed edges** (reasoned from model.rs:387-399): an uppercase `.GIT`
suffix is not stripped, so the remote misses and the folder fallback decides;
a `file:///Users/me/repo` URL parses as owner `Users`, repo `me`; `insteadOf`
shorthands are not expanded by `origin_slug` but are by `git::remote_url`
(git.rs:988), so the URL drawn and the URL matched can differ.

**F7. Non-GitHub remote.** The host is ignored. A GitLab mirror
`gitlab.com/Gaurgle/fleetz` matches the GitHub row `Gaurgle/fleetz` by remote.
A GitLab repo `gitlab.com/someone/api` in a folder `api` claims GitHub
`company/api` by folder name. **Confirmed by reading** (model.rs:380-382,
discovery.rs:66-75).

**F8. Fork with `origin` the fork and `upstream` the parent.** Dataset has
`Gaurgle/tool` (fork) and `acme/tool` (org, pushed later). `merge_sources`
keeps `acme/tool`. The clone's origin `Gaurgle/tool` is not wanted, so the
folder `tool` claims `acme/tool`: the fork's local state under the parent's CI,
PRs and owner. `Gaurgle/tool` is absent. If only the fork is in the dataset, it
matches correctly by remote. `upstream` is never read. **Confirmed by reading.**
That `viewer.repositories` returns forks is **unconfirmed** (GitHub behavior).

**F9. Dot-prefixed clone.** `~/.dotfiles` as a scan root matches GitHub
`dotfiles` and reports the undotted name. **Confirmed by tests**
`find_in_primary_matches_a_dot_prefixed_clone`, `repo_name_for_path_strips_a_leading_dot`,
`agrees_with_repoz_on_a_hidden_scan_root`. With both `~/repos/dotfiles` and an
`extra_dirs` root `~/.dotfiles`, there is no race: `find_in_primary` walks the
roots in configured order with `repo_dir` first (main.rs:2383-2384), so
`~/repos/dotfiles` wins and `~/.dotfiles` is hidden from pass 3 by its name.
**Confirmed by reading** (discovery.rs:94-104).

**F10. Late sweep result merging by path after a reload (FZ-02 follow-up).**
A sweep snapshots the row at path P; a reload replaces the list; the row now
at P may represent a different remote (origin changed, or the directory was
re-cloned). `apply_repo_updated` lands the sweep-owned fields on that row.
Every sweep-owned field is a probe of the directory at P
(`rebuild_repo_status`, main.rs:1962-2054), and probes are reused from the
snapshot only when P's `WorkKey` is unchanged (main.rs:1981-1991). So the
landed data describes P, which is what the row at P displays. The only
side effect that reads across rows is `repo_moved` setting `push_detected`
(main.rs:1672-1674), which can trigger one early poll. **Confirmed by
reading**; no test pins the different-remote case.

## 3. Design

### 3.1 Definitions

- **Remote identity** (`RemoteId`): `(host, owner, name)`, each lowercased for
  comparison, original spelling kept for display. Every dataset row today has
  host `github.com`, because the only endpoint is `api.github.com`
  (github.rs:545). A dataset entry whose owner is unknown (`owner: None`,
  which `parse_repos_tolerates_missing_owner` allows) has no `RemoteId`; it is
  the legacy case (m2, 3.5).
- **Checkout identity** (`CheckoutId`): the absolute path of the working tree.
  Canonicalized for **dedupe and claim keys only**, never for display, since
  `tilde_display` of a canonical path would turn a symlinked `~/repos` into
  `/Volumes/...` on screen. When `canonicalize` fails, the raw path is the key.
- **Origin reference** (`RemoteRef`): what an `origin` URL parses to:
  `(host, owner, name)`, or none. Evidence for matching, not an identity. Two
  sources feed one parser:
  - Passes 1 and 2 read `.git/config` directly (`origin_slug`), for cost. No
    `insteadOf`, and a linked worktree has none.
  - LocalOnly carrier selection (3.5) reads the row's stored `remote_url`,
    from `git remote get-url origin` (git.rs:988). That source applies
    `insteadOf`, and in a linked worktree it returns the main clone's origin.

  The two sources can disagree under `insteadOf`, so a checkout can miss in
  pass 1 and still be a carrier through pass 3.
- **Row**: at most one `RemoteId` (what the GitHub-owned fields describe) and
  at most one `CheckoutId` (what the sweep-owned fields describe). Not cloned:
  remote only. Primary and Elsewhere: both. Local-only: checkout only, and
  GitHub fields only when it is the carrier for its origin's remote identity.
- **One carrier per remote identity.** GitHub-owned fields live on exactly one
  row per `RemoteId`, at load and in the poll. Two rows carrying one repo's
  PRs and CI would double-count in `Summary::from_refs` (the objection to
  model B), and the poll and `remotes_that_moved` then have exactly one row to
  write or fetch per `RemoteId`. The load chooses the carrier and marks it by
  setting `owner` (3.5).

### 3.2 Core model (D1: model A)

**A. Remote-keyed rows with derived keys (chosen).** A row's remote key is
computed, not stored: its `owner` plus a name. For a Primary, Elsewhere or
NotCloned row, that name is `name`. For a LocalOnly carrier, it is the repo
name parsed from `remote_url`, since a LocalOnly row's `name` is its folder.
Its checkout key is the existing `path`. One row per `RemoteId` carries
GitHub fields; extra checkouts of the same remote become local rows with local
fields only (D5, D6).

**Name-only matching is the legacy path only (m2).** A Primary, Elsewhere or
NotCloned row with no `owner` matches a dataset entry by name, whether or not
the entry has an owner. That is today's behavior, and it only arises from a
payload without `login`. The load-owned `owner` then stays unset until the
next reload, even if a later poll carries owners. A **LocalOnly row is never
matched by name**, at load or in the poll. A LocalOnly row with no `owner` is
never written by the poll. That is what keeps F4 from coming back through the
fallback.

Cost: `FoundRepo` gains an `owner` field (D8). No new `RepoStatus` field, no
change to `visible_repos`, sections, summary or rendering geometry. Existing
tests keep passing by reading:
- Fixtures with owners on both sides (`poll_says_passed`,
  `app_with_pending_repo`) match by owner.
- Fixtures with `owner: None` and Primary rows (`gh` with `cloned`, built on
  `test_repo`, location Primary) take the legacy name path.
- Discovery fixtures built with `by_name` have no `.git/config`, so they reach
  rule 2 of 3.3.

**Alternatives considered and not chosen.**

- **B. One row per checkout.** Every discovered checkout is a row keyed by
  path; GitHub data attaches by `RemoteId`. Two clones would render as two
  rows carrying the same CI and PR counts (`Summary::from_refs` double-counts
  PRs), and it touches `visible_repos` ordering, a domain invariant. A product
  change, not an identity fix.
- **C. A stored typed identity** (`id: RepoId` on `GithubRepo`, `FoundRepo`,
  `RepoStatus`). Same behavior as A, but every `RepoStatus` struct literal
  changes (at least six fixture constructors) and so do the exhaustive
  destructures in `apply_github` and `apply_local_sweep`. Can follow A later
  as a mechanical refactor if a persisted feature (FZ-15) needs a stable key.

### 3.3 Matching rules

A checkout in passes 1 and 2 is matched to a wanted (window-filtered) dataset
row in this order. Every check of "is this name unique" or "is this repo
known" uses the **full** dataset, not the window (M1).

1. **Exact remote.** `origin` parses to an `owner/name` that equals a wanted
   `RemoteId` (host per 3.4). Unambiguous by construction.
2. **No usable origin** (no `origin`, unparseable URL, or a linked worktree,
   D10): fall back to the folder name, dotted spelling included, as today,
   against wanted repos.
   - **Safe** when the folder name matches a wanted repo, and the **full**
     dataset has no other owner for that name. Also safe when every
     same-named entry has `owner: None` (legacy payload).
   - **Ambiguous** when the full dataset has two or more owners for the name.
     The window must not decide this: with a 1-day window, `alice/api` pushed
     today and `company/api` pushed 60 days ago, `api` is still ambiguous
     (S3b). Claim none (D4). The in-window GitHub rows stay "not cloned", the
     checkout goes to pass 3 like any unclaimed repo (shown when dirty), and
     the load sets one status line: `api: ~/repos/api matches 2 GitHub repos,
     add an origin remote`. When several checkouts are ambiguous, the line
     names the first in sorted path order (o3), so it does not change between
     launches.
3. **Origin parses but matches nothing wanted (D3).** Fall back to the folder
   name only when **both** hold:
   - the origin's owner equals (case-insensitively) the owner of the wanted
     repo the folder name points at, and
   - the origin's `owner/name` is not itself a repo in the full dataset.

   Otherwise no fallback: the checkout goes to pass 3. A renamed repo in a
   renamed folder (S9b) keeps matching, since GitHub keeps the old URL
   working. A fork (S12), a clone whose origin is the upstream (S12b), a
   foreign-host project (S11a), a transferred repo (S9c) or a different known
   repo of the same owner (S9d) does not claim the row. The owner condition
   selects at most one candidate, so rule 3 never reaches the ambiguity case.
   - **Legacy candidate (confirmed ruling).** If the wanted repo the folder
     points at has `owner: None`, the owner condition cannot be evaluated; it
     is the legacy path and keeps today's fallback.
   - **"Not in the full dataset" means "not known to fleetz" (accepted
     limitation).** The dataset is the top `repo_limit` repos by push plus
     the org pages. An old same-owner repo below that cut, cloned into a
     folder named like a current repo, passes both conditions and claims the
     row. Closing this needs a per-repo request.

The returned row name stays the folder's spelling for a folder-name match, as
today (`find_in_primary_is_case_insensitive` expects `MyRepo` for GitHub
`myrepo`, and that test must not be edited). Comparison is case-insensitive
everywhere, so the only effect is display.

**Primary checkout when several match one `RemoteId`.** Pass 1 before pass 2
(load-bearing, unchanged). Within a pass, in order:
1. Exact remote beats folder fallback.
2. An origin on host `github.com` beats other hosts (o2).
3. A `.git` directory beats a `.git` file.
4. An earlier configured root beats a later one.
5. Fewer path components.
6. Lexicographic path.

This replaces filesystem order with a stable rule. The losers are unclaimed and
go to pass 3. Until FZ-05f, pass 3 keeps today's bare-name filter, so a loser
whose folder has the winner's name stays hidden. After FZ-05f, pass 3 dedupes
by canonical path only (D5), and a dirty loser appears as a local row with no
GitHub fields (3.5).

### 3.4 Host scoping and case normalization (D2)

- **Case.** GitHub owner logins and repo names are compared
  case-insensitively, which is how GitHub treats them (external fact,
  **unconfirmed** here, and already assumed by every lowercasing site in
  section 1). Hosts are lowercased. Display keeps the API's spelling.
- **Host today.** The dataset only ever comes from `api.github.com`, so every
  `RemoteId` has host `github.com`. fleetz has **no Enterprise support**: no
  configurable endpoint, no per-host token, no per-host cache. This design
  does not add any.
- **Host on the checkout side.** The host is parsed into `RemoteRef` and
  carried. It orders primary checkouts (o2) but does not gate a match. SSH
  host aliases (`git@github-work:owner/repo`, also `github.com-work`) are real
  GitHub remotes that a strict host check would reject, and
  `repo_parsed_with_the_git_suffix_stripped` pins the alias case.
- **Accepted cost.** A GitHub Enterprise or GitLab checkout with the same
  `owner/name` as a github.com repo claims its row (S11b). Carrying the host
  lets FZ-09 (cache by host) and any later Enterprise work scope on it without
  re-parsing.
- **Rejected:** resolving aliases through `~/.ssh/config` or `ssh -G`. That
  means parsing `Include` and `Match`, or a process per candidate, which
  `origin_slug` exists to avoid (discovery.rs:166-169).

### 3.5 The carrier and how load and poll use it (D6, M2)

**The load decides.** `assemble_view` gives each `RemoteId` in the full
dataset at most one carrier row. It marks the carrier by setting `owner`, and
it also writes the carrier's GitHub fields (CI, workflow, duration, PRs, PR
count, kind) from the same dataset entry, through `RepoStatus::apply_github`.
So a LocalOnly carrier shows its GitHub data from the first frame, not from
the first poll. S6b, S9d, S13b and S17 expect the fields at load. The carrier
is the first of:

1. The Primary or Elsewhere row matched to the `RemoteId` (3.3).
2. Otherwise, a LocalOnly row whose stored `remote_url` parses to that
   `RemoteId`. If several qualify, the first by the primary-checkout order of
   3.3. When the `RemoteId` is in the window, this row also **claims** it, so
   the not-cloned loop emits no NotCloned row for it.
3. Otherwise, the NotCloned row, when the `RemoteId` is in the window.

Every other row is not a carrier. In particular, a LocalOnly row that is not
the carrier keeps `owner: None`, with CI Unknown, PRs 0 and kind Unknown. This
covers an extra checkout of a remote that already has a Primary or Elsewhere
row (S4, S5) and a checkout with no origin (S14). Under D1 such a row gets no
glyph from a name match either. A checkout with no origin has no GitHub
identity, so no glyph is the honest answer. **Visible change**: a
remote-less, out-of-window `~/.dotfiles` loses the glyph it shows today.

**The poll follows the mark.** No ordering is recomputed in the poll:
- A Primary, Elsewhere or NotCloned row is written by `(owner, name)`. When it
  has no `owner`, it is written by name alone (legacy, m2).
- A LocalOnly row is written only when its `owner` is set, keyed by that owner
  plus the repo name parsed from its `remote_url`.
- `remotes_that_moved` returns the carrier's index only. An extra checkout is
  therefore not fetched when its remote moves; `F` (fetch all) still reaches
  it.

Load and poll use different code paths, so agreement between them is not
assumed. The S4 and S5 load and poll twin tests (FZ-05d) are the proof.

**Checked against the code.**
- `local_only_status` (main.rs:2627-2688) sets `owner: None` unconditionally.
  FZ-05d sets it afterwards for the carrier, in `assemble_view`, from the
  dataset entry.
- `RepoStatus::owner_display` (model.rs:432-437) prefers `owner` and falls
  back to the URL. A non-carrier LocalOnly row still shows the URL's owner in
  the detail pane, as today; only GitHub fields are withheld.
- The `RepoStatus` ownership doc (model.rs:140-155) says `owner` "comes from
  GitHub data but is only ever set at load". That still holds: the carrier's
  owner is copied from the dataset entry at load.
- The field doc on `owner` (model.rs:191-194) says "Local-only clones leave it
  None". That becomes **false** for a LocalOnly carrier. FZ-05d must rewrite
  that comment to "set at load on the one row carrying this remote's GitHub
  fields".
- "Owner is only ever set from a GitHub match" still holds, if an exact match
  of a stored origin against the dataset counts as a GitHub match. It does:
  the value comes from the API's `login`, never from the URL.
- The ruling's "carries GitHub fields if and only if the load gave it an
  owner" has one stated exception. A legacy row (no `login` in the payload)
  carries by name with `owner: None`. That case is m2's.

**Edge, recorded.** A remote-less dirty checkout at depth 4 under a scan root,
named like an in-window repo, is missed by pass 1 (maxdepth 3) and reached by
pass 3 (maxdepth 4). Today pass 3 claims the name and suppresses the NotCloned
row. After FZ-05d the claim against the not-cloned loop is by `RemoteId`, so
the NotCloned row stays beside a LocalOnly row of the same name. This is
consistent with the fourth invariant change (3.10, M5). A depth-4 checkout
**with** an origin is the carrier (S17).

### 3.6 Late sweep result by path (F10)

Keep path as the merge key. Under the FZ-02 split every field a sweep writes
is a property of the checkout, so a result for path P is correct for whatever
row now sits at P, whichever remote that row represents. No generation
counter is needed for correctness. Pin the decision with a test (scenario
S15) so a later change to the split notices. If FZ-11 (shared scheduler) adds
stale-result rejection, it should reject by `(CheckoutId, generation)`, not by
remote.

The one real gap is the display path. Two spellings of one directory (a
symlinked root) are two keys. Canonical claim keys (3.1) stop discovery from
producing two rows for one directory, so the sweep key never sees two
spellings.

### 3.7 repoz parity

- `discovery::repoz_parity` compares `find_all_local` only (discovery.rs:709-761).
  Nothing here changes `find_all_local`, `PRUNE_DIRS` or the walker, so both
  parity tests are unaffected.
- The documented divergence ("repoz still matches on names only") stays and
  narrows further: the folder fallback becomes conditional (3.3).
- Two changes move fleetz **toward** repoz. repoz deduplicates the dataset by
  `nameWithOwner` (`unique_by(.nameWithOwner)`, `~/repos/repoz/repoz` line
  784) where fleetz uses the bare name (FZ-05e). repoz skips pass 3
  candidates by path (`checked_paths`, lines 1115 and 1277) where fleetz uses
  the name (FZ-05f, D5).
- Other divergences found on the way are in section 6.

### 3.8 Persisted state

| Store | Change | Approval |
|---|---|---|
| API cache file | **None.** `owner.login` is already in the payload, so `RemoteId` is derived after parse. A payload without `login` takes the legacy name path (m2) | None for FZ-05. Per-account and per-host file keys are FZ-09 |
| `config.groups` | **None now (D7 deferred).** A possible additive `owner/name` member syntax is parked as FZ-05h | Andreas's explicit approval of the config contract change, if ever wanted |
| Window state | None | n/a |
| GraphQL query | None. `nameWithOwner`, `url` and `id` are free fields but not needed | Widening the fetch would need the cache check required by `CLAUDE.md` |

The ticket's exclusions (no cache schema change, no migration, no provider
expansion) hold for every promoted ticket.

### 3.9 Selection and display

- Selection is an index (app.rs:1464-1472), not an identity. Restoring the
  cursor by row key after a reload is cheap once keys exist, but it is a UX
  change independent of collisions: optional ticket FZ-05i.
- Two rows with the same name are indistinguishable in the list and the
  compact grid (ui/detail.rs:344, ui/compact.rs:159). D9 default: draw
  `owner/name` only on the colliding rows, and the parent folder for a
  colliding local-only row (FZ-05g). The compact grid's `column_count`
  (ui/compact.rs:108-127) must size cells from that drawn label, not from
  `name`. This is the same rule as the geometry invariant: what sizes the
  grid and what draws it must agree (o1).
- Fuzzy search stays name-only. Multi-field fuzzy matching is an open design
  question in `docs/ideas.md` ("Filter and search strip").
- The local contribution strip (`App::local_calendar`, app.rs:2112-2121) sums
  `daily` over every visible row. A dirty extra checkout double-counts the
  same commits. It already happens today with a differently named copy (S5).
  After FZ-05f it also happens with a same-named copy (S4). This is an
  accepted cost of D5. Follow-up candidate, not ticketed: exclude a
  non-carrier checkout of an already-carried remote from the local calendar.

### 3.10 Documented invariants this changes

> **These change text in `CLAUDE.md` ("Domain invariants"). The coordinator
> edits `CLAUDE.md`; no FZ-05 ticket does. All four need Andreas's sign-off.**

1. **"A clone is matched by its `origin` remote first, its folder name
   second."** Becomes: folder name when there is no usable origin and the
   name is unambiguous across the full dataset, or when the origin names an
   unknown repo of the same owner (the rename case, 3.3 rule 3). The paragraph
   also claims "the owner is part of the key, so a clone of someone else's
   same-named repo can't claim yours". Today that holds only for the remote
   match (F8); after FZ-05b it becomes true.
2. **"Pass 3 dedupes by path as well as name."** Under D5 it dedupes by
   canonical path only, which is what repoz does. This restores parity with
   repoz, the design goal `CLAUDE.md` names. Visible effect: dirty extra
   checkouts and same-named local-only repos now get rows (S4, S8b).
   **Andreas may veto before FZ-05f is promoted.**
3. **"match, glyph lookup and dedupe all key on the GitHub name."** Becomes
   "GitHub owner and name". LocalOnly rows are matched only through their
   stored origin, and only the carrier gets GitHub fields. The dot-stripping
   rule is unchanged.
4. **"Discovery pass order is load-bearing ... A local clone must win over
   that fallback."** Becomes: a local clone wins over the not-cloned fallback
   only through its exact origin, or through an unambiguous folder name in
   passes 1 and 2. The code comment carrying today's version sits in
   `assemble_view` before pass 3 (main.rs:2493-2497), where a dirty pass 3
   repo claims the name. A remote-less pass 3 clone no longer suppresses the
   NotCloned row (3.5, edge).

Kept unchanged: discovery pass order itself, hidden-directory rules,
`PRUNE_DIRS`, repoz parity tests, `visible_repos` order, "batch on GraphQL,
target on git" (`remotes_that_moved` still fetches only moved remotes, now
the carrier only), the cache rules.

## 4. Fixture scenarios

Each scenario lists the setup, the dataset rows (`owner/name`, push time), the
rows expected after FZ-05a to f, and whether it fails today. Unless noted, the
root is `R` (a temp `repo_dir`), all clones have `.git/config` written as in
`discovery::tests::clone_with_origin`, and "dirty" means one modified tracked
file. "Full dataset only" means present in the dataset but outside the time
window. Load-side scenarios run through the FZ-05c seam with a temp `$HOME`.

| # | Setup | Dataset | Expected rows after | Today |
|---|---|---|---|---|
| S1 | `R/api` origin `git@github.com:company/api.git`; `R/alice-api` origin `https://github.com/alice/api` | `alice/api` 10:00 (viewer); `company/api` 09:00 (org) | Primary `api` owner `alice` at `R/alice-api`; Primary `api` owner `company` at `R/api`; each with its own CI | **Fails**: one row, owner `alice`, path by walk order; `company/api` missing |
| S2 | `R/api` origin `company/api` only | `alice/api` 10:00; `company/api` 09:00 | Primary `company/api` at `R/api` (rule 1); NotCloned `alice/api` | **Fails**: `R/api` shown as `alice/api` with alice's CI |
| S3 | `R/api` with no remote, dirty | `alice/api`; `company/api`, both in window | NotCloned `alice/api`; NotCloned `company/api`; LocalOnly `api` at `R/api`, no GitHub fields, no glyph; status line (D4) | **Fails**: `R/api` claims `alice/api` |
| S3b | As S3; window 1 day | `alice/api` pushed today; `company/api` pushed 60 days ago (full dataset only) | NotCloned `alice/api`; no row for `company/api`; LocalOnly `api` at `R/api` when dirty, no GitHub fields; status line. Ambiguity is judged on the full dataset (M1) | **Fails**: `R/api` claims `alice/api`. Guards against judging ambiguity on the window |
| S4 | `R/fleetz` and `R/old/fleetz`, both origin `Gaurgle/fleetz`; `R/old/fleetz` dirty | `Gaurgle/fleetz` | Primary at `R/fleetz` (fewer components), the carrier; LocalOnly `fleetz` at `R/old/fleetz` with local fields only: `owner` None, CI Unknown, PRs 0, kind Unknown, at load and after a poll | **Fails**: winner by walk order; dirty copy hidden |
| S5 | `R/fleetz` and `R/fleetz-copy`, both origin `Gaurgle/fleetz`; copy dirty | `Gaurgle/fleetz` | Primary at `R/fleetz`, the carrier; LocalOnly `fleetz-copy` with local fields only (CI Unknown, PRs 0), at load and after a poll | **Partly fails**: copy shown, but which path is Primary is walk order. The local strip already counts both today |
| S6 | `R/site` origin `Gaurgle/portfolio-site`; `R/site-feature` linked worktree (`.git` file) of it, dirty | `Gaurgle/portfolio-site` | Primary `portfolio-site` at `R/site`, the carrier. LocalOnly `site-feature`, whose stored `remote_url` (from `git remote get-url`) names the same remote, so it is a non-carrier with local fields only | Passes (guard). Discovery half covered by `a_linked_worktree_does_not_match_through_its_main_clones_remote` |
| S6b | Only `R/site-feature` (worktree, dirty) is under a scan root; its main clone lives outside every root and outside `$HOME` depth 4 | `Gaurgle/portfolio-site` (in window) | One row: LocalOnly `site-feature`, the carrier through its stored `remote_url`, with GitHub fields; no NotCloned row (m1) | **Differs**: today LocalOnly with no CI at load, CI after a poll only if the name matched (it does not), plus a NotCloned `portfolio-site` row |
| S7 | `R/api` is a linked worktree of some unrelated repo | `company/api` only | Primary `company/api` at `R/api` (unambiguous name fallback, D10) | Passes; documents the accepted limitation |
| S8a | `R/scratch` no remote, dirty | none named `scratch` | LocalOnly `scratch` | Passes (guard) |
| S8b | `R/a/notes` and `R/b/notes`, no remotes, both dirty | none | Two LocalOnly `notes` rows (D5, FZ-05f) | **Fails**: one row |
| S9a | `R/old-name` origin `Gaurgle/old-name`, dirty | `Gaurgle/new-name` | NotCloned `new-name`; LocalOnly `old-name` | Passes (unchanged): folder points at nothing wanted |
| S9b | `R/new-name` origin `Gaurgle/old-name` (renamed on GitHub, folder renamed, remote not) | `Gaurgle/new-name` | Primary `new-name` at `R/new-name`: same owner, `Gaurgle/old-name` not in the dataset (rule 3) | Passes today by folder fallback; still passes |
| S9c | `R/x` origin `Gaurgle/x`; repo transferred to `acme`, so GitHub has only `acme/x` | `acme/x` | NotCloned `acme/x`; LocalOnly `x` when dirty, no GitHub fields. **Accepted limitation**; the fix is `git remote set-url origin` | **Differs**: today the folder fallback claims `acme/x` |
| S9d | `R/tool` origin `Gaurgle/other`, dirty | `Gaurgle/tool` (in window); `Gaurgle/other` (full dataset only) | NotCloned `Gaurgle/tool`; LocalOnly `tool` at `R/tool`, the carrier for `Gaurgle/other` (no other row has it), with its GitHub fields. If `Gaurgle/other` were in the window, rule 1 would make it Primary `other` at `R/tool` instead | **Fails**: `R/tool` claims `Gaurgle/tool` by folder |
| S10 | `R/x` with each origin in turn: `git@github.com:Gaurgle/fleetz.git`, `git@github.com:Gaurgle/fleetz`, `https://github.com/Gaurgle/fleetz.git`, `https://github.com/gaurgle/FLEETZ/`, `ssh://git@github.com:22/Gaurgle/fleetz.git`, `git@github-work:Gaurgle/fleetz.git` | `Gaurgle/fleetz` | Primary `fleetz` at `R/x` in every case (rule 1; folder named `x` so only the remote can match) | Passes by reading (model and discovery tests cover parse and match). `.GIT` uppercase suffix: **unconfirmed**, likely no match |
| S11a | `R/api` origin `git@gitlab.com:someone/api.git`, dirty | `company/api` | NotCloned `company/api`, the carrier; `R/api` LocalOnly, `owner` None, no GitHub fields, no glyph | **Fails**: `R/api` claims `company/api` by folder |
| S11b | `R/x` origin `git@gitlab.com:Gaurgle/fleetz.git` | `Gaurgle/fleetz` | Primary `fleetz` at `R/x` (D2 host-agnostic, rule 1) | Passes; documents the accepted cost |
| S12 | `R/tool` origin `git@github.com:Gaurgle/tool.git`, remote `upstream` `acme/tool` | `Gaurgle/tool` 08:00 (fork); `acme/tool` 10:00 (org) | Primary `Gaurgle/tool` at `R/tool` (rule 1, once FZ-05e keeps both entries); NotCloned `acme/tool` | **Fails**: `R/tool` shown as `acme/tool`; `Gaurgle/tool` missing |
| S12b | `R/tool` origin `git@github.com:acme/tool.git` (the upstream), remote `mine` `Gaurgle/tool` | `Gaurgle/tool` (in window); `acme/tool` not in the dataset | NotCloned `Gaurgle/tool`; LocalOnly `tool` when dirty, no GitHub fields. Rule 3 refuses: owner `acme` is not `Gaurgle`. **Accepted regression** (m3). Remedy: make the fork the `origin` (`git remote rename`), or the optional FZ-05j | **Differs**: today the folder fallback matches `Gaurgle/tool` |
| S13 | `~/.dotfiles` as an `extra_dirs` root; variant 1 origin `Gaurgle/dotfiles`, variant 2 no origin | `Gaurgle/dotfiles` (in window) | Primary `dotfiles` at `~/.dotfiles` in both variants (rule 1, then rule 2) | Passes (guard). Name half covered by `find_in_primary_matches_a_dot_prefixed_clone` |
| S13b | `~/.dotfiles` as an `extra_dirs` root, origin `Gaurgle/dotfiles`, dirty; window 1 day | `Gaurgle/dotfiles` pushed 60 days ago (full dataset only) | LocalOnly `dotfiles`, the carrier (no other row has the remote): glyph, CI and PRs at load and after a poll | **Differs**: today glyph at load by name, CI only after the first poll by name |
| S14 | `R/api` no remote, dirty; window 1 day | `company/api` pushed 60 days ago (full dataset only) | LocalOnly `api`: `owner` None, CI Unknown, kind Unknown, PRs 0, at load **and after a poll** carrying `company/api` | **Fails**: glyph from `company/api` at load; CI and PRs appear after the first poll |
| S15 | Row at `~/repos/r` for `Gaurgle/r`; sweep snapshot taken; list replaced by a reload whose row at `~/repos/r` is `Gaurgle/y`; sweep result lands | `Gaurgle/y` | Row at `~/repos/r` keeps name `y`, owner, CI and groups; takes the sweep's branches, dirty, commits | Passes by reading; pin with a test in `main.rs` beside `a_late_result_for_a_removed_repo_adds_no_row` |
| S16 | Dataset rows `alice/api` and `company/api`, neither cloned | as stated | Two NotCloned rows, each with its own CI after a poll | Not reachable today (dataset dedupe). **Fails if `merge_sources` is fixed before the poll path**: both rows take the first entry's CI. Sets the ticket order |
| S17 | `R/a/b/c/fleetz` (depth 4) origin `Gaurgle/fleetz`, dirty. Pass 1 stops at depth 3 (`find_in_primary`, discovery.rs:95), pass 2 excludes scan roots, pass 3 reaches depth 4 (`find_all_local`, discovery.rs:219) | `Gaurgle/fleetz` (in window) | One row: LocalOnly `fleetz`, the carrier through its stored `remote_url`, with GitHub fields; no NotCloned row | Today: one LocalOnly row with no CI at load, CI after a poll by name; NotCloned suppressed by name. Load half **differs** (gains CI) |

**Checks of scenarios against D3.** S2: the origin `company/api` is wanted,
so rule 1 decides and rule 3 never runs. S11a: the folder points at
`company/api`, the origin owner is `someone`, so the owner condition fails.
S12 after FZ-05e: rule 1 matches `Gaurgle/tool`. S12 before FZ-05e (dataset
still name-deduped, `Gaurgle/tool` dropped): the owner condition fails and the
clone goes to pass 3 rather than claiming the parent (section 5.1 covers what
that interim looks like). All three come out as written.

**One carrier per remote identity.** Scenarios with more than one checkout
naming one remote are S4, S5 and S6; in each, the Primary is the only
carrier. In S6b, S9d, S13b and S17 a single LocalOnly checkout is the only row
naming its remote, so it is the carrier. S1 and S16 have two rows for two different remotes. No scenario
has two rows carrying one `RemoteId`.

S1 and S3 together are the collision path and the local-only path the
reviewer traced (acceptance criterion 7). S14 and S11a guard D1's rule that a
LocalOnly row is never matched by name.

## 5. Implementation tickets

Ordered a, b, c, d, e, f. "Approval" lists anything beyond the safe-merge
rule. All tickets keep the three standard checks and add the named scenarios
as tests beside the code. Every ticket keeps the signatures of functions that
existing tests call (`Wanted::new`, `find_in_primary`, `find_elsewhere`,
`unclaimed_local`, `stale_remotes`, `sort_by_activity`, `remotes_that_moved`,
`apply_github_poll`), so no existing test call changes.

| Ticket | Outcome | Files owned | Depends on | Regression scenarios | Approval |
|---|---|---|---|---|---|
| FZ-05a | `RemoteRef { host, owner, name }` parsed from a URL, with a lowercased comparison key; `repo_from_remote_url` keeps its signature and results | `src/model.rs` | This design | S10 parse table; `file://` and `.GIT` edges pinned as current behavior | None |
| FZ-05b | Owner-aware discovery. Keyed by `(owner, name)` plus name-only entries for owner-less rows, with the full dataset available for the uniqueness and "known repo" checks (M1, D3). Adds a constructor taking the active and the full dataset and keeps `Wanted::new` as is; `Wanted::new` treats its one input as both the wanted set and the full dataset for the M1 and D3 checks, so existing tests that go through it keep today's semantics. By reading, none of them reaches a case rule 3 now refuses: `a_remote_under_another_owner_does_not_match` uses a folder that names nothing, and the `by_name` fixtures have no origin. Production uses the new constructor. `FoundRepo` carries `owner`. `without` and `seen` are keyed. Rules 1 to 3 of 3.3 and the primary-checkout order. Ambiguities are returned as data, sorted by path, through a new function, without changing `find_in_primary`'s return type | `src/discovery.rs`; in `src/main.rs` `assemble_view`: the `Wanted` construction (main.rs:2404-2405), `found_set` (2409-2412) and the `find_elsewhere` call (2416) that consumes it; compile-forced `FoundRepo` literal additions in `src/main.rs` tests | a, FZ-03 integrated | S2, S3, S3b, S6, S7, S9a to S9d, S11a, S12 and S12b (discovery half), S13; S4 and S5 primary choice | D8 conditions below |
| FZ-05c | Test seam, behavior unchanged: the assembly core takes `home` and the scan roots as parameters instead of calling `dirs::home_dir()` (main.rs:2415) and `config.resolved_extra_dirs()`; `assemble_view` becomes a thin caller. Reviewed as a pure refactor | `src/main.rs` (`assemble_view` and a new core function) | b | One smoke test through the seam with a temp `$HOME` reproducing today's S8a | None |
| FZ-05d | Owner-aware assembly **and** poll in one ticket (M4). Assembly: `build_repo_status`, `stale_remotes`, `sort_by_activity`, `kind_by_name`, pass 1/2 `seen`, not-cloned loop by claimed `RemoteId`, pass 3 glyph, carrier selection and marking by `owner` (3.5). Poll: `apply_github_poll` and `remotes_that_moved` follow the mark. The D4 status line carried from discovery to `App` through the load result and `apply_load_result`, set after the `Reloaded` and `Window: ...` messages so it wins (N2). Rewrite the `owner` field doc (model.rs:191-194). **Must not change** the pass 3 bare-name filter in `unclaimed_local` or the LocalOnly name claim (main.rs:2529-2535); `checked_names` keeps today's contents and uses | `src/main.rs` (assembly, poll and sweep landing, load result); the `owner` doc comment in `src/model.rs` | a, b, c | S1 and S16 (load and poll); S3 status line, including the status test below; S4 and S5 twin tests below; S6, S6b, S9d, S11a, S13b, S14 (load and poll), S15, S17 | None |
| FZ-05e | `merge_sources` dedupes by `(owner, name)`, falling back to name when owner is missing; still collapses one repo returned by both sources | `src/github.rs` | d | S1, S3, S3b, S12, S16; a repo in both viewer and org lists stays one entry | None (no query or payload change) |
| FZ-05f | Pass 3 dedupes by canonical path only (raw path when `canonicalize` fails); extra checkouts appear when dirty, as non-carriers. Keeps `unclaimed_local`'s signature | `src/main.rs` (`unclaimed_local`, pass 3 claim) | c, d | S4, S8b through the seam; repoz parity tests untouched | **Andreas may veto before it is promoted** (changes a documented invariant). If an existing assertion encodes the name dedupe, the worker stops and it goes to Andreas with the veto; D8 does not extend to it. Coordinator updates `CLAUDE.md` |
| FZ-05g (optional) | Colliding names draw `owner/name`, or parent folder for a LocalOnly row (D9 default); `compact::column_count` sizes from the drawn label (o1) | `src/ui/detail.rs`, `src/ui/compact.rs` | d | S1, S8b rendered; a colliding long label does not overflow its compact column | Andreas accepts the look on the branch build; **stops at Ready to integrate** |
| FZ-05h (optional, **not promoted**) | Group members may be `owner/name` | `src/main.rs` (`apply_groups`), `README.md` | d | Bare member still matches both S1 rows; qualified member matches one | **D7 deferred**: needs Andreas's explicit approval of the config contract change |
| FZ-05i (optional) | Cursor restored by row key after reload; branch overlay re-sweeps its own path, not `selected_repo_index()` | `src/app.rs`, `src/main.rs` (branch overlay) | d | Reload that reorders rows keeps the cursor on the same repo | Andreas tries it (interaction) |
| FZ-05j (optional, **not promoted**) | Match through a non-`origin` remote, exact `RemoteId` match only, tried after `origin` and before the folder fallback | `src/discovery.rs` | b | S12b becomes Primary `Gaurgle/tool`; S12 unchanged | Coordinator promotes if S12b proves common |

**FZ-05b fixture conditions (D8).** Adding a new field's neutral value
(`owner: None`) to `FoundRepo` struct literals in existing test fixtures
(`found_targets` and `sorting_by_activity_does_not_change_where_a_repo_was_found`
in `main.rs` tests) is a compile-forced fixture update, not editing a test to
make code pass. Conditions:

- No assertion, expected value or test name changes.
- The reviewer of FZ-05b checks the fixture hunks line by line.
- If any existing assertion would have to change, the worker stops and
  reports.

The same conditions apply to any later ticket in this series that adds a
field. They do not cover changing a function signature that a test calls,
which is why the tickets above keep those signatures.

**FZ-05d twin tests for S4 and S5 (load and poll).**
- *Load half*, through the FZ-05c seam: R contains `fleetz` and `fleetz-copy`,
  both with origin `Gaurgle/fleetz`, the copy dirty, and the dataset has one
  `Gaurgle/fleetz` entry with `CiStatus::Passed` and one PR. Assert the
  Primary has `owner` `Gaurgle`, CI Passed and `pr_count` 1. Assert the
  LocalOnly copy has `owner` None, CI Unknown, `pr_count` 0 and kind Unknown.
- *Poll half*: those two rows, then `apply_github_poll` with a
  `Gaurgle/fleetz` entry carrying `CiStatus::Failed` and two PRs. Assert only
  the Primary changes. A second poll whose `pushed_at` moved asserts that
  `remotes_that_moved` returns only the Primary's index.

**FZ-05d poll test for S14.** A `LocalOnly` row named `api` with
`owner: None` and `remote_url: None`, then `apply_github_poll` with a
`company/api` entry carrying `CiStatus::Passed`, one PR and `RepoKind::Org`.
Assert `ci_status` stays `Unknown`, `pr_count` stays 0 and `repo_kind` stays
`Unknown`. A second case gives the dataset entry `owner: None` (legacy) and
asserts the same: the legacy name path does not reach LocalOnly rows.

**FZ-05d status line test (N2).** `apply_load_result` already calls
`set_status` with `Reloaded` for a Reload and `Window: ...` for a WindowApply
(main.rs:552-560). The ambiguity line is set after that match, so `r`, `R` or a
window change cannot overwrite it. Test: for each of Initial, Reload and
WindowApply, a load result carrying one ambiguity leaves the ambiguity line as
the status; a load result with none leaves today's message. A window apply
runs discovery too (`spawn_window_apply` calls `assemble_view`), so it carries
the ambiguities like any load. Consequence, accepted by the ruling: the hint
replaces the `Reloaded` and `Window: ...` confirmations on every load until
the user adds an origin remote.

### 5.1 Interim states and the choice of ordering (M4)

Two options were evaluated, (i) the poll rule before assembly, and (ii)
assembly and poll as one ticket. Both put the test seam (FZ-05c) before any
assembly change, and both leave the pass 3 bare-name filter and the LocalOnly
name claim to FZ-05f.

- **(i)** a, b, poll-only, seam, assembly-only, e, f. The poll-only ticket
  applies "LocalOnly only when owner is set". Before assembly marks carriers,
  no LocalOnly row has an owner, so the poll stops writing to all of them.
- **(ii)** a, b, seam, assembly plus poll, e, f.

Walk through the requested scenarios, plus S13b, which decides it. "Clean" and
"dirty" refer to the checkout in question. States after FZ-05a equal today.

| Scenario | Today | After b (both options) | (i) after poll-only, before assembly | After assembly and poll (both) | After e | After f |
|---|---|---|---|---|---|---|
| S1 (both clean) | One `api` row, alice's data, path by walk order; company missing | One `api` row at `R/alice-api` (rule 1), deterministic; `R/api` refused by rule 3 and, being clean, not listed; company missing. **Worse than today** whenever `R/api` would have won the walk | Same as after b | Same: dataset still has only `alice/api` | Two Primary rows, correct | Unchanged |
| S1 (`R/api` dirty) | Depending on `read_dir` order, `R/api` can be the visible `api` row, showing its uncommitted work (mislabeled as alice's) | `R/api` is always refused by rule 3, then hidden by the pass 3 bare-name filter because `R/alice-api` holds the name `api`. Its uncommitted work is no longer on screen. **Worse than today** | Same as after b | Same | `R/api` is Primary `company/api`, visible | Unchanged |
| S2 (clean) | `R/api` (origin `company/api`) is a visible Primary row labeled `alice/api`, but with its own branches and counts | Rule 3 refuses `R/api` (owner `company` is not `alice`); `local_only_status` drops it as clean; only NotCloned `alice/api` shows. `R/api` has no row. **Worse than today** | Same as after b | Same | Primary `company/api` at `R/api`; NotCloned `alice/api` | Unchanged |
| S3 | `R/api` claims alice | Same as today (dataset still deduped, so the full dataset shows one owner) | Same | Same | Ambiguous: two NotCloned, LocalOnly `api`, status line | Unchanged |
| S4 | Walk-order Primary, dirty copy hidden | Deterministic Primary; copy hidden | Same | Same; Primary is the carrier | Same | Copy appears as non-carrier |
| S11a (dirty) | Primary `api` with company's CI | LocalOnly `api` claims the name, NotCloned suppressed; company's glyph at load, company's CI after the first poll. Same information as today, in the local section | LocalOnly `api` with company's glyph and **no CI**; company's CI shown **nowhere**. **Worse than today** | LocalOnly `api` without GitHub fields, plus NotCloned `company/api` carrying CI. Correct | Unchanged | Unchanged |
| S12 (dirty fork) | Primary `tool` with acme's CI | LocalOnly `tool` claims the name, NotCloned acme suppressed; acme's CI arrives on it by the old poll. Same information as today | LocalOnly `tool` with no CI; acme's CI shown nowhere. **Worse than today** | LocalOnly `tool` (non-carrier) plus NotCloned `acme/tool` | Primary `Gaurgle/tool` plus NotCloned `acme/tool` | Unchanged |
| S12 (clean fork) | Primary `tool` with acme's CI over the fork's state | NotCloned `acme/tool`; the clean fork has no row until e, because `Gaurgle/tool` is not in the deduped dataset. **Worse than today**: the checkout is no longer listed | Same as after b | Same | Primary `Gaurgle/tool` plus NotCloned `acme/tool` | Unchanged |
| S14 | Company's glyph at load, CI after first poll | Same as today | Glyph still company's; CI no longer written (F4 fixed early) | No glyph, no CI. Correct | Unchanged | Unchanged |
| S13b | Glyph at load by name, CI after first poll by name | Same as today | Glyph by name, **CI lost**: no LocalOnly row has an owner yet. **Worse than today** | Carrier: glyph and CI at load and poll. Correct | Unchanged | Unchanged |

**Choice: (ii).** Option (i) has everything option (ii) has in the interim,
plus three more states worse than today (S11a dirty, S12 dirty, S13b): GitHub
data that today reaches the screen, even if on the wrong row, reaches it
nowhere until assembly lands. Option (ii) is therefore strictly better, but it
is **not** free of interim states worse than today. Cost of (ii): FZ-05d is the
largest ticket in the series, all in `main.rs`, and its reviewer must trace
both the load and the poll.

**The interim gap, from FZ-05b until FZ-05e (both options).** A checkout
whose origin repo was dropped by the name-deduped dataset has no row from b
until e. A clean one is not listed (S2 clean, S12 clean, and S1 clean when it
would have won the walk). A dirty one is hidden when a found target holds its
folder name (S1 with `R/api` dirty): uncommitted work that today can be on
screen, mislabeled, is not shown. The cause is that FZ-05b starts refusing
such checkouts (rule 3, primary order) while the dataset still lacks their
repo; FZ-05e restores the repo and the row. The gap affects **only fleets that
have two same-named repos among the fetched repos** (viewer and org lists
together, before dedupe). Every other fleet sees no difference in the
interim.

**Constraint on the series (mitigation).** `main` is the staging line and the
installed binary is the release line. From the merge of FZ-05b until the merge
of FZ-05e:
- nothing is tagged;
- Andreas is told not to install from `main` (`cargo install --path .`);
- the coordinator repeats this in each owner-facing report in that span and
  lifts it when FZ-05e is integrated.

**Alternative considered, not adopted: switch rule 3's refusal on in FZ-05e.**
FZ-05b would ship rules 1 and 2, the primary-checkout order and the ambiguity
data, but keep today's unconditional folder fallback where rule 3 would
refuse. FZ-05e, which restores the dropped repos, would then switch the
refusal on. Effect:
- It removes the clean half of the gap where the refused checkout is the only
  one claiming the name (S2 clean, S12 clean): they keep today's mislabeled
  but visible row until e.
- It does **not** remove S1 (clean or dirty). There `R/api` loses to
  `R/alice-api` through the deterministic primary order (exact remote beats
  folder fallback), not through rule 3. It is then hidden by the pass 3
  bare-name filter until e.
- It does not reopen S16: the S16 hazard concerns `merge_sources` landing
  before the keyed lookups in d, which this does not change.

Cost: FZ-05e grows from `github.rs` to `github.rs` plus `discovery.rs`. The
refusals S9c, S9d, S11a, S12 and S12b arrive with e instead of b, so the
mislabeling of F7 and F8 lasts two tickets longer. The no-install window is
still needed for S1. Not adopted, per the ruling. Recorded so the coordinator
can choose it if the window proves costly.

**What `checked_names` holds after FZ-05d** (a trace gap the reviewer had to
guess). Exactly what it holds today: the lowercased names of pass 1 and 2
targets, plus the names of pass 3 rows as they are claimed. It is read only by
the pass 3 bare-name filter in `unclaimed_local` and the LocalOnly name claim.
The not-cloned loop no longer reads it: it reads a separate set of claimed
`RemoteId`s, from Primary and Elsewhere rows and LocalOnly carriers. FZ-05f
then replaces the name filter and claim with canonical paths.

**Who puts the D4 status line on screen** (the other trace gap). FZ-05b
returns the ambiguities from discovery as data. FZ-05d carries the first one,
in sorted path order, through the load result to `apply_load_result`, which
calls `App::set_status` after its `Reloaded` or `Window: ...` message, so the
ambiguity line wins (N2).

### 5.2 Ticket letters against the first draft

The letters follow the order. From the first draft:
- a and b are unchanged.
- c is new: the test seam (M7).
- d merges the draft's c (assembly) and d (poll).
- e, f, g, h and i keep their meaning.
- j is new and not promoted.

### 5.3 Parallelism

FZ-03 owns `src/github.rs`, `src/cache.rs`, `src/app.rs`, `src/main.rs`,
`src/ui/mod.rs` and `README.md`, and runs before the FZ-05 implementation
tickets.
- **FZ-05a**, in `src/model.rs` only, is the one ticket that can run beside
  FZ-03.
- **FZ-05b, c, d and f** all touch `main.rs`. They run serially, after FZ-03
  integrates, and serially with FZ-04, which also edits `main.rs` event
  handling.
- **FZ-05d** also edits a doc comment in `model.rs`, so it runs after FZ-05a.
- **FZ-05e** touches `github.rs` and runs after d.
- **FZ-05g** touches only UI files outside FZ-03's `ui/mod.rs`. It can run in
  parallel with e or f once d has landed.
- **FZ-05j** touches `discovery.rs` after b and is not promoted.

### 5.4 Interactions with other tickets

- **FZ-03 (GitHub freshness).** The accepted freshness contract
  (`docs/freshness-contract.md` on main, not in this branch's base) covers the
  dataset as a whole and needs no identity. FZ-03 should not start keying
  anything new by bare name.
- **FZ-04 (fetch outcomes).** Per-repo fetch state belongs to the checkout:
  key it by the display path that `RepoUpdated` and `refresh_fetches` already
  use. `remotes_that_moved` decides *what* to fetch by remote (the carrier)
  and FZ-04 reports *how it went* by checkout.
- **FZ-06 (cache invalidation, `WorkKey`).** `WorkKey` and snapshot reuse are
  per checkout (path), which this design confirms as the checkout key (3.6).
  Linked worktree common-dir handling is FZ-06's; D10 is revisited there.
- **FZ-09 (coverage and cache isolation by account and host).** First, the
  host component of `RemoteId` and FZ-09's host key should be the same
  normalized string (lowercased host, `github.com` today). Isolation by
  account is FZ-09's alone. Second, coverage: a row whose `RemoteId` drops
  out of a poll (it fell below `repo_limit`) keeps its old GitHub fields
  silently today, because `apply_github_poll` only writes matches. This is
  FZ-09's. Saying which rows those are needs the carrier mark from FZ-05d.

## 6. Findings outside identity

Reported here so they are not lost; none is in an FZ-05 ticket.

- **Remote URL drawn verbatim.** The detail pane draws `repo.remote_url` as
  `git remote get-url origin` returns it (ui/detail.rs:711-717, git.rs:988).
  An HTTPS remote with an embedded credential
  (`https://user:token@github.com/...`) would show on screen. **Unconfirmed**
  (not run). Candidate for FZ-04's credential handling or its own issue.
- **`CLAUDE.md` statements the code contradicts.**
  - "The owner is part of the key, so a clone of someone else's same-named
    repo can't claim yours" holds only for the remote match. The folder-name
    fallback lets such a clone claim the row when its folder has the same
    name (F8, S12). The test `a_remote_under_another_owner_does_not_match`
    uses a folder named `their-fork`, so it never exercises this.
  - "reports the undotted name: match, glyph lookup and dedupe all key on the
    GitHub name": the folder fallback returns the folder's spelling, not the
    GitHub name's (`find_in_primary_is_case_insensitive` expects `MyRepo` for
    GitHub `myrepo`). Comparisons ignore case, so only display differs.
- **Undocumented divergences from repoz** (confirmed by reading
  `~/repos/repoz/repoz` at `c0f0065`):
  - repoz dedupes the dataset by `nameWithOwner` (line 784), fleetz by bare
    name (github.rs:460-461). Addressed by FZ-05e.
  - repoz skips pass 3 candidates by path (`checked_paths`, lines 1115 and
    1277), fleetz by path and name (main.rs:2341-2356 and 2529-2535).
    Addressed by FZ-05f.
  - repoz lists a pass 3 repo when it has changes, unpushed commits, an
    in-progress operation or stashes (line 1288), with no time gate. fleetz
    requires changes or unpushed commits (`local_only_status`,
    main.rs:2637-2639), and also drops a dirty repo that fails
    `local_repo_is_current` under the time window (main.rs:2645-2648). A repo
    with only a stash or a paused rebase, or a dirty repo idle past the
    window without unpushed commits, is listed by repoz and not by fleetz.
    Not an identity issue.

## 7. Claims that remain unconfirmed

Nothing was executed. Each item names the test that would confirm it.

- Which clone wins a tie **within one scan root** in F1 and F2. It depends on
  `read_dir` order. Confirm with S1 and S4 as `discovery.rs` tests asserting a
  deterministic path, which fail today only when the order disagrees. (F9 is
  not in this list: across roots the order is the configured one.)
- `merge_sources` dropping `company/api`. Confirm with a `github.rs` test
  calling `split_viewer` on a viewer with both entries (S1 dataset half).
- The poll writing CI onto a local-only row (F4, S14). Confirm with the FZ-05d
  S14 poll test run against the current code, where it should fail.
- `.GIT` uppercase suffix and `file://` URLs (F6). Confirm with
  `repo_from_remote_url` cases in `model.rs` tests.
- That `git remote get-url origin` in a linked worktree returns the main
  clone's origin (m1, S6, S6b). Standard git behavior (the config lives in
  the common directory); confirm with a temp repo and `git worktree add`.
- That forks appear under `viewer.repositories` (F8), that GitHub compares
  owner and name case-insensitively (3.4), and that a transferred repo's old
  URL keeps working (S9c). External behavior; confirm with a live query,
  which is out of scope here.
- S16 ordering hazard. Confirm with a `main.rs` test feeding a two-`api`
  dataset to the not-cloned loop and the poll on the current code.
- The section 5.1 interim states. Reasoned, not run; confirm by running the
  S1 (clean and dirty), S2 clean, S11a, S12 and S13b fixtures at each
  intermediate commit.
