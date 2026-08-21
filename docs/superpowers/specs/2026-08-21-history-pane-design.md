# History pane (selectable Commits / Reflog / Graph views)

Date: 2026-08-21
Status: **draft, not approved** — open questions below need answers first

## Problem

The repo pane (`[2]`) is seven things stacked in one scrolling paragraph:

    status header      sync state, path, remote, branch, stashes, PRs, CI
    branches           local branches, worktrees inline, unfetched remotes
    CI detail          workflow name and duration
    recent commits     hash, author, message, diffstat
    history            git log --graph, replayed verbatim
    reflog             recent HEAD movements
    other worktrees    those not matched to a branch
    stashes            count plus each entry

Everything after the header is history in some form, and all of it competes
for the same scroll offset. Wanting to see the graph means scrolling past the
commits; wanting the reflog means scrolling past both. Nothing can be looked
at side by side with anything else, and `detail_scroll` is a single number
covering all of it.

lazygit solves this by giving each kind of history its own view in one pane,
switched rather than scrolled. Its commit list, reflog and graph are the same
pane wearing three hats, and each is dense because it does one job.

## Decisions

Settled in discussion:

- **The repo pane stops carrying history.** It keeps the status header,
  branches, worktrees and stashes: the reference facts you glance at. That is
  what it is good at and what its name implies.
- **A history pane with switchable views**, defaulting to the commit list.
  Commits, Reflog and Graph are three renderings of the same underlying
  question and should share a pane rather than a scroll offset.
- **Ordinary commits render as `⚬`, merges keep `*`.** Shipped separately
  ahead of this spec; see "Already landed" below.

## Open questions

These change the design materially. Answer before implementation.

### 1. Is the inspector a peer view, or does it follow the selection?

The original suggestion was that the inspector becomes a fourth view of the
history pane, and its default.

**The objection:** the inspector shows the selected commit's detail. As a peer
view it cannot be on screen at the same time as the list it describes, which
is the only reason to have both. Today they are stacked and you see both at
once; making them peers is a regression in the one flow they exist for.

**The alternative:** the *list* is what switches (Commits / Reflog / Graph),
and the inspector stays its own pane, showing whatever the active view has
selected. That generalises it: selecting a reflog entry shows that commit,
selecting a node in Graph shows that commit. The inspector stops being
commit-list-specific and becomes "detail of the current selection", which is a
better concept than it has now.

Recommended: the alternative. Needs confirmation, because it decides the pane
count below.

### 2. Where do the views live, and how many panes result?

**Option A — four panes in the right column.**

    [2] info       status · branches · worktrees · stashes
    [3] history    ( Commits | Reflog | Graph )
    [4] inspector  detail of [3]'s selection

Cleanest separation, and each pane does one job. Costs a third stacked pane in
a column that is already tight: `split_right_column` currently reserves
`DETAIL_MIN_HEIGHT = 8` and `INSPECTOR_MIN_HEIGHT = 8`, and a third pane needs
its own floor. On a 40-row terminal the right column is ~38 rows, so three
panes plus borders is roughly 10 rows each. Workable but not generous, and the
contributions strip takes 10 more when open.

**Option B — views inside the repo pane, inspector unchanged.**

    [2] repo       status header (pinned)
                   ( Info | Commits | Reflog | Graph )
    [3] inspector  detail of [2]'s selection

Keeps the pane count and the existing geometry entirely. The status header
stays pinned above the switchable body, the way the selected-commit readout is
pinned today. Less structurally clean, but no new floors, no new focus target,
and `split_right_column` is untouched.

Recommended: **B first**, with A as a later move if the pane proves cramped.
B delivers the whole benefit (history stops sharing a scroll offset with
everything else) for a fraction of the layout risk, and the invariant this
repo guards hardest is the one A would disturb.

### 3. Does the Graph view replace the inline graph, or duplicate it?

If Graph is a view, the `history` section inside the repo pane should go —
otherwise the same `git log --graph` output renders in two places with two
scroll offsets. Recommended: it goes. Same for `recent commits` and `reflog`,
which become the Commits and Reflog views.

Open: does the Graph view show `--all` (every branch, as the current inline
one does) or only the current branch? lazygit's is `--all`. The current inline
graph is scoped to local branches. Changing it is a behaviour change either
way.

### 4. How are views switched?

Candidates, not mutually exclusive:

- **A cycle key** (`tab`, or a letter) walks the views in order. Cheapest, one
  binding, no numbers to remember. Poor for jumping straight to Graph.
- **Number keys within the pane.** Collides directly: `1`–`4` already focus
  panes, which is a documented binding and muscle memory.
- **Named letters** (`c` / `r` / `g`). `c` is already the contributions strip
  and `r` is refresh, so these would need to be pane-local, active only while
  the history pane has focus. That is a new modality the app does not have.

Recommended: a cycle key, plus `shift`+that key to cycle backwards. Defer
direct jumps until the cycle proves annoying.

### 5. Does the view choice persist?

`persist_pane_sizes` already establishes the pattern of opt-in persistence
written at teardown. The contributions source persists unconditionally via its
own config key. Open: which of the two this follows.

## Already landed

Ahead of this spec, because it is separable and needed no layout change:

`git log --graph` draws every commit as `*`, which is a heavy glyph repeated
on every row. The graph format gained `%P`, so `GraphLine::is_merge` is known
from the parent count, and ordinary commits render as `⚬` while merges keep
`*`. This is most of the visual tightening lazygit gets from its own graph
renderer, without replacing git's.

`⚬` is U+26AC, East Asian **Narrow**. That is load-bearing rather than
incidental: the graph column is padded to the widest prefix and drawn per
cell, so an Ambiguous-width node would be one column in some terminals and two
in others, misaligning every row after it. The obvious candidates — `●` `○`
`•` `◆` `⊙` — are all Ambiguous. `◦` `∘` `◊` `⊛` are not, and are the fallback
set if `⚬` renders poorly in some font.

## Design sketch

Written against Option B. Rework if question 2 resolves to A.

### State

    /// Which rendering of history the repo pane's body is showing.
    pub enum HistoryView { Info, Commits, Reflog, Graph }

On `App`: `history_view: HistoryView`, defaulting to `Info` so the pane opens
looking as it does today. A `cycle_history_view(&mut self, back: bool)`
alongside the existing `cycle_contributions_source`.

### Rendering

`render_detail_pane` splits into a pinned header (status rows, unchanged) and
a body dispatched on `history_view`. Each view is its own function returning
`Vec<Line>`, so the existing `wrapped_row_count` / `detail_max_scroll`
machinery applies unchanged to whichever is active.

**Scroll offset per view or shared?** Shared is one field and wrong the moment
you switch back to a view you had scrolled. Per-view is four fields on `App`
and correct. Recommended: per-view, as a small `[u16; 4]` indexed by the enum.

### Selection

`detail_selected` currently indexes `repo.commits`. Under Reflog and Graph it
must index those lists instead, or the inspector shows an unrelated commit. It
becomes per-view too, and the inspector resolves the selected *hash* rather
than an index, so it works the same whichever list produced it.

That is the piece most likely to be underestimated: the inspector's data
source today is `repo.commits[detail_selected]`, and it needs to become "look
up this hash", which for a reflog entry or graph node may need a commit fetch
those lists do not carry today.

### Geometry

Option B touches no geometry: no change to `split_main`,
`split_right_column`, `split_main_area` or `panel_rects`. That is the main
argument for it.

Option A adds a third pane to the right column and therefore a new floor, a
new `PanelRects` field, a new focus target, a new divider, and hit-testing for
both. Every one of those is the drift `split_right_column` exists to prevent.

### Tab strip

One row at the top of the body, above the scrolling content so it does not
scroll away. Active view in `LAVENDER` (the focus colour used elsewhere),
inactive in `SUBTEXT` — not `OVERLAY`, which is furniture. Separator in
`SURFACE`, matching the `·` dividers already used in the header.

## Testing

- View cycling wraps in both directions and lands on every view.
- Each view renders its own content and none of the others'.
- Scroll offset and selection survive a round trip through the other views.
- The inspector follows the active view's selection, including from Reflog.
- The header stays pinned: it renders identically in every view.
- Under Option A only: `panel_rects` reports the third pane exactly where
  `render` drew it, at several widths and heights, including the ones where a
  pane is dropped for want of room.

## Rejected

- **Views as a fourth `View` alongside Detail and Compact.** Those are
  whole-app layouts; this is one pane's body. Wrong axis.
- **An overlay, like the branches or help overlays.** Those are modal and
  dismissed; history is something you sit in while doing other things.
- **Making the inspector a peer view.** See question 1. It is the flow both
  panes exist to serve, and peers cannot be on screen together.
