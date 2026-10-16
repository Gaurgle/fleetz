# Worktrees section in the Info view

**Date:** 2026-10-16
**Status:** accepted by Andreas on 2026-10-16 ("yes, looks perfect"), ticket
B of batch 1, issue #72

## Problem

A linked worktree is drawn today as a path under its branch in the Info
view (`ui/detail.rs::branch_row_lines`, shared with the Branches view) and,
when its branch is not among the local branches, in an `other worktrees`
section at the bottom of Info. That is all fleetz knows about a worktree:
`model::Worktree` carries a path and a branch name. Nothing says whether the
worktree holds uncommitted work, whether its branch is ahead of or behind its
upstream, or when it was last committed to. A worktree with a day of
uncommitted agent work looks exactly like a clean one. The README's unbuilt
"first-class worktrees" note names the gap: "uncommitted work in a worktree
stops being invisible".

A worktree whose HEAD is detached is not listed at all: `parse_worktree_output`
drops entries with no `branch` line.

The screen today, fleetz selected, one linked worktree:

```
branches
  ci/45-pull-request-trigger no upstream
    worktree ~/repos/fleetz/.claude/worktrees/issue-45
* main ✓
  43 remote branches not fetched
```

## What is being built

A `worktrees` section in the Info view, one row per linked worktree, that
answers the developer's questions at a glance: which branch, is it dirty, is
it ahead or behind, how long since its last commit, where it is.

```
worktrees
  ci/45-pull-request-trigger   2!  1↑        6d   .claude/worktrees/issue-45
  fix/fz-02-preserve-state     ✓             2h   .claude/worktrees/fz-02
  detached @ 9a2eb79           ·   ·        14d   ~/scratch/bisect
```

The branches above it lose their inline `worktree <path>` row in Info; the
`other worktrees` section goes. The Branches view is unchanged.

## Decisions, and why

### A section, not an overlay

The README's idea is a `w` overlay with create and prune. That is FZ-16
(#29): it adds keys and a confirm flow and is a later, bigger step. The
glance comes first and costs no interaction: the Info view is "the glance
(branches, worktrees, stashes, newest commits)" by the README's own words,
and a section is where a worktree's state belongs. The overlay, when it
comes, reuses the rows.

### The state comes from git, per worktree, every sweep

Each linked worktree gets two probes on each sweep, run where
`linked_worktrees` is already called (`main.rs`, the cheap tier of the
sweep's status build):

- `git -C <worktree> status --porcelain`, counted the way `dirty_count`
  counts the main checkout (`None` when git fails, so a broken worktree is
  drawn as unknown, never as clean).
- `git -C <worktree> log -1 --format=%ct`, the HEAD commit's time.

Ahead and behind need no probe: a linked worktree's branch is a local branch
of the same repository, so `branch_statuses` already has its counts and its
upstream. The row joins on the branch name at render time.

Why every sweep: worktrees are not refs, so the sweep's `WorkKey` cannot see
them (FZ-06c reads the list fresh for that reason), and dirtiness never
touches the key either (the same reason `dirty_count` is not gated). Two
short processes per linked worktree per sweep; the probes run concurrently,
and only a clone with a `<common>/worktrees` directory pays anything. The
fleet this was measured on has one linked worktree.

**Rejected: the index's mtime as "last touched".** It reads as activity, but
`git status` itself may rewrite the index, and fleetz runs `status` there
every sweep, so the age would read "now" forever. The last commit's time is
honest and stable; the column is labelled by `format_age`, as `last touched`
is on the local-only header.

### Detached worktrees are listed

`git worktree list --porcelain` prints `HEAD <sha>` for every entry and
`detached` instead of `branch` when HEAD is detached. `parse_worktree_output`
keeps the entry, with the short HEAD and a `detached` flag, and the row
reads `detached @ 9a2eb79` with no ahead or behind (there is no branch to
compare), drawn as `·` in the counters' columns. A detached worktree is a
bisect or a review checkout: exactly the thing that is forgotten.

`model::Worktree` keeps `branch: String` (empty for detached) and gains
`detached: bool`, `head: String` (short sha), `dirty: Option<u32>` and
`head_at: Option<DateTime<Utc>>`. Fields are added, none changes type, so
existing tests that build a `Worktree` compile under the standing
compile-forced-field exception, with the new fields set to defaults and no
assertion touched. If an existing test pins that a detached entry is
dropped, the worker stops and the coordinator takes it to Andreas.

### Rows: one vocabulary, one ladder

- **Branch** in `TEXT`, the widest name in the section setting the column,
  capped at 32 cells and cut with the pane's usual ellipsis.
- **Counters** use the repo list's words and colours: `N!` for dirty,
  `N↑` and `N↓` for ahead and behind, through the existing helpers in
  `ui::badges` and `badges::status_color`, not a second copy (`CLAUDE.md`,
  "One color ladder per concept"). A clean, level worktree with an upstream
  shows `✓` in green, as a branch row does; a branch with no upstream shows
  `no upstream` in `OVERLAY`, as a branch row does; a dirty probe that
  failed shows `?` in `OVERLAY`.
- **Age** of the last commit, `format_age`, in `OVERLAY`; `·` when unknown.
- **Path** in `OVERLAY`. When the worktree is under the repository's own
  path it is drawn relative to it (`.claude/worktrees/issue-45`); otherwise
  as today, with `~` for home. The worktree path and the repo path are both
  already `~`-collapsed strings; the shortening is a pure function with its
  own tests.
- The section's label is `worktrees`, through `section_label`, placed after
  the `branches` block (including its remote-count line) and before `recent
  commits`. It is drawn only when the repo has at least one linked worktree.
  It is not part of the pinned region: the local branches are pinned because
  the behind count is their sum; a worktree list is bounded by how many
  checkouts you have made, usually one or two, and sits a line below.

### Info drops the inline path, Branches keeps it

`branch_row_lines` stays shared and unchanged. The Info caller passes it an
empty worktree slice, so Info draws no `worktree <path>` row under a branch
(the section holds the path now, with state) and the `other worktrees`
section is removed as a whole. The Branches view still passes the real list
and keeps the inline row, since it has no worktrees section and the row is
the only place that view says "checked out elsewhere". `shown_worktrees`
therefore stays for the Branches view only.

**Rejected: a marker on the Info branch row** (`· worktree` after the
name). It would need `branch_row_lines` to draw differently per view, which
is the drift the shared function exists to prevent.

### Not in this ticket

- The list row: a dirty worktree changing the `1WT` badge or the repo's `!`
  counter. That is the follow-up ticket "dirty worktree in the list row",
  and it needs its own decision about whether a worktree's changes count as
  the repo's.
- Keys, selection, the inspector: the section is text, like `branches` in
  Info. Enter on a worktree (lazygit or a shell there) belongs to FZ-16.
- `-d` dirty mode and the compact grid.
- Header row 3 (`1 worktree`), which ticket C recolours.

## Interfaces

- `git::linked_worktrees(path) -> Vec<Worktree>` returns the list with the
  new fields filled; a new `git::worktree_state(worktree_path) -> (Option<u32>,
  Option<DateTime<Utc>>)` (or two functions) runs the two probes through
  `run_git`, so they share its hardening and 30 s ceiling.
- `ui::detail`: a pure `worktree_rows(repo: &RepoStatus) -> Vec<Line>` (or
  the equivalent in the existing style) that the Info body calls; the
  relative-path shortening is its own function.
- No change to `ui::panel_rects` or any geometry; no new key; no config key.

## Tests

- `parse_worktree_output` keeps a detached entry with its short HEAD and the
  flag; a branch entry carries `detached: false`.
- A real temporary repository with one linked worktree: `worktree_state`
  reports the dirty count after a file is written there and `None` on a
  path that is not a worktree; the HEAD time matches the commit made.
- Info with one linked worktree draws the `worktrees` label, the branch,
  the counters in the badge colours, the age and the relative path; the
  branch row above it has no `worktree` line; no `other worktrees` label.
- Info with no linked worktree draws no `worktrees` label.
- The Branches view with the same repo still draws the inline `worktree`
  row (the existing Branches tests pass unchanged).
- The relative-path function: under the repo path, outside it, equal to it.
- A detached worktree draws `detached @ <sha>` and `·` counters.

## Cost and risk

Two processes per linked worktree per sweep, concurrent, bounded by
`GIT_TIMEOUT`. A worktree on a slow disk (network mount) could hold a sweep's
status build for up to the ceiling; the same is true of the main checkout's
probes today. Changes the screen: stops at Ready to integrate for the owner's
look.
