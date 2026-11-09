use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use super::changes::render_changes_inspector;
use super::layout::INSPECTOR_TWO_LANE_WIDTH;
use super::worktrees::render_worktree_inspector;
use super::{commit_age, more_line, section_label, span_width, tab_switcher, wrap_line};
use crate::app::{App, FocusedPanel, HistoryView};
use crate::ui::colors;

/// Most changed files listed in the inspector before the rest are collapsed
/// into a "… and N more" line -- keeps a huge commit from turning the pane
/// into an endless scroll.
const INSPECTOR_FILE_CAP: usize = 30;

/// Render the `[3] inspector` pane: the full details of the commit the detail
/// pane's cursor is on -- a two-line masthead (hash, then author · date), the
/// full message (subject bold, body untruncated), and the per-file diffstat --
/// so you can read a commit in full without leaving the dashboard. The block's
/// padding insets every line uniformly, so wrapped prose keeps its left edge
/// instead of falling back to column zero.
/// What the inspector says when it has a selection it cannot draw.
///
/// Three different facts, and none of them may be told as silence: nothing is
/// selected, the commit is real but was not loaded, or the commit is real and
/// simply is not reachable from here.
///
/// The reflog reaches back further than the commit list does, so an old entry
/// names a commit that was not loaded -- a range problem. A remote-only branch
/// tip is a different thing: `git::commits` and `git::graph` walk the union of
/// the *local* branches, so that commit is not outside the range, it is off
/// the refs fleetz reads at all. Saying "range" there would send someone
/// looking for a limit to raise.
fn inspector_empty_text(app: &App) -> String {
    let Some(hash) = app.inspected_hash() else {
        return "no commit selected".to_string();
    };
    match app.history_view {
        HistoryView::Branches => format!("{hash} is not on any local branch"),
        _ => format!("{hash} is outside the loaded commit range"),
    }
}

pub(super) fn render_inspector(frame: &mut Frame, area: Rect, app: &App) {
    if app.inspector_folded {
        crate::ui::render_folded(
            frame,
            area,
            crate::ui::INSPECTOR_NUM,
            "inspector",
            app.focused_panel == FocusedPanel::Inspector,
        );
        return;
    }
    // A digest line's detail (FZ-45), ahead of the `ci` view's run, which
    // the digest hides.
    if let Some(digest) = app.digest(chrono::Local::now()) {
        crate::ui::digest::render_inspector(frame, area, app, &digest);
        return;
    }
    if app.inspects_ci_run() {
        render_ci_inspector(frame, area, app);
        return;
    }
    if app.worktrees_wanted() {
        render_worktree_inspector(frame, area, app);
        return;
    }
    if app.changes_wanted() {
        render_changes_inspector(frame, area, app);
        return;
    }
    let focused = app.focused_panel == FocusedPanel::Inspector;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(2, 2, 1, 0))
        .title_top(crate::ui::panel_title(
            crate::ui::INSPECTOR_NUM,
            "inspector",
            focused,
        ));
    let inner = block.inner(area);

    let Some(c) = app.inspected_commit() else {
        // Distinguish "nothing selected" from "selected something this pane
        // cannot show". The reflog reaches back further than the commit list
        // does, so picking an old entry names a real commit that simply was
        // not loaded -- saying "no commit selected" there would be a lie.
        let text = inspector_empty_text(app);
        let empty = Paragraph::new(text)
            .style(Style::default().fg(colors::OVERLAY))
            .block(block);
        frame.render_widget(empty, area);
        return;
    };

    // Masthead: hash on its own line (bold, the pane's anchor), then author and
    // relative date beneath -- two calm lines rather than one crowded row --
    // a blank, then the subject (bold, the heading). Body and files follow.
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            c.hash.clone(),
            Style::default()
                .fg(colors::HASH_CLR)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                c.author.clone(),
                Style::default().fg(colors::author_color(&c.author)),
            ),
            Span::styled(" · ", Style::default().fg(colors::SURFACE)),
            Span::styled(commit_age(c), Style::default().fg(colors::OVERLAY)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            c.message.clone(),
            Style::default()
                .fg(colors::TEXT)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    // Body below the subject, untruncated; wraps within the block padding.
    if !c.body.is_empty() {
        lines.push(Line::from(""));
        for body_line in c.body.lines() {
            lines.push(Line::from(Span::styled(
                body_line.to_string(),
                Style::default().fg(colors::TEXT),
            )));
        }
    }

    // Changed files, with the commit's totals on the section header. Capped so
    // a sprawling commit can't wall off the pane -- the rest collapse into a
    // trailing "… and N more".
    let mut file_lines: Vec<Line> = Vec::new();
    if !c.files.is_empty() {
        let mut header = vec![section_label(&format!("changed files ({})", c.files.len()))];
        if c.insertions > 0 {
            header.push(Span::styled(
                format!("   +{}", c.insertions),
                Style::default().fg(colors::DIFF_ADD),
            ));
        }
        if c.deletions > 0 {
            header.push(Span::styled(
                format!(" -{}", c.deletions),
                Style::default().fg(colors::DIFF_DEL),
            ));
        }
        file_lines.push(Line::from(header));

        for f in c.files.iter().take(INSPECTOR_FILE_CAP) {
            file_lines.push(file_stat_line(f));
        }
        if c.files.len() > INSPECTOR_FILE_CAP {
            file_lines.push(more_line(c.files.len() - INSPECTOR_FILE_CAP));
        }
    }

    // Wide enough, the files get a lane of their own beside the message, so
    // a long body no longer pushes them below the fold and a long file list
    // no longer buries the prose. Narrower, they stack as one column.
    let lanes: Vec<(Rect, Vec<Line>)> =
        if inner.width >= INSPECTOR_TWO_LANE_WIDTH && !file_lines.is_empty() {
            let h = Layout::horizontal([Constraint::Percentage(50), Constraint::Min(0)])
                .spacing(2)
                .split(inner);
            vec![(h[0], lines), (h[1], file_lines)]
        } else {
            if !file_lines.is_empty() {
                lines.push(Line::from(""));
                lines.extend(file_lines);
            }
            vec![(inner, lines)]
        };

    // Clamp against the *wrapped* row count, not the raw line count -- long
    // prose wraps to several rows, so a raw count would stop scrolling short of
    // the real end.
    // Recorded back so `App::scroll_inspector` clamps against it, rather than
    // letting the offset run past the end and stall the way back.
    // Wrapped here rather than by the `Paragraph`, so one line is one rendered
    // row and the offset below is an exact row index. See `wrap_line`.
    //
    // One offset scrolls every lane, clamped to the tallest, so the end of
    // either lane is always reachable.
    let wrapped: Vec<(Rect, Vec<Line>)> = lanes
        .into_iter()
        .map(|(rect, lines)| {
            let lines = lines
                .iter()
                .flat_map(|l| wrap_line(l, rect.width as usize))
                .collect();
            (rect, lines)
        })
        .collect();
    let max_scroll = wrapped
        .iter()
        .map(|(rect, lines)| (lines.len() as u16).saturating_sub(rect.height))
        .max()
        .unwrap_or(0);
    app.inspector_max_scroll.set(max_scroll);
    let scroll = app.inspector_scroll.min(max_scroll);
    frame.render_widget(block, area);
    for (rect, lines) in wrapped {
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), rect);
    }
}

/// The inspector's body tabs on a run (FZ-64), `[/] JOBS · LOG · SCRIPT`,
/// right of a title that no longer names the body. The shown one wears the
/// `ci` tab's colour, the view whose run it details.
fn ci_body_switcher(app: &App, focused: bool, budget: usize) -> Line<'static> {
    let labels: Vec<String> = crate::app::CiBody::ALL
        .iter()
        .map(|b| b.label().to_uppercase())
        .collect();
    tab_switcher(
        &labels,
        app.ci_body.index(),
        colors::history_view_color(HistoryView::Ci),
        focused,
        budget,
    )
}

/// The inspector while pane 2 shows `ci` (FZ-30f): the highlighted run in
/// the shown body (`ui::ci::inspector_body`), wrapped here so one line is
/// one row, and scrolled by that body's own offset, clamped to what this
/// frame measured.
fn render_ci_inspector(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focused_panel == FocusedPanel::Inspector;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let title = crate::ui::panel_title(crate::ui::INSPECTOR_NUM, "inspector", focused);
    // Two corners and a column of border either side, as pane 2 spends.
    let mut budget = (area.width as usize)
        .saturating_sub(2)
        .saturating_sub(span_width(&title.spans) + 2);
    // The resize grip is painted over the middle of this border after the
    // panes, so the tabs keep to the cells right of it, a column of rule
    // between, short of the corner.
    if let Some((_, grip_right)) = crate::ui::grips::inspector_grip_span(area) {
        let corner = (area.x + area.width).saturating_sub(1);
        budget = budget.min(corner.saturating_sub(grip_right + 1) as usize);
    }
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(2, 2, 1, 0))
        .title_top(title);
    // Only a highlighted run has bodies to step (`App::press_bracket`'s own
    // condition); loading, empty or offline, the brackets step pane 2.
    if app.ci_highlighted_run().is_some() {
        block = block.title_top(ci_body_switcher(app, focused, budget));
    }
    let inner = block.inner(area);
    let (body, marked) = crate::ui::ci::inspector_lines(app, chrono::Local::now());
    // Each body line's first wrapped row, and one past the last, so the
    // marked job's lines map to the rows they take at this width.
    let mut row_starts = Vec::with_capacity(body.len() + 1);
    let mut head: Vec<Line> = Vec::new();
    for line in &body {
        row_starts.push(head.len());
        head.extend(wrap_line(line, inner.width as usize));
    }
    row_starts.push(head.len());
    // A held whole log is wrapped once per job and width and kept, and only
    // the rows on screen are handed to the paragraph: up to 5000 lines
    // (`ci::LOG_LINE_CAP`) were re-wrapped and copied on every frame.
    let mut cache = app.ci_log_wrap.borrow_mut();
    let log: &[Line<'static>] = match crate::ui::ci::shown_log(app) {
        Some((job_id, log)) => {
            let fresh = cache
                .as_ref()
                .is_some_and(|w| w.job_id == job_id && w.width == inner.width);
            if !fresh {
                let lines = log
                    .iter()
                    .flat_map(|l| wrap_line(&crate::ui::ci::log_line(l), inner.width as usize))
                    .collect();
                *cache = Some(crate::app::CiLogWrap {
                    job_id,
                    width: inner.width,
                    lines,
                });
            }
            cache.as_ref().map_or(&[][..], |w| &w.lines[..])
        }
        None => &[],
    };
    let total = head.len() + log.len();
    let max_scroll = u16::try_from(total)
        .unwrap_or(u16::MAX)
        .saturating_sub(inner.height);
    app.inspector_max_scroll.set(max_scroll);
    if let Some(lines) = marked
        && app.ci_job_follow.get() == crate::app::JobFollow::Due
    {
        let rows = row_starts[lines.start]..row_starts[lines.end];
        let followed = follow_rows(app.ci_body_scroll(), rows, inner.height).min(max_scroll);
        app.ci_job_follow.set(crate::app::JobFollow::At(followed));
    }
    let scroll = app.ci_body_scroll().min(max_scroll);
    let visible: Vec<Line> = head
        .iter()
        .chain(log.iter())
        .skip(scroll as usize)
        .take(inner.height as usize)
        .cloned()
        .collect();
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(visible), inner);
}

/// The least scroll from `scroll` that shows `rows` in a pane `height` rows
/// tall, or its first row when they are taller than the pane.
fn follow_rows(scroll: u16, rows: std::ops::Range<usize>, height: u16) -> u16 {
    let top = u16::try_from(rows.start).unwrap_or(u16::MAX);
    let bottom = u16::try_from(rows.end).unwrap_or(u16::MAX);
    if top < scroll {
        top
    } else if bottom > scroll.saturating_add(height) {
        bottom.saturating_sub(height).min(top)
    } else {
        scroll
    }
}

/// One "changed file" row for the inspector: a fixed-width +A/-B stat column
/// (green adds, red deletes) then the path, so paths line up in a column no
/// matter how big each file's diff is. Binary files (0/0) show only the path.
fn file_stat_line(f: &crate::model::FileStat) -> Line<'static> {
    const STAT_W: usize = 9;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut stat_len = 0;
    if f.added > 0 {
        let s = format!("+{}", f.added);
        stat_len += s.chars().count();
        spans.push(Span::styled(s, Style::default().fg(colors::DIFF_ADD)));
    }
    if f.removed > 0 {
        if stat_len > 0 {
            spans.push(Span::raw(" "));
            stat_len += 1;
        }
        let s = format!("-{}", f.removed);
        stat_len += s.chars().count();
        spans.push(Span::styled(s, Style::default().fg(colors::DIFF_DEL)));
    }
    // Pad the stat column so every path starts at the same column (+1 so even
    // an over-long stat keeps a gap before the path).
    spans.push(Span::raw(" ".repeat(STAT_W.saturating_sub(stat_len) + 1)));
    spans.push(Span::styled(
        f.path.clone(),
        Style::default().fg(colors::TEXT),
    ));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::detail::tests::{branch_fixture, busy_repo, changes_view_app, held_changes};

    /// The failure this closes: a remote-only branch's tip is not in the union
    /// of local branches that `git::commits` walks, so the inspector had
    /// nothing to draw. Reporting it as a *range* problem would send someone
    /// looking for a limit to raise; it is a reachability problem.
    #[test]
    fn an_unreachable_branch_tip_says_it_is_not_on_a_local_branch() {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
        app.history_view = HistoryView::Branches;
        app.repos[0].branches = vec![branch_fixture("gone", None, 0, false)];
        app.repos[0].branches[0].tip = "beefbee".to_string();
        app.set_detail_selected(0);
        assert_eq!(
            inspector_empty_text(&app),
            "beefbee is not on any local branch"
        );
    }

    /// The reflog keeps its own wording: there the commit really is outside a
    /// range that could be widened.
    #[test]
    fn an_unloaded_reflog_commit_still_reports_a_range() {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
        app.history_view = HistoryView::Reflog;
        app.repos[0].reflog = vec![crate::model::ReflogEntry {
            hash: "beefbee".to_string(),
            action: "commit".to_string(),
            description: "x".to_string(),
        }];
        app.set_detail_selected(0);
        assert_eq!(
            inspector_empty_text(&app),
            "beefbee is outside the loaded commit range"
        );
    }

    /// The inspector draws the highlighted file's diff with added and
    /// removed lines in the churn column's green and red, a failed read's
    /// error in place, and scrolls with j/k when focused.
    #[test]
    fn the_changes_inspector_draws_the_diff_in_churn_colors_and_scrolls() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let draw = |app: &crate::app::App| {
            let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
            term.draw(|f| render_inspector(f, f.area(), app)).unwrap();
            term.backend().buffer().clone()
        };
        let row_of = |buf: &ratatui::buffer::Buffer, needle: &str| {
            (0..12u16).find_map(|y| {
                let row: String = (0..80u16).map(|x| buf[(x, y)].symbol()).collect();
                row.find(needle)
                    .map(|x| (row[..x].chars().count() as u16, y))
            })
        };
        let mut app = changes_view_app(held_changes());
        let read = app.claim_change_diff().expect("the highlighted file");
        let diff: String = "@@ -1 +1 @@\n-old line\n+new line\n"
            .chars()
            .chain((0..30).flat_map(|i| format!(" context {i}\n").chars().collect::<Vec<_>>()))
            .collect();
        app.apply_change_diff(read.clone(), Ok(diff));
        let buf = draw(&app);
        let (x, y) = row_of(&buf, "+new line").expect("added line drawn");
        assert_eq!(buf[(x, y)].fg, colors::DIFF_ADD);
        let (x, y) = row_of(&buf, "-old line").expect("removed line drawn");
        assert_eq!(buf[(x, y)].fg, colors::DIFF_DEL);

        app.focused_panel = FocusedPanel::Inspector;
        app.inspector_nav(5);
        let buf = draw(&app);
        assert!(row_of(&buf, "+new line").is_none(), "scrolled past it");

        app.change_diff = None;
        let read = app.claim_change_diff().expect("again");
        app.apply_change_diff(read, Err("boom".to_string()));
        let buf = draw(&app);
        assert!(row_of(&buf, "could not read: boom").is_some());
    }
}
