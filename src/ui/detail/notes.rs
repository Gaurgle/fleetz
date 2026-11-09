use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::header::header_lines;
use super::layout::{MIN_BODY_ROWS, split_notes_pane};
use super::{
    clip_spans, fit_spans, headliner, pad_to, pane2_block, render_rows, render_scrolled,
    section_label, span_width, wash, wrap_line,
};
use crate::app::{App, FocusedPanel, HistoryView};
use crate::model::RepoStatus;
use crate::ui::badges;
use crate::ui::colors;

#[cfg(test)]
mod tests;

/// Pane 2 of the `notes` layout, split by its divider (FZ-68).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotesPane {
    /// The tree's column, inside the frame, its full inner height: the
    /// header, the tree and the file's facts divider at its foot.
    pub column: Rect,
    /// The rows the header pins in that column, `None` when the pane is too
    /// short to spare them.
    pub header: Option<Rect>,
    /// The tree, under the header.
    pub body: Rect,
    /// The divider's screen column, from under the top border to the bottom
    /// one.
    pub divider: u16,
    /// Right of the divider on the header's rows above the rule: blank but
    /// for `notes preview` on the last of them, level with `NOTES`; the
    /// window sits on pane 1's border (FZ-95). `None` when the header is
    /// not pinned.
    pub facts: Option<Rect>,
    /// The tree column's last row: a divider carrying the highlighted
    /// file's facts (FZ-93). `None` when the tree has no row to spare.
    pub foot: Option<Rect>,
    /// The row of the `NOTES` rule, which runs pane 2's inner width and
    /// crosses the divider. `None` when the header is not pinned.
    pub rule: Option<u16>,
    /// The preview's text: one column in from the divider, on the tree's
    /// rows under the rule, or the column's full inner height when the
    /// header is not pinned.
    pub reader: Rect,
}

/// Pane 2 of the `notes` layout inside its frame (FZ-67, FZ-68): the tree's
/// column left of the divider, holding the pinned header, the tree and the
/// file's facts on a divider at its foot (FZ-93); right of it the fleet's
/// state beside the header and the preview beside the tree, the `NOTES` rule between them across the whole pane
/// (FZ-68 round 2). `None` when pane 2 shows no tree
/// (a section's digest, or no repo). Shared by `render_detail_pane` and
/// `ui::panel_rects`, so the preview is hit-tested where it is drawn.
pub fn notes_pane_body(pane: Rect, app: &App) -> Option<NotesPane> {
    let repo = app.selected_repo()?;
    let (column, right) = split_notes_pane(pane2_block(colors::SURFACE).inner(pane));
    // A column of padding either side of the divider, as the frame keeps
    // one inside its border.
    let text = Rect {
        x: right.x.saturating_add(2).min(right.right()),
        width: right.width.saturating_sub(2),
        ..right
    };
    let below = column;
    let header_rows =
        u16::try_from(header_lines(app, repo, below.width as usize).len()).unwrap_or(u16::MAX);
    let (header, body) = if below.height < header_rows.saturating_add(MIN_BODY_ROWS) {
        (None, below)
    } else {
        let split =
            Layout::vertical([Constraint::Length(header_rows), Constraint::Min(1)]).split(below);
        (Some(split[0]), split[1])
    };
    // The file's facts divider takes the tree's last row while the tree
    // keeps its minimum; the tree's clicks and wheel follow `body`.
    let (body, foot) = if body.height > MIN_BODY_ROWS {
        let split = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(body);
        (split[0], Some(split[1]))
    } else {
        (body, None)
    };
    // The header ends on the headliner's two rows, `NOTES` and its rule:
    // the facts take the rows above the rule, the preview the rows below.
    let (facts, rule, reader) = match header {
        Some(h) if h.height >= 2 => (
            Some(Rect {
                y: h.y,
                height: h.height - 1,
                ..text
            }),
            Some(h.bottom() - 1),
            Rect {
                y: h.bottom(),
                height: text.bottom().saturating_sub(h.bottom()),
                ..text
            },
        ),
        _ => (None, None, text),
    };
    Some(NotesPane {
        column,
        header,
        body,
        divider: right.x,
        facts,
        foot,
        rule,
        reader,
    })
}

/// Pane 2's frame in the `notes` layout (FZ-68), drawn over the one
/// `render_detail_pane` has rendered with its whole top border: the
/// divider from under that border to `┴`, and each side's outline below
/// the top border in its own focus, the tree's left and the preview's
/// right. The top border keeps the pane's colour, lit for either side.
pub(super) fn render_notes_frame(frame: &mut Frame, pane: Rect, notes: &NotesPane, app: &App) {
    let lit = |on: bool| Style::default().fg(notes_lit(on));
    let (top, bottom) = (pane.y, pane.bottom().saturating_sub(1));
    let x = notes.divider;
    let below_top = top.saturating_add(1);
    let buf = frame.buffer_mut();
    let tree = lit(app.focused_panel == FocusedPanel::Detail);
    let preview = lit(app.focused_panel == FocusedPanel::Reader);
    let right_edge = pane.right().saturating_sub(1);
    for (rect, style) in [
        (Rect::new(pane.x, below_top, 1, pane.height), tree),
        (Rect::new(pane.x, bottom, x.saturating_sub(pane.x), 1), tree),
        (Rect::new(right_edge, below_top, 1, pane.height), preview),
        (
            Rect::new(x, bottom, pane.right().saturating_sub(x), 1),
            preview,
        ),
    ] {
        buf.set_style(rect.intersection(pane), style);
    }
    let divider = lit(notes_divider_lit(app));
    for y in below_top..=bottom {
        let symbol = match y == bottom {
            true => "┴",
            false => "│",
        };
        // Off the buffer on a pane squeezed to a column or two.
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_symbol(symbol).set_style(divider);
        }
    }
}

/// The `notes` frame's border colour, lit or not.
fn notes_lit(on: bool) -> ratatui::style::Color {
    match on {
        true => colors::LAVENDER,
        false => colors::SURFACE,
    }
}

/// Whether the `notes` divider is lit: while either side has focus.
fn notes_divider_lit(app: &App) -> bool {
    matches!(
        app.focused_panel,
        FocusedPanel::Detail | FocusedPanel::Reader
    )
}

/// Above the preview in the `notes` layout (FZ-68 round 2), drawn after the
/// pinned header: the file's facts beside the repo header, `notes preview`
/// level with `NOTES` in the view's colour, and the `NOTES` rule across
/// pane 2's whole inner width, crossing the divider with `┼`. Nothing when
/// the header is not pinned.
pub(super) fn render_notes_heads(frame: &mut Frame, pane: Rect, notes: &NotesPane, app: &App) {
    let (Some(facts), Some(rule)) = (notes.facts, notes.rule) else {
        return;
    };
    let inner = pane2_block(colors::SURFACE).inner(pane);
    // The headliner's own rule at the pane's width, over the column-wide one
    // the header drew, so the fade runs the whole pane.
    if let Some(line) = headliner(app.history_view, inner.width as usize).pop() {
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(inner.x, rule, inner.width, 1),
        );
    }
    if let Some(cell) = frame.buffer_mut().cell_mut((notes.divider, rule)) {
        cell.set_symbol("┼")
            .set_style(Style::default().fg(notes_lit(notes_divider_lit(app))));
    }
    if facts.height == 0 {
        return;
    }
    // Only `notes preview` here: the window sits on pane 1's border (FZ-95).
    let label_row = facts.bottom() - 1;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            READER_LABEL,
            Style::default()
                .fg(colors::history_view_color(HistoryView::Notes))
                .add_modifier(Modifier::BOLD),
        ))),
        Rect {
            y: label_row,
            height: 1,
            ..facts
        },
    );
}

/// The tree's foot in the `notes` layout (FZ-93): a static divider with
/// the highlighted file's facts (`notes_foot_spans`). Not a pane: no focus,
/// no clicks.
pub(super) fn render_notes_foot(frame: &mut Frame, foot: Rect, app: &App) {
    let spans = notes_foot_spans(app, foot.width as usize);
    frame.render_widget(Paragraph::new(Line::from(spans)), foot);
}

/// The `notes` tree's two modes in the words the mode line, the empty states
/// and the help overlay's `m` row all use (FZ-57). `markdown` rather than
/// `all markdown`: the help row has 26 columns at 80x24, and the shorter
/// word still says what the second mode adds.
const NOTES_MODES: [&str; 2] = ["notez", "markdown"];

/// A note file's name in the tree, and in the facts beside the header.
const NOTE_NAME_COLOR: ratatui::style::Color = colors::TEXT;

/// A note's kind glyph in the tree, and its kind word in the facts.
const NOTE_GLYPH_COLOR: ratatui::style::Color = colors::OVERLAY;

/// The lit mark of a two-state switch in `notes`: the mode line's active
/// mode, and the facts' `rendered` or `raw`.
fn notes_mode_mark_style() -> Style {
    Style::default()
        .fg(colors::history_view_color(HistoryView::Notes))
        .add_modifier(Modifier::BOLD)
}

/// The `notes` view's first line: both modes as a two-state switch, the
/// active one bracketed and lit as the tab strip lights its view, the other
/// dim, and the key that flips them (FZ-57): `notes: [notez] markdown  (m)`.
pub(super) fn notes_mode_line(app: &App) -> Line<'static> {
    notes_mode_line_for(app.notes_all_markdown)
}

/// `notes_mode_line` for either mode, so the tree's width floor can measure
/// the line it has to hold (`notes_tree_floor`).
pub(super) fn notes_mode_line_for(all_markdown: bool) -> Line<'static> {
    let active = usize::from(all_markdown);
    let mut spans = vec![Span::styled(
        "notes: ",
        Style::default().fg(colors::OVERLAY),
    )];
    for (i, mode) in NOTES_MODES.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(match i == active {
            true => Span::styled(format!("[{mode}]"), notes_mode_mark_style()),
            false => Span::styled(mode.to_string(), Style::default().fg(colors::SUBTEXT)),
        });
    }
    spans.push(Span::styled("  (", Style::default().fg(colors::OVERLAY)));
    spans.push(Span::styled(
        "m",
        Style::default()
            .fg(colors::TEXT)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(")", Style::default().fg(colors::OVERLAY)));
    Line::from(spans)
}

/// The `notes` view's tree for `repo` (FZ-53): its lines, the line each file
/// starts on, in cursor order, and the line the cursor's file is on. A row
/// with nothing to list says why in one line.
pub(super) fn notes_rows(
    app: &App,
    repo: &RepoStatus,
    width: usize,
) -> (Vec<Line<'static>>, Vec<usize>, Option<usize>) {
    let say = |text: String| {
        let line = Line::from(Span::styled(text, Style::default().fg(colors::OVERLAY)));
        (vec![line], Vec::new(), None)
    };
    if repo.path.is_none() {
        return say("not cloned: no notes to read".to_string());
    }
    let notes = match app
        .selected_notes()
        .and_then(|(_, state)| state?.tree.as_ref())
    {
        None => return say("reading notes…".to_string()),
        Some(Err(e)) => return say(format!("could not read the repo: {e}")),
        Some(Ok(notes)) => notes,
    };
    let rows = notes.rows(app.notes_all_markdown);
    if rows.is_empty() {
        // Named by the mode it belongs to, in the mode line's words.
        return say(match app.notes_all_markdown {
            false => format!("{}: no notez/ or .notez/ directory", NOTES_MODES[0]),
            true => format!("{}: no markdown files", NOTES_MODES[1]),
        });
    }
    let files = rows
        .iter()
        .filter(|r| matches!(r, crate::notes::Row::File { .. }))
        .count();
    let cursor = app.detail_selected().min(files.saturating_sub(1));
    let mut lines = Vec::with_capacity(rows.len());
    let mut tops = Vec::with_capacity(files);
    let mut selected = None;
    // Every row is cut to one rendered row (`clip_spans`): the tops are
    // counted in lines, and a long name the paragraph wrapped put the
    // highlight that many rows below where the follow kept it (FZ-65).
    for row in rows {
        match row {
            crate::notes::Row::Section(label) => {
                if !lines.is_empty() {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(clip_spans(vec![section_label(&label)], width)));
            }
            crate::notes::Row::Dir { depth, name } => {
                let dir = Span::styled(
                    format!("  {}{name}/", "  ".repeat(depth)),
                    Style::default().fg(colors::OVERLAY),
                );
                lines.push(Line::from(clip_spans(vec![dir], width)));
            }
            crate::notes::Row::File { depth, name, file } => {
                let is_sel = tops.len() == cursor;
                if is_sel {
                    selected = Some(lines.len());
                }
                tops.push(lines.len());
                // The kind's glyph before the name (FZ-62), dim so the name
                // stays what the eye reads.
                let entry = notes.files.get(file);
                let glyph = entry.map_or("", |f| badges::note_kind_icon(f.kind()));
                let mut spans = vec![
                    Span::styled(
                        if is_sel { "▸ " } else { "  " },
                        Style::default().fg(colors::SAPPHIRE),
                    ),
                    Span::raw("  ".repeat(depth)),
                    Span::styled(format!("{glyph} "), Style::default().fg(NOTE_GLYPH_COLOR)),
                    Span::styled(name, Style::default().fg(NOTE_NAME_COLOR)),
                ];
                // Every board is named `TODO.md` under `todos` (FZ-63); its
                // place, dim, tells them apart.
                if let Some(f) = entry.filter(|f| f.kind() == crate::notes::NoteKind::Todo) {
                    spans.push(Span::styled(
                        format!("  {}", f.place()),
                        Style::default().fg(colors::OVERLAY),
                    ));
                }
                spans = clip_spans(spans, width);
                if is_sel {
                    spans = wash(pad_to(spans, width));
                }
                lines.push(Line::from(spans));
            }
        }
    }
    (lines, tops, selected)
}

/// The highlighted note's body as last read, when it is that file's.
fn held_note_body(app: &App) -> Option<&crate::app::NoteBody> {
    let path = app.notes_highlighted()?;
    app.note_body.as_ref().filter(|b| b.path == path)
}

/// A file time as the facts write it, the local date: the facts share a
/// row, and the sketch the owner accepted drew dates (FZ-68 round 2).
fn card_time(at: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(at)
        .format("%Y-%m-%d")
        .to_string()
}

/// git's `%cI` date as the facts write it, the way `card_time` writes a
/// file time; git's own spelling when it does not parse.
fn card_commit_time(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(at) => at
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string(),
        Err(_) => iso.to_string(),
    }
}

/// The highlighted file's facts in the `notes` layout, for the divider at
/// the tree's foot (FZ-93; beside the repo header from FZ-68 round 2, the
/// `[3] file` card's before it), in display order, each with the rank it
/// gives way at when the foot is narrow, higher first: where it lives (6),
/// the markdown mode (5), the kind (4), the last commit (3), modified (2),
/// the size (1); the name (0) never. The name and kind wear the tree's
/// colours for the file and its glyph, the mode the mode line's mark,
/// sizes and dates are dim. Size and times wait for the background read
/// (`spawn_note_read`), the last commit for git (`AppEvent::NoteCommit`).
fn notes_fact_pieces(app: &App) -> Vec<(u8, Vec<Span<'static>>)> {
    let dim = |text: String| Span::styled(text, Style::default().fg(colors::OVERLAY));
    let Some(file) = app.notes_highlighted_file() else {
        return vec![(0, vec![dim("no file selected".to_string())])];
    };
    let name = file
        .rel
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let held = held_note_body(app);
    let facts = held.and_then(|b| b.facts.as_ref());

    let mut pieces = vec![
        (
            0,
            vec![Span::styled(name, Style::default().fg(NOTE_NAME_COLOR))],
        ),
        (
            4,
            vec![Span::styled(
                file.kind().label().to_string(),
                Style::default().fg(NOTE_GLYPH_COLOR),
            )],
        ),
    ];
    // A board counts what is open, unchecked and half done (FZ-63).
    if let Some(board) = held_board(app) {
        pieces.push((4, vec![dim(format!("{} open", board.open()))]));
    }
    if let Some(size) = facts.and_then(|f| f.size) {
        pieces.push((1, vec![dim(crate::notes::size_label(size))]));
    }
    // A board is drawn as one whatever `p` says, so it names no mode.
    if file.kind().is_markdown() && file.kind() != crate::notes::NoteKind::Todo {
        let mode = match app.reader_raw {
            true => "raw",
            false => "rendered",
        };
        pieces.push((5, vec![Span::styled(mode, notes_mode_mark_style())]));
    }
    let lives = match &file.notez {
        Some(dir) => format!("{}/", dir.display()),
        None if file.is_personal() => "the personal store".to_string(),
        None => "the repo".to_string(),
    };
    pieces.push((
        6,
        vec![Span::styled(lives, Style::default().fg(colors::TEXT))],
    ));
    match facts {
        None => pieces.push((1, vec![dim("reading…".to_string())])),
        Some(facts) => {
            if let Some(at) = facts.modified {
                pieces.push((2, vec![dim(format!("modified {}", card_time(at)))]));
            }
            use crate::notes::LastCommit;
            let commit = match held.and_then(|b| b.last_commit.as_ref()) {
                Some(LastCommit::At(iso)) => card_commit_time(iso),
                Some(LastCommit::Untracked) => "untracked".to_string(),
                None => "reading…".to_string(),
            };
            pieces.push((3, vec![dim(format!("last commit {commit}"))]));
        }
    }
    pieces
}

/// The tree's foot as one row of `width`: a rule carrying the file's facts
/// (`notes_fact_pieces`), drawn as the compact view's status line is. The
/// facts that do not fit give way in their rank's order, so the row never
/// wraps and the name always stays; a name wider than the row is cut.
fn notes_foot_spans(app: &App, width: usize) -> Vec<Span<'static>> {
    let rule = |n: usize| Span::styled("─".repeat(n), Style::default().fg(colors::SURFACE));
    let pieces = notes_fact_pieces(app);
    let line = |max_rank: u8| {
        let mut spans = vec![rule(1), Span::raw(" ")];
        for (i, (_, piece)) in pieces.iter().filter(|(r, _)| *r <= max_rank).enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
            }
            spans.extend(piece.iter().cloned());
        }
        spans.push(Span::raw(" "));
        spans
    };
    let spans = (0..=6u8)
        .rev()
        .map(line)
        .find(|spans| span_width(spans) <= width)
        .unwrap_or_else(|| fit_spans(line(0), width));
    let rest = width.saturating_sub(span_width(&spans));
    let mut spans = spans;
    spans.push(rule(rest));
    spans
}

/// The preview's headliner word, right of the divider level with `NOTES`
/// (FZ-68 round 2): the preview is a page of the `notes` view.
const READER_LABEL: &str = "notes preview";

/// Where `render_notes_heads` draws the reader's `notes preview` title in
/// pane 2, for the copy flash (FZ-81). `None` when the header is not pinned
/// and the title is not drawn.
pub fn reader_label_rect(pane: Rect, app: &App) -> Option<Rect> {
    let facts = notes_pane_body(pane, app)?.facts?;
    (facts.height > 0).then(|| Rect {
        y: facts.bottom() - 1,
        width: (READER_LABEL.len() as u16).min(facts.width),
        height: 1,
        ..facts
    })
}

/// The highlighted file's todoz board, parsed from the body already read,
/// here on the UI thread: measured cheap for a capped file (FZ-63). `None`
/// for any other kind, and until the read lands.
fn held_board(app: &App) -> Option<crate::ui::todo::Board> {
    app.notes_highlighted_file()
        .filter(|f| f.kind() == crate::notes::NoteKind::Todo)?;
    let text = held_note_body(app)?.text.as_ref()?.as_ref().ok()?;
    Some(crate::ui::todo::Board::parse(text))
}

/// The held body's markdown render, when the reader draws one: a markdown
/// file, `p` not on raw, and a render landed (FZ-59). Until one lands the
/// reader shows the source. A `TODO.md` draws as a board instead
/// (`held_board`, FZ-63).
fn reader_render(app: &App) -> Option<&crate::app::RenderedNote> {
    let markdown = app
        .notes_highlighted_file()
        .is_some_and(|f| f.kind().is_markdown() && f.kind() != crate::notes::NoteKind::Todo);
    if !markdown || app.reader_raw {
        return None;
    }
    let body = held_note_body(app)?;
    matches!(body.text, Some(Ok(_)))
        .then_some(body.rendered.as_ref())
        .flatten()
}

/// The `notes` layout's reader, right of the tree in pane 2 (FZ-62): the
/// highlighted file's body, wrapped to the pane, with its own scroll
/// (`App::reader_scroll`). A markdown file is drawn rendered
/// (`ui::markdown`, FZ-59), its rows laid out off the UI thread at the
/// width this pass records; `p` shows its source, which every other kind
/// always shows, raw as `notes::read_body` delivers it. A cut body says so
/// on its last line, as the read leaves it.
pub(super) fn render_reader(frame: &mut Frame, inner: Rect, app: &App) {
    // No frame of its own: pane 2's border and divider hold the column and
    // carry its titles (`render_notes_frame`, FZ-68).
    // One `H`/`L` page and the render width, which only this pass can
    // measure.
    let board = held_board(app);
    app.reader_view_height.set(inner.height);
    app.reader_width.set(inner.width);
    if let Some(rendered) = reader_render(app) {
        let rows: std::borrow::Cow<[Line<'static>]> = match rendered.width == inner.width {
            true => std::borrow::Cow::Borrowed(&rendered.lines),
            // Laid out for the width before a resize, until the render
            // at this one lands (`App::claim_note_render`).
            false => std::borrow::Cow::Owned(
                rendered
                    .lines
                    .iter()
                    .flat_map(|l| wrap_line(l, inner.width as usize))
                    .collect(),
            ),
        };
        render_rows(
            frame,
            inner,
            &rows,
            app.reader_scroll,
            &app.reader_max_scroll,
        );
        return;
    }
    let dim = |text: String| Line::from(Span::styled(text, Style::default().fg(colors::OVERLAY)));
    let lines: Vec<Line<'static>> = match app.notes_highlighted_file() {
        None => vec![dim("no file selected".to_string())],
        Some(_) if board.is_some() => board
            .as_ref()
            .map_or_else(Vec::new, |b| b.lines(inner.width as usize)),
        Some(_) => match held_note_body(app).and_then(|b| b.text.as_ref()) {
            None => vec![dim("reading…".to_string())],
            Some(Err(e)) => vec![Line::from(Span::styled(
                format!("could not read: {e}"),
                Style::default().fg(colors::PEACH),
            ))],
            Some(Ok(text)) => text
                .lines()
                .map(|l| {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(colors::TEXT),
                    ))
                })
                .collect(),
        },
    };
    render_scrolled(
        frame,
        inner,
        &lines,
        app.reader_scroll,
        &app.reader_max_scroll,
    );
}
