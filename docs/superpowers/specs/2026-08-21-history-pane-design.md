# History pane (selectable Commits / Reflog / Graph views)

Date: 2026-08-21
Status: **shipped**, Option B. Kept for the reasoning; the open questions
below record what was decided and what is still open.

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

## Questions, as resolved

### Resolved

- **Q1, the inspector: it follows the selection.** Not a peer view. It
  resolves the selected *hash* against the loaded commit list, so Commits,
  Reflog and Graph all reach the same inspector, and `Info` borrows the
  commit view's selection rather than blanking it.
- **Q2, layout: Option B.** Views live inside the repo pane with the header
  pinned above them. No geometry moved: `split_main`, `split_right_column`,
  `split_main_area` and `panel_rects` are untouched.
- **Q3: the Graph view replaces the inline graph**, and `recent commits` and
  `reflog` became views for the same reason. Still open: whether Graph should
  show `--all` rather than the current local-branch scope.
- **Q4: `[` and `]`.** lazygit's own binding for the gesture, and unlike a
  shifted letter it gives backwards for free. `V` was tried first and
  rejected in use.
- **Q5: the view does not persist.** It resets to `Info` each launch, like
  the pane sizes do by default.

### Revised after first use

The first cut put the commit list under `commits` and left `info` as
reference-only. Using it showed that wrong in three ways, all fixed:

- **`info` needs the newest commits.** It is the glance, and a glance with no
  commits in it is not one. It lists `RECENT_COMMIT_COUNT` = 6 with their
  diffstats, which is the one place per-commit churn is shown: the graph
  column layout has no room for it.
- **`commits` is the graph list, complete.** git's topology column, then hash,
  author, decorations and subject -- lazygit's commits pane. `HISTORY_COUNT`
  = 200 rather than the old 10, and `reflog` the same rather than 6, which is
  what made both look truncated.
- **`graph` is git's *expanded* `log --graph`**, one block per commit with
  Merge/Author/Date and the full message, reproduced rather than recomposed.
  The first cut used the compact list here, which was not the thing being
  asked for. Its node stays `*`: that format spells merges out on their own
  line, so swapping the glyph would only stop it looking like what it copies.

Two things fell out of the rework:

- Every view has its own selection, `info` included, so `HistoryView::is_list`
  is gone -- there is no view without a list any more.
- The pinned readout was still indexing `repo.commits` whatever was on screen,
  so under `commits`, `reflog` and `graph` it named a commit from a different
  list. It resolves the hash now, like the inspector. A test caught it.

### Second rework: the tight graph

Driven by lazygit's commits pane, which is denser than the first cut managed.

- **The graph is translated, not replayed.** git's ASCII becomes box drawing
  (`│ ╯ ╮ ─`); the node becomes `⚬`, or `⊛` for a merge -- the asterisk git
  already used, circled. Box drawing is East Asian Ambiguous, which was the
  stated objection to it earlier in this document. That objection was wrong:
  every pane border in the app is already drawn with the same characters, so
  a terminal that mis-sized them would have broken fleetz long ago. The nodes
  stay Narrow regardless, being the one glyph free to choose.
- **Columns follow lazygit: hash, author, graph, subject.** The topology is
  context for a row rather than its identity, and leading with it pushed the
  columns you actually read rightwards by however wide the branching was.
- **Selection is a whole-row wash plus a `▸`**, not a bolded hash. Bold marks
  one token and the eye still has to find which; in a two-hundred row list
  that is the work.
- **`info` carries the same list**, capped at `RECENT_COMMIT_COUNT` = 6, with
  the diffstat dot-filled right. One renderer for both, so they cannot drift.
- **The expanded view anchors a selected commit to the top** of the pane
  rather than merely bringing it on screen: its blocks run to twenty lines,
  so a header at the last visible row leaves its body below the fold.

Two bugs worth remembering, both self-inflicted and both caught by tests
rather than by looking:

- **The pane fought itself.** `detail_nav_up` unwound the scroll offset and
  the next frame's follow-the-selection put it straight back, so `k` did
  nothing at all. The renderer now follows only when the selection *moved*: a
  flag the cursor sets and the render pass clears. Free scrolling and
  cursor-dragging are different gestures and must not share a mechanism.
- **Rows overflowed.** Refs long enough to fill the pane pushed the diffstat
  onto a line of its own, and a forced `.max(1)` dot made it worse. Refs are
  dropped whole on a narrow pane now and the subject keeps a 24-column floor.

Measured, not guessed: 1300 lines of expanded graph render in under 1ms a
frame, so the paragraph was never the bottleneck and slicing it would have
been the wrong fix. The sweep did get dearer -- `log -200 --numstat` is
0.23s against 0.05s for `-6` -- but it is gated on `WorkKey`, so it is paid
once per changed repo rather than per sweep.

### Third rework: the graph is computed, not parsed

The second rework translated `git log --graph`'s ASCII into box drawing. That
was the wrong mechanism, and reading lazygit's source made it obvious why:
**lazygit never passes `--graph` at all.** Its log format is
`--pretty=format:+%H%x00%at%x00%aN%x00%ae%x00%P%x00%m%x00%D%x00%s` -- `%P` is
all it needs -- and it computes the topology itself in
`pkg/gui/presentation/graph/`. That buys two things translation cannot:

- **One row per commit, always.** git's graph emits a filler row for every
  fan-out and every collapse. Those rows carry no commit, so the hash, author
  and subject columns had to be held open on a line that said nothing, and
  every consumer had to count commits separately from rows. A merge now fans
  out *on the merge commit's own row*. Over 26 commits of `pinz`, git needs 46
  lines to say what this says in 26.
- **Two columns per lane.** The second column is where the horizontal `─`
  lives. git packs a diagonal into one column, which cannot be translated: a
  `╯` with no `─` to its left is punctuation floating beside the line it is
  supposed to join, which is exactly how the translated version looked.

`src/graph.rs` is the port. Pipes carry `from_pos`/`to_pos`/`from_hash`/
`to_hash`/kind; each commit terminates the pipes pointing at it, starts one
per parent, and slides continuing pipes left into freed lanes. Cells carry
`up/down/left/right` flags and resolve through a 16-entry box-drawing table.
Nine of lazygit's own fixtures are ported alongside it, which is the point of
porting rather than approximating: the cases they cover (a lane with room to
move left, a merge filling a gap before a continuing pipe, an unreferenced
tip) are easy to get subtly wrong and hard to notice.

Four deliberate deviations:

- **Node glyphs stay `⚬` and `⊛`.** lazygit's `◯` and `⏣` are both East Asian
  Ambiguous, and the node is the one glyph here free to choose.
- **Pipes are coloured by lane of origin, not by author.** lazygit's scheme
  paints a single-maintainer repo in one flat hue. Its *mechanism* is better
  than the old one, though, and is kept: the style rides on the pipe, so a
  branch holds its colour as it migrates lanes rather than being recoloured by
  whichever lane it currently occupies.
- **The column is padded, not ragged.** lazygit lets each row be as wide as it
  needs, so subjects step rightwards under a merge. A dashboard is read by
  scanning a column.
- **No pipe highlight for the selected commit.** The row wash and the `▸`
  already say which row; a third signal is noise.

Two things measurement changed:

- **The padding needed a cap.** `pinz` reaches twenty lanes -- 39 columns --
  over 200 commits, and padding the other 190 rows to that spends a third of
  the pane on whitespace. The column is capped at a quarter of the pane width;
  past the cap a row keeps its whole graph and gives up subject width instead,
  which is honest, because that row really does have twenty branches in it.
- **`MIN_MESSAGE_W` is no longer a floor on the rendered message.** It was,
  and on a row deeper than the cap it forced the row one column past the pane
  edge, wrapping it and shifting every row below -- the same class of bug the
  second rework thought it had fixed. It now governs only whether the ref list
  is drawn, which is the lever that can actually yield. A test builds a
  24-lane history and asserts no row exceeds the pane at three widths; it
  failed on its first run.

The expanded view (`graph`) still reproduces `git log --graph` verbatim,
filler rows and all, because reproducing git's block layout is what it is for.
The two views therefore draw topology differently, which is a loose end.

### Fourth pass: the switcher moves to the frame, and width buys information

Two things this document got wrong, both found by using it.

- **The switcher was never pinned.** This spec said it sat "above the
  scrolling content so it does not scroll away", and the doc comment on
  `view_tabs` repeated the claim. It was the scrolling body's first row, so
  paging down took it off screen -- exactly when "what am I looking at" gets
  asked. It is a right-aligned title on the pane's top border now, which is
  structurally scroll-proof, and it carries the keys that move it: `[` and `]`
  drawn as two lit keys either side of a dimmed `/`, so the pair does not read
  as the single `/` key, which is bound to search. Too narrow for all four and
  it shows the active one alone. The body gets its two rows back.
- **Only two of the four views marked their selection.** `info` and `commits`
  had the caret and the row wash; `reflog` and `graph` bolded the hash and
  nothing else, which marks one token and leaves the eye to find which. All
  four now share `▸` plus the wash. In `graph`, whose rows are lines rather
  than commits, the marker lands on the `commit <hash>` line that opens the
  block -- the same row the scroll already anchors to, so the two agree.

Separately, and from the same look at the app as a whole: **more terminal
width was buying no more information anywhere.** Every horizontal dimension
was a percentage or a constant.

- The compact grid was `columns = if width >= 48 { 2 } else { 1 }` -- two
  columns at 48 and two at 300. It now derives the count from what the visible
  repos need (widest name capped at `NAME_TARGET`, widest badge run, a minimum
  dot fill), clamped to six. Twenty-nine repos went from fifteen rows to eight
  at 200 columns. The old cell arithmetic, `width / columns - 1`, lands exactly
  on `width` at two columns and one past it for every column after, so it had
  to be fixed before more columns were possible; a test pins the row inside the
  pane at six widths.
- The repo list was a flat 35% of the terminal, which is 70 columns at 200 for
  a row that needs about 45. It is capped at `REPO_LIST_MAX_COLS`, and the
  surplus goes to the right column, whose commit subjects grow into whatever
  they are given. Only the default yields: once the divider has been dragged
  the number is the user's answer and is honoured exactly, however wide the
  terminal. The cap lives in `split_main`, the one function both `render` and
  `panel_rects` reach the split through, so hit testing cannot drift from it --
  and a test asserts that directly, because that drift fails silently.

### Still open

- Backward cycling, if four views proves enough to want it.
- Direct jumps to a named view, which needs a pane-local key mode the app
  does not otherwise have.
- Whether Graph shows `--all`.
- Whether the expanded view should also drop `--graph` and share the computed
  layout, so the two views agree on what the topology looks like.
- Whether a reflog selection outside the loaded commit range should trigger a
  fetch for that one commit, rather than the inspector reporting it is out of
  range.

## Original open questions

Kept because they record the reasoning, not just the outcome.

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
