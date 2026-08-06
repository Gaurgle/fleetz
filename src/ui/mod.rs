pub mod badges;
pub mod branches;
pub mod colors;
pub mod compact;
pub mod detail;
pub mod footer;
pub mod help;
pub mod settings;
pub mod timewindow;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};

use crate::app::{App, FilterKind, FocusedPanel, Overlay, View};

/// A compact relative age ("3m ago", "5h ago", "2d ago", "4mo ago").
///
/// Computed from an absolute timestamp at draw time, deliberately: commit
/// ages used to be pre-rendered by `git log --date=relative` and frozen into
/// the model, so they sat stale until the next sweep. Formatting here instead
/// costs one subtraction per drawn row and keeps every age current on any
/// repaint, which watch mode already triggers.
pub fn format_age(created: chrono::DateTime<chrono::Utc>) -> String {
    let diff = chrono::Utc::now() - created;
    let days = diff.num_days();
    if days == 0 {
        let hours = diff.num_hours();
        if hours == 0 {
            format!("{}m ago", diff.num_minutes().max(1))
        } else {
            format!("{hours}h ago")
        }
    } else if days == 1 {
        "1d ago".to_string()
    } else if days < 30 {
        format!("{days}d ago")
    } else {
        format!("{}mo ago", days / 30)
    }
}

/// The running build's version, straight from Cargo.toml. Single source: bump
/// the manifest and every surface that shows it follows.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Columns between the edge of a pane and the first cell of its content: one
/// for the border, one for the block's horizontal padding.
///
/// The footer draws no border of its own, so it has to reproduce this inset
/// by hand or its summary starts two columns left of every pane above it.
/// Named rather than written as a literal 2 so the two cannot drift apart.
pub const PANE_CONTENT_INSET: u16 = 2;

/// Braille spinner frames, stepped by `app.spinner_frame` (one per tick, so
/// `TICK_INTERVAL` sets the spin rate). Every frame is a single cell wide, so
/// the header never jitters.
///
/// Each frame is a full braille cell with exactly one dot missing, and the
/// missing dot walks the perimeter. Reading it as a hole orbiting a solid block
/// is what makes it legible at one cell wide.
///
/// Listed so the hole travels **clockwise**: down the right column (dots 4, 5,
/// 6), across the bottom (8, 7), then up the left (3, 2, 1).
///
/// The full cell `⣿` is deliberately not a frame: with no missing dot there is
/// nothing to track, so it reads as a dropped frame rather than part of the
/// rotation.
pub const SPINNER: [&str; 8] = ["⣷", "⣯", "⣟", "⡿", "⢿", "⣻", "⣽", "⣾"];

/// Hues the spinner drifts through while it runs, in order. All Catppuccin
/// Mocha accents, so the animation stays inside the palette the rest of the UI
/// uses rather than introducing a second one.
const SPINNER_HUES: [Color; 5] = [
    colors::LAVENDER,
    colors::SAPPHIRE,
    colors::GREEN,
    colors::PEACH,
    colors::MAUVE,
];

/// Ticks spent crossing from one hue to the next. At a 100ms tick that is two
/// seconds per leg and ten for the full loop: slow enough to read as a drift
/// rather than a strobe competing with the spin itself.
const SPINNER_HUE_TICKS: usize = 20;

/// The spinner's color at a given frame: a linear blend between two adjacent
/// entries of `SPINNER_HUES`, wrapping back to the first.
///
/// Deliberately independent of the frame glyph, which cycles far faster, so the
/// two animations do not beat against each other.
pub fn spinner_color(frame: usize) -> Color {
    let leg = (frame / SPINNER_HUE_TICKS) % SPINNER_HUES.len();
    let next = (leg + 1) % SPINNER_HUES.len();
    let t = (frame % SPINNER_HUE_TICKS) as u32;
    blend(
        SPINNER_HUES[leg],
        SPINNER_HUES[next],
        t,
        SPINNER_HUE_TICKS as u32,
    )
}

/// A spinner and its label as two spans: only the glyph drifts through the
/// palette. Fading the word along with it made the label harder to read at the
/// dim end of a leg, and the motion is what the color is meant to draw the eye
/// to anyway, so the text holds a steady `PEACH`.
fn spinner_group(frame: usize, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!(" {}", SPINNER[frame % SPINNER.len()]),
            Style::default().fg(spinner_color(frame)),
        ),
        Span::styled(format!(" {label} "), Style::default().fg(colors::PEACH)),
    ]
}

/// Blend two colors `num`/`den` of the way from `a` to `b`, per channel.
/// Non-RGB colors have no components to interpolate, so they return `a`
/// unchanged rather than guessing.
fn blend(a: Color, b: Color, num: u32, den: u32) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return a;
    };
    let mix = |x: u8, y: u8| -> u8 {
        let x = x as u32;
        let y = y as u32;
        ((x * (den - num) + y * num) / den) as u8
    };
    Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
}

/// The screen rectangles of the main panels, recomputed from the terminal
/// area with the same layout math as `render`, so mouse coordinates can be
/// hit-tested against them. `detail`/`inspector` are None in Compact view (or
/// when the column is too short for the inspector).
pub struct PanelRects {
    /// The whole area the panels are laid out in, so a drag can be converted
    /// back into a percentage of it.
    pub main: Rect,
    pub repo_list: Rect,
    pub detail: Option<Rect>,
    pub inspector: Option<Rect>,
}

/// A pane boundary the mouse can grab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Divider {
    /// Between the repo list and the right column.
    Vertical,
    /// Between the detail pane and the inspector.
    Horizontal,
}

/// How far from a divider a click still counts as grabbing it. One column
/// either side: the gap itself is a single cell, which is too small a target
/// to hit reliably.
const GRAB_SLOP: u16 = 1;

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

    /// Which divider, if any, the given cell grabs. Checked before `at`, so a
    /// click landing in the slop zone starts a drag rather than just moving
    /// focus. The vertical divider wins ties: it spans the full height, so a
    /// cell near both is nearly always aimed at it.
    pub fn divider_at(&self, col: u16, row: u16) -> Option<Divider> {
        if !self.main.contains(Position { x: col, y: row }) {
            return None;
        }
        let split_x = self.repo_list.x + self.repo_list.width;
        if col.abs_diff(split_x) <= GRAB_SLOP {
            return Some(Divider::Vertical);
        }
        if let Some(inspector) = self.inspector
            && col >= inspector.x
            && row.abs_diff(inspector.y) <= GRAB_SLOP
        {
            return Some(Divider::Horizontal);
        }
        None
    }

    /// The repo-list width the given column would produce, as a percentage of
    /// the main area. Unclamped: `App::set_repo_list_pct` owns the bounds.
    pub fn repo_list_pct_at(&self, col: u16) -> u16 {
        if self.main.width == 0 {
            return 0;
        }
        let offset = col.saturating_sub(self.main.x);
        ((offset as u32 * 100) / self.main.width as u32).min(100) as u16
    }

    /// The inspector height the given row would produce, as a percentage of
    /// the right column. Measured from the bottom, since the inspector is
    /// docked there and the divider is its top edge.
    pub fn inspector_pct_at(&self, row: u16) -> u16 {
        let column_height = self.main.height;
        if column_height == 0 {
            return 0;
        }
        let bottom = self.main.y + column_height;
        let height = bottom.saturating_sub(row);
        ((height as u32 * 100) / column_height as u32).min(100) as u16
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
            main,
            repo_list: main,
            detail: None,
            inspector: None,
        },
        View::Detail => {
            let (repo_list, right) = detail::split_main(main, app.repo_list_pct);
            let (detail, inspector) = detail::split_right_column(right, app.inspector_pct);
            PanelRects {
                main,
                repo_list,
                detail: Some(detail),
                inspector,
            }
        }
    }
}

/// The color a quick filter's `#tag` renders in: the same color that filter's
/// own data already uses in the badge column (`ui::badges`), so the strip and
/// the rows can never disagree about what "dirty" or "ahead" looks like.
fn filter_color(kind: FilterKind) -> Color {
    match kind {
        FilterKind::Dirty => colors::YELLOW,
        // Behind and ahead share PEACH because `badges::status_color` does;
        // the tag word itself is what distinguishes them.
        FilterKind::Behind | FilterKind::Ahead => colors::PEACH,
        FilterKind::CiFailed => colors::RED,
        FilterKind::HasPrs => colors::MAUVE,
        FilterKind::HasStashes => colors::OVERLAY,
    }
}

/// The search strip: a fuzzy search box that doubles as the quick-filter
/// input, modeled on notez/todoz's own strip.
///
/// There is deliberately no row of filter indicator dots. Six unlabeled dots
/// could not be toggled (only typing a `#tag` ever set one), had no legend, and
/// duplicated information the query text already carries in color. The typed
/// query is now the single indicator of what is filtered.
///
/// Four states:
/// - **editing** (`/` active): `/` + the query with a block cursor, plus a dim
///   syntax hint while the query is still empty.
/// - **set but not editing**: `/` + the query + an "esc to clear" reminder.
/// - **unknown tag typed**: the offending word turns red and the reminder is
///   replaced by a named complaint, so a typo can't silently filter nothing.
/// - **idle**: a dim `/search` prompt.
pub fn filter_strip_line(app: &App) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];

    if app.search_active {
        spans.push(Span::styled("/", Style::default().fg(colors::YELLOW)));
        spans.extend(query_spans(&app.search_query));
        // Block cursor sits at the end (fleetz edits at the tail only).
        spans.push(Span::styled(" ", Style::default().bg(colors::SAPPHIRE)));
        if app.search_query.is_empty() {
            spans.push(Span::styled(
                "  fuzzy name, or #dirty #behind #ahead #ci #prs #stash",
                Style::default().fg(colors::SUBTEXT),
            ));
        }
    } else if !app.search_query.is_empty() {
        spans.push(Span::styled("/", Style::default().fg(colors::YELLOW)));
        spans.extend(query_spans(&app.search_query));
    } else {
        spans.push(Span::styled("/", Style::default().fg(colors::YELLOW)));
        spans.push(Span::styled("search", Style::default().fg(colors::OVERLAY)));
    }

    if !app.search_query.is_empty() {
        let unknown = crate::app::parse_query(&app.search_query).unknown_tags;
        if unknown.is_empty() {
            if !app.search_active {
                spans.push(Span::styled(
                    "  esc to clear",
                    Style::default().fg(colors::OVERLAY),
                ));
            }
        } else {
            // Name the bad tags rather than just reddening them: the failure
            // mode being fixed is a filter that silently narrows nothing.
            let label = if unknown.len() == 1 { "tag" } else { "tags" };
            spans.push(Span::styled(
                format!("  unknown {label}: #{}", unknown.join(" #")),
                Style::default().fg(colors::RED),
            ));
        }
    }

    Line::from(spans)
}

/// Render a query string as colored word spans: a recognized `#tag` in its
/// filter's color, an unrecognized one in the alert color, a bare `#`
/// (mid-typing) dim, and free text in the default text color.
/// Separators are emitted *between* words rather than after each one. Trailing
/// one space per word put a phantom gap between the query and the block cursor,
/// and an empty query rendered as a lone space, so `/` sat one cell away from
/// its own caret.
fn query_spans(query: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, word) in query.split(' ').enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        if word.is_empty() {
            continue;
        }
        let style = match word.strip_prefix('#') {
            None => Style::default().fg(colors::TEXT),
            // A lone "#" is the first keystroke of every tag, so it reads as
            // incomplete rather than wrong.
            Some("") => Style::default().fg(colors::OVERLAY),
            Some(name) => {
                let name = name.to_lowercase();
                let matched = crate::app::FILTER_TAGS
                    .iter()
                    .find(|(_, tag)| tag.starts_with(&name));
                match matched {
                    Some((kind, _)) => Style::default()
                        .fg(filter_color(*kind))
                        .add_modifier(Modifier::BOLD),
                    None => Style::default()
                        .fg(colors::RED)
                        .add_modifier(Modifier::BOLD),
                }
            }
        };
        spans.push(Span::styled(word.to_string(), style));
    }
    spans
}

/// A commit author's short code, for the tight author column in the
/// commit/history lists. Sized to fit a 3-wide column (see AUTHOR_W):
/// - one word -> its first three letters, original case
///   ("Gaurgle" -> "Gau", "torvalds" -> "tor")
/// - two or more words -> the first two words' initials, uppercased
///   ("Andreas Roos" -> "AR", "Lars Nils Andreas" -> "LN", "Maria-Hazell" -> "MH")
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
        // The number is the key you press to focus this pane, so it has to
        // stay readable when the pane is not focused -- that is exactly when
        // you need it. Matched to its own label rather than sitting a shade
        // below it, which is how it read before.
        (colors::OVERLAY, Style::default().fg(colors::OVERLAY))
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
        Overlay::TimeWindow(two) => timewindow::render(frame, area, two),
        Overlay::Settings(so) => settings::render(frame, area, so),
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
    // Version rides next to the name in the dimmest color, so "which build am
    // I looking at" is answerable without quitting. First thing the fit loop
    // drops: it is the least urgent piece on the line by a wide margin.
    let version_span = Span::styled(format!(" {VERSION}"), Style::default().fg(colors::SUBTEXT));
    let header_span = (!app.header.is_empty()).then(|| {
        Span::styled(
            format!(" — {}", app.header),
            Style::default().fg(colors::OVERLAY),
        )
    });
    let group_span = app
        .active_group_label()
        .map(|group| Span::styled(format!("  [{group}]"), Style::default().fg(colors::MAUVE)));

    let view_hint = match app.view {
        View::Detail => "[v] compact",
        View::Compact => "[v] detail",
    };
    // Shown while the initial load or an R reload is in flight. Never dropped
    // by the fit loop below: it is small, and while it lasts it is the most
    // important thing on the line.
    let loading_span = app
        .loading
        .then(|| spinner_group(app.spinner_frame, "loading"));
    // Age, not a wall-clock stamp: "swept 8s ago" answers the question the
    // header is actually asked ("is this current?") without arithmetic. While
    // a sweep runs it becomes a spinner instead.
    // Only a sweep the user asked for takes over this slot. A watch-timer
    // sweep runs silently and leaves the "swept Xs ago" clock in place, so an
    // interval refresh cannot repaint a moving colored indicator over a
    // dashboard being read.
    let ts_span = if app.sweep_started.is_some() && app.sweep_manual {
        Some(spinner_group(app.spinner_frame, "sweeping"))
    } else {
        app.last_refreshed.map(|ts| {
            let secs = (chrono::Local::now() - ts).num_seconds().max(0);
            let label = if secs < 5 {
                "just now".to_string()
            } else if secs < 90 {
                format!("{secs}s ago")
            } else {
                format!("{}m ago", secs / 60)
            };
            // A step dimmer than the rest of the header text on purpose: this
            // is reassurance you glance at, not information you read.
            vec![Span::styled(
                format!(" swept {label}  "),
                Style::default().fg(colors::OVERLAY),
            )]
        })
    };
    // Persistent indicator so the `w` toggle's state is visible at a glance,
    // not just in the transient status message when it flips.
    //
    // Pulses between the ringed and solid disk on the blink, which separates it
    // from the CI lamps: those hold one shape and say what they mean by color,
    // this one is the only glyph in the UI that changes shape on its own. Both
    // frames are one cell wide, so the header never shifts.
    let watch_span = app.watch_mode.then(|| {
        let glyph = if app.blink_on { "◉" } else { "●" };
        Span::styled(
            format!(" {glyph} watch "),
            Style::default().fg(colors::GREEN),
        )
    });
    let view_span = Span::styled(view_hint, Style::default().fg(colors::SUBTEXT));
    let help_span = Span::styled("[?] help ", Style::default().fg(colors::SUBTEXT));

    let mut include_version = true;
    let mut include_ts = true;
    let mut include_view = true;
    let mut include_group = true;
    let mut include_header = true;

    loop {
        let mut left: Vec<Span> = vec![name.clone()];
        if include_version {
            left.push(version_span.clone());
        }
        if include_header && let Some(ref h) = header_span {
            left.push(h.clone());
        }
        if include_group && let Some(ref g) = group_span {
            left.push(g.clone());
        }

        let mut right: Vec<Span> = Vec::new();
        if let Some(ref l) = loading_span {
            right.extend(l.iter().cloned());
        }
        if let Some(ref w) = watch_span {
            right.push(w.clone());
        }
        if include_ts {
            if let Some(ref t) = ts_span {
                right.extend(t.iter().cloned());
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
        let exhausted =
            !include_version && !include_ts && !include_view && !include_group && !include_header;

        if left_w + right_w < width || exhausted {
            let block = Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(colors::SURFACE))
                .title_bottom(Line::from(left).left_aligned())
                .title_bottom(Line::from(right).right_aligned());
            frame.render_widget(block, area);
            return;
        }

        if include_version {
            include_version = false;
        } else if include_ts {
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
    use super::*;

    fn rects() -> PanelRects {
        // 100x40 main area, repo list 35 wide, inspector 16 tall at the bottom.
        let main = Rect::new(0, 1, 100, 40);
        PanelRects {
            main,
            repo_list: Rect::new(0, 1, 35, 40),
            detail: Some(Rect::new(36, 1, 64, 24)),
            inspector: Some(Rect::new(36, 25, 64, 16)),
        }
    }

    #[test]
    fn spinner_hole_travels_clockwise() {
        // Every frame is a full cell minus exactly one dot; the dot's bit
        // position is what the eye tracks. Braille bits: 1,2,3 are the left
        // column top-to-bottom, 4,5,6 the right, 7 and 8 the bottom row.
        // Clockwise from the top-left means right column down, bottom, then
        // left column up.
        let missing: Vec<u32> = SPINNER
            .iter()
            .map(|f| {
                let cell = f.chars().next().unwrap() as u32 - 0x2800;
                (0xFF ^ cell).trailing_zeros() + 1 // 1-indexed dot number
            })
            .collect();
        assert_eq!(missing, vec![4, 5, 6, 8, 7, 3, 2, 1]);
    }

    #[test]
    fn every_spinner_frame_is_one_cell_wide() {
        // A wider frame would jitter the header on every tick.
        for f in SPINNER {
            assert_eq!(f.chars().count(), 1, "{f:?}");
        }
    }

    #[test]
    fn only_the_spinner_glyph_fades_not_its_label() {
        // The glyph drifts through the palette; the word beside it must hold
        // one steady color so it stays legible at every point in the cycle.
        let label_color = |frame: usize| spinner_group(frame, "loading")[1].style.fg;
        let glyph_color = |frame: usize| spinner_group(frame, "loading")[0].style.fg;

        // Two frames a full leg apart, so the glyph is definitely a different
        // hue by the second one.
        assert_ne!(glyph_color(0), glyph_color(SPINNER_HUE_TICKS));
        assert_eq!(label_color(0), label_color(SPINNER_HUE_TICKS));
        assert_eq!(label_color(0), Some(colors::PEACH));
    }

    #[test]
    fn spinner_group_keeps_the_glyph_and_label_separate() {
        let spans = spinner_group(0, "sweeping");
        assert_eq!(
            spans.len(),
            2,
            "one span each, so they can be colored apart"
        );
        assert!(spans[0].content.contains(SPINNER[0]));
        assert!(spans[1].content.contains("sweeping"));
        // Together they must still read as one phrase with its old spacing.
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, format!(" {} sweeping ", SPINNER[0]));
    }

    #[test]
    fn spinner_color_walks_the_palette_and_wraps() {
        // Lands exactly on each hue at the start of its leg...
        for (i, hue) in SPINNER_HUES.iter().enumerate() {
            assert_eq!(spinner_color(i * SPINNER_HUE_TICKS), *hue, "leg {i}");
        }
        // ...and returns to the first after a full loop, so there's no seam.
        assert_eq!(
            spinner_color(SPINNER_HUES.len() * SPINNER_HUE_TICKS),
            SPINNER_HUES[0]
        );
    }

    #[test]
    fn spinner_color_is_between_its_two_endpoints_mid_leg() {
        // Midway from LAVENDER to SAPPHIRE: every channel must sit between
        // the two, which is what makes it read as a fade rather than a jump.
        let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (SPINNER_HUES[0], SPINNER_HUES[1])
        else {
            panic!("palette must be RGB to interpolate");
        };
        let Color::Rgb(r, g, b) = spinner_color(SPINNER_HUE_TICKS / 2) else {
            panic!("blend of two RGB colors must be RGB");
        };
        assert!(r >= ar.min(br) && r <= ar.max(br), "red {r}");
        assert!(g >= ag.min(bg) && g <= ag.max(bg), "green {g}");
        assert!(b >= ab.min(bb) && b <= ab.max(bb), "blue {b}");
    }

    #[test]
    fn an_empty_query_renders_nothing_before_the_caret() {
        // The reported gap: an empty query used to emit a lone space, so `/`
        // sat one cell away from its own block cursor.
        assert!(query_spans("").is_empty());
        // And a typed query must not trail a space into the caret either.
        let text: String = query_spans("fle")
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(text, "fle");
    }

    #[test]
    fn query_spans_keep_interior_spacing() {
        let text: String = query_spans("a #dirty")
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert_eq!(text, "a #dirty");
    }

    #[test]
    fn vertical_divider_is_grabbable_with_slop() {
        let r = rects();
        // The gap itself is one cell; a single-cell target is unhittable, so
        // one column either side counts too.
        assert_eq!(r.divider_at(35, 10), Some(Divider::Vertical));
        assert_eq!(r.divider_at(34, 10), Some(Divider::Vertical));
        assert_eq!(r.divider_at(36, 10), Some(Divider::Vertical));
        assert_eq!(r.divider_at(20, 10), None, "well inside the repo list");
    }

    #[test]
    fn horizontal_divider_only_grabs_over_the_right_column() {
        let r = rects();
        assert_eq!(r.divider_at(60, 25), Some(Divider::Horizontal));
        // Same row, but over the repo list, which has no horizontal divider.
        assert_eq!(r.divider_at(10, 25), None);
    }

    #[test]
    fn a_click_outside_the_main_area_grabs_nothing() {
        let r = rects();
        assert_eq!(r.divider_at(35, 0), None, "header row");
        assert_eq!(r.divider_at(35, 99), None, "below the panels");
    }

    #[test]
    fn drag_position_converts_back_to_a_percentage() {
        let r = rects();
        assert_eq!(r.repo_list_pct_at(50), 50, "midpoint of a 100-wide area");
        assert_eq!(r.repo_list_pct_at(25), 25);
        assert_eq!(r.repo_list_pct_at(0), 0, "clamping is the caller's job");

        // The inspector is docked at the bottom, so its size is measured from
        // there: dragging its top edge upward makes it bigger.
        assert_eq!(r.inspector_pct_at(21), 50, "half the 40-row column");
        assert_eq!(r.inspector_pct_at(31), 25);
    }

    #[test]
    fn a_zero_sized_area_does_not_divide_by_zero() {
        let empty = PanelRects {
            main: Rect::new(0, 0, 0, 0),
            repo_list: Rect::new(0, 0, 0, 0),
            detail: None,
            inspector: None,
        };
        assert_eq!(empty.repo_list_pct_at(10), 0);
        assert_eq!(empty.inspector_pct_at(10), 0);
    }

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
