use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::{wash, wrap_text};
use crate::model::RepoStatus;
use crate::ui::colors;
use crate::ui::format_age;

/// Colorize a `git log --graph` ASCII prefix by character column ("lane"),
/// so parallel branch lines read as distinct dull hues instead of one flat,
/// nearly-invisible color -- see colors::graph_lane_color.
/// A commit's decorations, coloured per ref kind: `(HEAD -> main, origin/main)`
/// with HEAD, the local branch and the remote branch each in their own colour
/// rather than the whole blob in one.
///
/// Returns the leading space too, so a commit with no decorations contributes
/// nothing at all rather than a stray column. Must agree with
/// `GraphLine::refs_width`, which budgets the message column against it.
fn ref_spans(refs: &[crate::model::GitRef]) -> Vec<Span<'static>> {
    if refs.is_empty() {
        return Vec::new();
    }
    let punct = || Style::default().fg(colors::OVERLAY);
    let mut spans = vec![Span::styled(" (", punct())];
    for (i, r) in refs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(", ", punct()));
        }
        if r.is_head {
            spans.push(Span::styled(
                crate::model::HEAD_ARROW,
                Style::default().fg(colors::ref_color(crate::model::RefKind::Head)),
            ));
        }
        spans.push(Span::styled(
            r.name.clone(),
            Style::default().fg(colors::ref_color(r.kind)),
        ));
    }
    spans.push(Span::styled(")", punct()));
    spans
}

/// Commits shown in the `info` view: a glance, not a history. `commits`
/// carries the complete list.
///
/// Shared with `App::selected_commit_count`, which bounds the cursor: a
/// second copy of this number let the cursor stop one row short of the list
/// -- or one row past it -- depending on which of the two was edited.
pub(crate) const RECENT_COMMIT_COUNT: usize = 6;

/// The compact graph list, shared by `info` and `commits` so the two cannot
/// drift into rendering the same rows differently.
///
/// `limit` caps the rows drawn, which is also the number of commits: the graph
/// is computed from parent hashes, so a fan-out happens on the merge commit's
/// own row rather than on a connector row of its own.
pub(super) fn graph_list_lines(
    repo: &RepoStatus,
    width: usize,
    selected: usize,
    limit: Option<usize>,
    selected_block: &mut Option<(usize, usize)>,
) -> Vec<Line<'static>> {
    // Lanes are two columns each. Padded to the widest row rather than left
    // ragged the way lazygit leaves it: a dashboard is read by scanning down a
    // column, and a subject that starts two columns further right on the rows
    // that happen to sit under a merge defeats that.
    // Lanes are two columns each, padded to a common width so the subject
    // column lines up down the pane rather than running ragged the way lazygit
    // leaves it: a dashboard is read by scanning a column, and a subject that
    // starts further right on whichever rows happen to sit under a merge
    // defeats that.
    //
    // Capped, though, and the measurement is what forced the cap. A long
    // PR-merge history puts many lanes in flight for a handful of rows -- pinz
    // reaches twenty over two hundred commits -- and padding the other hundred
    // and ninety to that spends a third of the pane on whitespace. Past the cap
    // a row keeps its whole graph and gives up subject width instead, which is
    // the honest trade: that row really does have twenty branches in it.
    let widest = 2 * repo.graph.iter().map(|g| g.lanes.len()).max().unwrap_or(0);
    let lane_w = match widest {
        0 => 0,
        // Rounded down to a whole cell, and never narrower than one.
        w => w.min(((width / 4) & !1).max(2)),
    };
    let author_w = crate::ui::AUTHOR_W;
    let hash_w = repo
        .graph
        .iter()
        .map(|g| g.hash.chars().count())
        .max()
        .unwrap_or(7);
    // The two churn columns. `+2` and `+338` have to share a right edge, and
    // so do their deletions. Right-aligning the pair as one blob -- which is
    // what the tail did -- puts every row's `+` in a different column, so the
    // one part of the row that is read by scanning down was the one part left
    // ragged. Same argument as `lane_w` and `hash_w` above, and measured the
    // same way.
    //
    // Over the rows this call will actually draw, not the whole history:
    // `info` shows six of a hundred and fifty, and measuring all of them would
    // reserve width for churn that is not on screen.
    let (add_w, del_w) = repo
        .graph
        .iter()
        .take(limit.unwrap_or(usize::MAX))
        .filter_map(|g| repo.commits.iter().find(|c| c.hash == g.hash))
        .fold((0usize, 0usize), |(add, del), c| {
            (
                add.max(stat_part_width(c.insertions)),
                del.max(stat_part_width(c.deletions)),
            )
        });
    let stat_len = stat_tail_width(add_w, del_w);

    let mut lines = Vec::new();

    // One row per commit, so the row index and the commit index are the same
    // number. They were not while the graph came from `git log --graph`: a
    // fan-out or a collapse got a row of its own carrying no commit, and the
    // two had to be counted separately.
    for (row, g) in repo.graph.iter().enumerate() {
        if limit.is_some_and(|n| row >= n) {
            break;
        }
        let is_sel = row == selected;
        if is_sel {
            // Relative to this function's own output, which the caller offsets
            // to the pane: these lines are a slice of the body, not the body.
            *selected_block = Some((row, row));
        }

        // lazygit's column order: who and what first, then the topology, then
        // the subject. The graph is context for the row, not its identity, so
        // leading with it pushed the two columns you actually read rightwards
        // by however wide the branching happened to be.
        //
        // A row deeper than the cap sets its own width, so the budget below
        // measures what the row will really draw rather than the cap, which
        // would overrun the pane.
        let row_lane_w = lane_w.max(2 * g.lanes.len());
        let lane_pad = " ".repeat(row_lane_w - 2 * g.lanes.len());

        let author = crate::ui::author_code(&g.author);
        let author_pad = " ".repeat(author_w.saturating_sub(author.chars().count()));
        let hash_pad = " ".repeat(hash_w.saturating_sub(g.hash.chars().count()));
        // Decorations are dropped whole on a narrow pane rather than
        // truncated, and the subject keeps a floor: a row that spends its
        // width naming three remotes and then shows nine characters of what
        // was done has the priority backwards. A branch name is recoverable
        // from the branches view; the subject is why you are reading the row.
        let fixed = MARKER_W + hash_w + 1 + author_w + 1 + row_lane_w;
        let refs_budget = width.saturating_sub(fixed + MIN_MESSAGE_W);
        let refs_len = match g.refs.is_empty() {
            true => 0,
            false => 1 + g.refs_width(),
        };
        let show_refs = refs_len > 0 && refs_len <= refs_budget;
        let refs_len = if show_refs { refs_len } else { 0 };
        // The churn, looked up by hash: the graph rows and the commit list are
        // separate probes, so a row deeper than the commit window simply has
        // none rather than showing a wrong one.
        //
        // The width comes from the columns rather than from this row, so a row
        // with no churn still reserves them. That is what makes the leader dots
        // stop where the `+` column begins on every row instead of running
        // under it on the rows that happen to have nothing to show.
        let churn = repo.commits.iter().find(|c| c.hash == g.hash);
        let right = if stat_len > 0 { 1 + stat_len } else { 0 };
        let used = fixed + refs_len;
        // No floor here. `show_refs` above already guarantees the subject
        // `MIN_MESSAGE_W` columns by dropping the decorations that would have
        // taken them, so a floor on top of that only fires when the row is out
        // of width for reasons no floor can fix -- a graph twenty lanes deep --
        // and forcing one there pushed the row past the pane edge and wrapped
        // it, shifting every row below.
        let msg_budget = width.saturating_sub(used + 2 + right);
        let message = crate::ui::truncate(&g.message, msg_budget);
        let msg_len = message.chars().count();
        // Two spaces are spent inside the row -- before the message and before
        // the dots -- and `right` already carries its own leading one.
        // Not `.max(1)`: a row whose refs and subject already fill the width
        // has no room for a fill, and forcing one is what pushed the tail
        // onto a line of its own.
        let dot_count = width.saturating_sub(used + msg_len + 2 + right);
        // The second of the two spaces the budget reserved, between the
        // subject and whatever follows it. It used to be emitted only
        // alongside the dots, so a row whose subject filled its whole budget
        // -- leaving no dots to draw -- came out one column short and slid its
        // churn tail left by one, breaking the column against the rows that
        // did have dots. Which rows truncate depends on the pane width, which
        // is why the tail looked aligned at one size and ragged at another.
        //
        // Withheld only when the row is already over budget, the case
        // `dot_count`'s saturation absorbs: a graph deep enough to overrun the
        // pane should not be handed another column to overrun it with.
        let fits = used + msg_len + 2 + right <= width;

        // A pointer as well as the wash: the band says which row, the caret
        // survives a terminal that renders backgrounds poorly.
        let mut row = vec![Span::styled(
            if is_sel { "▸ " } else { "  " },
            Style::default().fg(colors::SAPPHIRE),
        )];
        row.push(Span::styled(g.hash.clone(), hash_style(is_sel)));
        row.push(Span::raw(hash_pad));
        row.push(Span::raw(" "));
        row.push(Span::styled(
            author,
            Style::default().fg(colors::author_color(&g.author)),
        ));
        row.push(Span::raw(author_pad));
        row.push(Span::raw(" "));
        row.extend(lane_spans(&g.lanes));
        row.push(Span::raw(lane_pad));
        if show_refs {
            row.extend(ref_spans(&g.refs));
        }
        row.push(Span::raw(" "));
        row.push(Span::styled(message, Style::default().fg(colors::TEXT)));
        if fits {
            row.push(Span::raw(" "));
        }
        if dot_count > 0 {
            row.push(Span::styled(
                "·".repeat(dot_count),
                Style::default().fg(colors::SURFACE),
            ));
        }
        if stat_len > 0 {
            row.push(Span::raw(" "));
            row.extend(stat_spans(churn, add_w, del_w));
        }
        if is_sel {
            row = wash(row);
        }
        lines.push(Line::from(row));
    }
    lines
}

/// Width of the selection marker column, held open on every row so the rows
/// do not shift sideways as the cursor moves.
const MARKER_W: usize = 2;

/// Columns the subject keeps whatever the decorations want.
///
/// The one lever that yields to it is the ref list, which is dropped whole
/// rather than truncated: a branch name is recoverable from the branches view,
/// while the subject is why you are reading the row. It is not a floor on the
/// rendered message -- see the budget in `graph_list_lines`.
const MIN_MESSAGE_W: usize = 24;

/// A row's graph column: two spans per cell, since a cell's connector often
/// belongs to a different branch than its node and so carries its own colour.
fn lane_spans(cells: &[crate::graph::Cell]) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(cells.len() * 2);
    for cell in cells {
        // Nodes are plain text, not lane-coloured: a commit belongs to the
        // history rather than to whichever lane it landed in, and colouring
        // them made the node column change hue every time a lane was reused.
        let glyph_color = match cell.is_node() {
            true => colors::TEXT,
            false => colors::graph_lane_color(cell.style as usize),
        };
        spans.push(Span::styled(
            cell.glyph.to_string(),
            Style::default().fg(glyph_color),
        ));
        spans.push(Span::styled(
            cell.connector.to_string(),
            Style::default().fg(colors::graph_lane_color(cell.connector_style as usize)),
        ));
    }
    spans
}

/// `git log --graph`'s own ASCII prefix, coloured per column.
///
/// Only the expanded view still needs this: it reproduces git's block layout
/// verbatim, so it gets git's drawing too. The compact list computes its own
/// (`lane_spans`).
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
                _ => {
                    // git writes two columns per lane: the glyph (|, \, /, _) and
                    // a horizontal run beside it (space or connector). Palette must
                    // be indexed by lane, not column, or adjacent lanes land two
                    // apart and the first six lanes only use three of six colours.
                    colors::graph_lane_color(i / 2)
                }
            };
            Span::styled(ch.to_string(), Style::default().fg(color))
        })
        .collect()
}

/// `+12 -3` split so additions and deletions keep their own colours, each half
/// right-aligned in its own column.
///
/// A half the row does not have is drawn as blanks rather than skipped: the
/// blanks are what hold the other half's column when one row has deletions and
/// the row above it does not. `None` -- a graph row deeper than the commit
/// window -- is the same case with both halves missing.
///
/// No trailing space: the caller budgeted the row against exactly
/// `stat_tail_width`, so an extra column here pushes the tail onto a line of
/// its own.
fn stat_spans(
    commit: Option<&crate::model::Commit>,
    add_w: usize,
    del_w: usize,
) -> Vec<Span<'static>> {
    let (insertions, deletions) = commit.map_or((0, 0), |c| (c.insertions, c.deletions));
    let mut spans = Vec::new();
    if add_w > 0 {
        let text = match insertions {
            0 => String::new(),
            n => format!("+{n}"),
        };
        spans.push(Span::styled(
            format!("{text:>add_w$}"),
            Style::default().fg(colors::DIFF_ADD),
        ));
    }
    if del_w > 0 {
        if add_w > 0 {
            spans.push(Span::raw(" "));
        }
        let text = match deletions {
            0 => String::new(),
            n => format!("-{n}"),
        };
        spans.push(Span::styled(
            format!("{text:>del_w$}"),
            Style::default().fg(colors::DIFF_DEL),
        ));
    }
    spans
}

/// Columns a `+N` or `-N` token occupies, zero when the count is zero and the
/// token is not drawn at all.
pub(super) fn stat_part_width(n: u32) -> usize {
    match n {
        0 => 0,
        n => 1 + n.to_string().chars().count(),
    }
}

/// Columns the churn tail occupies, separator included. Zero when no row in
/// the list has any churn, so such a list spends no width on it.
pub(super) fn stat_tail_width(add_w: usize, del_w: usize) -> usize {
    match (add_w, del_w) {
        (0, 0) => 0,
        (only, 0) | (0, only) => only,
        (add, del) => add + 1 + del,
    }
}

/// The selected row's hash goes bold in every list, so the pane always says
/// which commit the inspector below is showing.
pub(super) fn hash_style(selected: bool) -> Style {
    let base = Style::default().fg(colors::HASH_CLR);
    if selected {
        base.add_modifier(Modifier::BOLD)
    } else {
        base
    }
}

/// One line of `git log --graph`'s expanded output, coloured.
///
/// Split at the first character that is not part of the graph drawing, so the
/// topology keeps its per-lane colours and the text after it is styled by what
/// git put there. The commit whose hash matches `selected` goes bold, which is
/// the only way this view can say what the inspector is showing.
/// Whether this line of `git log --graph`'s expanded output is the `commit
/// <hash>` line that opens `hash`'s block.
pub(super) fn opens_commit(raw: &str, hash: &str) -> bool {
    !hash.is_empty() && commit_hash_at(raw) == Some(hash)
}

/// The characters git draws the lanes with. Space is one of them, so the
/// split below also eats a message body's indentation.
const GRAPH_CHARS: [char; 6] = ['*', '|', '\\', '/', '_', ' '];

/// Split a raw `git log --graph` row into its lane prefix and the text after
/// it. One function so the marker, the block bounds and the renderer cannot
/// disagree about where the drawing stops and the text starts.
fn split_graph_prefix(raw: &str) -> (&str, &str) {
    let split = raw
        .char_indices()
        .find(|(_, c)| !GRAPH_CHARS.contains(c))
        .map_or(raw.len(), |(i, _)| i);
    raw.split_at(split)
}

/// The hash of the commit this row opens, or `None` for every other row.
///
/// The hash is validated, not just located: a substring test for "commit "
/// matched message bodies, and this repo's own log says "that commit had the
/// mechanism" and "one wheel notch jumped a whole commit block". Every one of
/// those opened a phantom block, so the graph reported thirty block starts for
/// twenty-five commits and the marker indexed past the commit it was on.
pub(super) fn commit_hash_at(raw: &str) -> Option<&str> {
    let word = split_graph_prefix(raw)
        .1
        .strip_prefix("commit ")?
        .split_whitespace()
        .next()?;
    // Decorations follow after a space, so the first word is the bare hash.
    (word.len() >= 4 && word.chars().all(|c| c.is_ascii_hexdigit())).then_some(word)
}

/// Rewrite git's `Date:` row as an age measured now, or `None` for any other
/// row.
///
/// `git::graph_expanded` asks for `--date=iso-strict` so this can: a relative
/// date is frozen at the moment git rendered it, and that output is only
/// refetched when the repo's WorkKey moves. An idle repo sat on "3 minutes
/// ago" all night while `commits` and `reflog` beside it correctly said "16h
/// ago", because those two carry a timestamp and format it per frame. This is
/// the same trade `format_age` documents, applied to the one surface that
/// still had git do the formatting.
fn refresh_date_line(rest: &str) -> Option<String> {
    let stamp = rest.strip_prefix("Date:")?.trim();
    let parsed = chrono::DateTime::parse_from_rfc3339(stamp).ok()?;
    // Git's own column: `Date:` and three spaces, so the value stays under
    // the one on the `Author:` row above it.
    Some(format!(
        "Date:   {}",
        format_age(parsed.with_timezone(&chrono::Utc))
    ))
}

/// One raw `git log --graph` line, as the rows it occupies at this width.
///
/// Returns a `Vec` because a wrapped line is several rows and the caller
/// needs the true count: the pane's scroll offset is a row index, so one
/// source line has to stay one rendered row. Letting the `Paragraph` wrap
/// instead would split row index from line index, which is exactly the split
/// the computed graph exists to avoid. Wrapping earlier avoids the problem
/// that wrapping later creates.
pub(super) fn expanded_graph_lines(
    raw: &str,
    selected_hash: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let (prefix, rest) = split_graph_prefix(raw);
    // Git's `Date:` row, recomputed as of now. See `refresh_date_line`.
    let refreshed = refresh_date_line(rest);
    let rest = refreshed.as_deref().unwrap_or(rest);

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

    let budget = width.saturating_sub(2 + prefix.chars().count());
    wrap_text(rest, budget)
        .into_iter()
        .map(|chunk| {
            // The caret column is held open on every row: `mark_cursor_row`
            // fills it in on the one row the cursor is on, and a column that
            // appeared only there would shift the whole drawing sideways.
            let mut spans = vec![Span::styled(
                "  ".to_string(),
                Style::default().fg(colors::SAPPHIRE),
            )];
            spans.extend(graph_prefix_spans_raw(prefix));
            spans.push(Span::styled(chunk, style));
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{COMMIT_NODE, MERGE_NODE};
    use crate::model::{GitRef, RefKind};
    use crate::ui::detail::tests::busy_repo;

    fn git_ref(kind: RefKind, name: &str, is_head: bool) -> GitRef {
        GitRef {
            kind,
            name: name.to_string(),
            is_head,
        }
    }

    fn span_text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// `*` on every row is a heavy glyph that turns a long history into
    /// noise. Ordinary commits get a light node; `*` is kept for merges,
    /// which is the one row worth picking out of a column.
    /// git draws every commit as `*` and its lanes as punctuation. Both are
    /// translated: the lanes to box drawing, so the column reads as a drawn
    /// line, and the node to a circle -- with merges keeping the asterisk,
    /// circled, so they stay pickable out of the column.
    #[test]
    fn the_graph_is_drawn_with_box_characters_not_ascii() {
        // A merge and a collapse, so every shape the column can draw appears.
        let spec: [(&str, &[&str]); 4] = [
            ("aaaaaaa", &["bbbbbbb", "ccccccc"]),
            ("ccccccc", &["ddddddd"]),
            ("bbbbbbb", &["ddddddd"]),
            ("ddddddd", &[]),
        ];
        let parents: Vec<Vec<String>> = spec
            .iter()
            .map(|(_, p)| p.iter().map(|s| (*s).to_string()).collect())
            .collect();
        let commits: Vec<crate::graph::Commit<'_>> = spec
            .iter()
            .zip(&parents)
            .map(|((hash, _), parents)| crate::graph::Commit { hash, parents })
            .collect();

        let mut r = busy_repo();
        r.graph = crate::graph::layout(&commits)
            .into_iter()
            .zip(spec)
            .map(|(lanes, (hash, parents))| crate::model::GraphLine {
                lanes,
                hash: hash.to_string(),
                is_merge: parents.len() > 1,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: "subject".to_string(),
            })
            .collect();
        let mut selected_row = None;
        let text: String = graph_list_lines(&r, 96, 0, None, &mut selected_row)
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        for drawn in [COMMIT_NODE, MERGE_NODE, '│', '─', '╮', '╯'] {
            assert!(text.contains(drawn), "{drawn} missing from:\n{text}");
        }
        for ascii in ['*', '|', '\\', '_'] {
            assert!(!text.contains(ascii), "git's own {ascii} leaked through");
        }
    }

    #[test]
    fn a_zero_count_draws_no_token_at_all() {
        assert_eq!(stat_part_width(0), 0);
        assert_eq!(stat_part_width(9), 2);
        assert_eq!(stat_part_width(338), 4);
    }

    /// The separator is only spent when both columns exist, so a list with
    /// nothing but insertions does not reserve a blank for deletions.
    #[test]
    fn the_churn_tail_spends_a_separator_only_between_two_columns() {
        assert_eq!(stat_tail_width(0, 0), 0);
        assert_eq!(stat_tail_width(4, 0), 4);
        assert_eq!(stat_tail_width(0, 3), 3);
        assert_eq!(stat_tail_width(4, 3), 8);
    }

    /// A row must not spill past the pane. The refs and the subject can each
    /// eat the width on their own, so the dot fill has to be allowed to
    /// vanish rather than being forced to one -- that single dot was enough
    /// to push the diffstat onto a line of its own.
    #[test]
    fn no_commit_row_overflows_the_pane() {
        let mut r = busy_repo();
        r.commits = vec![crate::model::Commit {
            hash: "a1b2c3d".to_string(),
            author: "Gaurgle".to_string(),
            date: Some(chrono::Utc::now()),
            message: "x".repeat(200),
            body: String::new(),
            insertions: 4321,
            deletions: 8765,
            files: vec![],
        }];
        r.graph = vec![crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: "a1b2c3d".to_string(),
            is_merge: false,
            refs: vec![
                crate::model::GitRef {
                    kind: crate::model::RefKind::Local,
                    name: "a-rather-long-branch-name".to_string(),
                    is_head: true,
                },
                crate::model::GitRef {
                    kind: crate::model::RefKind::Remote,
                    name: "origin/a-rather-long-branch-name".to_string(),
                    is_head: false,
                },
            ],
            author: "Gaurgle".to_string(),
            message: "x".repeat(200),
        }];

        for width in [60usize, 80, 100, 140] {
            let mut row_marker = None;
            let lines = graph_list_lines(&r, width, 0, None, &mut row_marker);
            for line in &lines {
                let drawn: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
                assert!(
                    drawn <= width,
                    "row is {drawn} wide in a {width} pane: {:?}",
                    line.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                );
            }
        }
    }

    /// The expanded view reproduces `git log --graph`, so its node stays `*`.
    /// That format spells merges out on their own `Merge:` line, and swapping
    /// the glyph would only stop it looking like the thing it is copying.
    #[test]
    fn the_expanded_graph_keeps_gits_own_node() {
        let text: String = expanded_graph_lines("* commit a1b2c3d", "", 80)
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(text.contains("* commit"), "got {text:?}");
        assert!(!text.contains(COMMIT_NODE), "no substitution here");

        // The compact list is the opposite: it computes its own topology and
        // draws it in box characters. See
        // `the_graph_is_drawn_with_box_characters_not_ascii`.
    }

    /// A node is a commit, not a branch. Colouring it by lane made the node
    /// column change hue whenever a lane was reused, which is a signal that
    /// means nothing -- the lanes either side already say what the topology
    /// is.
    #[test]
    fn graph_nodes_are_plain_text_not_lane_coloured() {
        let spec: [(&str, &[&str]); 4] = [
            ("aaaaaaa", &["bbbbbbb", "ccccccc"]),
            ("ccccccc", &["ddddddd"]),
            ("bbbbbbb", &["ddddddd"]),
            ("ddddddd", &[]),
        ];
        let parents: Vec<Vec<String>> = spec
            .iter()
            .map(|(_, p)| p.iter().map(|s| (*s).to_string()).collect())
            .collect();
        let commits: Vec<crate::graph::Commit<'_>> = spec
            .iter()
            .zip(&parents)
            .map(|((hash, _), parents)| crate::graph::Commit { hash, parents })
            .collect();

        let rows = crate::graph::layout(&commits);
        let mut nodes = 0usize;
        let mut lanes = 0usize;
        for cells in &rows {
            for span in lane_spans(cells) {
                let glyph = span.content.chars().next().unwrap_or(' ');
                if glyph == crate::graph::COMMIT_NODE || glyph == crate::graph::MERGE_NODE {
                    nodes += 1;
                    assert_eq!(span.style.fg, Some(colors::TEXT), "a node is lane-coloured");
                } else if glyph == '\u{2502}' {
                    lanes += 1;
                    assert_ne!(span.style.fg, Some(colors::TEXT), "a lane went plain");
                }
            }
        }
        assert!(nodes >= 4, "fixture drew no nodes");
        assert!(lanes >= 1, "fixture drew no lanes to contrast against");
    }

    /// `refs_width` budgets the message column and `ref_spans` draws the
    /// refs. Two pieces of arithmetic over the same thing: when they drift,
    /// the message is truncated to the wrong width and nothing fails.
    #[test]
    fn the_measured_ref_width_matches_what_gets_drawn() {
        let cases: Vec<Vec<GitRef>> = vec![
            vec![],
            vec![git_ref(RefKind::Local, "main", true)],
            vec![git_ref(RefKind::Head, "HEAD", false)],
            vec![
                git_ref(RefKind::Local, "main", true),
                git_ref(RefKind::Remote, "origin/main", false),
                git_ref(RefKind::Tag, "v1.0", false),
            ],
        ];
        for refs in cases {
            let line = crate::model::GraphLine {
                lanes: vec![crate::graph::commit_cell()],
                hash: "a1b2c3d".to_string(),
                is_merge: false,
                refs: refs.clone(),
                author: "Alice".to_string(),
                message: "feat: thing".to_string(),
            };
            let drawn = span_text(&ref_spans(&refs));
            // refs_width excludes the leading space; ref_spans emits it.
            let measured = if refs.is_empty() {
                0
            } else {
                line.refs_width() + 1
            };
            assert_eq!(drawn.chars().count(), measured, "width drift on {drawn:?}");
        }
    }

    #[test]
    fn an_attached_head_draws_the_arrow_and_a_detached_one_does_not() {
        let attached = span_text(&ref_spans(&[
            git_ref(RefKind::Local, "main", true),
            git_ref(RefKind::Remote, "origin/main", false),
        ]));
        assert_eq!(attached, " (HEAD -> main, origin/main)");

        // Detached HEAD sitting on a tag: git separates these with a comma,
        // not an arrow. HEAD does not point at the tag.
        let detached = span_text(&ref_spans(&[
            git_ref(RefKind::Head, "HEAD", false),
            git_ref(RefKind::Tag, "v1.0", false),
        ]));
        assert_eq!(detached, " (HEAD, v1.0)");
    }

    /// Regression class this pane has been bitten by once already: a row that
    /// budgets its width against something other than what it draws wraps, and
    /// a wrapped row silently shifts every row beneath it.
    ///
    /// The graph column is where that can happen now, because it is padded to a
    /// cap rather than to the widest row. A history deep enough to blow past
    /// the cap has to give up subject width, not spill.
    #[test]
    fn a_history_deeper_than_the_cap_still_fits_the_pane() {
        // Every commit a merge whose second parent is never listed, so lanes
        // pile up and none is ever reclaimed.
        let hashes: Vec<String> = (0..24).map(|i| format!("h{i:06}")).collect();
        let parents: Vec<Vec<String>> = (0..24)
            .map(|i| {
                let mut p: Vec<String> = hashes.get(i + 1).cloned().into_iter().collect();
                p.push(format!("never{i:03}"));
                p
            })
            .collect();
        let commits: Vec<crate::graph::Commit<'_>> = hashes
            .iter()
            .zip(&parents)
            .map(|(hash, parents)| crate::graph::Commit { hash, parents })
            .collect();

        let mut r = busy_repo();
        r.graph = crate::graph::layout(&commits)
            .into_iter()
            .zip(&hashes)
            .map(|(lanes, hash)| crate::model::GraphLine {
                lanes,
                hash: hash.clone(),
                is_merge: true,
                refs: vec![],
                author: "Gaurgle".to_string(),
                message: "a subject long enough to want every column it can get".to_string(),
            })
            .collect();

        let deepest = r.graph.last().map(|g| g.lanes.len()).unwrap_or(0);
        assert!(deepest > 12, "fixture is not deep enough: {deepest} lanes");

        for width in [72, 100, 140] {
            let mut selected_row = None;
            for line in graph_list_lines(&r, width, 0, None, &mut selected_row) {
                assert!(
                    line.width() <= width,
                    "a {}-lane row ran {} past a {width}-column pane: {:?}",
                    deepest,
                    line.width() - width,
                    line.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                );
            }
        }
    }

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

    /// git writes two columns per lane (glyph and horizontal run), so the
    /// palette must be indexed by lane not by column. If this reverts to raw
    /// column indices, adjacent lanes land two palette entries apart and six
    /// lanes only use three of six colours. This guards against that regression.
    #[test]
    fn pipes_are_coloured_by_lane_not_by_column() {
        let spans = graph_prefix_spans_raw("| | | | ");
        let colour_of = |i: usize| spans[i].style.fg.expect("every span is styled");

        // Columns 0, 2, 4, 6 are lanes 0, 1, 2, 3 respectively. Each should
        // get a different palette entry if indexed by lane (i / 2), or land on
        // the same entry if mistakenly indexed by raw column (i).
        assert_eq!(
            colour_of(0),
            colors::graph_lane_color(0),
            "column 0 is lane 0"
        );
        assert_eq!(
            colour_of(2),
            colors::graph_lane_color(1),
            "column 2 is lane 1"
        );
        assert_eq!(
            colour_of(4),
            colors::graph_lane_color(2),
            "column 4 is lane 2"
        );
        assert_eq!(
            colour_of(6),
            colors::graph_lane_color(3),
            "column 6 is lane 3"
        );

        // Adjacent lanes must use different palette entries.
        assert_ne!(
            colour_of(0),
            colour_of(2),
            "lanes 0 and 1 must be distinguishable"
        );
    }

    /// The graph view truncated every line to the pane width, so a long
    /// subject or a long decoration list was simply unreadable. Wrapping
    /// happens in the builder rather than in the `Paragraph` so one source
    /// line stays one rendered row, which the scroll arithmetic depends on.
    #[test]
    fn a_long_graph_line_wraps_instead_of_truncating() {
        let raw = "|     feat: rotate the contribution window's from date past GitHub's cache";
        let rows = expanded_graph_lines(raw, "", 40);
        assert!(rows.len() > 1, "the line was truncated, not wrapped");
        for row in &rows {
            let w: usize = row.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(w <= 40, "a wrapped row is {w} wide, past the pane's 40");
        }
    }

    /// Every row carries the lane prefix. A gap in the pipes reads as a
    /// topology change that did not happen.
    #[test]
    fn wrapped_rows_repeat_the_lane_prefix() {
        let raw = "| | |     a subject long enough to need more than one row here";
        let rows = expanded_graph_lines(raw, "", 30);
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
        let wide = expanded_graph_lines(raw, "", 60).len();
        let narrow = expanded_graph_lines(raw, "", 30).len();
        assert!(
            narrow > wide,
            "wrapping did not follow the width: {narrow} vs {wide}"
        );
    }

    /// The staleness bug: git rendered "3 minutes ago" once, the expanded
    /// graph was only refetched when the repo moved, and an idle repo kept
    /// showing it for hours while the commit list beside it said "16h ago".
    #[test]
    fn the_graph_block_date_is_aged_at_draw_time() {
        let then = chrono::Utc::now() - chrono::Duration::hours(16);
        let line = format!("Date:   {}", then.to_rfc3339());
        assert_eq!(
            refresh_date_line(&line).as_deref(),
            Some("Date:   16h ago"),
            "the frozen relative date survived to the screen"
        );
    }

    /// Only the `Date:` row is rewritten. Everything else is git's output and
    /// is reproduced verbatim, which is the whole point of this view.
    #[test]
    fn other_graph_rows_are_left_exactly_as_git_wrote_them() {
        assert_eq!(refresh_date_line("Author: someone <a@b.c>"), None);
        assert_eq!(refresh_date_line("commit 1c51c6e"), None);
        assert_eq!(refresh_date_line("    a subject line"), None);
        assert_eq!(
            refresh_date_line("Date:   Mon Aug 25 09:12:33 2026 +0200"),
            None,
            "an unparseable stamp must pass through, not vanish"
        );
    }

    /// End to end: the rewritten row reaches the drawn line.
    #[test]
    fn the_drawn_graph_block_shows_a_current_age() {
        let then = chrono::Utc::now() - chrono::Duration::days(3);
        let raw = format!("| | Date:   {}", then.to_rfc3339());
        let rows = expanded_graph_lines(&raw, "", 80);
        let text: String = rows[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(text.contains("3d ago"), "drew {text:?}");
    }

    /// Phantom blocks: a substring test for "commit " matched message bodies,
    /// so this repo's own log reported thirty block starts for twenty-five
    /// commits and the marker indexed past the commit it was on.
    #[test]
    fn only_gits_commit_row_opens_a_block() {
        assert_eq!(commit_hash_at("* commit 7b1f067"), Some("7b1f067"));
        assert_eq!(
            commit_hash_at("| | commit 7b1f067 (HEAD -> main, origin/main)"),
            Some("7b1f067")
        );
        assert_eq!(
            commit_hash_at("|     notch jumped a whole commit block, which is"),
            None,
            "a message body opened a phantom block"
        );
        assert_eq!(
            commit_hash_at("|     is the same symptom. That commit had the mechanism"),
            None
        );
        assert_eq!(
            commit_hash_at("|     commit blocks are tall"),
            None,
            "a body starting with the word is still not a commit row"
        );
        assert_eq!(commit_hash_at("| Author: Gaurgle <a@b.c>"), None);
    }
}
