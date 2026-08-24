# Graph View Scrolling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make fleetz's Graph view scroll, wrap and breathe without changing its character as a verbatim rendering of `git log --graph`.

**Architecture:** Four independent changes to the detail pane. Node colour is a
one-branch fix. Wrapping moves from `ui::truncate` into the line builder, which
keeps one source line equal to one rendered row. Scrolling separates the view
offset from the selection and replaces top-pinning with bring-into-view.
Inspector suppression goes through the single `split_right_column` both the
renderer and the hit-tester already call.

**Tech Stack:** Rust, ratatui, crossterm. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-24-graph-view-scrolling-design.md`

## Global Constraints

- **Never use em-dashes or en-dashes** in any output: chat, files, or code.
  Use a hyphen, a colon, parentheses, or rewrite the sentence.
- **Conventional Commits** (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`,
  `cleanup:`), first line under 72 characters. Body is for the why.
- **No `Co-Authored-By` lines.** Ever.
- **Do not commit.** Present the full `git commit -m "..."` command for review.
  The human commits and pushes. (Steps below show the command to present.)
- **Never edit, delete or disable a test to make code pass.** Fix the code.
- Tests live beside the code in `#[cfg(test)] mod tests`, not in `tests/`.
- Every behavior change lands with a test that would have caught the old
  behavior.
- `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` must all
  be clean before each commit.
- `git commit -m` runs under zsh: backticks inside a double-quoted message are
  executed by the shell. Use `-F <file>` for any message containing backticks.

---

### Task 1: Nodes are not lane-coloured

`git log --graph` writes `*` for every commit. The expanded renderer colours it
by column like a pipe, so a commit's hue changes with whichever lane it landed
in. The compact graph already exempts nodes (`graph::Cell::is_node`); this
gives the expanded renderer the same rule.

**Files:**
- Modify: `src/ui/detail.rs` (`graph_prefix_spans_raw`, near line 1470)
- Test: `src/ui/detail.rs` (`#[cfg(test)] mod tests`, near line 1627)

**Interfaces:**
- Consumes: `colors::TEXT`, `colors::SURFACE`, `colors::graph_lane_color`
- Produces: nothing new; `graph_prefix_spans_raw` keeps its signature
  `fn graph_prefix_spans_raw(prefix: &str) -> Vec<Span<'static>>`

- [ ] **Step 1: Write the failing test**

Add inside `mod tests` in `src/ui/detail.rs`:

```rust
/// `git log --graph` writes `*` for every commit, and the node used to take
/// the lane's hue, so the same commit changed colour depending on which
/// column its branch happened to occupy. The compact graph already paints
/// nodes in the plain text colour (`graph::Cell::is_node`): a commit belongs
/// to the history, not to whichever lane it sits in.
#[test]
fn graph_nodes_are_not_lane_coloured() {
    let spans = graph_prefix_spans_raw("| | * |");
    let colour_of = |i: usize| spans[i].style.fg.expect("every span is styled");

    assert_eq!(colour_of(4), colors::TEXT, "the node took a lane colour");
    assert_eq!(
        colour_of(0),
        colors::graph_lane_color(0),
        "pipes must keep their lane hue"
    );
    assert_ne!(
        colour_of(0),
        colour_of(6),
        "two pipes in different lanes must stay distinguishable"
    );
}

/// A node is a node in every column, which is the whole point: the old rule
/// made column 0 and column 4 different colours for the same kind of thing.
#[test]
fn a_node_is_the_same_colour_in_every_column() {
    let first = graph_prefix_spans_raw("* ");
    let fifth = graph_prefix_spans_raw("| | * ");
    assert_eq!(
        first[0].style.fg.unwrap(),
        fifth[4].style.fg.unwrap(),
        "node colour still depends on the column"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test ui::detail::tests::graph_nodes_are_not_lane_coloured -- --exact`

Expected: FAIL, `assertion left == right failed: the node took a lane colour`.

- [ ] **Step 3: Write the implementation**

Replace the body of `graph_prefix_spans_raw` in `src/ui/detail.rs`:

```rust
fn graph_prefix_spans_raw(prefix: &str) -> Vec<Span<'static>> {
    prefix
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let color = match ch {
                ' ' => colors::SURFACE,
                // `git log --graph` marks every commit with `*`. A node is
                // part of the history, not part of whichever lane it landed
                // in, so it keeps the plain text colour whatever its column.
                // The compact graph has always done this (`Cell::is_node`);
                // this is the expanded renderer catching up.
                '*' => colors::TEXT,
                _ => colors::graph_lane_color(i),
            };
            Span::styled(ch.to_string(), Style::default().fg(color))
        })
        .collect()
}
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`

Expected: all pass, clippy clean, fmt clean.

- [ ] **Step 5: Present the commit for review**

```bash
git add src/ui/detail.rs
git commit -m "fix: paint graph nodes in the text colour, not the lane's"
```

---

### Task 2: Wrap in the builder, not with a truncation

`expanded_graph_line` cuts every line to width, so nothing ever reaches the
`Paragraph`'s own wrap. Wrapping in the builder (rather than letting the
`Paragraph` do it) is what keeps one source line equal to one rendered row,
so the scroll arithmetic in Task 3 stays exact.

**Files:**
- Modify: `src/ui/detail.rs` (`expanded_graph_line` near line 1483, and its
  one call site near line 758)
- Test: `src/ui/detail.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `hash_style`, `wash`, `pad_to`, `graph_prefix_spans_raw`,
  `colors::{SAPPHIRE, OVERLAY, TEXT}`
- Produces:
  - `fn wrap_text(text: &str, budget: usize) -> Vec<String>` (private)
  - `fn expanded_graph_lines(raw: &str, selected_hash: &str, is_selected: bool, width: usize) -> Vec<Line<'static>>`
    replaces `expanded_graph_line`, which is deleted. Task 3 relies on the
    returned `Vec` length being the number of rendered rows.

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests` in `src/ui/detail.rs`:

```rust
/// The graph view truncated every line to the pane width, so a long subject
/// or a long decoration list was simply unreadable. Wrapping happens in the
/// builder rather than in the `Paragraph` so that one source line stays one
/// rendered row, which the scroll arithmetic depends on.
#[test]
fn a_long_graph_line_wraps_instead_of_truncating() {
    let raw = "|     feat: rotate the contribution window's from date past \
               GitHub's per-from-date response cache";
    let rows = expanded_graph_lines(raw, "", false, 40);
    assert!(rows.len() > 1, "the line was truncated, not wrapped");
    for row in &rows {
        let w: usize = row.spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(w <= 40, "a wrapped row is {w} wide, past the pane's 40");
    }
}

/// Every row carries the lane prefix. A gap in the pipes reads as a topology
/// change that did not happen, so the continuation repeats it.
#[test]
fn wrapped_rows_repeat_the_lane_prefix() {
    let raw = "| | |     a subject long enough to need more than one row here";
    let rows = expanded_graph_lines(raw, "", false, 30);
    assert!(rows.len() > 1, "fixture did not wrap");
    for (i, row) in rows.iter().enumerate() {
        let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.starts_with("  | | |"),
            "row {i} lost the lane prefix: {text:?}"
        );
    }
}

/// "Dynamic" means it re-wraps to whatever the pane is now, so a narrower
/// pane must produce more rows from the same input.
#[test]
fn a_narrower_pane_produces_more_rows() {
    let raw = "|     a subject long enough to wrap at several different widths";
    let wide = expanded_graph_lines(raw, "", false, 60).len();
    let narrow = expanded_graph_lines(raw, "", false, 30).len();
    assert!(narrow > wide, "wrapping did not follow the width: {narrow} vs {wide}");
}

/// A single token longer than the pane must still render, and must terminate.
/// Breaking only at spaces would loop forever on a 200-character URL.
#[test]
fn a_token_longer_than_the_pane_still_renders() {
    let long = "x".repeat(120);
    let rows = wrap_text(&long, 20);
    assert_eq!(rows.len(), 6, "expected 120 chars to fill six 20-wide rows");
    for row in &rows {
        assert!(row.chars().count() <= 20);
    }
    assert_eq!(rows.concat(), long, "characters were dropped");
}

/// Empty input is one empty row, not zero rows: the blank lines between
/// git's blocks are load-bearing spacing.
#[test]
fn empty_text_wraps_to_a_single_empty_row() {
    assert_eq!(wrap_text("", 20), vec![String::new()]);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test ui::detail::tests::a_long_graph_line_wraps_instead_of_truncating -- --exact`

Expected: FAIL to compile, `cannot find function expanded_graph_lines`.

- [ ] **Step 3: Write the implementation**

Add `wrap_text` above `expanded_graph_line` in `src/ui/detail.rs`:

```rust
/// Split `text` into chunks that each fit `budget` columns.
///
/// Breaks at spaces where it can and mid-token where it cannot, because a
/// space-only break loops forever on a token wider than the pane (a long URL
/// in a commit message is the common case). Always returns at least one
/// chunk: git's blocks use blank lines as spacing, and dropping them would
/// close the gaps between commits.
fn wrap_text(text: &str, budget: usize) -> Vec<String> {
    if budget == 0 {
        return vec![String::new()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut len = 0usize;
    for word in text.split_inclusive(' ') {
        let wlen = word.chars().count();
        if len > 0 && len + wlen > budget {
            out.push(std::mem::take(&mut line));
            len = 0;
        }
        if wlen > budget {
            for ch in word.chars() {
                if len == budget {
                    out.push(std::mem::take(&mut line));
                    len = 0;
                }
                line.push(ch);
                len += 1;
            }
        } else {
            line.push_str(word);
            len += wlen;
        }
    }
    out.push(line);
    out
}
```

Replace `expanded_graph_line` entirely with:

```rust
/// One raw `git log --graph` line, as the rows it occupies at this width.
///
/// Returns a `Vec` rather than a `Line` because a wrapped line is several
/// rows, and the caller needs the true count: the pane's scroll offset is a
/// row index, so one source line must equal one rendered row. Letting the
/// `Paragraph` wrap instead would split row index from line index, which is
/// the split the compact graph exists to avoid.
fn expanded_graph_lines(
    raw: &str,
    selected_hash: &str,
    is_selected: bool,
    width: usize,
) -> Vec<Line<'static>> {
    const GRAPH_CHARS: [char; 6] = ['*', '|', '\\', '/', '_', ' '];
    let split = raw
        .char_indices()
        .find(|(_, c)| !GRAPH_CHARS.contains(c))
        .map_or(raw.len(), |(i, _)| i);
    let (prefix, rest) = raw.split_at(split);

    let style = if let Some(hash) = rest.strip_prefix("commit ") {
        // "commit 1c51c6e (tag: v0.3.0)" -- the hash is the first word.
        let bold = hash
            .split_whitespace()
            .next()
            .is_some_and(|h| !selected_hash.is_empty() && h == selected_hash);
        hash_style(bold)
    } else if ["Author:", "Date:", "Merge:"]
        .iter()
        .any(|k| rest.starts_with(k))
    {
        // Metadata rows: present, but not what you are reading the block for.
        Style::default().fg(colors::OVERLAY)
    } else {
        Style::default().fg(colors::TEXT)
    };

    let prefix_w = prefix.chars().count();
    let budget = width.saturating_sub(2 + prefix_w);
    wrap_text(rest, budget)
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            // The caret marks the block, not every row of it.
            let marker = if is_selected && i == 0 { "▸ " } else { "  " };
            let mut spans = vec![Span::styled(
                marker.to_string(),
                Style::default().fg(colors::SAPPHIRE),
            )];
            spans.extend(graph_prefix_spans_raw(prefix));
            spans.push(Span::styled(chunk, style));
            if is_selected {
                spans = wash(pad_to(spans, width));
            }
            Line::from(spans)
        })
        .collect()
}
```

Update the one call site in `render_detail_pane` (near line 758). Replace:

```rust
            lines.push(expanded_graph_line(raw, &selected_hash, is_sel, width));
```

with:

```rust
            lines.extend(expanded_graph_lines(raw, &selected_hash, is_sel, width));
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`

Expected: all pass. The graph view now wraps; scrolling is still wrong, which
Task 3 fixes.

- [ ] **Step 5: Present the commit for review**

```bash
git add src/ui/detail.rs
git commit -m "feat: wrap graph view lines instead of truncating them"
```

---

### Task 3: Bring the selection into view instead of pinning it to the top

`anchor_top` is unconditionally true in Graph view, so every selection change
slams the selected commit to the top of the pane. With Task 2 landed, a block
can also be taller than one row, so the follow step needs the block's extent
rather than its first row.

**Files:**
- Modify: `src/ui/detail.rs` (`render_detail_pane`: the `selected_row` local
  near line 723, its three producers near lines 737, 755 and 776, and the
  follow block near line 962)
- Modify: `src/app.rs` (`cycle_history_view` doc comment near line 1757)
- Test: `src/ui/detail.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `expanded_graph_lines` from Task 2
- Produces: `fn bring_into_view(block: (u16, u16), scroll: u16, height: u16) -> u16`
  (private, in `src/ui/detail.rs`) so the rule is testable without a frame.

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests` in `src/ui/detail.rs`:

```rust
/// The graph view pinned the selected commit to the top of the pane on every
/// selection change, so stepping one commit moved the whole view. A block
/// already fully on screen must not move it at all.
#[test]
fn a_visible_block_does_not_move_the_view() {
    // Block occupies rows 12..=17, the pane shows rows 10..=29.
    assert_eq!(bring_into_view((12, 17), 10, 20), 10);
}

/// A block above the viewport scrolls up to sit at the top.
#[test]
fn a_block_above_the_viewport_scrolls_to_its_top() {
    assert_eq!(bring_into_view((4, 9), 10, 20), 4);
}

/// A block below the viewport scrolls down by the least that reveals it.
#[test]
fn a_block_below_the_viewport_scrolls_the_minimum() {
    // Block ends at row 34; a 20-row pane must start at 15 to show it.
    assert_eq!(bring_into_view((30, 34), 10, 20), 15);
}

/// A block taller than the pane cannot be shown whole, so its top wins: the
/// header names the commit, and scrolling to the bottom of an oversized block
/// leaves you looking at the tail of a message with nothing to say whose it
/// is. This is the one thing the old top-pinning got right.
#[test]
fn an_oversized_block_shows_its_top() {
    // Block occupies rows 30..=60, which is 31 rows in a 20-row pane.
    assert_eq!(bring_into_view((30, 60), 10, 20), 30);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test ui::detail::tests::a_visible_block_does_not_move_the_view -- --exact`

Expected: FAIL to compile, `cannot find function bring_into_view`.

- [ ] **Step 3: Write the implementation**

Add near `wrapped_row_count` in `src/ui/detail.rs`:

```rust
/// The scroll offset that brings `block` (an inclusive row range) into view,
/// moving as little as possible.
///
/// Replaces the old rule, which pinned the selected commit to the top of the
/// pane on every selection change. That kept an oversized block's header on
/// screen but made stepping one commit move the entire view, so the pane
/// never felt scrolled, only jumped.
///
/// A block taller than the pane still resolves to its top: the header is what
/// names the commit, and the tail of a long message with no header above it
/// says nothing about whose message it is.
fn bring_into_view(block: (u16, u16), scroll: u16, height: u16) -> u16 {
    let (top, bottom) = block;
    if top < scroll {
        return top;
    }
    if bottom >= scroll + height {
        // `min(top)` keeps the header on screen when the block cannot fit.
        return top.min(bottom + 1 - height);
    }
    scroll
}
```

In `render_detail_pane`, change the local near line 723 from:

```rust
    let mut selected_row: Option<usize> = None;
```

to:

```rust
    // The selected block's inclusive row range. The expanded graph's blocks
    // run to twenty rows and wrap on top of that, so bringing "the selection"
    // into view means bringing a range into view, not a row. The compact
    // graph and the reflog report a one-row range and behave as before.
    let mut selected_block: Option<(usize, usize)> = None;
```

Update the compact-graph producer near line 737: `graph_list_lines` takes
`&mut selected_row`; change its parameter to `&mut Option<(usize, usize)>` and
inside it replace `*selected_row = Some(lines.len());` with
`*selected_block = Some((lines.len(), lines.len()));`.

Update the reflog producer near line 776, replacing:

```rust
                selected_row = Some(lines.len());
```

with:

```rust
                selected_block = Some((lines.len(), lines.len()));
```

Replace the expanded-graph loop near line 748 with:

```rust
        let selected_hash = app.inspected_hash().unwrap_or_default().to_string();
        let mut open: Option<usize> = None;
        for raw in &repo.graph_expanded {
            // A block runs from the line that names its commit to the line
            // before the next one names another. `opens_commit` answers only
            // "does this line open *this* commit", so the end of the selected
            // block is found by watching for the next `commit ` line.
            let starts_any = raw.contains("commit ");
            if starts_any && let Some(start) = open.take() {
                selected_block = Some((start, lines.len().saturating_sub(1)));
            }
            let is_sel = opens_commit(raw, &selected_hash);
            if is_sel {
                open = Some(lines.len());
            }
            lines.extend(expanded_graph_lines(raw, &selected_hash, is_sel, width));
        }
        // The selected block was the last one, so it runs to the end.
        if let Some(start) = open {
            selected_block = Some((start, lines.len().saturating_sub(1)));
        }
        lines.push(Line::from(""));
```

Replace the follow block near line 962 with:

```rust
    if app.follow_selection.get()
        && let Some((top, bottom)) = selected_block
    {
        scroll = bring_into_view((top as u16, bottom as u16), scroll, area.height);
        scroll = scroll.min(max_scroll);
        app.set_detail_scroll(scroll);
        app.follow_selection.set(false);
    }
```

In `src/app.rs`, `cycle_history_view`'s comment claims the expanded graph
"anchors its selection to the top of the pane", which stops being true. Replace
that comment's last sentence with:

```rust
        // Deliberately does not follow the selection. Each view keeps its own
        // scroll offset, so arriving restores where you left that view --
        // which is where its cursor is anyway, since the two were left in
        // step. Following on arrival would scroll the view you just asked
        // for to wherever its cursor happened to be.
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`

Expected: all pass.

- [ ] **Step 5: Present the commit for review**

```bash
git add src/ui/detail.rs src/app.rs
git commit -m "fix: bring the selected commit into view instead of pinning it"
```

---

### Task 4: The wheel scrolls

**Scope correction, found during self-review.** `Shift+j`/`Shift+k` and
`Shift+↑`/`Shift+↓` already exist (`src/main.rs:1152-1155`), already call
`scroll_detail` through `App::scroll_focused`, and are already listed in the
help overlay as `⇧ J/K ↑/↓`. Do not reimplement them. The spec asked for
"regular scroll, and/or shift+up/down/jk"; only the first half is missing.

What is actually missing is the wheel, which calls `detail_nav_down/up` and so
moves the selection rather than the view. `src/ui/help.rs:23` already tells the
user "wheel scrolls", which is false for this pane today and becomes true here.

**Files:**
- Modify: `src/main.rs` (mouse handler near line 836)
- Modify: `README.md` (mouse row of the keybindings table, line 99)
- Test: `src/app.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `App::scroll_detail(delta: i32)`, `App::detail_selected()`
- Produces: `const DETAIL_WHEEL_ROWS: i32` in `src/main.rs`

- [ ] **Step 1: Write the failing test**

The key and mouse dispatch in `handle_key` / `handle_mouse_input` needs a live
`Terminal`, so it is not reachable from a unit test. The property that matters
is testable at the `App` level: scrolling must not disturb the selection. Add
inside `mod tests` in `src/app.rs`:

```rust
/// Scrolling and the cursor are independent: the wheel used to call
/// `detail_nav_down`, so it moved the selection instead of the view and the
/// pane could only ever jump from commit to commit.
#[test]
fn scrolling_the_detail_pane_leaves_the_selection_alone() {
    let mut app = make_app(3);
    app.history_view = HistoryView::Graph;
    app.set_detail_selected(2);
    let before = app.detail_selected();

    app.scroll_detail(5);
    app.scroll_detail(-2);

    assert_eq!(app.detail_selected(), before, "scrolling moved the cursor");
}

/// The other half: scrolling must not ask the pane to chase the selection,
/// or the next frame would undo the scroll.
#[test]
fn scrolling_does_not_request_follow() {
    let mut app = make_app(3);
    app.follow_selection.set(false);
    app.scroll_detail(5);
    assert!(
        !app.follow_selection.get(),
        "a scroll asked the renderer to follow, which would undo it"
    );
}
```

- [ ] **Step 2: Run the tests to verify they pass or fail honestly**

Run: `cargo test app::tests::scrolling_the_detail_pane_leaves_the_selection_alone -- --exact`

Expected: PASS. `scroll_detail` already leaves both alone; these tests pin
behaviour the rebinding depends on, so that a future change to `scroll_detail`
cannot quietly reintroduce the coupling. If either fails, stop: the premise of
this task is wrong.

- [ ] **Step 3: Write the implementation**

In `src/main.rs`, add near the other consts (by `GITHUB_POLL_MIN`):

```rust
/// Rows the detail pane scrolls per wheel notch.
///
/// The wheel used to call `detail_nav_down/up`, moving the selection rather
/// than the view, so the pane jumped a whole commit block per notch. Three
/// rows is a notch that feels like scrolling without outrunning the eye.
const DETAIL_WHEEL_ROWS: i32 = 3;
```

Replace the two `FocusedPanel::Detail` arms in the mouse handler near line 836:

```rust
        MouseEventKind::ScrollDown => match panel {
            FocusedPanel::RepoList => app.move_down(),
            FocusedPanel::Detail => app.scroll_detail(DETAIL_WHEEL_ROWS),
            FocusedPanel::Inspector => app.scroll_inspector(1),
            FocusedPanel::Contributions => {}
        },
        MouseEventKind::ScrollUp => match panel {
            FocusedPanel::RepoList => app.move_up(),
            FocusedPanel::Detail => app.scroll_detail(-DETAIL_WHEEL_ROWS),
            FocusedPanel::Inspector => app.scroll_inspector(-1),
            FocusedPanel::Contributions => {}
        },
```

Do not touch `handle_key`. The Shift bindings at `src/main.rs:1152-1155`
already do this and already route through `scroll_focused`.

Update the mouse row of the keybindings table in `README.md` (line 99) so it
says the wheel scrolls the detail pane rather than moving its cursor.
`src/ui/help.rs:23` already claims "wheel scrolls" and needs no change: this
task is what makes that claim true.

- [ ] **Step 4: Run the full suite**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`

Expected: all pass.

- [ ] **Step 5: Verify by hand, since the dispatch is not unit-testable**

Run: `cargo run` then focus the detail pane (`2`), switch to the graph view
(`]` until the title reads `graph`), and check: the wheel scrolls without
moving the caret; `J`/`K` still scroll one row as before; `j`/`k` move the
caret and pull the view back to it.

- [ ] **Step 6: Present the commit for review**

```bash
git add src/main.rs src/app.rs README.md
git commit -m "fix: scroll the detail pane on the wheel instead of stepping"
```

---

### Task 5: No inspector in Graph view

The graph block already carries the hash, author, date and full message, so
the inspector repeats them while taking the rows the block needs.

**Files:**
- Modify: `src/app.rs` (add `shows_inspector`, `toggle_panel_focus` near line
  1478, `focus_panel` near line 1499, `cycle_history_view` near line 1762)
- Modify: `src/ui/detail.rs` (`split_right_column` near line 40)
- Modify: `src/ui/mod.rs` (both `split_right_column` call sites, lines 321 and
  334)
- Modify: `CLAUDE.md` (the `panel_rects` invariant)
- Test: `src/app.rs` and `src/ui/mod.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `App::history_view`, `HistoryView::Graph`
- Produces:
  - `pub fn shows_inspector(&self) -> bool` on `App`
  - `pub fn split_right_column(area: Rect, pct: u16, show_inspector: bool) -> (Rect, Option<Rect>)`
    (third parameter added)

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests` in `src/app.rs`:

```rust
/// The graph block already carries hash, author, date and message, so the
/// inspector repeats them while taking the rows the block needs.
#[test]
fn the_graph_view_hides_the_inspector() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Commits;
    assert!(app.shows_inspector());
    app.history_view = HistoryView::Graph;
    assert!(!app.shows_inspector());
}

/// Tab must not park focus on a pane that is not drawn: every later keypress
/// would go somewhere invisible.
#[test]
fn focus_cycling_skips_a_hidden_inspector() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Graph;
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::Detail;
    app.toggle_panel_focus();
    assert_ne!(
        app.focused_panel,
        FocusedPanel::Inspector,
        "focus landed on a pane the graph view does not draw"
    );
}

/// Same trap by the other door: `3` jumps straight to the inspector.
#[test]
fn jumping_to_a_hidden_inspector_is_refused() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Graph;
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::Detail;
    app.focus_panel(FocusedPanel::Inspector);
    assert_eq!(app.focused_panel, FocusedPanel::Detail, "focus went to a hidden pane");
}

/// And by the third: switching into the graph view while the inspector holds
/// focus has to hand it back.
#[test]
fn entering_the_graph_view_moves_focus_off_the_inspector() {
    let mut app = make_app(1);
    app.view = View::Detail;
    app.history_view = HistoryView::Reflog;
    app.focused_panel = FocusedPanel::Inspector;
    // HistoryView::ALL is [Info, Commits, Reflog, Graph], so Reflog steps
    // forward into Graph.
    app.cycle_history_view(false);
    assert_eq!(app.history_view, HistoryView::Graph, "fixture assumption");
    assert_eq!(app.focused_panel, FocusedPanel::Detail, "focus was left on a hidden pane");
}
```

Add inside `mod tests` in `src/ui/mod.rs`:

```rust
/// `panel_rects` must report exactly what `render` draws, or clicks land on
/// the wrong pane silently. Both call `split_right_column`, so suppressing
/// the inspector in one suppresses it in the other by construction.
#[test]
fn panel_rects_reports_no_inspector_in_the_graph_view() {
    let mut app = crate::app::App::new(Vec::new(), String::new(), 120, 44);
    app.view = crate::app::View::Detail;
    app.history_view = crate::app::HistoryView::Graph;
    let rects = panel_rects(Rect::new(0, 1, 120, 40), &app);
    assert!(rects.inspector.is_none(), "the graph view still docks an inspector");

    let detail = rects.detail.expect("detail is always drawn in Detail view");
    app.history_view = crate::app::HistoryView::Commits;
    let with = panel_rects(Rect::new(0, 1, 120, 40), &app);
    assert!(
        detail.height > with.detail.unwrap().height,
        "suppressing the inspector did not give its rows to the graph"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test the_graph_view_hides_the_inspector -- --exact app::tests::the_graph_view_hides_the_inspector`

Expected: FAIL to compile, `no method named shows_inspector`.

- [ ] **Step 3: Write the implementation**

In `src/app.rs`, add next to `visible_repos`:

```rust
    /// Whether the inspector is drawn at all.
    ///
    /// The graph view's blocks already carry the hash, author, date and full
    /// message, so an inspector there repeats the block while taking the rows
    /// the block needs. Read by both `ui::panel_rects` and `ui::render`
    /// through `detail::split_right_column`, so the two cannot disagree about
    /// whether the pane exists.
    pub fn shows_inspector(&self) -> bool {
        self.history_view != HistoryView::Graph
    }
```

In the same file, guard `toggle_panel_focus`'s Detail arm:

```rust
        self.focused_panel = match self.focused_panel {
            FocusedPanel::RepoList => FocusedPanel::Detail,
            FocusedPanel::Detail if self.shows_inspector() => FocusedPanel::Inspector,
            FocusedPanel::Detail if strip => FocusedPanel::Contributions,
            FocusedPanel::Detail => FocusedPanel::RepoList,
            FocusedPanel::Inspector if strip => FocusedPanel::Contributions,
            FocusedPanel::Inspector | FocusedPanel::Contributions => FocusedPanel::RepoList,
        };
```

Guard `focus_panel`, immediately after the `Contributions` arm:

```rust
        if panel == FocusedPanel::Inspector && !self.shows_inspector() {
            return;
        }
```

And in `cycle_history_view`, after `self.history_view = HistoryView::ALL[next];`:

```rust
        // The inspector may have just stopped being drawn; focus cannot stay
        // on a pane that is not on screen.
        if self.focused_panel == FocusedPanel::Inspector && !self.shows_inspector() {
            self.focused_panel = FocusedPanel::Detail;
        }
```

In `src/ui/detail.rs`, change `split_right_column`:

```rust
pub fn split_right_column(area: Rect, pct: u16, show_inspector: bool) -> (Rect, Option<Rect>) {
    // The graph view gives the whole column to the history: its blocks
    // already say what an inspector would.
    if !show_inspector || area.height < DETAIL_MIN_HEIGHT + INSPECTOR_MIN_HEIGHT {
        return (area, None);
    }
    let height = (area.height * pct / 100)
        .clamp(INSPECTOR_MIN_HEIGHT, INSPECTOR_MAX_HEIGHT)
        .min(area.height - DETAIL_MIN_HEIGHT);
    let v = Layout::vertical([Constraint::Min(1), Constraint::Length(height)]).split(area);
    (v[0], Some(v[1]))
}
```

In `src/ui/mod.rs`, update both call sites (lines 321 and 334) to:

```rust
                    detail::split_right_column(right_column, app.inspector_pct, app.shows_inspector());
```

In `CLAUDE.md`, extend the `panel_rects` invariant with one sentence:

```
Whether a pane exists at all goes through the same function (`shows_inspector`
feeds `split_right_column`), so the two cannot disagree about that either.
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`

Expected: all pass. Check `CLAUDE.md` stays under 200 lines with
`wc -l CLAUDE.md`; trim prose elsewhere if it does not.

- [ ] **Step 5: Verify by hand**

Run: `cargo run`, press `]` to reach the graph view, and check: no inspector,
the graph fills the column, Tab cycles repo list to detail and back without
stopping anywhere invisible, `3` does nothing, and leaving the graph view
brings the inspector back at its previous size.

- [ ] **Step 6: Present the commit for review**

```bash
git add src/app.rs src/ui/detail.rs src/ui/mod.rs CLAUDE.md
git commit -m "feat: give the graph view the whole column, no inspector"
```

---

## Self-Review

**Spec coverage.** Node colour, Task 1. Wrapping with repeated prefix and
whole-block scope, Task 2. Extent plus bring-into-view and the deleted
`HistoryView::Graph` clause, Task 3. Wheel rebind and Shift keys, Task 4.
Inspector suppression, focus in all three of its entry points, and the
`panel_rects` invariant, Task 5. The spec's two open questions are answered in
the plan: `DETAIL_WHEEL_ROWS` stays independent of `CONTRIB_SCROLL_WEEKS`
(different units, Task 4), and `+`/`-` are left silent in Graph view (no
status hint added; revisit if it confuses in use).

**Placeholders.** None. Every code step carries the code.

**Type consistency.** `expanded_graph_lines` returns `Vec<Line<'static>>` in
Task 2 and is consumed with `lines.extend(...)` in Tasks 2 and 3.
`bring_into_view` takes and returns `u16` throughout, and Task 3 casts the
`usize` block bounds at the call. `split_right_column`'s third parameter is
`bool` in Task 5's definition and both call sites. `shows_inspector` is
`&self -> bool` in its definition and all five uses.

**Known gap.** Task 4's mouse dispatch is not unit-testable in this codebase,
since `handle_mouse_input` needs a live `Terminal`. The plan pins the
`App`-level property instead and adds a manual verification step. This is
stated rather than papered over.

**Correction made during self-review.** Task 4 originally added `Shift+j/k`
and `Shift+arrow` bindings. They already exist at `src/main.rs:1152-1155`,
already call `scroll_detail` via `scroll_focused`, and are already documented
in the help overlay. The task now covers only the wheel, which is the half
that was genuinely missing. `DETAIL_WHEEL_ROWS` is still new.

**Pre-existing issue this plan does not fix.** `selected_block` holds *line*
indices, which equal *row* indices only where no line exceeds the pane width.
Task 2 guarantees that for the graph view by pre-wrapping, but `info`,
`commits` and `reflog` still let the `Paragraph` wrap, so a long subject there
can put the follow step a row or two out. That is today's behaviour, not a
regression, and fixing it means pre-wrapping those views too. Out of scope
here; worth a note in `docs/ideas.md` if it ever bites.
