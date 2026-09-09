# Branches view

**Date:** 2026-09-09
**Status:** design approved, unimplemented

## Problem

The `info` view draws every remote-only branch between the pinned local
branches and the `recent commits` section. On a busy repo that is twenty-plus
`not fetched` rows, so the commit list starts below the fold, and the selection
lives in that commit list: `set_detail_selected` sets `follow_selection`, and
the next frame calls `bring_into_view` on the selected block
(`ui/detail.rs`). The pane therefore scrolls past the branches the moment the
cursor asserts itself.

Scrolling back up does not settle it, because scrolling up fights the thing
that scrolled down: the next `j`/`k` sets `follow_selection` again and drags
the view back to the commit.

`docs/ideas.md`, "The behind count and the branches that make it up", already
records the near miss. The pinned region was extended through the *local*
branches, and pinning the whole block was rejected there for the right reason:
"The remote-only list is bounded by what everyone else has pushed, not by what
you have." That reasoning stands. This design does not revisit it; it removes
the need for it.

There is also a second, larger want behind the bug report: somewhere to see
**all** branches, deferred ones as well as active, which fleetz has never had.

## What is being built

A fifth history view in pane 2, `branches`, and a collapsed remote-branch
summary in `info`.

## Decisions, and why

### A fifth view, not a new pane

lazygit's Branches panel does not transplant. Its left column is Status /
Files / Branches / Commits / Stash, five panels about the one repo you are in;
fleetz spends that column on the repo list, because fleetz is about the fleet.
The fleetz equivalent of a lazygit panel is pane 2's view cycle.

A fifth `HistoryView` inherits the pinned identity header, per-view scroll and
selection memory (`detail_selections` and `detail_scrolls` are already
`[_; HistoryView::COUNT]`), the tab strip, `J`/`K` panel driving, and the
docked inspector. A view you can park on also fixes the reported bug outright:
`info` stays the glance, and the branches view has its own remembered scroll
that the commit cursor cannot disturb.

**Rejected: a full-screen overlay.** It would buy a genuine two-column layout
and could go fleet-wide ("which repos have branches rotting on them"), which
is a genuinely interesting tool. But an overlay is something you open and
close, not something you sit in, which fights "show me branches while I look
at this repo". The fleet-wide question is a separate feature and should not
enter through the door marked "fix the info pane".

### Read-only

fleetz already deletes branches, in the `b`/`s` cleanup overlay
(`open_branch_overlay`, `main.rs`), which loads `git::all_branches` and
`git::merged_branches`, marks each entry `is_merged` / `is_current` /
`is_protected`, previews commits per branch, and deletes with a confirm step.

The new view does not mutate anything. Deletion stays in that overlay.

**Why not fold them together now.** Whether you want to delete from a
reference list, or whether deleting is a separate deliberate errand, is a
question the built view answers better than we can in advance. Building the
merged version first commits to an answer. Revisit once the view has been
used.

### Nothing new in the sweep

Merged status stays lazily loaded by the overlay. `git branch --merged` in the
fleet sweep would be a git call per repo per refresh, against the cost rule in
`CLAUDE.md`: "Batch on GraphQL, target on git."

The three data additions below are free or near-free, and none of them adds a
subprocess.

## Data

| Change | Cost |
|---|---|
| `BranchStatus` gains a tip hash | One more tab field, `%(objectname:short)`, on the `for-each-ref` in `git::branch_statuses` that already asks for `%(HEAD)` |
| `RepoStatus` gains the full remote branch list | Zero. `git::remote_branches(path)` is already called in the sweep (four sites in `main.rs`) and the full list is reduced to the unfetched subset and discarded |
| `git::remote_branches` returns name plus tip | One more format field; four call sites update |

`remote_only_branches` stays as it is. Both the `not fetched` marker and the
`info` summary count read it.

## The view

Two `section_label` blocks: `local branches`, then `remotes`.

**Local rows are the rows `info` already draws**: `*` on HEAD in the HEAD
colour, name, `-> origin/wip` only when the upstream differs from the branch's
own name, `N behind` / `N ahead`, `no upstream` or `✓`, worktrees indented
beneath.

That renderer is extracted from `render_detail_pane` into one function both
views call. This is not tidying: two copies of a row vocabulary drift exactly
the way two copies of layout arithmetic do, which is the failure
`detail::split_inspector` exists to prevent, and the reasons those rows read
the way they do (the `*` marker, the conditional arrow, `no upstream` rather
than a bare row) are all recorded in `docs/ideas.md` and would be lost by a
second implementation.

**Remote rows list every remote branch**, not only the unfetched ones, marked
`not fetched` where no local branch claims it: the existing
`git::unfetched_branches` predicate, which counts a remote as claimed when a
local branch shares its short name *or* names it as upstream.

## Inspector

`detail_nth_hash` gains a `Branches` arm returning the highlighted branch's
tip hash. The inspector then draws that commit exactly as it does for every
other view.

**Known limitation, and it lands on the Remotes section.** The inspector
resolves a hash against `repo.commits` / `repo.graph`, which walk the union of
**local** branches capped at 200 commits. A remote-only branch's tip is by
definition not on a local branch, so it will not resolve. The inspector says
`not on any local branch` in words rather than drawing an empty pane, per the
standing rule that categorical states are words and silence must not stand in
for a state (`RepoStatus::is_synced()` sets the precedent).

Fixing it properly means widening what `git::commits` and `git::graph` walk.
Both must be widened together or the invariant in `CLAUDE.md` breaks -- "The
commit list and the graph must walk the same refs" -- and that changes the
`commits` and `graph` views for every repo in the fleet. Out of scope here.
Revisit if the empty inspector proves annoying in practice.

## The info pane

The remote-only list collapses to one row: `23 remote branches not fetched`.
`B` toggles it expanded in place; expanded, it draws exactly what it draws
today.

**Why a bare count rather than a sample.** The house pattern is a sample plus
a count -- `PR_NUMBERS_SHOWN = 2`, `INSPECTOR_FILE_CAP = 30` -- and this
departs from it deliberately. Those samples are *identifying*: you recognise
PR #412, and a changed path says what a commit touched. A remote-only branch
name is not. `origin/feature/loading-questions` is one of twenty-three feature
branches a colleague pushed, and three arbitrary ones tell you nothing the
count does not, while costing three rows of the commit list that the count
does not.

The toggle is an app-level bool, persisting across repos for the session and
resetting on launch: it is a preference about how you want `info` to read, not
a fact about one repo.

Local branches are untouched and stay pinned, including the
`[pinned_rows, header_rows]` fallback ladder.

## Tests

Beside the code, in `#[cfg(test)] mod tests`, each naming the regression it
guards:

- Both views render an identical local-branch row for the same `BranchStatus`
  -- the drift the shared renderer exists to prevent.
- A branch whose upstream differs from its name shows `-> origin/x`; one whose
  upstream matches stays bare.
- A branch with no upstream says `no upstream` rather than rendering bare,
  which reads as "fine".
- `info` with 23 remote branches keeps the `recent commits` label on the first
  screen. This is the reported bug.
- `B` round-trips the collapse, and the expanded list matches what is drawn
  today.
- A remote branch tip absent from the local union reports it in words rather
  than leaving the inspector blank.
- The remotes section lists every remote branch, not only the unfetched ones.

## Out of scope

Revisit each once the view has been lived with.

- Any mutation from this view. Delete stays in the `b` overlay.
- Merged / unmerged annotations in the view.
- Sorting or grouping by age, and the `for-each-ref --sort=-committerdate`
  probe that would need.
- Anything fleet-wide.
- Widening `git::commits` / `git::graph` to include remote refs.
