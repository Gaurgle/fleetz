use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Padding, Paragraph};

use crate::app::{App, ContributionsMode, Overlay, View};
use crate::model::{NOT_CLONED, Summary};
use crate::ui::colors;

/// Render the footer: summary stats on the left, keybind hints on the right.
///
/// Never lets the two sides collide: hints drop their descriptions first
/// (keys only) when tight, and the summary drops entirely if even that
/// doesn't fit, rather than risk a negative spacer width.
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

    let summary = app.summary();
    // The fit and spacer math below is about the *padded* width, not the
    // footer's full rect: the block insets the content on both sides so the
    // summary lines up with the pane content above it rather than starting two
    // columns further left, hard against the terminal edge.
    let width = area.width.saturating_sub(2 * crate::ui::PANE_CONTENT_INSET) as usize;

    // A transient status message (fetch/switch/refresh result) takes over the
    // left side while present, so single-shot actions actually report back --
    // otherwise the summary just sits there and the action looks like a no-op.
    let left = match &app.status_message {
        Some(msg) => {
            let color = if msg.is_error {
                colors::RED
            } else {
                colors::GREEN
            };
            vec![Span::styled(msg.text.clone(), Style::default().fg(color))]
        }
        None => summary_spans(&summary),
    };

    let full_hints = keybind_hints(app, true);
    let compact_hints = keybind_hints(app, false);

    let (mut spans, hints) = if fits(width, &left, &full_hints) {
        (left, full_hints)
    } else if fits(width, &left, &compact_hints) {
        (left, compact_hints)
    } else {
        (Vec::new(), compact_hints)
    };

    let spacer_width = width.saturating_sub(line_width(&spans) + line_width(&hints));
    spans.push(Span::raw(" ".repeat(spacer_width.max(1))));
    spans.extend(hints);

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(colors::SURFACE))
            .padding(Padding::horizontal(crate::ui::PANE_CONTENT_INSET)),
    );
    frame.render_widget(paragraph, area);
}

fn summary_spans(summary: &Summary) -> Vec<Span<'static>> {
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
        let width = snug_width(&summary, &full);

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
        let width = 160u16;
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
        for width in (20u16..=160).rev() {
            let row = footer_row(&app, width);
            let state = (row.contains("synced"), row.contains(" lazygit"));
            if states.last() != Some(&state) {
                states.push(state);
            }
        }
        assert_eq!(states, vec![(true, true), (true, false), (false, false)]);
    }

    #[test]
    fn quit_stays_last() {
        // Muscle memory: the rightmost hint should not move as hints are added.
        assert!(text(&keybind_hints(&app_in(View::Detail), true)).ends_with(" quit"));
        assert!(text(&keybind_hints(&app_in(View::Detail), false)).ends_with("[q]"));
    }
}
