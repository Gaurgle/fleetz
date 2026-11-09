use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding, Paragraph};

use super::layout::{REPO_LIST_MAX_COLS, split_summary_box};
use super::{fit_spans, span_width, wash};
use crate::app::{App, FocusedPanel, ListRow};
use crate::model::RepoStatus;
use crate::ui::badges;
use crate::ui::colors;

pub(super) fn render_repo_list(frame: &mut Frame, area: Rect, app: &App) {
    let mut items: Vec<ListItem> = Vec::new();
    // Width available for a row inside the block: the pane minus its border (2)
    // and horizontal padding (2, from `Padding::horizontal(1)` on the block).
    let col_width = area.width.saturating_sub(4) as usize;

    let dim_pending = app.watch_mode && !app.blink_on;

    // The rows come from `App::list_rows`, the list the cursor walks and
    // clicks resolve against, so the three cannot disagree about what sits
    // where. While searching it is one ranked set under "results"; otherwise
    // the sections (github activity, local changes, also active), each headed
    // once another is present, which `App::section_rank` orders the cursor to
    // match. The cursor's row, a section header included (FZ-45), is told to
    // ListState so that exact row is kept in view.
    let visible = app.visible_repos();
    let total = visible.len();
    let rows_drawn = app.list_rows();
    let cursor = app.cursor_row(&rows_drawn);
    for (i, row) in rows_drawn.iter().enumerate() {
        let on_cursor = cursor == Some(i);
        items.push(match *row {
            ListRow::Results => section_header_item("results"),
            ListRow::Gap => ListItem::new(Line::from("")),
            ListRow::Header(section) if on_cursor => selected_header_item(section.label()),
            ListRow::Header(section) => section_header_item(section.label()),
            ListRow::Repo(pos) => {
                let repo = visible[pos].1;
                let shown = app.ci_shown(repo);
                let ci = badges::RowCi {
                    status: &shown.status,
                    side_run: app.ci_side_run(repo),
                };
                ListItem::new(Line::from(repo_row_spans_ci(
                    repo,
                    ci,
                    on_cursor,
                    col_width,
                    dim_pending,
                    app.is_not_in_last_poll(repo),
                    app.search_match_field(repo).and_then(|f| f.mark()),
                )))
            }
        });
    }
    let selected_display_idx = cursor;

    let focused = app.focused_panel == FocusedPanel::RepoList;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let mut title = crate::ui::panel_title(1, "repos", focused);
    // The time window on this border, between the title and the count
    // (FZ-95, Andreas 2026-11-09).
    let count_w = format!(" {total} ").chars().count();
    let budget = (area.width as usize).saturating_sub(2 + span_width(&title.spans) + count_w + 2);
    title.spans.extend(list_title_window(app, budget));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1))
        .title_top(title)
        .title_top(
            Line::from(Span::styled(
                format!(" {total} "),
                Style::default().fg(colors::OVERLAY),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Filter strip + divider, then the repo list, then the fleet's summary
    // box at the foot (FZ-95).
    let summary = summary_box_lines(app, inner.width as usize);
    let (inner, summary_area) = split_summary_box(inner, summary.len());
    if let Some(box_area) = summary_area {
        let mut lines = vec![crate::ui::divider_line(box_area.width as usize)];
        lines.extend(summary);
        frame.render_widget(Paragraph::new(lines), box_area);
        // The footer, drawn after, leaves the summary and the message out.
        app.fleet_drawn.set(true);
    }
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);
    frame.render_widget(Paragraph::new(crate::ui::filter_strip_line(app)), rows[0]);
    frame.render_widget(
        Paragraph::new(crate::ui::divider_line(rows[1].width as usize)),
        rows[1],
    );

    // A fresh ListState each frame is fine -- ratatui's List recomputes the
    // scroll offset from `.select()` on every render, so the selected row is
    // always brought into view without needing to persist offset ourselves.
    // The padding keeps the pane 2 margin past the cursor, shrunk by ratatui
    // in a short pane as `scrolloff_for` shrinks it (FZ-55).
    let list = List::new(items).scroll_padding(usize::from(crate::app::SCROLLOFF));
    let mut state = ListState::default();
    state.select(selected_display_idx);
    frame.render_stateful_widget(list, rows[2], &mut state);
    // Where the rows landed, for clicks and hovers (FZ-45).
    app.list_hits.set(crate::app::ListHits {
        area: rows[2],
        offset: state.offset(),
    });
}

/// The time window on pane 1's top border (FZ-95), `· ◷ since 1 week [t] `,
/// in the short form (`· ◷ 1w [t] `) where `budget` columns cannot hold
/// it, and nothing where they cannot hold that either. Before a load names
/// the window, `· ◷ [t] `. Pane 2's sections never read this width.
fn list_title_window(app: &App, budget: usize) -> Vec<Span<'static>> {
    let key = Style::default()
        .fg(colors::TEXT)
        .add_modifier(Modifier::BOLD);
    let label = Style::default()
        .fg(colors::TEAL)
        .add_modifier(Modifier::BOLD);
    [window_label(app), window_short_label(&app.header)]
        .into_iter()
        .map(|text| {
            vec![
                Span::styled("· ", Style::default().fg(colors::SURFACE)),
                Span::styled(
                    match text.is_empty() {
                        true => WINDOW_MARK.to_string(),
                        false => format!("{WINDOW_MARK} {text}"),
                    },
                    label,
                ),
                Span::raw(" "),
                Span::styled("[t]", key),
                Span::raw(" "),
            ]
        })
        .find(|spans| span_width(spans) <= budget)
        .unwrap_or_default()
}

/// The most rows the summary takes in words before it switches to glyphs
/// and counts (Andreas, 2026-11-06: "when wide screen: write it out ...
/// but when narrow: 2!, 2PRs").
const SUMMARY_WORD_ROWS: usize = 2;

/// Pane 1's summary box (FZ-95), under its divider: the active group
/// (`[work]`) and the fleet's counters, wrapped at the pane's width between
/// counters, in words (`3↓ · 1! · 17 synced · 2 not cloned · 8 PRs`) while
/// they fit `SUMMARY_WORD_ROWS` rows and as glyphs and counts
/// (`3↓ 1! 17✓ 2- 8 PRs`) where they do not. Then the activity row: a
/// manual refresh's spinner and the last action's message, blank when
/// there is none. The activity row is always there, so a message moves
/// nothing.
fn summary_box_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let p = crate::ui::fleet_pieces(app, chrono::Local::now());
    let group: Vec<Vec<Span<'static>>> = p.group.into_iter().map(|g| vec![g]).collect();
    let mut words: Vec<Span<'static>> = Vec::new();
    for g in group.iter().flatten() {
        words.push(g.clone());
        words.push(Span::raw(" · "));
    }
    words.extend(p.summary);
    let mut lines = wrap_counters(words, width);
    if lines.len() > SUMMARY_WORD_ROWS {
        let mut glyphs = group;
        glyphs.extend(glyph_counters(&app.summary()));
        lines = wrap_groups(glyphs, Span::raw(" "), width);
    }
    if lines.is_empty() {
        lines.push(Line::from(""));
    }
    let mut activity: Vec<Span<'static>> = Vec::new();
    if let Some(spinner) = p.refreshing {
        activity.extend(
            spinner
                .into_iter()
                .map(|s| Span::styled(s.content.trim().to_string(), s.style)),
        );
        activity.push(Span::raw("  "));
    }
    if let Some(message) = p.message {
        activity.push(message);
    }
    lines.push(Line::from(fit_spans(activity, width)));
    lines
}

/// The summary's counters wrapped at `width`, breaking only at the ` · `
/// between two counters, so a counter is never split.
fn wrap_counters(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    let dot = Span::styled(" · ", Style::default().fg(colors::SURFACE));
    wrap_groups(counter_groups(spans), dot, width)
}

/// The summary's spans split into its counters at the ` · ` between them.
fn counter_groups(spans: Vec<Span<'static>>) -> Vec<Vec<Span<'static>>> {
    let mut groups: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    for span in spans {
        if span.content.trim() == "·" {
            groups.push(Vec::new());
        } else if let Some(group) = groups.last_mut() {
            group.push(span);
        }
    }
    groups.retain(|g| !g.is_empty());
    groups
}

/// `groups` laid out at `width`, `sep` between two on a row, a new row
/// where the next does not fit; a group wider than a row is cut with `…`.
fn wrap_groups(
    groups: Vec<Vec<Span<'static>>>,
    sep: Span<'static>,
    width: usize,
) -> Vec<Line<'static>> {
    let sep_w = sep.content.chars().count();
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    for group in groups {
        match lines.last_mut() {
            Some(line) if span_width(line) + sep_w + span_width(&group) <= width => {
                line.push(sep.clone());
                line.extend(group);
            }
            _ => lines.push(fit_spans(group, width)),
        }
    }
    lines.into_iter().map(Line::from).collect()
}

/// The summary's narrow form (Andreas, 2026-11-06): every counter a count
/// and a glyph, the count first as the list's counters write it (`3↓ 1!`),
/// synced a green `✓`, not cloned a grey `-`, the PRs kept in words. From
/// the one table the words form reads (`footer::summary_counters`).
fn glyph_counters(s: &crate::model::Summary) -> Vec<Vec<Span<'static>>> {
    crate::ui::footer::summary_counters(s)
        .into_iter()
        .map(|c| vec![Span::styled(c.glyphs, Style::default().fg(c.color))])
        .collect()
}

/// A section header the list cursor stands on (FZ-45): the marker repo rows
/// wear, and the label lit, so the stop reads as the cursor's.
fn selected_header_item(label: &str) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::raw("▸ "),
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(colors::MAUVE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ──", Style::default().fg(colors::SURFACE)),
    ]))
}

/// A section divider row inside the repo list, matching repoz's
/// `section_div` bash helper: "── label ──" in the dim/mauve pairing.
fn section_header_item(label: &str) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled("── ", Style::default().fg(colors::SURFACE)),
        Span::styled(label.to_string(), Style::default().fg(colors::MAUVE)),
        Span::styled(" ──", Style::default().fg(colors::SURFACE)),
    ]))
}

/// Widest a repo row is drawn: the repo list's column cap less its two
/// borders and two padding columns. The slot digest (FZ-45) draws its rows'
/// shared columns at this width, as pane 1 does at its default size.
pub const REPO_ROW_MAX_W: usize = REPO_LIST_MAX_COLS as usize - 4;

/// A repo row's spans, `col_width` wide: marker, kind glyph, name, dot fill
/// and the badge group flush right. The one grammar a repo row has: the repo
/// list draws it, and so do the slot digest's lines (FZ-45), so every
/// indicator keeps its place and its colour in both. Drawn with the fleet
/// poll's lamp; the screens draw `repo_row_spans_ci` (FZ-83), so this one
/// is kept for the tests.
#[cfg(test)]
pub fn repo_row_spans(
    repo: &RepoStatus,
    is_selected: bool,
    col_width: usize,
    dim_pending: bool,
    not_in_last_poll: bool,
    match_mark: Option<&'static str>,
) -> Vec<Span<'static>> {
    let ci = badges::RowCi {
        status: &repo.ci_status,
        side_run: false,
    };
    repo_row_spans_ci(
        repo,
        ci,
        is_selected,
        col_width,
        dim_pending,
        not_in_last_poll,
        match_mark,
    )
}

/// `repo_row_spans` with the CI part as `ci` says (FZ-83): the repo list
/// passes `App::ci_shown` and `App::ci_side_run`.
pub fn repo_row_spans_ci(
    repo: &RepoStatus,
    ci: badges::RowCi,
    is_selected: bool,
    col_width: usize,
    dim_pending: bool,
    not_in_last_poll: bool,
    match_mark: Option<&'static str>,
) -> Vec<Span<'static>> {
    let marker = if is_selected { "▸ " } else { "  " };
    let name_style = if is_selected {
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::SAPPHIRE)
    };

    // Badges are built tight and right-aligned flush to the inner edge, with
    // a dot-fill running from the name out to them (repoz style -- see
    // ui::badges::badge_spans). The marker is a fixed display width, so the
    // badges land at the same right edge whether or not the row is selected;
    // only differing badge sets between rows shift a status token, exactly
    // as in repoz. Truncate the name rather than let it overflow when narrow.
    let (badge_spans, badge_w) = badges::badge_spans_ci(repo, ci, dim_pending, not_in_last_poll);
    let right_len = badge_w + 1; // +1 for space after dots
    // A search hit on the owner or an author says so after the name (FZ-56),
    // in the filter strip's dim, so it does not read as a name match.
    let mark = match_mark.map(|m| format!(" {m}")).unwrap_or_default();
    let mark_w = mark.chars().count();
    let reserved = badges::MARKER_W + badges::GLYPH_W + 1 + right_len + mark_w;
    let name_budget = col_width.saturating_sub(reserved).max(3);
    let name = crate::ui::truncate(&repo.name, name_budget);

    let left_len = badges::MARKER_W + badges::GLYPH_W + name.chars().count() + mark_w + 1; // +1 for space before dots
    let dot_count = col_width.saturating_sub(left_len + right_len).max(1);
    let dots = "·".repeat(dot_count);

    let mut spans = vec![
        Span::raw(marker.to_string()),
        badges::glyph_span(repo),
        Span::styled(name, name_style),
    ];
    if !mark.is_empty() {
        spans.push(Span::styled(mark, Style::default().fg(colors::OVERLAY)));
    }
    // The cursor's row wears the history lists' band (FZ-66). The dots'
    // resting colour, `SURFACE`, is lighter than the band, so they step up to
    // `OVERLAY` and bold to stay seen; `·` is one cell either way, so no
    // column moves.
    let dot_style = if is_selected {
        Style::default()
            .fg(colors::OVERLAY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::SURFACE)
    };
    spans.push(Span::styled(format!(" {dots} "), dot_style));
    spans.extend(badge_spans);
    if is_selected { wash(spans) } else { spans }
}

/// The time window's mark on pane 1's border.
const WINDOW_MARK: &str = "◷";

/// The window's label as pane 1's border draws it: the header's own words,
/// empty before a load names it.
fn window_label(app: &App) -> String {
    app.header.clone()
}

/// The window's short form for a narrow border (FZ-95): `since 4 weeks` as
/// `4w`, `since 2 hours` as `2h`; a slot window by its slot (`Work`);
/// anything else cut to `SHORT_WINDOW_MAX` columns with `…`. A coverage
/// note after the label is dropped with the rest.
fn window_short_label(header: &str) -> String {
    let words: Vec<&str> = header.split_whitespace().collect();
    if let ["since", n, unit, ..] = words.as_slice()
        && n.parse::<u32>().is_ok()
    {
        let short = match unit.trim_end_matches('s') {
            "hour" => Some("h"),
            "day" => Some("d"),
            "week" => Some("w"),
            "month" => Some("mo"),
            "year" => Some("y"),
            _ => None,
        };
        if let Some(short) = short {
            return format!("{n}{short}");
        }
    }
    if let Some(slot) = words
        .iter()
        .find(|w| matches!(**w, "Work" | "Evening" | "Night"))
    {
        return (*slot).to_string();
    }
    let spans = fit_spans(vec![Span::raw(header.to_string())], SHORT_WINDOW_MAX);
    spans.iter().map(|s| s.content.as_ref()).collect()
}

/// The widest a short window label is drawn.
const SHORT_WINDOW_MAX: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RepoKind, RepoLocation};
    use crate::ui::detail::tests::{
        app_with_a_stale_row, cloned_repo, draw_people_search, frame_rows, header_block, repo,
        two_repo_app,
    };
    use ratatui::style::Color as TermColor;

    /// FZ-95: the summary and the activity live in pane 1's box. A message
    /// or a manual refresh lands on the activity row and moves neither the
    /// summary rows nor anything in pane 2's header (his look, 2026-11-05:
    /// "keep their places static").
    #[test]
    fn a_message_or_a_refresh_moves_nothing() {
        let box_rows = |setup: &dyn Fn(&mut crate::app::App)| {
            let mut app =
                crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), 140, 40);
            setup(&mut app);
            summary_box_lines(&app, 40)
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
        };
        let quiet = |_: &mut crate::app::App| {};
        let message =
            |app: &mut crate::app::App| app.set_status("copied 1abc123".to_string(), false);
        let refresh = |app: &mut crate::app::App| {
            app.sweep_started = Some(std::time::Instant::now());
            app.sweep_manual = true;
            app.refresh_awaits_poll = true;
        };
        let idle = box_rows(&quiet);
        let busy_message = box_rows(&message);
        let busy_refresh = box_rows(&refresh);
        let last = idle.len() - 1;
        assert!(
            idle[last].trim().is_empty(),
            "activity row not blank: {idle:?}"
        );
        assert!(
            busy_message[last].contains("copied 1abc123"),
            "{busy_message:?}"
        );
        assert!(busy_refresh[last].contains("checking"), "{busy_refresh:?}");
        for busy in [&busy_message, &busy_refresh] {
            assert_eq!(busy.len(), idle.len(), "the box grew");
            assert_eq!(busy[..last], idle[..last], "a summary row moved");
        }
        assert_eq!(header_block(140, message), header_block(140, |_| {}));
        assert_eq!(header_block(140, refresh), header_block(140, |_| {}));
    }

    /// FZ-95: the summary wraps between counters at pane 1's width, never
    /// inside one, and the box keeps its activity row.
    #[test]
    fn the_summary_wraps_between_counters() {
        let spans = |text: &str| -> Vec<Span<'static>> {
            let mut out = Vec::new();
            for (i, piece) in text.split(" · ").enumerate() {
                if i > 0 {
                    out.push(Span::raw(" · "));
                }
                out.push(Span::raw(piece.to_string()));
            }
            out
        };
        let summary = "3↓ · 1! · 17 synced · 2 not cloned · 8 PRs";
        let one = wrap_counters(spans(summary), 60);
        assert_eq!(one.len(), 1);
        let two: Vec<String> = wrap_counters(spans(summary), 24)
            .iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(two, vec!["3↓ · 1! · 17 synced", "2 not cloned · 8 PRs"]);
    }

    /// FZ-95: the window's short form on a narrow pane 1 border, and
    /// nothing where the border cannot hold that either.
    #[test]
    fn the_border_window_has_a_short_form() {
        for (header, want) in [
            ("since 4 weeks", "4w"),
            ("since 1 week", "1w"),
            ("since 2 hours (50 of 95 repos)", "2h"),
            ("since 3 days", "3d"),
            ("since 1 month", "1mo"),
            ("since 2 years", "2y"),
            ("since 2026-11-09  Work (09-17)", "Work"),
        ] {
            assert_eq!(window_short_label(header), want, "{header}");
        }
        let odd = window_short_label("something else entirely");
        assert_eq!(odd.chars().count(), SHORT_WINDOW_MAX, "{odd}");
        assert!(odd.ends_with('…'), "{odd}");

        let app = crate::app::App::new(Vec::new(), "since 4 weeks".to_string(), 80, 40);
        let drawn = |budget: usize| Line::from(list_title_window(&app, budget)).to_string();
        assert_eq!(drawn(80), "· ◷ since 4 weeks [t] ");
        assert_eq!(drawn(21), "· ◷ 4w [t] ");
        assert_eq!(drawn(10), "");
    }

    /// FZ-95 review 3: before a load names the window, the border draws
    /// `◷ [t]` with one space between them, not two.
    #[test]
    fn before_a_load_the_border_window_has_one_space() {
        let app = crate::app::App::new(Vec::new(), String::new(), 80, 40);
        let drawn = Line::from(list_title_window(&app, 80)).to_string();
        assert_eq!(drawn, "· ◷ [t] ");
    }

    /// FZ-95 review 3: the glyph form draws every counter in the colour the
    /// words form draws it in, in the same order, from one table.
    #[test]
    fn the_glyph_and_word_summaries_share_one_colour_table() {
        let summary = crate::model::Summary {
            behind_count: 3,
            ahead_count: 2,
            dirty_count: 1,
            synced_count: 17,
            not_cloned_count: 4,
            pr_count: 8,
        };
        let words: Vec<_> = crate::ui::footer::summary_spans(&summary)
            .into_iter()
            .filter(|s| s.content.trim() != "·")
            .map(|s| s.style.fg)
            .collect();
        let glyphs: Vec<_> = glyph_counters(&summary)
            .into_iter()
            .flatten()
            .map(|s| s.style.fg)
            .collect();
        assert_eq!(words.len(), 6, "{words:?}");
        assert_eq!(glyphs, words);
    }

    /// FZ-95: the summary in words where they fit two rows, as glyphs and
    /// counts where they do not (Andreas, 2026-11-06), and the active group
    /// first (review 1, B2).
    #[test]
    fn the_summary_is_words_when_wide_and_glyphs_when_narrow() {
        let mut behind = cloned_repo();
        behind.name = "behind".to_string();
        behind.behind = 3;
        let mut dirty = cloned_repo();
        dirty.name = "dirty".to_string();
        dirty.dirty = 2;
        let mut absent = repo("absent", RepoKind::Public, Some("Gaurgle"));
        absent.path = None;
        absent.location = RepoLocation::NotCloned;
        let mut app = crate::app::App::new(
            vec![cloned_repo(), behind, dirty, absent],
            "since 2 weeks".to_string(),
            160,
            40,
        );
        let text = |app: &crate::app::App, w: usize| -> Vec<String> {
            let mut lines: Vec<String> = summary_box_lines(app, w)
                .iter()
                .map(|l| l.to_string())
                .collect();
            lines.pop(); // the activity row
            lines
        };
        let wide = text(&app, 60).join(" ");
        assert!(wide.contains("1 synced"), "{wide}");
        assert!(wide.contains("1 not cloned"), "{wide}");
        let narrow = text(&app, 8);
        let joined = narrow.join(" ");
        assert!(!joined.contains("synced"), "{narrow:?}");
        // Repos behind and dirty, counted as the words form counts them.
        assert!(joined.contains("1↓") && joined.contains("1!"), "{narrow:?}");
        assert!(joined.contains("1✓") && joined.contains("1-"), "{narrow:?}");
        let spans: Vec<Span> = summary_box_lines(&app, 8)
            .into_iter()
            .flat_map(|l| l.spans)
            .collect();
        let fg = |text: &str| {
            spans
                .iter()
                .find(|s| s.content == text)
                .and_then(|s| s.style.fg)
        };
        assert_eq!(fg("1✓"), Some(colors::GREEN));
        assert_eq!(fg("1-"), Some(colors::OVERLAY));

        app.group_names = vec!["work".to_string()];
        app.active_group = Some(0);
        // No repo is in the group, so its summary is empty: the group alone.
        let grouped = text(&app, 60);
        assert_eq!(grouped, vec!["[work]".to_string()]);
    }

    /// FZ-95 review 1, B3: the summary box takes exactly its own rows off
    /// the list, under a divider, and a click on it lands on no list row.
    #[test]
    fn the_summary_box_takes_exactly_its_rows_off_the_list() {
        let mut app =
            crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), 120, 50);
        app.view = crate::app::View::Detail;
        let (rows, _) = frame_rows(&app, 120, 50);
        let pane1 = crate::ui::panel_rects(Rect::new(0, 0, 120, 50), &app).repo_list;
        let inner = Rect::new(pane1.x + 2, pane1.y + 1, pane1.width - 4, pane1.height - 2);
        let box_h = summary_box_lines(&app, inner.width as usize).len() as u16 + 1;
        let list = app.list_hits.get();
        assert_eq!(list.area.y, inner.y + 2, "under the strip and its divider");
        assert_eq!(list.area.bottom() + box_h, inner.bottom(), "{list:?}");
        let divider: String = rows[usize::from(list.area.bottom())]
            .chars()
            .skip(usize::from(inner.x))
            .take(usize::from(inner.width))
            .collect();
        assert!(divider.chars().all(|c| c == '─'), "{divider:?}");
        assert_eq!(list.at(inner.x, list.area.bottom()), None);
        assert!(list.at(inner.x, list.area.bottom() - 1).is_some());
    }

    /// The invariant CLAUDE.md names: the cursor walks `App::visible_repos`
    /// while the eye walks what `render_repo_list` draws. Nothing checked
    /// that the two agreed about section order, so reordering one and
    /// forgetting the other misrouted every click below the first section
    /// with nothing failing. This reads the drawn headers back off the
    /// buffer and holds them against the order the cursor uses.
    #[test]
    fn drawn_section_order_matches_the_order_the_cursor_walks() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut repos = vec![
            repo("elsewhere-one", RepoKind::Public, None),
            repo("local-one", RepoKind::Public, None),
            repo("primary-one", RepoKind::Public, None),
        ];
        repos[0].location = RepoLocation::Elsewhere;
        repos[1].location = RepoLocation::LocalOnly;
        repos[2].location = RepoLocation::Primary;

        let app = crate::app::App::new(repos, "t".to_string(), 120, 40);

        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let screen: Vec<String> = (0..40u16)
            .map(|y| (0..120u16).map(|x| buf[(x, y)].symbol()).collect())
            .collect();

        // Headers as drawn, top to bottom.
        let drawn: Vec<&str> = screen
            .iter()
            .filter_map(|row| {
                ["github activity", "local changes", "also active"]
                    .into_iter()
                    .find(|label| row.contains(label))
            })
            .collect();
        assert_eq!(
            drawn,
            vec!["github activity", "local changes", "also active"],
            "drew: {screen:#?}"
        );

        // And the same order out of the cursor's own list, so the two cannot
        // drift apart without this failing.
        let walked: Vec<&str> = app
            .visible_repos()
            .iter()
            .map(|(_, r)| match r.location {
                RepoLocation::Primary => "github activity",
                RepoLocation::LocalOnly => "local changes",
                RepoLocation::Elsewhere | RepoLocation::NotCloned => "also active",
            })
            .collect();
        assert_eq!(walked, drawn, "cursor order disagrees with drawn order");
    }

    /// A hit on the owner, organisation or an author is marked after the
    /// name, dimmed; a name hit carries no mark.
    #[test]
    fn a_non_name_search_hit_is_marked_with_its_field() {
        let row = |screen: &[String], name: &str| {
            screen
                .iter()
                .find(|row| row.contains(name))
                .cloned()
                .unwrap_or_else(|| panic!("{name} not drawn: {screen:#?}"))
        };
        let (_, screen) = draw_people_search("kestrel");
        assert!(
            row(&screen, "tools-kestrel").contains("tools-kestrel ·"),
            "a name hit is unmarked"
        );
        assert!(row(&screen, "tools-web").contains("tools-web org ·"));
        assert!(row(&screen, "tools-scratch").contains("tools-scratch author ·"));

        let (_, screen) = draw_people_search("gaurgle");
        assert!(row(&screen, "tools-kestrel").contains("tools-kestrel owner ·"));

        // Dimmed in the strip's own grey, not a new color.
        let (app, _) = draw_people_search("gaurgle");
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let y = (0..40u16)
            .find(|&y| {
                (0..120u16)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .contains("owner")
            })
            .unwrap();
        let line: String = (0..120u16).map(|x| buf[(x, y)].symbol()).collect();
        let x = line.find("owner").unwrap();
        let x = line[..x].chars().count() as u16;
        assert_eq!(buf[(x, y)].fg, colors::OVERLAY);
    }

    /// The colour of the CI lamp and of the PR count on the list row naming
    /// `name`, as drawn.
    fn list_colors(app: &crate::app::App, name: &str) -> (TermColor, TermColor) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), app)).unwrap();
        let buf = term.backend().buffer().clone();
        let y = (0..12u16)
            .find(|&y| {
                let row: String = (0..60u16).map(|x| buf[(x, y)].symbol()).collect();
                row.contains(&format!(" {name} "))
            })
            .expect("row drawn");
        let at = |symbol: &str| {
            (0..60u16)
                .find(|&x| buf[(x, y)].symbol() == symbol)
                .map(|x| buf[(x, y)].fg)
                .expect("cell drawn")
        };
        (at("◉"), at("P"))
    }

    #[test]
    fn a_row_not_in_the_last_poll_is_drawn_dimmed_and_its_lamp_holds_still() {
        let mut app = app_with_a_stale_row();
        for blink_on in [true, false] {
            app.blink_on = blink_on;
            assert_eq!(
                list_colors(&app, "stale"),
                (badges::EARLIER_REFRESH, badges::EARLIER_REFRESH),
                "blink_on {blink_on}"
            );
        }
    }

    #[test]
    fn a_row_in_the_last_poll_is_drawn_as_before() {
        let mut app = app_with_a_stale_row();
        app.blink_on = true;
        assert_eq!(list_colors(&app, "fresh"), (colors::PEACH, colors::PR_CLR));
        app.blink_on = false;
        assert_eq!(
            list_colors(&app, "fresh"),
            (colors::SURFACE, colors::PR_CLR)
        );
    }

    #[test]
    fn the_cursor_row_wears_the_band_and_bold_dots_and_others_do_not() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = two_repo_app();
        let (w, h) = (60u16, 12u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_repo_list(f, f.area(), &app)).unwrap();
        let buf = term.backend().buffer().clone();
        // Inside the border and the one column of padding on each side.
        let inner = 2..w - 2;
        let row_of = |name: &str| {
            (0..h)
                .find(|&y| {
                    let row: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
                    row.contains(&format!(" {name} "))
                })
                .expect("row drawn")
        };
        let (a, b) = (row_of("alpha"), row_of("beta"));
        let (on, off) = if buf[(2, a)].symbol() == "▸" {
            (a, b)
        } else {
            (b, a)
        };
        assert_eq!(buf[(2, on)].symbol(), "▸", "one row is the cursor's");
        assert_eq!(buf[(2, off)].symbol(), " ");

        for x in inner.clone() {
            assert_eq!(buf[(x, on)].bg, colors::SELECTED_BG, "band at x {x}");
            assert_eq!(buf[(x, off)].bg, TermColor::Reset, "no band at x {x}");
        }
        let dots = |y: u16| -> Vec<u16> {
            inner
                .clone()
                .filter(|&x| buf[(x, y)].symbol() == "·")
                .collect()
        };
        assert!(!dots(on).is_empty() && !dots(off).is_empty());
        for x in dots(on) {
            let cell = &buf[(x, on)];
            assert_eq!(cell.fg, colors::OVERLAY, "cursor dot at x {x}");
            assert!(cell.modifier.contains(Modifier::BOLD), "bold at x {x}");
        }
        for x in dots(off) {
            let cell = &buf[(x, off)];
            assert_eq!(cell.fg, colors::SURFACE, "dot at x {x}");
            assert!(!cell.modifier.contains(Modifier::BOLD), "plain at x {x}");
        }
    }
}
