pub mod badges;
pub mod branches;
pub mod colors;
pub mod compact;
pub mod contributions;
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

/// How the last sweep is labelled in the header: a wall-clock timestamp, not a
/// relative age.
///
/// A relative age is an animation by construction. "swept 14s ago" can only
/// stay true by repainting, so it is a moving part whose entire job is keeping
/// itself honest -- it churned away at an empty terminal all through lunch to
/// say what one timestamp says at a glance. This is written once per sweep and
/// then sits still.
///
/// The date appears only when the sweep was not today. fleetz lives in a tmux
/// pane and is regularly open across midnight, where a bare clock time reads
/// as today and silently means yesterday.
pub fn swept_label(
    ts: chrono::DateTime<chrono::Local>,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    if ts.date_naive() == now.date_naive() {
        ts.format("%H:%M").to_string()
    } else {
        ts.format("%b %-d %H:%M").to_string()
    }
}

/// Refresh intervals that must elapse before the timestamp reads as overdue.
/// Generous on purpose: a sweep landing a little late is ordinary, and a
/// warning that cries wolf is one you learn to ignore.
const OVERDUE_INTERVALS: i64 = 2;

/// Whether the last sweep is old enough that watch mode is probably wedged.
///
/// Only meaningful under watch: with no promised cadence there is nothing to
/// be late against, so a manually refreshed dashboard never nags. Colouring on
/// this is a change that happens once, not an animation, so it buys the "watch
/// is stuck" signal back without reintroducing motion.
pub fn sweep_overdue(
    watch_mode: bool,
    refresh_interval: u64,
    ts: chrono::DateTime<chrono::Local>,
    now: chrono::DateTime<chrono::Local>,
) -> bool {
    watch_mode && (now - ts).num_seconds() > refresh_interval as i64 * OVERDUE_INTERVALS
}

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
    /// The area the detail/inspector split was performed on. Separate from
    /// `main` because in Detail view the strip is carved off this column
    /// alone, so it is shorter than `main` and the inspector's percentage
    /// must be measured against it rather than against the full height.
    pub right_column: Rect,
    pub repo_list: Rect,
    pub detail: Option<Rect>,
    pub inspector: Option<Rect>,
    /// The contribution strip, when it is open and the area had room for it.
    pub contributions: Option<Rect>,
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
        if self.contributions.is_some_and(|r| r.contains(p)) {
            return Some(FocusedPanel::Contributions);
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
        let column_height = self.right_column.height;
        if column_height == 0 {
            return 0;
        }
        let bottom = self.right_column.y + column_height;
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
    let open = app.contributions.is_open();

    match app.view {
        // Compact has no right column for the strip to dock under, so it
        // comes off the bottom of the whole area and the list shrinks by it.
        View::Compact => {
            let (body, contributions) = contributions::split_main_area(main, open);
            PanelRects {
                main: body,
                right_column: body,
                repo_list: body,
                detail: None,
                inspector: None,
                contributions,
            }
        }
        // Detail has a choice, and it turns on width.
        //
        // Wide: split horizontally FIRST, so the repo list runs the full
        // height and only the right column pays for the strip. The list is a
        // single column of names, so vertical room is the one thing it can
        // always use, and the strip is right-aligned and gives up only its
        // oldest weeks by being narrower.
        //
        // Narrow: that trade stops paying. A right column too thin to show
        // even a year turns the strip into a keyhole, which costs more than
        // the rows the list gains, so the strip spans the full width and the
        // panes above shrink for it as they used to.
        View::Detail => {
            let (wide_list, wide_right) = detail::split_main(main, app.repo_list_pct);
            if contributions::worth_docking_in_column(wide_right) {
                let (right_column, contributions) =
                    contributions::split_main_area(wide_right, open);
                let (detail, inspector) =
                    detail::split_right_column(right_column, app.inspector_pct);
                PanelRects {
                    main,
                    right_column,
                    repo_list: wide_list,
                    detail: Some(detail),
                    inspector,
                    contributions,
                }
            } else {
                let (body, contributions) = contributions::split_main_area(main, open);
                let (repo_list, right_column) = detail::split_main(body, app.repo_list_pct);
                let (detail, inspector) =
                    detail::split_right_column(right_column, app.inspector_pct);
                PanelRects {
                    main: body,
                    right_column,
                    repo_list,
                    detail: Some(detail),
                    inspector,
                    contributions,
                }
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
        FilterKind::HasPrs => colors::PR_CLR,
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

    // Main content area. Drawn from `panel_rects` itself rather than from a
    // parallel copy of the same arithmetic: hit-testing is then guaranteed to
    // run against the exact rectangles that were painted, instead of merely
    // being kept in step with them by hand. The strip's position depends on
    // the view now, which is one more thing two copies would have to agree
    // about.
    let rects = panel_rects(area, app);
    // A load that failed with nothing already on screen gets the main area to
    // itself. Both views derive their panes from `app.repos`, so an empty
    // fleet and a failed fetch draw the same empty furniture; without this the
    // only difference between them was a status line the next keypress wiped.
    //
    // Guarded on `repos.is_empty()` so a failed *reload* never blanks a
    // dashboard that still has good data to show: that case keeps its rows and
    // says so in the status line and the header instead.
    if app.shows_load_error() {
        render_load_error(frame, rects.main, app);
    } else {
        match app.view {
            View::Detail => {
                if let Some(detail_area) = rects.detail {
                    detail::render(frame, rects.repo_list, detail_area, rects.inspector, app);
                }
            }
            View::Compact => compact::render(frame, rects.main, app),
        }
    }
    if let Some(strip) = rects.contributions {
        contributions::render(frame, strip, app);
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

/// Draw why there is nothing to show, in the space the repos would have
/// filled. Only reached when a load failed and left the fleet empty.
fn render_load_error(frame: &mut Frame, area: Rect, app: &App) {
    let Some(reason) = app.load_error.as_deref() else {
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            "Could not load the fleet",
            Style::default()
                .fg(colors::PEACH)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(reason, Style::default().fg(colors::TEXT))),
        Line::from(""),
        Line::from(Span::styled(
            "R retries against GitHub, r rebuilds from cache",
            Style::default().fg(colors::OVERLAY),
        )),
    ];
    // Rate limits are the common way this happens and they clear on their own,
    // which is worth saying: the reflex otherwise is to go looking for a bug.
    if reason.contains("rate limit") || reason.contains("RATE_LIMIT") {
        lines.push(Line::from(Span::styled(
            "GitHub rate limits reset hourly; this clears on its own.",
            Style::default().fg(colors::OVERLAY),
        )));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::SURFACE));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true }),
        inner,
    );
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
    // The one key hint in the header, and only because it sits on the thing it
    // edits: `t` opens the picker for exactly the window named to its left.
    // Every other key is a global action and lives in the footer with the rest.
    // Dropped with the header text it annotates, since a bare `[t]` next to
    // nothing explains nothing.
    let window_key_span = (!app.header.is_empty()).then(|| {
        Span::styled(
            " [t]",
            Style::default()
                .fg(colors::TEXT)
                .add_modifier(Modifier::BOLD),
        )
    });
    let group_span = app
        .active_group_label()
        .map(|group| Span::styled(format!("  [{group}]"), Style::default().fg(colors::MAUVE)));
    // Shown while the initial load or an R reload is in flight. Never dropped
    // by the fit loop below: it is small, and while it lasts it is the most
    // important thing on the line.
    let loading_span = app
        .loading
        .then(|| spinner_group(app.spinner_frame, "loading"));
    // Only a sweep the user asked for takes over this slot. A watch-timer
    // sweep runs silently and leaves the timestamp in place, so an interval
    // refresh cannot repaint a moving colored indicator over a dashboard
    // being read.
    let ts_span = if app.sweep_started.is_some() && app.sweep_manual {
        Some(spinner_group(app.spinner_frame, "sweeping"))
    } else {
        app.last_refreshed.map(|ts| {
            let now = chrono::Local::now();
            // Dimmer than the rest of the header on purpose: reassurance you
            // glance at, not information you read. Unless it has gone stale,
            // which is the one time it has something to say.
            let color = if sweep_overdue(app.watch_mode, app.refresh_interval, ts, now) {
                colors::PEACH
            } else {
                colors::OVERLAY
            };
            vec![Span::styled(
                format!(" swept {}  ", swept_label(ts, now)),
                Style::default().fg(color),
            )]
        })
    };
    // A GitHub poll that keeps failing has to say so somewhere that survives
    // a keypress. The status message it also sets is cleared by the next key,
    // so a rate limit -- which lasts an hour and is the common way this fails
    // -- left the CI lamps and the contribution strip frozen at their last
    // good values with nothing on screen to say they had stopped moving.
    //
    // `PEACH`, not `RED`: red already means CI failed, and this is the same
    // thing the overdue `swept` timestamp beside it means, which is that a
    // refresh is not keeping up. Never dropped by the fit loop, for the same
    // reason `loading` is not: while it lasts it is the most important thing
    // on the line.
    let gh_fail_span = app
        .github_poll_failing
        .then(|| Span::styled(" ⚠ github ", Style::default().fg(colors::PEACH)));
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
    let mut include_version = true;
    let mut include_ts = true;
    let mut include_group = true;
    let mut include_header = true;

    loop {
        let mut left: Vec<Span> = vec![name.clone()];
        if include_version {
            left.push(version_span.clone());
        }
        if include_header && let Some(ref h) = header_span {
            left.push(h.clone());
            if let Some(ref k) = window_key_span {
                left.push(k.clone());
            }
        }
        if include_group && let Some(ref g) = group_span {
            left.push(g.clone());
        }

        let mut right: Vec<Span> = Vec::new();
        if let Some(ref l) = loading_span {
            right.extend(l.iter().cloned());
        }
        if let Some(ref g) = gh_fail_span {
            right.push(g.clone());
        }
        if let Some(ref w) = watch_span {
            right.push(w.clone());
        }
        if include_ts && let Some(ref t) = ts_span {
            right.extend(t.iter().cloned());
        }

        let left_w: usize = left.iter().map(|s| s.content.len()).sum();
        let right_w: usize = right.iter().map(|s| s.content.len()).sum();
        let exhausted = !include_version && !include_ts && !include_group && !include_header;

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
            // No strip in this fixture, so the right column is the full
            // height and the inspector percentage is measured against it.
            right_column: Rect::new(36, 1, 64, 40),
            repo_list: Rect::new(0, 1, 35, 40),
            detail: Some(Rect::new(36, 1, 64, 24)),
            inspector: Some(Rect::new(36, 25, 64, 16)),
            contributions: None,
        }
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .single()
            .expect("unambiguous local time")
    }

    /// The rendered header line, border and all.
    fn header_row(header: &str, width: u16) -> String {
        header_row_with(header, width, |_| {})
    }

    /// The same, with a hook to set app state the header reads.
    fn header_row_with(
        header: &str,
        width: u16,
        setup: impl FnOnce(&mut crate::app::App),
    ) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::app::App::new(Vec::new(), header.to_string(), width, 40);
        setup(&mut app);
        let app = app;
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal.draw(|f| render_header(f, f.area(), &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>()
    }

    #[test]
    fn the_window_key_sits_on_the_window_it_edits() {
        // The only key hint in the header, and it earns its place by being
        // attached to the state it changes.
        let row = header_row("since 5 days", 100);
        assert!(row.contains("since 5 days [t]"), "got {row:?}");
    }

    #[test]
    fn global_action_keys_are_not_in_the_header() {
        // They live in the footer with the rest. `[v]` used to appear in both,
        // which is the duplication this guards against coming back.
        let row = header_row("since 5 days", 100);
        assert!(
            !row.contains("[v]"),
            "view hint belongs to the footer: {row:?}"
        );
        assert!(
            !row.contains("[?]"),
            "help hint belongs to the footer: {row:?}"
        );
    }

    #[test]
    fn the_window_key_goes_with_the_label_it_annotates() {
        // A bare `[t]` next to nothing explains nothing, so it is dropped
        // whenever the header text it points at is absent.
        let row = header_row("", 100);
        assert!(!row.contains("[t]"), "got {row:?}");
    }

    #[test]
    fn a_sweep_today_shows_only_the_clock_time() {
        let ts = at(2026, 8, 6, 11, 42);
        let now = at(2026, 8, 6, 14, 3);
        assert_eq!(swept_label(ts, now), "11:42");
    }

    #[test]
    fn a_sweep_before_today_shows_its_date() {
        // The tmux-across-midnight case: a bare "23:11" reads as today and
        // would silently mean yesterday.
        let ts = at(2026, 8, 5, 23, 11);
        let now = at(2026, 8, 6, 9, 0);
        assert_eq!(swept_label(ts, now), "Aug 5 23:11");
    }

    #[test]
    fn the_label_is_stable_as_time_passes() {
        // The whole point of dropping the relative age: the same sweep renders
        // identically an hour later, so nothing has to repaint to stay true.
        let ts = at(2026, 8, 6, 11, 42);
        assert_eq!(
            swept_label(ts, at(2026, 8, 6, 11, 43)),
            swept_label(ts, at(2026, 8, 6, 18, 30))
        );
    }

    #[test]
    fn overdue_needs_watch_mode_and_two_intervals() {
        let ts = at(2026, 8, 6, 11, 0);
        let interval = 60;

        // Not watching: no promised cadence, so never late.
        assert!(!sweep_overdue(false, interval, ts, at(2026, 8, 6, 12, 0)));
        // Watching, but inside the grace window.
        assert!(!sweep_overdue(true, interval, ts, at(2026, 8, 6, 11, 1)));
        // Watching and past two intervals.
        assert!(sweep_overdue(true, interval, ts, at(2026, 8, 6, 11, 5)));
    }

    #[test]
    fn overdue_scales_with_the_configured_interval() {
        // A five-minute interval must not be judged against a one-minute one.
        let ts = at(2026, 8, 6, 11, 0);
        let now = at(2026, 8, 6, 11, 8);
        assert!(sweep_overdue(true, 60, ts, now), "60s interval is late");
        assert!(!sweep_overdue(true, 600, ts, now), "600s interval is not");
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
            right_column: Rect::new(0, 0, 0, 0),
            repo_list: Rect::new(0, 0, 0, 0),
            detail: None,
            inspector: None,
            contributions: None,
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

    fn layout_app() -> crate::app::App {
        crate::app::App::new(Vec::new(), "test".to_string(), 120, 44)
    }

    /// `panel_rects` and `render` must agree about the strip, or clicks land
    /// on a pane that is not where the eye sees it.
    #[test]
    fn panel_rects_reports_the_strip_only_when_it_is_drawn() {
        // Wide enough that the right column can still hold a year, which is
        // what buys the full-height repo list.
        let area = Rect::new(0, 0, 200, 44);
        let mut app = layout_app();
        app.view = crate::app::View::Detail;

        let closed = panel_rects(area, &app);
        assert!(closed.contributions.is_none());

        app.contributions = crate::app::ContributionsMode::Github;
        let open = panel_rects(area, &app);
        let strip = open.contributions.expect("strip present");
        assert_eq!(strip.height, contributions::CONTRIB_HEIGHT);
        assert_eq!(
            open.at(strip.x + 2, strip.y + 2),
            Some(FocusedPanel::Contributions),
            "a click in the strip lands on the strip"
        );

        // In Detail the strip is carved out of the right column alone, so the
        // repo list runs the full height either way and only the right column
        // pays for it. It used to come off the bottom of everything, which
        // wasted the one dimension a single column of names can always use.
        assert_eq!(
            open.repo_list.height, closed.repo_list.height,
            "the repo list should not give up rows to the strip"
        );
        assert!(
            open.right_column.height < closed.right_column.height,
            "the right column is what shrinks"
        );
        // The strip must sit under the right column, not under the list.
        assert_eq!(strip.x, open.right_column.x);
        assert_eq!(strip.width, open.right_column.width);
        assert!(strip.x >= open.repo_list.x + open.repo_list.width);
    }

    /// The repo list reaching past the strip only helps if a click down there
    /// still selects a repo. `at` walks the same rects the renderer drew, so
    /// this catches the strip's rows being claimed by the wrong pane.
    #[test]
    fn a_click_beside_the_strip_still_lands_on_the_repo_list() {
        let area = Rect::new(0, 0, 200, 44);
        let mut app = layout_app();
        app.view = crate::app::View::Detail;
        app.contributions = crate::app::ContributionsMode::Github;
        let rects = panel_rects(area, &app);
        let strip = rects.contributions.expect("strip present");

        // Same rows as the strip, but over on the list's side.
        assert_eq!(
            rects.at(rects.repo_list.x + 2, strip.y + 2),
            Some(FocusedPanel::RepoList),
        );
    }

    /// The full-height list only pays while the right column can still show a
    /// useful strip. Squeeze the terminal and the strip must go back to
    /// spanning the whole width, with the panes above giving up the rows,
    /// rather than becoming a keyhole in a narrow column.
    #[test]
    fn a_narrow_terminal_gives_the_strip_the_full_width_back() {
        let mut app = layout_app();
        app.view = crate::app::View::Detail;
        app.contributions = crate::app::ContributionsMode::Github;

        let narrow = panel_rects(Rect::new(0, 0, 120, 44), &app);
        let strip = narrow.contributions.expect("strip present");
        assert_eq!(strip.x, narrow.main.x, "strip starts at the left edge");
        assert_eq!(strip.width, narrow.main.width, "strip spans the width");
        assert!(
            narrow.repo_list.height < 44 - contributions::CONTRIB_HEIGHT,
            "the list gives its rows back"
        );

        // The same app on a wide terminal takes the docked layout instead.
        let wide = panel_rects(Rect::new(0, 0, 200, 44), &app);
        let wide_strip = wide.contributions.expect("strip present");
        assert!(wide_strip.x > wide.main.x, "docked under the right column");
        assert_eq!(wide.repo_list.height, wide.main.height);
    }

    /// Compact has no detail or inspector, but it does have the strip.
    #[test]
    fn the_strip_is_laid_out_in_compact_view_too() {
        let area = Rect::new(0, 0, 120, 44);
        let mut app = layout_app();
        app.view = crate::app::View::Compact;
        app.contributions = crate::app::ContributionsMode::Local;
        let rects = panel_rects(area, &app);
        assert!(rects.contributions.is_some());
        assert!(rects.detail.is_none());
        assert!(rects.repo_list.height < area.height - contributions::CONTRIB_HEIGHT);
    }

    /// A terminal too short for both loses the strip, not the dashboard.
    #[test]
    fn a_short_terminal_drops_the_strip_rather_than_the_panes() {
        let mut app = layout_app();
        app.contributions = crate::app::ContributionsMode::Github;
        let rects = panel_rects(Rect::new(0, 0, 120, 18), &app);
        assert!(rects.contributions.is_none());
        assert!(rects.repo_list.height > 0, "the panes survive");
    }

    /// A GitHub poll that failed has to leave a mark that a keypress cannot
    /// clear. It used to set only a status message, so a rate limit -- an
    /// hour long -- was invisible within one keystroke, and the frozen CI
    /// lamps and contribution strip looked like current data.
    #[test]
    fn a_failing_github_poll_is_marked_in_the_header() {
        let row = header_row_with("", 80, |app| app.github_poll_failing = true);
        assert!(row.contains("⚠ github"), "no failure marker in: {row:?}");
    }

    /// The other half: a healthy poll must not leave a warning sitting there.
    #[test]
    fn a_healthy_github_poll_leaves_the_header_clean() {
        let row = header_row_with("", 80, |app| app.github_poll_failing = false);
        assert!(!row.contains("⚠"), "spurious failure marker in: {row:?}");
    }

    /// The fit loop drops the version, then the timestamp, then the group,
    /// then the header text. This marker is not on that list: a narrow
    /// terminal is not a reason to hide that the data stopped updating.
    #[test]
    fn the_failure_marker_survives_a_narrow_header() {
        let row = header_row_with("last 7 days", 30, |app| app.github_poll_failing = true);
        assert!(
            row.contains("⚠ github"),
            "marker dropped by the fit loop: {row:?}"
        );
    }

    /// The whole screen rendered, so the test sees what the user sees.
    fn screen(width: u16, height: u16, setup: impl FnOnce(&mut crate::app::App)) -> String {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::app::App::new(Vec::new(), String::new(), width, height);
        setup(&mut app);
        let app = app;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The failure this closes. A failed initial load left `repos` empty and
    /// said why only in a status message, which `clear_status` wipes on the
    /// next keypress -- so a rate-limited launch was a blank dashboard with no
    /// stated cause, identical to a fleet with nothing in the window.
    #[test]
    fn a_failed_load_says_why_instead_of_drawing_nothing() {
        let out = screen(80, 24, |app| {
            app.load_error = Some("GitHub API errors: API rate limit exceeded".to_string());
        });
        assert!(
            out.contains("Could not load the fleet"),
            "no cause given:\n{out}"
        );
        assert!(
            out.contains("rate limit exceeded"),
            "the reason itself is missing:\n{out}"
        );
    }

    /// A rate limit clears itself, and saying so is the difference between
    /// waiting and going bug-hunting.
    #[test]
    fn a_rate_limited_load_says_it_clears_on_its_own() {
        let out = screen(80, 24, |app| {
            app.load_error = Some("GitHub API errors: API rate limit exceeded".to_string());
        });
        assert!(out.contains("clears on its own"), "no reassurance:\n{out}");
        // A different failure must not claim to be self-healing.
        let other = screen(80, 24, |app| {
            app.load_error = Some("GitHub API request failed: dns error".to_string());
        });
        assert!(
            !other.contains("clears on its own"),
            "wrong advice:\n{other}"
        );
    }

    /// The clean case: nothing failed, so nothing is claimed.
    #[test]
    fn a_healthy_launch_shows_no_error_panel() {
        let out = screen(80, 24, |app| app.load_error = None);
        assert!(!out.contains("Could not load the fleet"));
    }
}
