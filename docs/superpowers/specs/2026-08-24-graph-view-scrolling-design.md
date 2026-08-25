# Graph view: white nodes, real scrolling, wrapping, full height

Date: 2026-08-24
Status: **designed**, approved in discussion, not yet implemented.

## Problem

The Graph view (`HistoryView::Graph`) is cramped and does not move the way a
pane of text should. Four complaints, which turn out to share two causes.

    the * node takes the branch colour
    stepping commits shifts the whole view instead of scrolling
    long lines are cut off, never wrapped
    a commit block is tall and the inspector eats the room it needs

The view is the one place fleetz reproduces `git log --graph` verbatim rather
than computing topology itself (`repo.graph_expanded`, rendered by
`detail::expanded_graph_line`). That is deliberate and stays: the compact
computed graph already exists in `info` and `commits`, so this view is the
dedicated full-block point of view, with the metadata interlocked into the
drawing the way git writes it.

### Causes

**Node colour.** `graph_prefix_spans_raw` colours every non-space prefix
character by its column via `colors::graph_lane_color`, `*` included. The
compact graph already exempts nodes (`graph::Cell::is_node`, whose comment
reads "a commit belongs to the history, not to whichever lane it happens to
sit in"). The expanded renderer never got the same rule.

Note `git log --graph` writes `*` for *every* commit, not only merges; merges
are marked by the `Merge:` line and the `|\` pipes. So the rule is "nodes are
not lane-coloured", which is the rule the compact graph already follows. The
compact graph's own glyphs (`⚬` commit, `*` merge, decided in the history-pane
spec) are unaffected.

**Movement.** Two separate faults compound:

- `MouseEventKind::ScrollDown/Up` over `FocusedPanel::Detail` calls
  `detail_nav_down/up`, so the wheel moves the *selection*, not the view.
  There is no wheel scrolling in that pane at all.
- `let anchor_top = view == HistoryView::Graph || row < scroll;` pins the
  selected commit to the top of the pane on **every** selection change. With
  the wheel bound to navigation, one wheel notch slams the view a whole block.

**Wrapping.** `expanded_graph_line` truncates with `ui::truncate`. The
enclosing `Paragraph` does carry `.wrap(Wrap { trim: false })`, but nothing
ever reaches it because every line is pre-cut to width.

**Height.** `split_right_column` always docks an inspector when the column can
afford one. In Graph view its masthead and message restate what the block
already says.

## Decisions

Settled in discussion:

- **The view keeps git's output.** Not rebuilt on the computed graph. The
  compact graph is already available in two other views; this one exists to
  look like what the terminal gives you.
- **Every `*` renders white** (`colors::TEXT`), lane hue reserved for pipes.
- **Wrapped continuation repeats the lane prefix**, so pipes stay unbroken and
  continuation text aligns under the text column. A gap in the pipes reads as a
  topology change that did not happen.
- **Wrapping applies to the whole block**, not only the message. `commit
  1c51c6e (HEAD -> main, origin/main, tag: v0.3.3)` gets wide too, and
  truncating it hides refs.
- **Dynamic means re-wrapped to the current pane width**, including after a
  divider drag or terminal resize. No toggle key.
- **The view and the selection are independent** (editor model). Scrolling
  does not move the selection; the selection may sit off screen; `j`/`k` bring
  it back into view and step.
  **Amended 2026-08-25, Graph view only:** the graph's cursor is a **row**,
  not a commit, and `Shift+J/K`, the wheel and `PgUp/PgDn` move it rather than
  scrolling raw. The highlight walks down the pane a line at a time and parks
  `GRAPH_SCROLLOFF` (4) rows from the edge, from where the text scrolls under
  it -- vim's `scrolloff`, less's default. `App::move_graph_cursor` and
  `keep_graph_cursor_in_view` own the arithmetic; `graph_cursor` is the row and
  `graph_block_tops`, recorded per frame by the renderer, maps it back to the
  commit it is inside (`sync_graph_selection`). `j`/`k` still step commit to
  commit and put the block four rows down the pane
  (`show_graph_cursor_from_top`): the minimal nudge would leave a stepped-to
  block in the *bottom* margin, four rows of it on screen and the rest below.

  Two earlier attempts got this wrong and are recorded so they are not retried.
  A commit-granular marker cannot express it: blocks run past twenty rows, so
  the highlight stood still through a whole commit and then jumped. Anchoring
  that marker to the block owning the top visible row is worse still -- the
  commit row of the block you are inside leaves the top immediately and does
  not come back, so the marker was off screen at every offset but the one where
  a block began exactly at the top row, which read as no cursor at all.

  None of it may set `follow_selection`: bring-into-view puts the row hard
  against an edge, and the next scroll pulls the scrolloff margin back, so the
  two fight.

  **The margin is not graph-only.** `bring_into_view` applies `scrolloff_for`
  in every view, so stepping `j`/`k` anywhere walks the selection down the pane
  and scrolls only once it nears an edge. The other views keep an entry cursor
  rather than a row cursor -- their lists are one row per entry -- so there a
  scroll moves the view and `keep_selection_on_screen` drags the selection to
  the nearest entry that still clears the margin. `detail_row_tops` carries the
  rows for whichever view is drawn, the graph's blocks included.

  Two exemptions, both deliberate. The margin collapses at the ends of the
  content, or the first and last entries could never be selected. And a view
  with no entry left inside the pane keeps its selection where it is: `info`
  draws branches, worktrees and stashes below its commit list, and scrolling
  past the last commit to read them is the point.

- **No inspector at all in Graph view.** The changed-file list remains
  reachable from `info` and `commits`.
- **The wheel scrolls the Detail pane in every history view**, not only Graph.
  Consistency beats a per-view exception, and the inspector already treats the
  wheel this way.

## Design sketch

### Node colour

`graph_prefix_spans_raw` gains one branch: `'*'` takes `colors::TEXT`, spaces
keep `colors::SURFACE`, everything else keeps `graph_lane_color(i)`.

### Wrapping, which restores the row arithmetic rather than breaking it

Wrapping normally splits row index from line index, which the codebase went to
some trouble to eliminate (CLAUDE.md: re-adding `--graph` "brings back filler
rows carrying no commit -- which forced row and commit indices apart
everywhere"). Wrapping *earlier* avoids that entirely.

Graph view draws only the graph section. Every other section in
`history_lines` is gated on `Info`, `Commits` or `Reflog`, so nothing else
contributes lines. `expanded_graph_line` therefore changes from returning
`Line` to returning `Vec<Line>`, wrapping `rest` to the width remaining after
the prefix and emitting one `Line` per visual row, each carrying the prefix
spans.

Because the builder emits exactly the rows that will be drawn, one line is one
rendered row and `lines.len()` is the true height. `wrapped_row_count` is not
needed for this view.

Wrapping is word-aware with a character-level fallback, so a single token
longer than the pane still renders rather than looping.

### Scroll model

`history_lines` currently reports `selected_row: Option<usize>`, the first row
of the selected block. It reports the block's **extent** instead,
`Option<(usize, usize)>`, so a whole block can be brought into view.

Three places set it: the compact graph list, the expanded graph, and the
reflog. Only the expanded graph has a block taller than one row, so the other
two report `(row, row)` and behave exactly as before.

The follow step becomes bring-into-view, with the `HistoryView::Graph` clause
deleted:

    if block top   <  scroll            -> scroll = top
    if block bottom >= scroll + height  -> scroll = min(top, bottom + 1 - height)
    otherwise                              leave scroll alone

Preferring `top` in the second arm keeps the header on screen for a block
taller than the pane, which is what the old top-pinning was protecting and the
only thing it got right.

Keys, in `handle_key`:

| Input | Before | After |
|---|---|---|
| wheel over Detail | `detail_nav_*` | `scroll_detail(±3)` |
| `Shift+j` / `Shift+k` | unbound | `scroll_detail(±1)` |
| `Shift+↓` / `Shift+↑` | unbound | `scroll_detail(±1)` |
| `j` / `k` | `detail_nav_*` | unchanged |
| `PageUp` / `PageDown` | `scroll_detail(±10)` | unchanged |

`follow_selection` is set only by navigation, never by scrolling, which is what
keeps the two independent.

### Inspector

`App::shows_inspector()` returns `self.history_view != HistoryView::Graph`.
`split_right_column` takes it and returns `(area, None)` when false.

Both `ui::panel_rects` and `ui::render` already call `split_right_column`, so
the invariant that they produce identical rectangles holds by construction, the
same way `split_main_area` does it for the contributions strip. No second copy
of the arithmetic appears.

Two consequences:

- `next_panel` must skip `Inspector` when it is not drawn, or Tab lands focus
  on a pane that is not on screen.
- Switching into Graph view while the inspector holds focus must move focus to
  `Detail`, for the same reason.

Short terminals need no special case: `split_right_column`'s existing
`area.height < DETAIL_MIN_HEIGHT + INSPECTOR_MIN_HEIGHT` early return already
yields `None`, and suppression just takes that branch deliberately.

## Testing

Every behaviour change lands with a test that would have caught the old
behaviour, per the repo convention.

- A `*` is `colors::TEXT` whichever column it sits in; a `|` in the same
  column still takes its lane hue.
- A line wider than the pane produces more than one row; every row carries the
  lane prefix; no row exceeds the width.
- The same input at a narrower width produces more rows, which is what dynamic
  means.
- A token longer than the pane still renders, and terminates.
- Scrolling leaves the selected hash unchanged.
- Navigating after scrolling away brings the block back into view.
- A block already fully visible does not move the view (the top-pinning
  regression).
- `split_right_column` returns `None` and the full height when the inspector is
  suppressed, and `panel_rects` reports no inspector rectangle in Graph view.
- Focus cycling skips a suppressed inspector, and switching into Graph view
  moves focus off it.

## Rejected

- **Rebuilding Graph view on the computed graph** (one row per commit, as
  lazygit does and as `info`/`commits` already do). It would make wrapping,
  scrolling and node colour fall out for free, but the compact graph already
  exists in two views; this one earns its place by looking like git's own
  output, and that is what was asked for.
- **Letting `Paragraph` do the wrapping.** Splits row index from line index
  and forces every scroll computation through `wrapped_row_count`. Wrapping in
  the builder keeps the indices identical.
- **Blanking the lane columns on continuation rows.** Reads as a break in the
  topology.
- **A wrap toggle key.** With the inspector gone there is no reason to want
  truncation back, and it is another key and another piece of state.
- **Scoping the wheel rebind to Graph view.** A pane whose wheel means two
  different things depending on a view is worse than either meaning.

## Open questions

- Should `scroll_detail`'s wheel step (3 rows) be a constant shared with the
  contributions strip's `CONTRIB_SCROLL_WEEKS`, or stay independent? They
  scroll different units.
- With the inspector suppressed, `inspector_pct` is still persisted and still
  applies on return to another view. That is probably right, but it means `+`
  and `-` do nothing visible while Graph view is active. Worth a status line
  hint, or worth leaving silent?
