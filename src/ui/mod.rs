pub mod badges;
pub mod branches;
pub mod colors;
pub mod compact;
pub mod detail;
pub mod footer;
pub mod help;

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

use crate::app::{App, FilterKind, FocusedPanel, Overlay, View};

/// The screen rectangles of the main panels, recomputed from the terminal
/// area with the same layout math as `render`, so mouse coordinates can be
/// hit-tested against them. `detail`/`inspector` are None in Compact view (or
/// when the column is too short for the inspector).
pub struct PanelRects {
    pub repo_list: Rect,
    pub detail: Option<Rect>,
    pub inspector: Option<Rect>,
}

impl PanelRects {
    /// Which panel, if any, contains the given screen cell.
    pub fn at(&self, col: u16, row: u16) -> Option<FocusedPanel> {
        let p = Position { x: col, y: row };
        if self.repo_list.contains(p) {
            return Some(FocusedPanel::RepoList);
        }
        if self.detail.is_some_and(|r| r.contains(p)) {
            return Some(FocusedPanel::Detail);
        }
        if self.inspector.is_some_and(|r| r.contains(p)) {
            return Some(FocusedPanel::Inspector);
        }
        None
    }
}

/// Recompute the panel layout for `area` -- must mirror `render` +
/// `detail::render` exactly so hit-testing lines up with what's drawn.
pub fn panel_rects(area: Rect, app: &App) -> PanelRects {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);
    let main = chunks[1];

    match app.view {
        View::Compact => PanelRects {
            repo_list: main,
            detail: None,
            inspector: None,
        },
        View::Detail => {
            let h = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
                .spacing(1)
                .split(main);
            let repo_list = h[0];
            if h[1].height > detail::INSPECTOR_HEIGHT + 6 {
                let v = Layout::vertical([
                    Constraint::Min(1),
                    Constraint::Length(detail::INSPECTOR_HEIGHT),
                ])
                .split(h[1]);
                PanelRects {
                    repo_list,
                    detail: Some(v[0]),
                    inspector: Some(v[1]),
                }
            } else {
                PanelRects {
                    repo_list,
                    detail: Some(h[1]),
                    inspector: None,
                }
            }
        }
    }
}

/// Quick filters in strip order, with the color each dot lights up in --
/// matching the color that filter's data already uses elsewhere (e.g. dirty
/// badges are YELLOW, so is this dot).
const FILTER_DEFS: [(FilterKind, Color); 6] = [
    (FilterKind::Dirty, colors::YELLOW),
    (FilterKind::Behind, colors::PEACH),
    (FilterKind::Ahead, colors::SAPPHIRE),
    (FilterKind::CiFailed, colors::RED),
    (FilterKind::HasPrs, colors::MAUVE),
    (FilterKind::HasStashes, colors::HASH_CLR),
];

/// The combined filter + search strip, modeled on notez/todoz's own strip:
/// a fixed row of six filter dots (lit in each filter's color when active,
/// muted "·" otherwise) followed inline by the search/filter input. The dots
/// never grow -- so the strip can't blow up in width the way the old
/// digit-keyed labels did -- and light up live as `#tag` words are typed.
///
/// The input portion has three states:
/// - **editing** (`/` active): `/` + the query with a block cursor, plus a
///   dim syntax hint while the query is still empty.
/// - **set but not editing**: `/` + the query (with `#tag` words colored in
///   their filter's hue) + an "esc to clear" reminder.
/// - **idle**: a dim `/filter` prompt.
pub fn filter_strip_line(app: &App) -> Line<'static> {
    let active = app.active_filters();
    let mut spans = vec![Span::raw("  ")];
    for (kind, color) in FILTER_DEFS.iter() {
        if active.is_active(*kind) {
            spans.push(Span::styled("●", Style::default().fg(*color)));
        } else {
            spans.push(Span::styled("·", Style::default().fg(colors::SURFACE)));
        }
        spans.push(Span::raw(" "));
    }
    spans.push(Span::raw("  "));

    if app.search_active {
        spans.push(Span::styled("/", Style::default().fg(colors::YELLOW)));
        spans.extend(query_spans(&app.search_query));
        // Block cursor sits at the end (fleetz edits at the tail only).
        spans.push(Span::styled(" ", Style::default().bg(colors::SAPPHIRE)));
        if app.search_query.is_empty() {
            spans.push(Span::styled(
                "  text + #tag to filter",
                Style::default().fg(colors::SURFACE),
            ));
        }
    } else if !app.search_query.is_empty() {
        spans.push(Span::styled("/", Style::default().fg(colors::YELLOW)));
        spans.extend(query_spans(&app.search_query));
        spans.push(Span::styled(
            "  esc to clear",
            Style::default().fg(colors::OVERLAY),
        ));
    } else {
        spans.push(Span::styled("/", Style::default().fg(colors::YELLOW)));
        spans.push(Span::styled("filter", Style::default().fg(colors::OVERLAY)));
    }

    Line::from(spans)
}

/// Render a query string as colored word spans: `#tag` words in their
/// filter's color (or plain yellow if the tag prefix matches nothing), free
/// text in the default text color.
fn query_spans(query: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for word in query.split(' ') {
        if word.is_empty() {
            spans.push(Span::raw(" "));
            continue;
        }
        let color = word.strip_prefix('#').and_then(|name| {
            let name = name.to_lowercase();
            FILTER_DEFS
                .iter()
                .find(|(kind, _)| {
                    crate::app::FILTER_TAGS
                        .iter()
                        .any(|(k, tag)| k == kind && tag.starts_with(&name) && !name.is_empty())
                })
                .map(|(_, c)| *c)
        });
        let style = match color {
            Some(c) => Style::default().fg(c).add_modifier(Modifier::BOLD),
            None if word.starts_with('#') => Style::default().fg(colors::YELLOW),
            None => Style::default().fg(colors::TEXT),
        };
        spans.push(Span::styled(format!("{word} "), style));
    }
    spans
}

/// A commit author's short code, for the tight author column in the
/// commit/history lists. Sized to fit a 3-wide column (see AUTHOR_W):
/// - one word          -> its first three letters, original case
///                        ("Gaurgle" -> "Gau", "torvalds" -> "tor")
/// - two or more words  -> the first two words' initials, uppercased
///                        ("Andreas Roos" -> "AR", "Lars Nils Andreas" -> "LN",
///                         "Maria-Hazell" -> "MH")
///
/// Words split on whitespace and '-'. The caller left-pads the result to
/// AUTHOR_W, so two-initial codes render as "AR " in the 3-wide field.
/// Falls back to "?" for an empty/degenerate name.
pub fn author_code(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|w| !w.is_empty())
        .collect();

    match words.as_slice() {
        [] => "?".to_string(),
        [single] => single.chars().take(3).collect(),
        [first, second, ..] => {
            let initials: String = first
                .chars()
                .take(1)
                .chain(second.chars().take(1))
                .collect();
            initials.to_uppercase()
        }
    }
}

/// Fixed display width of the author column in commit/history lists (fits a
/// single-word code like "Gau"; two-initial codes are left-padded into it).
pub const AUTHOR_W: usize = 3;

/// Truncate `s` to at most `budget` chars, replacing the tail with `…` when
/// it doesn't fit, rather than letting it silently overflow into whatever
/// renders next to it.
pub fn truncate(s: &str, budget: usize) -> String {
    if s.chars().count() <= budget {
        return s.to_string();
    }
    if budget == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(budget.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A thin divider rule, the width of `width`, separating the filter strip
/// from the panel content below it.
pub fn divider_line(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(width),
        Style::default().fg(colors::SURFACE),
    ))
}

/// A numbered panel title for a bordered pane, like `[1] repos`. The number
/// is the key that focuses this panel; the whole title goes bold+lavender
/// when the panel is focused and muted otherwise, so the active pane reads at
/// a glance (matching lazygit's numbered panels).
pub fn panel_title(num: u8, label: &str, focused: bool) -> Line<'static> {
    let (num_color, label_style) = if focused {
        (
            colors::LAVENDER,
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (colors::SURFACE, Style::default().fg(colors::OVERLAY))
    };
    Line::from(vec![
        Span::styled(format!(" [{num}] "), Style::default().fg(num_color)),
        Span::styled(format!("{label} "), label_style),
    ])
    .left_aligned()
}

/// Top-level render: header + main view + footer + optional overlay. Search
/// is not a chunk of its own -- it lives inline in the repo panel's filter
/// strip (see `filter_strip_line`), the way todoz folds its filter input
/// into the list header.
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let chunks = Layout::vertical([
        Constraint::Length(1), // header (a single divider line, tags embedded)
        Constraint::Min(1),    // main content
        Constraint::Length(2), // footer
    ])
    .split(area);

    // Header
    render_header(frame, chunks[0], app);

    // Main content area
    match app.view {
        View::Detail => detail::render(frame, chunks[1], app),
        View::Compact => compact::render(frame, chunks[1], app),
    }

    // Footer
    footer::render(frame, chunks[2], app);

    // Overlay (rendered last, on top)
    match &app.overlay {
        Overlay::Help => help::render(frame, area),
        Overlay::Branches(bo) => branches::render(frame, area, bo),
        Overlay::None => {}
    }
}

/// Render the header. The left (name/header/group) and right (refresh time/
/// view hint/help) titles are positioned independently by ratatui with no
/// mutual awareness, so on a narrow pane they can literally overlap
/// character-for-character. To avoid that, progressively drop the least
/// important piece from each side -- refresh timestamp first, then the view
/// hint, then the group tag, then the header description -- until both sides
/// fit side by side.
fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width as usize;

    let name = Span::styled(
        " fleetz",
        Style::default()
            .fg(colors::LAVENDER)
            .add_modifier(Modifier::BOLD),
    );
    let header_span = (!app.header.is_empty()).then(|| {
        Span::styled(
            format!(" — {}", app.header),
            Style::default().fg(colors::OVERLAY),
        )
    });
    let group_span = app.active_group_label().map(|group| {
        Span::styled(format!("  [{group}]"), Style::default().fg(colors::MAUVE))
    });

    let view_hint = match app.view {
        View::Detail => "[v] compact",
        View::Compact => "[v] detail",
    };
    let ts_span = app.last_refreshed.map(|ts| {
        Span::styled(
            format!(" refreshed {}  ", ts.format("%H:%M")),
            Style::default().fg(colors::SURFACE),
        )
    });
    let view_span = Span::styled(view_hint, Style::default().fg(colors::SURFACE));
    let help_span = Span::styled("[?] help ", Style::default().fg(colors::SURFACE));

    let mut include_ts = true;
    let mut include_view = true;
    let mut include_group = true;
    let mut include_header = true;

    loop {
        let mut left: Vec<Span> = vec![name.clone()];
        if include_header {
            if let Some(ref h) = header_span {
                left.push(h.clone());
            }
        }
        if include_group {
            if let Some(ref g) = group_span {
                left.push(g.clone());
            }
        }

        let mut right: Vec<Span> = Vec::new();
        if include_ts {
            if let Some(ref t) = ts_span {
                right.push(t.clone());
            } else {
                right.push(Span::raw(" "));
            }
        }
        if include_view {
            right.push(view_span.clone());
            right.push(Span::raw("  "));
        }
        right.push(help_span.clone());

        let left_w: usize = left.iter().map(|s| s.content.len()).sum();
        let right_w: usize = right.iter().map(|s| s.content.len()).sum();
        let exhausted = !include_ts && !include_view && !include_group && !include_header;

        if left_w + right_w < width || exhausted {
            let block = Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(colors::SURFACE))
                .title_bottom(Line::from(left).left_aligned())
                .title_bottom(Line::from(right).right_aligned());
            frame.render_widget(block, area);
            return;
        }

        if include_ts {
            include_ts = false;
        } else if include_view {
            include_view = false;
        } else if include_group {
            include_group = false;
        } else {
            include_header = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::author_code;

    #[test]
    fn one_word_uses_first_three_letters_original_case() {
        assert_eq!(author_code("Gaurgle"), "Gau");
        assert_eq!(author_code("torvalds"), "tor");
    }

    #[test]
    fn two_words_use_uppercase_initials() {
        assert_eq!(author_code("Andreas Roos"), "AR");
    }

    #[test]
    fn hyphenated_counts_as_two_words() {
        assert_eq!(author_code("Maria-Hazell"), "MH");
    }

    #[test]
    fn three_or_more_words_use_first_two_initials() {
        assert_eq!(author_code("Lars Nils Andreas"), "LN");
        assert_eq!(author_code("Jean Luc Picard"), "JL");
    }

    #[test]
    fn short_single_word_does_not_panic() {
        assert_eq!(author_code("x"), "x");
        assert_eq!(author_code("Bo"), "Bo");
    }

    #[test]
    fn empty_name_falls_back() {
        assert_eq!(author_code(""), "?");
        assert_eq!(author_code("   "), "?");
    }
}
