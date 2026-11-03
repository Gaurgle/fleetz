---
paths:
  - "src/discovery.rs"
  - "src/main.rs"
  - "src/model.rs"
---

# Discovery and assembly invariants

Binding wherever the session runs. Loaded when a matching file is read or
edited; a loader without path scoping reads this file before changing the
modules it names. Moved from the root `CLAUDE.md` on 2026-10-30, text
unchanged.

**Discovery pass order is load-bearing** (`main.rs::assemble_view`). Pass 1
scans the configured roots, pass 2 sweeps `$HOME`, pass 3 picks up local-only
repos, and only then is anything "not cloned". A local clone wins over that
fallback only through its exact origin, or through an unambiguous folder name
in passes 1 and 2, or a repo under a hidden path reports as absent. A pass 3
clone with no remote does not suppress the not-cloned row: it cannot say which
repo it is.

**GitHub-owned fields live on one carrier row per remote identity**, chosen at
load and marked by the row's `owner` (`main.rs::build_repo_status`,
`mark_local_carriers`). The poll, the moved-remote check, the startup fetch
baseline and the activity sort follow that mark through one predicate
(`carries`, `names_remote`), keyed by owner and name. A local-only row carries
a repo only through its stored origin, never by name: matching by name put a
stranger's CI badge on a same-named clone. Name-only matching survives in one
place, a row or payload entry with no owner (a cache from before the field was
asked for), and the first network poll upgrades it. Sweeps never touch the
mark, so a poll and a sweep can land in either order. A carrier row the poll
did not return keeps its GitHub fields and is marked "not in the last poll"
(`App::not_in_last_poll`, a set of remote keys, never a row field): drawn
dimmed, it no longer holds the 15 s `Pending` poll floor. Clearing those
fields was rejected because a blank lamp reads as "no CI". A successful poll
on another account rebuilds the rows from its dataset; a poll with no account
key never does.

**A clone is matched by its `origin` remote first, and by its folder name only
where the remote cannot decide** (`discovery::Wanted::match_clone`). The folder
name counts when there is no usable origin and the name is unambiguous across
the full dataset, or when the origin names an unknown repo of the same owner
(a rename). A folder name is only a convention: a clone of `portfolio-site` in
`~/repos/DevOps_Portfolio` reported as not cloned, and an unconditional
fallback let a checkout whose origin named another owner claim the row. The
remote is read from `.git/config`, not `git remote get-url`, since it runs for
every candidate in both sweeps. The owner is part of the key, so a clone of
someone else's same-named repo can't claim yours. A linked worktree has no
config of its own and keeps the name match, or it would race its main clone for
the row. When several clones match one repo, the primary is chosen by a fixed
order (exact remote, github.com host, a `.git` directory, earlier root,
shallower path, then path), never by filesystem order
(`discovery::CheckoutRank`, which assembly reuses for local carriers). Matching
diverges from repoz on purpose: repoz still matches clones on names only.

**Pass 3 dedupes by canonical path only** (`main.rs::unclaimed_local`, the raw
path when `canonicalize` fails), as repoz does, so one directory reached
through two root spellings is one row. A dirty extra checkout, or a same-named
local-only repo, gets its own row, which carries GitHub fields only if it is
its remote's carrier. Deduping by folder name hid a dirty second checkout and
drew two same-named local repos as one. The canonical path is a comparison key
only: a row displays the path as its root was spelled.

**A local clone may be dot-prefixed when its GitHub repo is not** (`dotfiles`
lives at `~/.dotfiles`). Discovery matches both spellings and reports the
undotted name: match, glyph lookup and dedupe all key on the GitHub owner and
name (`discovery::matched_name`, `discovery::repo_name_for_path`). A
local-only row is matched only through its stored origin, so a remote-less
`~/.dotfiles` outside the window shows no kind glyph.

**Hidden directories are traversed inside a configured scan root, but not in
the `$HOME` sweep.** A root you named (`repo_dir`, `extra_dirs`) yields its
repos hidden or not, as repoz does; the `$HOME` sweep blanket-skips
dot-directories, since descending every `~/.local` and `~/.npm` costs far more
than it finds. `PRUNE_DIRS` applies to both and mirrors repoz's `PRUNE_EXPR`.

**Discovery parity with repoz is enforced by test, not by memory.**
`discovery::repoz_parity` runs `repoz -d` against a fixture tree and diffs the
paths, skipping loudly when repoz is absent. Fixture repos are deliberately
dirty so a pure-local repoz run reports them all, and the fixture root is
canonicalized: repoz resolves `/var` to `/private/var` and fleetz does not.
