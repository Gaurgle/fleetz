use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Padding, Paragraph};

use crate::app::{App, ContributionsMode, FocusedPanel, HistoryView, Overlay, View};
use crate::model::{NOT_CLONED, Summary};
use crate::ui::colors;

/// Render the footer: keybind hints and nothing else.
///
/// The hints open with the focused pane's keys (`focus_hints`). When tight,
/// the global keys drop their descriptions, then the pane's keys do, then
/// the pane's keys go.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // An in-flight window apply takes over the whole footer with a gauge,
    // so the user sees real-time progress instead of a frozen list.
    if let Some(progress) = app.apply_progress {
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(colors::SURFACE))
                    // Inset to match the summary it replaces, so the footer
                    // does not jump two columns wider mid-apply.
                    .padding(Padding::horizontal(crate::ui::PANE_CONTENT_INSET)),
            )
            .gauge_style(Style::default().fg(colors::GREEN))
            .ratio(progress.ratio())
            .label(progress.label());
        frame.render_widget(gauge, area);
        return;
    }

    // The fit math below is about the *padded* width, not the footer's full
    // rect: the block insets the content on both sides so the keys line up
    // with the pane content above them rather than starting two columns
    // further left, hard against the terminal edge.
    let width = area.width.saturating_sub(2 * crate::ui::PANE_CONTENT_INSET) as usize;

    // The fleet's summary and activity live in pane 1's summary box (FZ-95)
    // and on the compact view's status line. Where this frame drew neither
    // (a pane 1 too short for the box), the footer carries
    // it, as it always did: a manual refresh's spinner, then the last
    // action's message or the summary. Otherwise the keys have the row.
    let left = if crate::ui::uses_status_line(app) || app.fleet_drawn.get() {
        Vec::new()
    } else {
        fallback_spans(app)
    };

    // Widest first (Andreas, 2026-10-17): the global row's words go before
    // the segment's, since they are the same every day and the segment is
    // the news; the segment goes before the left side, which outranks it.
    let with_segment = [
        hints_with_segment(app, true, true),
        hints_with_segment(app, true, false),
        hints_with_segment(app, false, false),
    ];
    let compact_hints = keybind_hints(app, false);
    let (mut spans, hints) = if let Some(hints) = with_segment
        .into_iter()
        .find(|hints| fits(width, &left, hints))
    {
        (left, hints)
    } else if fits(width, &left, &compact_hints) {
        (left, compact_hints)
    } else {
        (Vec::new(), compact_hints)
    };

    // Right-aligned. Measured in columns, so the row ends flush with the
    // content's right edge.
    let spacer_width = width.saturating_sub(line_width(&spans) + line_width(&hints));
    spans.push(Span::raw(" ".repeat(spacer_width.max(1))));
    spans.extend(hints);
    let line = Line::from(spans);
    // Under the compact view's status line the keys take the bottom row
    // alone, the grid's border dividing them; elsewhere a rule above them,
    // as it always was.
    let (area, borders) = if crate::ui::uses_status_line(app) {
        let last = area.bottom().saturating_sub(1);
        (
            Rect {
                y: last,
                height: 1,
                ..area
            },
            Borders::NONE,
        )
    } else {
        (area, Borders::TOP)
    };
    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(colors::SURFACE))
            .padding(Padding::horizontal(crate::ui::PANE_CONTENT_INSET)),
    );
    frame.render_widget(paragraph, area);
}

/// The footer's left side where no summary box was drawn: a manual
/// refresh's spinner, then the last action's message, or the summary when
/// there is none.
fn fallback_spans(app: &App) -> Vec<Span<'static>> {
    let p = crate::ui::fleet_pieces(app, chrono::Local::now());
    let mut spans: Vec<Span<'static>> = p
        .refreshing
        .into_iter()
        .flatten()
        .map(|s| Span::styled(s.content.trim().to_string(), s.style))
        .collect();
    if !spans.is_empty() {
        spans.push(Span::raw("  "));
    }
    match p.message {
        Some(message) => spans.push(message),
        None => spans.extend(p.summary),
    }
    spans
}

/// The fleet's counters, `3↓ · 2↑ · 4! · 17 synced`, as the status line
/// draws them.
pub(crate) fn summary_spans(summary: &Summary) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let dot = || Span::styled(" · ", Style::default().fg(colors::SURFACE));

    if summary.behind_count > 0 {
        spans.push(Span::styled(
            format!("{}↓", summary.behind_count),
            Style::default().fg(colors::PEACH),
        ));
        spans.push(dot());
    }
    if summary.ahead_count > 0 {
        spans.push(Span::styled(
            format!("{}↑", summary.ahead_count),
            Style::default().fg(colors::PEACH),
        ));
        spans.push(dot());
    }
    if summary.dirty_count > 0 {
        spans.push(Span::styled(
            format!("{}!", summary.dirty_count),
            Style::default().fg(colors::YELLOW),
        ));
        spans.push(dot());
    }
    if summary.synced_count > 0 {
        spans.push(Span::styled(
            format!("{} synced", summary.synced_count),
            Style::default().fg(colors::GREEN),
        ));
        spans.push(dot());
    }
    if summary.not_cloned_count > 0 {
        spans.push(Span::styled(
            format!("{} {NOT_CLONED}", summary.not_cloned_count),
            Style::default().fg(colors::OVERLAY),
        ));
        spans.push(dot());
    }
    // Not gated on the `-p` flag: the count rides every poll for free, and a
    // footer that stays silent about three open PRs because the detail list
    // was not asked for is the failure this whole summary exists to prevent.
    if summary.pr_count > 0 {
        spans.push(Span::styled(
            format!("{} PRs", summary.pr_count),
            Style::default().fg(colors::PR_CLR),
        ));
        spans.push(dot());
    }

    // Remove trailing dot
    if spans
        .last()
        .map(|s| s.content.contains('·'))
        .unwrap_or(false)
    {
        spans.pop();
    }

    spans
}

/// Keybind hints. `with_desc` shows the trailing word ("lazygit", "pull", ...);
/// set false to fall back to keys only when space is tight.
///
/// A key whose action has an "on" state lights up while it is on, so the row
/// doubles as a bank of switches: `r` and `R` while their work is in flight,
/// `v` in the compact view, `?` while help is open, and `c` in the strip's
/// own ladder colour. Watch is the exception: its state lives in the header,
/// where a blinking glyph says it is a process and not just a setting. `f`
/// stays plain because a single fetch records nothing about being in flight.
fn keybind_hints(app: &App, with_desc: bool) -> Vec<Span<'static>> {
    let key_style = Style::default()
        .fg(colors::TEXT)
        .add_modifier(Modifier::BOLD);
    let lit = |on: bool| {
        if on {
            key_style.fg(colors::GREEN)
        } else {
            key_style
        }
    };
    let desc_style = Style::default().fg(colors::OVERLAY);
    let gap = Span::raw(if with_desc { "  " } else { " " });

    let view_hint = match app.view {
        View::Detail => "compact",
        View::Compact => "detail",
    };
    let refreshing = app.is_refreshing();

    let mut hints = vec![Span::styled("[enter]", key_style)];
    if with_desc {
        hints.push(Span::styled(" lazygit", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[r]", lit(refreshing)));
    if with_desc {
        hints.push(Span::styled(" refresh", desc_style));
    }
    hints.push(gap.clone());
    // From the `notes` tree or reader `p` flips the reader (FZ-59).
    if app.reader_pages() {
        hints.push(Span::styled("[p]", key_style));
        if with_desc {
            hints.push(Span::styled(
                format!(" {}", reader_mode_hint(app)),
                desc_style,
            ));
        }
        hints.push(gap.clone());
    }
    hints.push(Span::styled("[R]", lit(app.loading)));
    if with_desc {
        hints.push(Span::styled(" reload", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[f]", key_style));
    if with_desc {
        hints.push(Span::styled(" fetch", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[w]", key_style));
    if with_desc {
        hints.push(Span::styled(" watch", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled(
        "[c]",
        contrib_key_style(app.contributions, app.contributions_folded),
    ));
    if with_desc {
        hints.push(Span::styled(" contrib", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[s]", key_style));
    if with_desc {
        hints.push(Span::styled(" switch", desc_style));
    }
    hints.push(gap.clone());
    // The yank chords (FZ-74), where the focused pane has a cursor on
    // something to copy: not the compact grid, the strip or a header. Not
    // on the repo list either, whose row has no column to spare for it.
    if app.view == View::Detail
        && !matches!(
            app.focused_panel,
            FocusedPanel::Contributions | FocusedPanel::RepoList
        )
        && app.on_header().is_none()
    {
        hints.push(Span::styled("[yy/yp]", key_style));
        if with_desc {
            hints.push(Span::styled(" yank", desc_style));
        }
        hints.push(gap.clone());
    }
    hints.push(Span::styled("[v]", lit(app.view == View::Compact)));
    if with_desc {
        hints.push(Span::styled(format!(" {view_hint}"), desc_style));
    }
    hints.push(gap.clone());
    // Help sits second-to-last rather than first: it is the least urgent hint
    // in normal use, but it is also the one that must never be dropped, since
    // it is how every other binding is discovered. The keys-only fallback
    // keeps `[?]` visible even when the descriptions go.
    hints.push(Span::styled(
        "[?]",
        lit(matches!(app.overlay, Overlay::Help)),
    ));
    if with_desc {
        hints.push(Span::styled(" help", desc_style));
    }
    hints.push(gap.clone());
    hints.push(Span::styled("[q]", key_style));
    if with_desc {
        hints.push(Span::styled(" quit", desc_style));
    }
    hints
}

/// Between the focused pane's keys and the global ones, wider than the gap
/// between two keys, so the two groups read apart.
const SEGMENT_GAP: &str = "   ";

/// What `p` does from the `notes` tree or reader (FZ-59): show the source,
/// or render it again.
fn reader_mode_hint(app: &App) -> &'static str {
    match app.reader_raw {
        true => "render",
        false => "raw",
    }
}

/// The keys that belong to the focused pane, which open the hint row: they
/// have no room beside what they change, so without this they were only in
/// the help overlay. Keys the global row already hints are not repeated,
/// except the strip's `[c]`, which is that pane's own key; it is drawn plain
/// here, since the global `[c]` is the one that lights in the strip's colour.
/// The compact view has one grid that takes keys, so it hints the list's
/// keys unless the strip is focused.
fn focus_hints(app: &App, with_desc: bool) -> Vec<Span<'static>> {
    let pane = match (app.view, app.focused_panel) {
        (_, FocusedPanel::Contributions) => FocusedPanel::Contributions,
        (View::Compact, _) => FocusedPanel::RepoList,
        (View::Detail, pane) => pane,
    };
    let mut keys: Vec<(String, &str)> = match pane {
        FocusedPanel::RepoList => vec![
            ("[j/k]".into(), "move"),
            ("[/]".into(), "search"),
            ("[g]".into(), "group"),
            ("[t]".into(), "window"),
            ("[e]".into(), "edit"),
            ("[o]".into(), "shell"),
        ],
        // The `ci` view's cursor walks runs, and its handoffs act on the
        // highlighted one (FZ-30e).
        FocusedPanel::Detail if app.history_view == HistoryView::Ci => vec![
            ("[j/k]".into(), "run"),
            ("[[ ]]".into(), "view"),
            ("[l]".into(), "log"),
            ("[a]".into(), "rerun"),
            ("[x]".into(), "cancel"),
            ("[O]".into(), "open"),
        ],
        // The `notes` tree (FZ-62): its cursor walks files, and the shifted
        // keys read the file beside it without leaving the tree.
        FocusedPanel::Detail if app.notes_layout() => vec![
            ("[j/k]".into(), "file"),
            ("[[ ]]".into(), "view"),
            ("[J/K]".into(), "read"),
            ("[H/L]".into(), "page"),
        ],
        FocusedPanel::Detail => vec![
            ("[j/k]".into(), "commit"),
            ("[[ ]]".into(), "view"),
            ("[J/K]".into(), "inspector"),
        ],
        // On a run (FZ-30f) `[`/`]` step its bodies, and in `jobs` the
        // `j`/`k` keys move the job mark rather than scroll.
        FocusedPanel::Inspector if app.inspects_ci_run() => vec![
            (
                "[j/k]".into(),
                match app.ci_body {
                    crate::app::CiBody::Jobs => "job",
                    _ => "scroll",
                },
            ),
            ("[[ ]]".into(), "body"),
            ("[+/-]".into(), "size"),
            (format!("[{}]", crate::ui::INSPECTOR_NUM), "fold"),
        ],
        FocusedPanel::Inspector => vec![
            ("[j/k]".into(), "scroll"),
            ("[+/-]".into(), "size"),
            (format!("[{}]", crate::ui::INSPECTOR_NUM), "fold"),
        ],
        // The `notes` view's reader (FZ-62): `Enter` opens the file it shows.
        FocusedPanel::Reader => vec![
            ("[j/k]".into(), "scroll"),
            ("[H/L]".into(), "page"),
            ("[Enter]".into(), "edit"),
        ],
        FocusedPanel::Contributions => vec![
            ("[h/l]".into(), "week"),
            ("[j/k]".into(), "day"),
            ("[Home/End]".into(), "ends"),
            ("[c]".into(), "source"),
            (format!("[{}]", crate::ui::CONTRIBUTIONS_NUM), "fold"),
        ],
    };
    // `B` folds `info`'s remote branch summary and does nothing elsewhere.
    if pane == FocusedPanel::Detail && app.history_view == HistoryView::Info {
        keys.push(("[B]".into(), "remotes"));
    }
    // On a section header pane 2 is the slot digest (FZ-45): its keys
    // replace the list's and pane 2's.
    if app.on_header().is_some() && matches!(pane, FocusedPanel::RepoList | FocusedPanel::Detail) {
        let walk = if pane == FocusedPanel::Detail {
            "line"
        } else {
            "move"
        };
        keys = vec![
            ("[j/k]".into(), walk),
            ("[t]".into(), "digest window"),
            ("[Enter]".into(), "go to repo"),
        ];
    }

    let key_style = Style::default()
        .fg(colors::TEXT)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(colors::OVERLAY);
    let gap = if with_desc { "  " } else { " " };
    let mut hints = Vec::new();
    for (i, (key, desc)) in keys.into_iter().enumerate() {
        if i > 0 {
            hints.push(Span::raw(gap));
        }
        hints.push(Span::styled(key, key_style));
        if with_desc {
            hints.push(Span::styled(format!(" {desc}"), desc_style));
        }
    }
    hints
}

/// The focused pane's keys, the segment gap, then the global keys.
fn hints_with_segment(app: &App, segment_desc: bool, global_desc: bool) -> Vec<Span<'static>> {
    let mut hints = focus_hints(app, segment_desc);
    hints.push(Span::raw(SEGMENT_GAP));
    hints.extend(keybind_hints(app, global_desc));
    hints
}

/// The `[c]` hint's style: the brightest rung of the source the strip is
/// showing, or the ordinary key colour while it is folded and `c` does
/// nothing.
fn contrib_key_style(mode: ContributionsMode, folded: bool) -> Style {
    let source = match mode {
        _ if folded => None,
        ContributionsMode::Off => None,
        ContributionsMode::Github => Some(crate::contributions::Source::Github),
        ContributionsMode::Local => Some(crate::contributions::Source::Local),
    };
    let Some(source) = source else {
        return Style::default()
            .fg(colors::TEXT)
            .add_modifier(Modifier::BOLD);
    };
    Style::default()
        .fg(colors::contrib_ladder(source)[4])
        .add_modifier(Modifier::BOLD)
}

/// Columns a span run draws, counted in characters as the rest of the UI
/// counts them: every glyph the footer draws is one column wide, and a byte
/// count charged the arrows and dots two or three columns each.
fn line_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// Whether summary + hints fit side by side in `width` with at least one
/// column of breathing room between them.
fn fits(width: usize, spans: &[Span], hints: &[Span]) -> bool {
    line_width(spans) + line_width(hints) < width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn app_in(view: View) -> App {
        let mut app = App::new(Vec::new(), String::new(), 120, 40);
        app.view = view;
        app
    }

    /// The foreground of one `[key]` span in the full hint row.
    fn key_color(app: &App, key: &str) -> Option<ratatui::style::Color> {
        keybind_hints(app, true)
            .into_iter()
            .find(|s| s.content == key)
            .unwrap_or_else(|| panic!("{key} not hinted"))
            .style
            .fg
    }

    /// The footer used to call this state "not local", the only place in the
    /// app that did. Two rows above it the repo list draws a "local changes"
    /// section, which is a different set entirely (LocalOnly, not NotCloned),
    /// so the two read as contradicting each other. Both sides now go through
    /// `model::NOT_CLONED`.
    #[test]
    fn the_summary_names_uncloned_repos_the_way_the_badge_does() {
        let summary = Summary {
            not_cloned_count: 3,
            ..Default::default()
        };
        let rendered = text(&summary_spans(&summary));
        assert_eq!(rendered, "3 not cloned", "got {rendered:?}");
        assert!(
            !rendered.contains("not local"),
            "'not local' negates the 'local changes' section header"
        );
    }

    #[test]
    fn help_is_hinted_in_the_footer_with_the_other_global_actions() {
        // It used to live in the header while every other action key was down
        // here, which is the inconsistency this guards.
        assert!(text(&keybind_hints(&app_in(View::Detail), true)).contains("[?] help"));
    }

    #[test]
    fn help_survives_the_keys_only_fallback() {
        // Help is how every other binding is discovered, so it is the one hint
        // that must stay visible when the descriptions are dropped for width.
        let compact = text(&keybind_hints(&app_in(View::Detail), false));
        assert!(compact.contains("[?]"), "got {compact:?}");
        assert!(!compact.contains("help"), "descriptions should be gone");
    }

    #[test]
    fn the_view_hint_names_the_view_it_switches_to() {
        assert!(text(&keybind_hints(&app_in(View::Detail), true)).contains("[v] compact"));
        assert!(text(&keybind_hints(&app_in(View::Compact), true)).contains("[v] detail"));
    }

    /// `c` switches the strip's source, and the footer is where every other
    /// action key is discovered, so it is hinted there too.
    #[test]
    fn contributions_are_hinted_beside_watch() {
        let hints = text(&keybind_hints(&app_in(View::Detail), true));
        assert!(hints.contains("[w] watch  [c] contrib"), "got {hints:?}");
        let keys = text(&keybind_hints(&app_in(View::Detail), false));
        assert!(keys.contains("[w] [c]"), "got {keys:?}");
    }

    /// The `c` key shows the strip's state: neutral while the strip is
    /// folded and the key does nothing, and the source's own ladder colour
    /// while it is up, so the hint and the cells above it can never disagree
    /// about which source is drawn.
    #[test]
    fn the_contributions_key_lights_up_in_the_strips_colour_when_up() {
        let mut app = app_in(View::Detail);
        app.contributions_folded = true;
        assert_eq!(key_color(&app, "[c]"), Some(colors::TEXT));
        app.contributions_folded = false;
        app.contributions = ContributionsMode::Github;
        assert_eq!(
            key_color(&app, "[c]"),
            Some(colors::contrib_ladder(crate::contributions::Source::Github)[4])
        );
        app.contributions = ContributionsMode::Local;
        assert_eq!(
            key_color(&app, "[c]"),
            Some(colors::contrib_ladder(crate::contributions::Source::Local)[4])
        );
    }

    /// Every other hint with an "on" state lights up the same way, so the
    /// footer reads as one row of switches rather than one odd key.
    #[test]
    fn a_manual_refresh_lights_the_r_key_while_it_runs() {
        let mut app = app_in(View::Detail);
        assert_eq!(key_color(&app, "[r]"), Some(colors::TEXT));
        app.sweep_started = Some(std::time::Instant::now());
        app.sweep_manual = true;
        assert_eq!(key_color(&app, "[r]"), Some(colors::GREEN));
        // A watch-timer sweep is not something the user switched on.
        app.sweep_manual = false;
        assert_eq!(key_color(&app, "[r]"), Some(colors::TEXT));
    }

    #[test]
    fn a_reload_lights_the_shift_r_key_while_it_runs() {
        let mut app = app_in(View::Detail);
        assert_eq!(key_color(&app, "[R]"), Some(colors::TEXT));
        app.loading = true;
        assert_eq!(key_color(&app, "[R]"), Some(colors::GREEN));
    }

    #[test]
    fn the_compact_view_lights_the_v_key() {
        assert_eq!(key_color(&app_in(View::Detail), "[v]"), Some(colors::TEXT));
        assert_eq!(
            key_color(&app_in(View::Compact), "[v]"),
            Some(colors::GREEN)
        );
    }

    #[test]
    fn an_open_help_overlay_lights_the_question_mark() {
        let mut app = app_in(View::Detail);
        assert_eq!(key_color(&app, "[?]"), Some(colors::TEXT));
        app.overlay = crate::app::Overlay::Help;
        assert_eq!(key_color(&app, "[?]"), Some(colors::GREEN));
    }

    /// Watch keeps its state in the header, where the blinking glyph says it
    /// is a running process; the footer key stays plain so the two do not
    /// compete.
    #[test]
    fn the_watch_key_never_lights_up() {
        let mut app = app_in(View::Detail);
        app.watch_mode = true;
        assert_eq!(key_color(&app, "[w]"), Some(colors::TEXT));
    }

    /// A clean repo with the given counters, so the summary carries its
    /// arrows and dots.
    fn repo(name: &str, behind: u32, ahead: u32, dirty: u32) -> crate::model::RepoStatus {
        crate::model::RepoStatus {
            last_local: None,
            daily: None,
            daily_key: None,
            name: name.to_string(),
            path: Some(format!("~/repos/{name}")),
            location: crate::model::RepoLocation::Primary,
            branches: vec![],
            commits: vec![],
            behind,
            ahead,
            dirty,
            pr_count: 0,
            prs: vec![],
            ci_status: crate::model::CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            groups: vec![],
            stash_count: 0,
            stash_entries: vec![],
            worktrees: vec![],
            reflog: vec![],
            graph: vec![],
            graph_expanded: Vec::new(),
            repo_kind: crate::model::RepoKind::Unknown,
            owner: None,
            remote_url: None,
            remote_only_branches: vec![],
            remote_branches: vec![],
            work_key: None,
            fetch: crate::model::FetchFacts::default(),
            authors: Default::default(),
        }
    }

    /// An app whose summary reads `1↓ · 1↑ · 1! · 1 synced`: two arrows of
    /// three bytes and three dots of two, so its byte length is seven more
    /// than the columns it draws.
    fn app_with_glyph_summary() -> App {
        let repos = vec![
            repo("a", 1, 0, 0),
            repo("b", 0, 1, 0),
            repo("c", 0, 0, 1),
            repo("d", 0, 0, 0),
        ];
        let mut app = App::new(repos, String::new(), 120, 40);
        app.view = View::Detail;
        app
    }

    /// The footer's content row as drawn at `width` columns.
    fn footer_row(app: &App, width: u16) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut terminal = Terminal::new(TestBackend::new(width, 2)).unwrap();
        terminal.draw(|f| render(f, f.area(), app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..width).map(|x| buffer[(x, 1)].symbol()).collect()
    }

    /// The narrowest footer that keeps `left` and `hints` side by side: the
    /// inset on both sides, the columns they draw, and one column between.
    fn snug_width(left: &str, hints: &str) -> u16 {
        let inset = 2 * crate::ui::PANE_CONTENT_INSET as usize;
        (inset + left.chars().count() + hints.chars().count() + 1) as u16
    }

    /// Regression: the fit was measured in bytes, so a summary's arrows and
    /// dots cost two or three columns each and the key descriptions went
    /// while there was room for them. At the snug width the summary and the
    /// full hints are both drawn; one column less, the descriptions go.
    #[test]
    fn the_key_descriptions_stay_while_they_fit_beside_a_glyph_summary() {
        let app = app_with_glyph_summary();
        let summary = text(&summary_spans(&app.summary()));
        assert_eq!(summary, "1↓ · 1↑ · 1! · 1 synced");
        assert!(
            summary.len() > summary.chars().count(),
            "no multi-byte glyph"
        );
        let full = text(&keybind_hints(&app, true));
        let segment = text(&focus_hints(&app, true));
        let width = snug_width(&summary, &format!("{segment}{SEGMENT_GAP}{full}"));

        let row = footer_row(&app, width);
        assert!(row.contains(&summary), "summary dropped: {row:?}");
        assert!(row.contains(&full), "descriptions dropped: {row:?}");

        let tighter = footer_row(&app, width - 1);
        assert!(tighter.contains(&summary), "{tighter:?}");
        assert!(!tighter.contains(" lazygit"), "{tighter:?}");
    }

    /// The same for the summary itself: at the width where it fits beside
    /// the keys-only hints, it is drawn rather than dropped.
    #[test]
    fn a_glyph_summary_stays_while_it_fits_beside_the_keys() {
        let app = app_with_glyph_summary();
        let summary = text(&summary_spans(&app.summary()));
        let keys = text(&keybind_hints(&app, false));
        let width = snug_width(&summary, &keys);

        let row = footer_row(&app, width);
        assert!(row.contains(&summary), "summary dropped: {row:?}");
        assert!(row.contains(&keys), "{row:?}");

        let tighter = footer_row(&app, width - 1);
        assert!(!tighter.contains("synced"), "{tighter:?}");
        assert!(tighter.contains(&keys), "{tighter:?}");
    }

    /// The spacer is measured the same way, so the hints end flush with the
    /// content's right edge: a byte count made the spacer short by one
    /// column per extra byte, leaving a gap after `quit`.
    #[test]
    fn the_hints_end_at_the_right_edge_beside_a_glyph_summary() {
        let app = app_with_glyph_summary();
        let width = 220u16;
        let row = footer_row(&app, width);
        let right_edge = width as usize - crate::ui::PANE_CONTENT_INSET as usize;
        let drawn = row.trim_end().chars().count();
        assert_eq!(drawn, right_edge, "{row:?}");
        assert!(row.trim_end().ends_with("[q] quit"), "{row:?}");
    }

    /// The drop order: the descriptions go first, then the left side. Read
    /// from the drawn footer at every width from wide to narrow, so a
    /// reordering, or a step that is skipped, fails.
    #[test]
    fn the_footer_drops_descriptions_before_the_summary() {
        let app = app_with_glyph_summary();
        let mut states: Vec<(bool, bool)> = Vec::new();
        for width in (20u16..=260).rev() {
            let row = footer_row(&app, width);
            let state = (row.contains("synced"), row.contains(" lazygit"));
            if states.last() != Some(&state) {
                states.push(state);
            }
        }
        assert_eq!(states, vec![(true, true), (true, false), (false, false)]);
    }

    /// Wide enough for every pane's segment and the global row, both with
    /// descriptions, beside the glyph summary.
    const WIDE: u16 = 260;

    const LIST_SEGMENT: &str = "[j/k] move  [/] search  [g] group  [t] window  [e] edit  [o] shell";
    const LIST_SEGMENT_BARE: &str = "[j/k] [/] [g] [t] [e] [o]";
    const REPO_SEGMENT_INFO: &str = "[j/k] commit  [[ ]] view  [J/K] inspector  [B] remotes";
    const REPO_SEGMENT_COMMITS: &str = "[j/k] commit  [[ ]] view  [J/K] inspector";

    fn inspector_segment() -> String {
        format!(
            "[j/k] scroll  [+/-] size  [{}] fold",
            crate::ui::INSPECTOR_NUM
        )
    }

    fn strip_segment() -> String {
        format!(
            "[h/l] week  [j/k] day  [Home/End] ends  [c] source  [{}] fold",
            crate::ui::CONTRIBUTIONS_NUM
        )
    }

    /// The hint row as drawn, for an app with no summary: everything but
    /// the inset and the spacer.
    fn hint_row(app: &App, width: u16) -> String {
        footer_row(app, width).trim().to_string()
    }

    fn global_row(app: &App) -> String {
        text(&keybind_hints(app, true))
    }

    #[test]
    fn the_focused_repo_list_opens_the_row_with_its_keys() {
        let app = app_in(View::Detail);
        assert_eq!(app.focused_panel, crate::app::FocusedPanel::RepoList);
        assert_eq!(
            hint_row(&app, WIDE),
            format!("{LIST_SEGMENT}   {}", global_row(&app))
        );
    }

    #[test]
    fn the_focused_inspector_opens_the_row_with_its_keys() {
        let mut app = app_in(View::Detail);
        app.focused_panel = crate::app::FocusedPanel::Inspector;
        assert_eq!(
            hint_row(&app, WIDE),
            format!("{}   {}", inspector_segment(), global_row(&app))
        );
    }

    /// FZ-30f: on a run the inspector hints `[[ ]] body`, and in `jobs`
    /// names `j`/`k` for the job mark they move.
    #[test]
    fn the_focused_inspector_on_a_run_hints_its_bodies() {
        let mut app = app_in(View::Detail);
        app.focused_panel = crate::app::FocusedPanel::Inspector;
        app.history_view = crate::app::HistoryView::Ci;
        let num = crate::ui::INSPECTOR_NUM;
        assert_eq!(
            text(&focus_hints(&app, true)),
            format!("[j/k] job  [[ ]] body  [+/-] size  [{num}] fold")
        );
        app.ci_body = crate::app::CiBody::Script;
        assert_eq!(
            text(&focus_hints(&app, true)),
            format!("[j/k] scroll  [[ ]] body  [+/-] size  [{num}] fold")
        );
    }

    #[test]
    fn the_focused_strip_opens_the_row_with_its_keys() {
        let mut app = app_in(View::Detail);
        app.focused_panel = crate::app::FocusedPanel::Contributions;
        assert_eq!(
            hint_row(&app, WIDE),
            format!("{}   {}", strip_segment(), global_row(&app))
        );
    }

    /// FZ-30d: in the `ci` view the pane's cursor walks runs, and the
    /// segment says so. Fails while it reads `commit` there.
    #[test]
    fn the_ci_view_hints_its_cursor_as_a_run() {
        let mut app = app_in(View::Detail);
        app.focused_panel = crate::app::FocusedPanel::Detail;
        app.history_view = HistoryView::Ci;
        assert_eq!(
            hint_row(&app, WIDE),
            format!(
                "[j/k] run  [[ ]] view  [l] log  [a] rerun  [x] cancel  [O] open   {}",
                global_row(&app)
            )
        );
    }

    /// FZ-59: from the `notes` tree or reader the global row adds `[p]`,
    /// which flips the reader, `raw` while rendered and `render` while raw;
    /// `[r]` refreshes there as everywhere, and `[p]` is hinted nowhere
    /// else. The pane's own segment does not repeat it.
    #[test]
    fn the_global_p_hint_names_the_raw_toggle_in_the_notes_tree_and_reader() {
        let mut app = app_in(View::Detail);
        app.focused_panel = crate::app::FocusedPanel::Detail;
        assert!(global_row(&app).starts_with("[enter] lazygit  [r] refresh  [R]"));

        app.history_view = HistoryView::Notes;
        let row = global_row(&app);
        assert!(
            row.starts_with("[enter] lazygit  [r] refresh  [p] raw  [R]"),
            "{row}"
        );
        assert_eq!(
            hint_row(&app, WIDE),
            format!("[j/k] file  [[ ]] view  [J/K] read  [H/L] page   {row}")
        );

        app.focused_panel = crate::app::FocusedPanel::Reader;
        app.toggle_reader_raw();
        let row = global_row(&app);
        assert!(
            row.starts_with("[enter] lazygit  [r] refresh  [p] render  [R]"),
            "{row}"
        );
        assert_eq!(
            hint_row(&app, WIDE),
            format!("[j/k] scroll  [H/L] page  [Enter] edit   {row}")
        );

        for panel in [
            crate::app::FocusedPanel::RepoList,
            crate::app::FocusedPanel::Inspector,
        ] {
            app.focused_panel = panel;
            let row = global_row(&app);
            assert!(row.contains("[r] refresh"), "{panel:?}");
            assert!(!row.contains("[p]"), "{panel:?}");
        }
    }

    /// `B` folds `info`'s remote branch summary and does nothing elsewhere,
    /// so it is hinted only in that view.
    #[test]
    fn the_repo_pane_hints_remotes_only_in_info() {
        let mut app = app_in(View::Detail);
        app.focused_panel = crate::app::FocusedPanel::Detail;
        app.history_view = crate::app::HistoryView::Info;
        assert_eq!(
            hint_row(&app, WIDE),
            format!("{REPO_SEGMENT_INFO}   {}", global_row(&app))
        );
        app.history_view = crate::app::HistoryView::Commits;
        let row = hint_row(&app, WIDE);
        assert_eq!(
            row,
            format!("{REPO_SEGMENT_COMMITS}   {}", global_row(&app))
        );
        assert!(!row.contains("[B]"), "{row:?}");
    }

    /// The compact view has one grid that takes keys, plus the strip.
    #[test]
    fn the_compact_view_hints_the_list_or_the_focused_strip() {
        let mut app = app_in(View::Compact);
        assert_eq!(
            hint_row(&app, WIDE),
            format!("{LIST_SEGMENT}   {}", global_row(&app))
        );
        app.focused_panel = crate::app::FocusedPanel::Contributions;
        assert_eq!(
            hint_row(&app, WIDE),
            format!("{}   {}", strip_segment(), global_row(&app))
        );
    }

    /// The owner's order of 2026-10-17, widest first: the global row's
    /// words go before the segment's, the segment's before the segment,
    /// and the summary last. Read from the drawn footer at every width for
    /// every pane, so a reordering, or a skipped step, fails.
    #[test]
    fn the_footer_degrades_in_the_owners_order_in_every_pane() {
        use crate::app::{FocusedPanel, HistoryView};
        let panes = [
            (FocusedPanel::RepoList, LIST_SEGMENT.to_string()),
            (FocusedPanel::Detail, REPO_SEGMENT_INFO.to_string()),
            (FocusedPanel::Inspector, inspector_segment()),
            (FocusedPanel::Contributions, strip_segment()),
        ];
        for (pane, segment) in panes {
            let mut app = app_with_glyph_summary();
            app.focused_panel = pane;
            app.history_view = HistoryView::Info;
            // The first key and the first description, which no other
            // pane's segment or the global row draws in that place.
            let first_key = segment.split(' ').next().unwrap().to_string();
            let first_desc: String = segment.split("  ").next().unwrap().to_string();
            let mut states: Vec<(bool, bool, bool, bool)> = Vec::new();
            for width in (20u16..=300).rev() {
                let row = footer_row(&app, width);
                let state = (
                    row.contains(&first_key),
                    row.contains(&format!("{first_desc}  ")),
                    row.contains(" lazygit"),
                    row.contains("synced"),
                );
                if states.last() != Some(&state) {
                    states.push(state);
                }
            }
            assert_eq!(
                states,
                vec![
                    (true, true, true, true),
                    (true, true, false, true),
                    (true, false, false, true),
                    (false, false, false, true),
                    (false, false, false, false),
                ],
                "{pane:?}"
            );
        }
    }

    /// Where the segment does not fit even bare, the footer is exactly the
    /// row it drew before the segment existed: the summary beside the bare
    /// keys, then the bare keys alone.
    #[test]
    fn without_room_for_the_segment_the_row_is_todays() {
        let app = app_with_glyph_summary();
        let summary = text(&summary_spans(&app.summary()));
        let keys = text(&keybind_hints(&app, false));
        let inset = crate::ui::PANE_CONTENT_INSET as usize;
        let n = |s: &str| s.chars().count();
        // The narrowest width the bare segment fits at, beside the summary.
        let segment_from = 2 * inset + n(&summary) + n(LIST_SEGMENT_BARE) + n("   ") + n(&keys) + 1;
        let mut with_summary = 0;
        let mut without = 0;
        for width in (2 * inset + n(&keys) + 1)..segment_from {
            let content = width - 2 * inset;
            let left = if n(&summary) + n(&keys) < content {
                with_summary += 1;
                summary.clone()
            } else {
                without += 1;
                String::new()
            };
            let spacer = " ".repeat(content - n(&left) - n(&keys));
            let pad = " ".repeat(inset);
            let today = format!("{pad}{left}{spacer}{keys}{pad}");
            assert_eq!(footer_row(&app, width as u16), today, "at {width}");
        }
        assert!(with_summary > 0 && without > 0);
    }

    /// Where pane 2 drew the fleet's state (FZ-93), the footer is keys only:
    /// no summary, and the full hints have the whole width.
    #[test]
    fn beside_a_drawn_fleet_column_the_footer_is_keys_only() {
        let app = app_with_glyph_summary();
        app.fleet_drawn.set(true);
        let full = text(&keybind_hints(&app, true));
        let segment = text(&focus_hints(&app, true));
        let width = snug_width("", &format!("{segment}{SEGMENT_GAP}{full}"));
        let row = footer_row(&app, width);
        assert!(row.contains(&full), "descriptions dropped: {row:?}");
        assert!(!row.contains("synced"), "summary in the footer: {row:?}");
        let tighter = footer_row(&app, width - 1);
        assert!(!tighter.contains(" lazygit"), "{tighter:?}");
    }

    /// Where it did not (a section's digest, no repo, a narrow pane 2), the
    /// footer carries the last action's message in the summary's place, so
    /// an action never ends in silence (FZ-93 review, B1).
    #[test]
    fn without_a_fleet_column_the_footer_carries_the_message() {
        let mut app = app_with_glyph_summary();
        app.set_status("Already refreshing".to_string(), true);
        let row = footer_row(&app, 200);
        assert!(row.contains("Already refreshing"), "{row:?}");
        assert!(
            !row.contains("synced"),
            "the message takes its place: {row:?}"
        );
        app.fleet_drawn.set(true);
        assert!(!footer_row(&app, 200).contains("Already refreshing"));
    }

    /// The strip's segment names `c` as that pane's key, but plainly: the
    /// global row's `[c]` is the one that lights in the strip's colour.
    #[test]
    fn the_strip_segment_draws_c_in_the_plain_key_style() {
        let mut app = app_in(View::Detail);
        app.focused_panel = crate::app::FocusedPanel::Contributions;
        app.contributions_folded = false;
        app.contributions = ContributionsMode::Github;
        let c = focus_hints(&app, true)
            .into_iter()
            .find(|s| s.content == "[c]")
            .expect("[c] in the strip segment");
        assert_eq!(c.style.fg, Some(colors::TEXT));
    }

    /// FZ-74: the yank chords are hinted where pane 2, the inspector or the
    /// reader has a cursor to copy from, and not on the repo list, the
    /// strip or the compact grid.
    #[test]
    fn the_yank_chords_are_hinted_beside_the_panes_that_copy() {
        let mut app = app_in(View::Detail);
        for panel in [
            FocusedPanel::Detail,
            FocusedPanel::Inspector,
            FocusedPanel::Reader,
        ] {
            app.focused_panel = panel;
            let row = global_row(&app);
            assert!(
                row.contains("[s] switch  [yy/yp] yank  [v]"),
                "{panel:?}: {row}"
            );
        }
        for panel in [FocusedPanel::RepoList, FocusedPanel::Contributions] {
            app.focused_panel = panel;
            assert!(!global_row(&app).contains("[yy/yp]"), "{panel:?}");
        }
        let mut app = app_in(View::Compact);
        app.focused_panel = FocusedPanel::Detail;
        assert!(!global_row(&app).contains("[yy/yp]"), "compact");
    }

    #[test]
    fn quit_stays_last() {
        // Muscle memory: the rightmost hint should not move as hints are added.
        assert!(text(&keybind_hints(&app_in(View::Detail), true)).ends_with(" quit"));
        assert!(text(&keybind_hints(&app_in(View::Detail), false)).ends_with("[q]"));
    }

    /// FZ-45: on a section header the focused pane hints the digest's keys.
    #[test]
    fn a_header_stop_hints_the_digest_keys() {
        use crate::digest::tests::repo;
        use crate::model::RepoLocation;
        let mut app = App::new(
            vec![
                repo("a", RepoLocation::Primary),
                repo("b", RepoLocation::LocalOnly),
            ],
            String::new(),
            120,
            40,
        );
        app.view = View::Detail;
        app.land_on(crate::app::ListRow::Header(
            crate::digest::Section::LocalChanges,
        ));
        let list = text(&focus_hints(&app, true));
        assert_eq!(list, "[j/k] move  [t] digest window  [Enter] go to repo");
        app.focused_panel = FocusedPanel::Detail;
        assert!(text(&focus_hints(&app, true)).starts_with("[j/k] line  [t]"));
        app.land_on(crate::app::ListRow::Repo(1));
        app.focused_panel = FocusedPanel::RepoList;
        assert!(text(&focus_hints(&app, true)).contains("[t] window"));
    }
}
