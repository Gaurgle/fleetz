use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use super::commits::{stat_part_width, stat_tail_width};
use super::{pad_to, section_label, wash};
use crate::app::{App, FocusedPanel, HistoryView};
use crate::model::RepoStatus;
use crate::ui::colors;

/// The `changes` view's two bases, in the mode line's and the help row's
/// words (FZ-60).
const CHANGES_BASES: [&str; 2] = ["upstream", "default"];

/// The `changes` view's first line: both bases as a two-state switch, drawn
/// as the `notes` mode line draws its modes: `vs: [upstream] default  (u)`.
pub(super) fn changes_mode_line(app: &App) -> Line<'static> {
    let on = colors::history_view_color(HistoryView::Changes);
    let active = usize::from(app.changes_base == crate::git::ChangesBase::DefaultBranch);
    let mut spans = vec![Span::styled("vs: ", Style::default().fg(colors::OVERLAY))];
    for (i, base) in CHANGES_BASES.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(match i == active {
            true => Span::styled(
                format!("[{base}]"),
                Style::default().fg(on).add_modifier(Modifier::BOLD),
            ),
            false => Span::styled(base.to_string(), Style::default().fg(colors::SUBTEXT)),
        });
    }
    spans.push(Span::styled("  (", Style::default().fg(colors::OVERLAY)));
    spans.push(Span::styled(
        "u",
        Style::default()
            .fg(colors::TEXT)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(")", Style::default().fg(colors::OVERLAY)));
    Line::from(spans)
}

/// A file's status columns: porcelain's staged column in green and its
/// unstaged one in the dirty counter's yellow, `??` dim; the other
/// section's one letter in the churn colors when it adds or deletes.
fn change_status_spans(status: &str) -> Vec<Span<'static>> {
    let styled = |text: String, color| Span::styled(text, Style::default().fg(color));
    if status == "??" {
        return vec![styled("??".to_string(), colors::OVERLAY)];
    }
    let mut chars = status.chars();
    let (x, y) = (chars.next().unwrap_or(' '), chars.next());
    match y {
        Some(y) => vec![
            styled(x.to_string(), colors::GREEN),
            styled(y.to_string(), colors::YELLOW),
        ],
        None => {
            let color = match x {
                'A' => colors::DIFF_ADD,
                'D' => colors::DIFF_DEL,
                _ => colors::TEXT,
            };
            vec![styled(format!("{x} "), color)]
        }
    }
}

/// The churn column of one changed file, in the widths its section uses.
fn churn_spans(churn: Option<(u32, u32)>, add_w: usize, del_w: usize) -> Vec<Span<'static>> {
    let (added, removed) = churn.unwrap_or((0, 0));
    let token = |n: u32, sign: char, w: usize| match n {
        0 => " ".repeat(w),
        n => format!("{:>w$}", format!("{sign}{n}")),
    };
    let mut spans = Vec::new();
    if add_w > 0 {
        spans.push(Span::styled(
            token(added, '+', add_w),
            Style::default().fg(colors::DIFF_ADD),
        ));
    }
    if del_w > 0 {
        if add_w > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            token(removed, '-', del_w),
            Style::default().fg(colors::DIFF_DEL),
        ));
    }
    spans
}

/// The `changes` view's lists for `repo` (FZ-60): its lines, the line each
/// file is on, in cursor order, and the line the cursor's file is on. A row
/// with nothing to list says why in one line.
pub(super) fn changes_rows(
    app: &App,
    repo: &RepoStatus,
    width: usize,
) -> (Vec<Line<'static>>, Vec<usize>, Option<usize>) {
    use crate::git::Outgoing;
    let dim = |text: String| Line::from(Span::styled(text, Style::default().fg(colors::OVERLAY)));
    let say = |text: String| (vec![dim(text)], Vec::new(), None);
    if repo.path.is_none() {
        return say("not cloned: nothing to read".to_string());
    }
    let changes = match app.selected_changes().and_then(|(_, s)| s?.held.as_ref()) {
        None => return say("reading changes…".to_string()),
        Some(Err(e)) => return say(format!("could not read the repo: {e}")),
        Some(Ok(changes)) => changes,
    };
    if let Outgoing::Unpushed {
        upstream, ahead: 0, ..
    } = &changes.outgoing
        && changes.uncommitted.is_empty()
    {
        return say(format!("clean and pushed to {upstream}"));
    }
    let total = changes.uncommitted.len() + changes.outgoing.files().len();
    let cursor = app.detail_selected().min(total.saturating_sub(1));
    let mut lines = Vec::new();
    let mut tops = Vec::with_capacity(total);
    let mut selected = None;
    let mut list = |lines: &mut Vec<Line<'static>>, files: &[crate::git::ChangedFile]| {
        let add_w = files
            .iter()
            .map(|f| stat_part_width(f.churn.map_or(0, |c| c.0)))
            .max()
            .unwrap_or(0);
        let del_w = files
            .iter()
            .map(|f| stat_part_width(f.churn.map_or(0, |c| c.1)))
            .max()
            .unwrap_or(0);
        let tail = stat_tail_width(add_w, del_w);
        let path_w = width.saturating_sub(2 + 2 + 2 + tail + usize::from(tail > 0) * 2);
        for file in files {
            let is_sel = tops.len() == cursor;
            if is_sel {
                selected = Some(lines.len());
            }
            tops.push(lines.len());
            let path = crate::ui::truncate(&file.path, path_w);
            let pad = path_w.saturating_sub(path.chars().count());
            let mut spans = vec![Span::styled(
                if is_sel { "▸ " } else { "  " },
                Style::default().fg(colors::SAPPHIRE),
            )];
            spans.extend(change_status_spans(&file.status));
            spans.push(Span::raw("  "));
            spans.push(Span::styled(path, Style::default().fg(colors::TEXT)));
            if tail > 0 {
                spans.push(Span::raw(" ".repeat(pad + 2)));
                spans.extend(churn_spans(file.churn, add_w, del_w));
            }
            if is_sel {
                spans = wash(pad_to(spans, width));
            }
            lines.push(Line::from(spans));
        }
    };
    match changes.uncommitted.len() {
        0 => lines.push(dim("nothing uncommitted".to_string())),
        n => {
            lines.push(Line::from(section_label(&format!("uncommitted ({n})"))));
            list(&mut lines, &changes.uncommitted);
        }
    }
    lines.push(Line::from(""));
    match &changes.outgoing {
        Outgoing::Unpushed {
            upstream, ahead: 0, ..
        } => lines.push(dim(format!("nothing unpushed to {upstream}"))),
        Outgoing::Unpushed {
            upstream,
            ahead,
            files,
        } => {
            let heading = format!("unpushed ({ahead} ahead of {upstream})");
            lines.push(Line::from(section_label(&heading)));
            list(&mut lines, files);
        }
        Outgoing::VsDefault { base, files } => {
            let heading = format!("on branch vs {base} ({})", files.len());
            lines.push(Line::from(section_label(&heading)));
            list(&mut lines, files);
        }
        Outgoing::NoUpstream { branch: Some(b) } => {
            lines.push(dim(format!("no upstream: {b} tracks no remote branch")))
        }
        Outgoing::NoUpstream { branch: None } => {
            lines.push(dim("no upstream: HEAD is detached".to_string()))
        }
        Outgoing::NoDefault => lines.push(dim("no default branch to compare with".to_string())),
    }
    (lines, tops, selected)
}

/// One line of a changed file's diff, in the churn column's green and red;
/// hunk heads in the section labels' mauve, file headers dim. Tabs widened
/// so the terminal draws them.
fn diff_line(line: &str) -> Line<'static> {
    let color = if line.starts_with("+++") || line.starts_with("---") {
        colors::OVERLAY
    } else if line.starts_with('+') {
        colors::DIFF_ADD
    } else if line.starts_with('-') {
        colors::DIFF_DEL
    } else if line.starts_with("@@") {
        colors::MAUVE
    } else if line.starts_with(' ') || line.is_empty() {
        colors::TEXT
    } else {
        colors::OVERLAY
    };
    Line::from(Span::styled(
        line.replace('\t', "    "),
        Style::default().fg(color),
    ))
}

/// The inspector's lines in the `changes` view: the highlighted file's
/// path, then its diff (an untracked file's text plain), or why there is
/// none.
fn changes_inspector_lines(app: &App) -> Vec<Line<'static>> {
    let dim = |text: &str| {
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(colors::OVERLAY),
        ))
    };
    let Some((_, file)) = app.changes_highlighted() else {
        return vec![dim("no file selected")];
    };
    let mut lines = vec![
        Line::from(Span::styled(
            file.path.clone(),
            Style::default()
                .fg(colors::TEXT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    match app.highlighted_change_diff().and_then(|d| d.text.as_ref()) {
        None => lines.push(dim("reading…")),
        Some(Err(e)) => lines.push(Line::from(Span::styled(
            format!("could not read: {e}"),
            Style::default().fg(colors::RED),
        ))),
        Some(Ok(text)) if file.status == "??" => lines.extend(text.lines().map(|l| {
            Line::from(Span::styled(
                l.replace('\t', "    "),
                Style::default().fg(colors::TEXT),
            ))
        })),
        Some(Ok(text)) => lines.extend(text.lines().map(diff_line)),
    }
    lines
}

/// The inspector while pane 2 shows `changes` (FZ-60). Lines are clipped,
/// not wrapped, as a diff reads, so one line is one row and only the rows
/// on screen are handed to the paragraph; scrolled by the inspector's
/// offset, clamped to what this frame measured.
pub(super) fn render_changes_inspector(frame: &mut Frame, area: Rect, app: &App) {
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
    let lines = changes_inspector_lines(app);
    let max_scroll = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_sub(inner.height);
    app.inspector_max_scroll.set(max_scroll);
    let scroll = app.inspector_scroll.min(max_scroll);
    let visible: Vec<Line> = lines
        .into_iter()
        .skip(scroll as usize)
        .take(inner.height as usize)
        .collect();
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(visible), inner);
}
