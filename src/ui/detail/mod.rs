mod changes;
mod commits;
mod header;
mod inspector;
mod layout;
mod list;
mod notes;
mod pane;
mod worktrees;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::{App, HistoryView};
use crate::model::Commit;
use crate::ui::colors;
use crate::ui::format_age;
use inspector::render_inspector;
use list::render_repo_list;
use notes::render_reader;
use pane::render_detail_pane;

pub(crate) use commits::RECENT_COMMIT_COUNT;
pub(crate) use layout::{INSPECTOR_TWO_LANE_WIDTH, repo_list_width, split_inspector, split_main};
pub(crate) use list::{REPO_ROW_MAX_W, repo_row_spans_ci};
pub(crate) use notes::{notes_pane_body, reader_label_rect};
pub(crate) use pane::{DetailClick, DetailHits};

#[cfg(test)]
pub(crate) use list::repo_row_spans;

/// The row a capped list ends on: how many entries it left out. The one
/// overflow marker the panes use (the inspector's file list, the slot
/// digest's lists).
pub fn more_line(hidden: usize) -> Line<'static> {
    Line::from(Span::styled(
        format!("… and {hidden} more"),
        Style::default().fg(colors::OVERLAY),
    ))
}

/// Render the detail view: the repo list on the left (35% of the width by
/// default, capped at `REPO_LIST_MAX_COLS`; see `split_main`) and the detail
/// pane on the right, with the inspector docked across the full width below
/// them.
/// Draw the three Detail panes into rectangles `ui::panel_rects` already
/// computed.
///
/// It takes the rects rather than an area to split, because the splitting is
/// no longer purely detail's business: the contribution strip is carved out
/// of the right column between the horizontal and vertical splits, so the
/// order lives one level up where both views can be seen at once.
pub fn render(
    frame: &mut Frame,
    list_area: Rect,
    detail_area: Rect,
    inspector_area: Option<Rect>,
    reader_area: Option<Rect>,
    app: &App,
) {
    render_repo_list(frame, list_area, app);
    render_detail_pane(frame, detail_area, app);
    // The `notes` layout's preview, right of pane 2's divider (FZ-68).
    if let Some(reader_area) = reader_area {
        render_reader(frame, reader_area, app);
    }
    // The inspector is docked under the detail pane only when there's room --
    // on a short terminal the detail pane keeps the whole column.
    if let Some(inspector_area) = inspector_area {
        render_inspector(frame, inspector_area, app);
    }
}

/// A commit's age, rendered fresh on every draw. Empty when git handed back
/// a timestamp we couldn't parse, so the separator collapses rather than the
/// row showing a bogus age.
pub fn commit_age(commit: &Commit) -> String {
    commit.date.map(format_age).unwrap_or_default()
}

/// Total display width of a span run.
fn span_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// Pane 2's frame, every view's: rounded, a column of padding either side
/// and a row above the header.
fn pane2_block(border_color: ratatui::style::Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(1, 1, 1, 0))
}

/// `spans` cut to `width` columns with `…`, or as they are when they fit.
fn fit_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    if span_width(&spans) <= width {
        spans
    } else {
        crate::ui::markdown::clip_spans(spans, width)
    }
}

/// `spans` padded with blanks to `width` columns.
fn pad_spans(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let w = span_width(&spans);
    if w < width {
        spans.push(Span::raw(" ".repeat(width - w)));
    }
    spans
}

/// `spans` cut to `width` and pushed flush right in it.
fn right_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let spans = fit_spans(spans, width);
    let mut out = vec![Span::raw(
        " ".repeat(width.saturating_sub(span_width(&spans))),
    )];
    out.extend(spans);
    out
}

/// Draw `lines` wrapped into `inner` from `scroll`, clamped to what this
/// frame measured, and report the largest useful offset through `max`.
fn render_scrolled(
    frame: &mut Frame,
    inner: Rect,
    lines: &[Line<'static>],
    scroll: u16,
    max: &std::cell::Cell<u16>,
) {
    let wrapped: Vec<Line<'static>> = lines
        .iter()
        .flat_map(|l| wrap_line(l, inner.width as usize))
        .collect();
    render_rows(frame, inner, &wrapped, scroll, max);
}

/// Draw `rows`, one per screen row as given, into `inner` from `scroll`,
/// clamped to what this frame measured, and report the largest useful
/// offset through `max`. Only the rows on screen are copied.
fn render_rows(
    frame: &mut Frame,
    inner: Rect,
    rows: &[Line<'static>],
    scroll: u16,
    max: &std::cell::Cell<u16>,
) {
    let max_scroll = u16::try_from(rows.len())
        .unwrap_or(u16::MAX)
        .saturating_sub(inner.height);
    max.set(max_scroll);
    let visible: Vec<Line> = rows
        .iter()
        .skip(scroll.min(max_scroll) as usize)
        .take(inner.height as usize)
        .cloned()
        .collect();
    frame.render_widget(Paragraph::new(visible), inner);
}

/// Estimate how many terminal rows `lines` occupy once wrapped to `width`, so
/// the inspector's scroll offset can be clamped to real content height. A
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
    // The same margin the cursor keeps when it walks: a block brought to the
    // very edge is a block you cannot see the next one past.
    let pad = crate::app::scrolloff_for(height);
    // scroll must be at most this for the top to clear the upper margin,
    // and at least this for the bottom to clear the lower one.
    let highest = top.saturating_sub(pad);
    let lowest = (bottom + 1 + pad).saturating_sub(height);
    if scroll > highest {
        return highest;
    }
    if scroll < lowest {
        // A block taller than the pane cannot clear both margins, so its top
        // wins: scrolling to the bottom of an oversized block leaves you
        // looking at the tail of a message with nothing to say whose it is.
        return lowest.min(highest);
    }
    scroll
}

/// `Paragraph` wraps long lines, so a raw line count under-counts. Guards a
/// zero width so a degenerate pane can't divide by zero.
fn wrapped_row_count(lines: &[Line], width: u16) -> u16 {
    let rows: usize = lines.iter().map(|line| line_rows(line, width)).sum();
    rows.min(u16::MAX as usize) as u16
}

/// Rows one line takes in a wrapping paragraph `width` columns wide: the
/// arithmetic `wrapped_row_count` and the click map share.
fn line_rows(line: &Line, width: u16) -> usize {
    let w = width.max(1) as usize;
    let len: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    if len == 0 { 1 } else { len.div_ceil(w) }
}

/// Paint a whole row as selected, keeping each span's own foreground.
///
/// The band has to cover the row rather than the text, so the caller pads to
/// the pane width before this: a wash that stops where the message stops
/// reads as a highlight on the words, not a cursor on the line.
pub(crate) fn wash(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    spans
        .into_iter()
        .map(|s| {
            let style = s.style.bg(colors::SELECTED_BG);
            Span::styled(s.content, style)
        })
        .collect()
}

/// Hold a row open to `width` so a selection wash covers the line rather than
/// stopping where its text does, which reads as a highlight on the words
/// instead of a cursor on the row.
fn pad_to(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let used = span_width(&spans);
    if used < width {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    spans
}

/// Split `text` into chunks that each fit `budget` columns.
///
/// Breaks at spaces where it can and mid-token where it cannot, because a
/// space-only break never terminates on a token wider than the pane, and a
/// long URL in a commit message is the common case. Always returns at least
/// one chunk: git's blocks use blank lines as spacing, and dropping them
/// would close the gaps between commits.
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

/// Re-flow one line's spans to `width`, keeping each chunk's styling.
///
/// The same word-aware break `wrap_text` gives the graph, applied to a line
/// that is already several spans. Wrapping in the builder rather than leaving
/// it to `Paragraph` makes one line one rendered row, which is what lets the
/// scroll offset be an exact row index: `wrapped_row_count` counts
/// `ceil(len / width)` while `Paragraph` breaks at spaces, so it under-reports
/// every wrapped paragraph and the tail of a long commit body could not be
/// scrolled to.
pub fn wrap_line<'a>(line: &Line<'a>, width: usize) -> Vec<Line<'a>> {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    if width == 0 || text.chars().count() <= width {
        return vec![line.clone()];
    }
    // One entry per character, so a chunk can be cut back into styled spans.
    // `wrap_text` neither adds nor drops characters, so the offsets line up.
    let styled: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(|c| (c, s.style)))
        .collect();

    let mut out = Vec::new();
    let mut at = 0usize;
    for chunk in wrap_text(&text, width) {
        let n = chunk.chars().count();
        let mut spans: Vec<Span<'a>> = Vec::new();
        for &(ch, style) in &styled[at.min(styled.len())..(at + n).min(styled.len())] {
            match spans.last_mut() {
                // Runs of one style stay one span, so a wrapped line does not
                // become one span per character.
                Some(last) if last.style == style => last.content.to_mut().push(ch),
                _ => spans.push(Span::styled(ch.to_string(), style)),
            }
        }
        at += n;
        out.push(Line::from(spans));
    }
    out
}

/// The pane's headliner: the view's name, and a divider under it that fades
/// out of the name's own colour.
///
/// Two rows, and pinned ones, so it costs the scrolling body one line at every
/// pane height. Worth it because the headliner is the thing the whole body
/// below it is an answer to, and as a single line of bold text sitting straight
/// on that body it read as the body's first line rather than as its title.
///
/// No box and no leading bar. Both were tried. A box drew two lines whose only
/// job was to meet each other, and cost a third pinned row to do it; a leading
/// bar then indented the name by two columns, so the one line that heads the
/// body did not line up with any line of it. The divider alone says the same
/// thing and leaves the name where the eye already is.
///
/// The divider starts in the view's own colour -- the same one its tab wears up
/// in the border, so the two agree about which view you are in -- and fades
/// into `SURFACE` across the pane: under the name it is part of the label, by
/// the far edge it is a divider, and structure is what that colour is for.
fn headliner(view: HistoryView, width: usize) -> Vec<Line<'static>> {
    let on = colors::history_view_color(view);
    // Per cell, through the same `blend` the spinner walks its palette with.
    // That one interpolates over time and this one over distance, but it is the
    // same arithmetic and there is no reason to have two of it. A cell each is
    // a couple of hundred spans on a wide pane, which is nothing next to the
    // commit rows underneath.
    let divider: Vec<Span<'static>> = (0..width)
        .map(|i| {
            Span::styled(
                "─".to_string(),
                Style::default().fg(crate::ui::blend(
                    on,
                    colors::SURFACE,
                    i as u32 + 1,
                    width as u32,
                )),
            )
        })
        .collect();

    vec![
        Line::from(Span::styled(
            view_name(view),
            Style::default().fg(on).add_modifier(Modifier::BOLD),
        )),
        Line::from(divider),
    ]
}

/// The view's name as both the tab strip and the pane's headliner draw it:
/// capitals, one cell per letter.
///
/// One form for both, so the tab you aim at and the title that confirms you
/// landed are the same string as well as the same colour. Capitals cost the
/// strip nothing -- the label is the same number of cells -- so its fit budget,
/// which drops the inactive tabs when the labels outgrow the border, is
/// untouched. Letterspacing was tried and dropped: it read as decoration in a
/// pane whose every other line is dense, and the box around the headliner
/// already does the work of setting it apart.
fn view_name(view: HistoryView) -> String {
    view.label().to_uppercase()
}

/// A bold section header inside the detail pane (e.g. "branches", "history").
/// The view switcher, drawn in the pane's top border rather than in its body.
///
/// It belongs in the frame because it answers "what am I looking at", and the
/// body scrolls: a strip at the top of the body is gone by the second page,
/// which is exactly when the question gets asked. The doc comment this
/// replaces claimed it was pinned. It never was.
///
/// The keys ride along with it, since a switcher that does not say how to
/// switch answers half the question. `[` and `]` are drawn as two lit keys
/// either side of a dimmed `/`, so the pair reads as "either bracket" rather
/// than as the single `/` key -- which is bound to search.
///
/// `budget` is the border width left over after the pane's own title, and
/// the strip takes the widest of three forms that fits it: every tab with
/// its full name; every tab cut to its first three letters (`INF · COM`),
/// same separators, colours and keys; and, when even that does not fit,
/// `[X/N]` alone, the active view's place in the cycle, its digits in the
/// view's colour and no key hint beside it.
fn view_switcher(active: HistoryView, focused: bool, budget: usize) -> Line<'static> {
    // " [/] " + every label + " · " between each + a trailing space, as
    // `tab_switcher` measures it.
    let strip_width = |labels: &[String]| -> usize {
        let cells: usize = labels.iter().map(|l| l.chars().count()).sum();
        5 + cells + 3 * (labels.len() - 1) + 1
    };
    let on = colors::history_view_color(active);
    let full: Vec<String> = HistoryView::ALL.iter().map(|v| view_name(*v)).collect();
    let short: Vec<String> = full.iter().map(|l| l.chars().take(3).collect()).collect();
    let labels = if strip_width(&full) <= budget {
        full
    } else if strip_width(&short) <= budget {
        short
    } else {
        let key = if focused {
            colors::TEXT
        } else {
            colors::OVERLAY
        };
        let bold = |c| Style::default().fg(c).add_modifier(Modifier::BOLD);
        let position = format!("{}/{}", active.index() + 1, HistoryView::COUNT);
        return Line::from(vec![
            Span::raw(" "),
            Span::styled("[", bold(key)),
            Span::styled(position, bold(on)),
            Span::styled("]", bold(key)),
            Span::raw(" "),
        ])
        .right_aligned();
    };
    tab_switcher(&labels, active.index(), on, focused, budget)
}

/// A border's `[/]` tab line: pane 2's views (`view_switcher`) and the CI
/// inspector's bodies (FZ-64) share it, so the two read and collapse alike.
/// `labels` are drawn as given, `labels[active]` bold in `on`; too tight for
/// all of them within `budget` and it shows the active one alone.
fn tab_switcher(
    labels: &[String],
    active: usize,
    on: ratatui::style::Color,
    focused: bool,
    budget: usize,
) -> Line<'static> {
    // The active tab wears its own view's colour in either state: which mode
    // the pane is in is worth reading from across the screen, and it is the
    // one thing here that a dimmed switcher must still say. Everything around
    // it -- the keys, the separators, the inactive tabs -- drops back when the
    // pane is not focused, so a bright switcher never pulls the eye to a pane
    // you are not in.
    let (key, slash, dot, off) = if focused {
        (
            colors::TEXT,
            colors::OVERLAY,
            colors::SURFACE,
            colors::SUBTEXT,
        )
    } else {
        (
            colors::OVERLAY,
            colors::SURFACE,
            colors::SURFACE,
            colors::SURFACE,
        )
    };
    let bold = |c| Style::default().fg(c).add_modifier(Modifier::BOLD);

    let mut spans = vec![
        Span::raw(" "),
        Span::styled("[", bold(key)),
        Span::styled("/", Style::default().fg(slash)),
        Span::styled("]", bold(key)),
        Span::raw(" "),
    ];

    // " [/] " + every label + " · " between each + a trailing space.
    let width: usize = labels.iter().map(|l| l.chars().count()).sum();
    let full = 5 + width + 3 * labels.len().saturating_sub(1) + 1;

    if full <= budget {
        for (i, label) in labels.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", Style::default().fg(dot)));
            }
            spans.push(match i == active {
                true => Span::styled(label.clone(), bold(on)),
                false => Span::styled(label.clone(), Style::default().fg(off)),
            });
        }
    } else if let Some(label) = labels.get(active) {
        spans.push(Span::styled(label.clone(), bold(on)));
    }
    spans.push(Span::raw(" "));
    Line::from(spans).right_aligned()
}

/// `spans` cut to `width` columns, the span that crosses the edge ending in
/// the pane's ellipsis and everything after it dropped. For a row in the
/// wrapping body that must stay one rendered row: the commit list's row tops
/// are counted in source lines, so a row that wrapped would put every commit
/// below it lower than recorded.
pub(crate) fn clip_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut left = width;
    let mut out = Vec::new();
    for span in spans {
        let w = span.content.chars().count();
        if w <= left {
            left -= w;
            out.push(span);
            continue;
        }
        if left > 0 {
            out.push(Span::styled(
                crate::ui::truncate(&span.content, left),
                span.style,
            ));
        }
        break;
    }
    out
}

fn section_label(text: &str) -> Span<'static> {
    Span::styled(
        text.to_string(),
        Style::default()
            .fg(colors::MAUVE)
            .add_modifier(Modifier::BOLD),
    )
}

#[cfg(test)]
mod tests;
