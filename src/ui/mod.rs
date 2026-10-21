pub mod badges;
pub mod branches;
pub mod colors;
pub mod compact;
pub mod contributions;
pub mod detail;
pub mod footer;
pub mod grips;
pub mod help;
pub mod intro;
pub mod settings;
pub mod timewindow;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

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

/// The words the header and the `R` status line use for a failed GitHub
/// attempt. One place, so the wording can change without hunting.
pub fn github_failure_label(failure: &crate::github::Failure) -> &'static str {
    use crate::github::FailureKind;
    match failure.kind {
        FailureKind::Offline => "offline",
        // Decided by the fetch layer: GitHub's error `type` for a body, the
        // status and rate headers for a 403 or 429.
        FailureKind::Api | FailureKind::Http if failure.rate_limited => "rate limited",
        FailureKind::Http => "http error",
        FailureKind::Api => "api error",
        FailureKind::Auth => "not signed in",
        FailureKind::Malformed => "bad response",
        FailureKind::TimedOut => "timed out",
        // A 5xx answer: GitHub's side, not the network's.
        FailureKind::Server => "server error",
        // A 401, or a 403 that is not a limit: the token or its permissions.
        FailureKind::Refused => "access refused",
    }
}

/// Every word fleetz draws for a failed `git fetch`, in one place so the
/// wording can change without hunting: the row marker's short form, the
/// detail pane's phrase, and the status line after a fetch someone asked for.
///
/// The short form is a word, not a number: a failed fetch is a state, and
/// counters are the only numeric badges. Colours are `PEACH` for the marker
/// and the detail line, the "a refresh is not keeping up" colour the stale
/// GitHub header uses; red already means CI failed.
pub fn fetch_failure_short(kind: crate::git::FetchFailureKind) -> &'static str {
    use crate::git::FetchFailureKind;
    match kind {
        FetchFailureKind::TimedOut => "timeout",
        FetchFailureKind::Auth => "auth",
        FetchFailureKind::Unreachable => "unreachable",
        FetchFailureKind::NoRemote => "no remote",
        FetchFailureKind::Other => "fetch",
        FetchFailureKind::TooBig => "too big",
    }
}

/// The detail pane's and the status line's phrase for a fetch failure kind.
pub fn fetch_failure_phrase(kind: crate::git::FetchFailureKind) -> &'static str {
    use crate::git::FetchFailureKind;
    match kind {
        FetchFailureKind::TimedOut => "timed out",
        FetchFailureKind::Auth => "authentication failed",
        FetchFailureKind::Unreachable => "remote unreachable",
        FetchFailureKind::NoRemote => "remote not found",
        FetchFailureKind::Other => "git error",
        FetchFailureKind::TooBig => "over the size cap",
    }
}

/// The row marker for a repo whose last fetch failed, or None.
pub fn fetch_marker(facts: &crate::model::FetchFacts) -> Option<String> {
    let failure = facts.failure()?;
    Some(format!("⚠ {}", fetch_failure_short(failure.kind)))
}

/// A byte count the way the row names it: git's binary units, named as
/// people say them. Never zero: a single byte is `1 KB`.
fn size_label(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    match bytes {
        b if b < MIB => format!("{} KB", b.div_ceil(KIB).max(1)),
        b if b < GIB => format!("{} MB", b / MIB),
        b => format!("{:.1} GB", b as f64 / GIB as f64),
    }
}

/// The row's words for a fetch running now, once bytes have arrived:
/// `fetching 312 MB` (FZ-35a). None before the first byte, and once the
/// fetch has landed.
pub fn fetch_progress_label(facts: &crate::model::FetchFacts) -> Option<String> {
    let bytes = facts.progress?.bytes;
    (bytes > 0).then(|| format!("fetching {}", size_label(bytes)))
}

/// The row's words for a repo the fetch cap holds until `f` (FZ-35b):
/// `fetch ~300 MB, press f` when GitHub says it grew that much since the
/// last fetch, `fetch over 200 MB, press f` when a fetch was cut at the cap
/// after that much. The label is the question and `f` the answer, so there
/// is no confirmation step.
pub fn fetch_hold_label(facts: &crate::model::FetchFacts) -> Option<String> {
    use crate::model::SizeHold;
    Some(match facts.size_hold()? {
        SizeHold::Grown(bytes) => format!("fetch ~{}, press f", size_label(bytes)),
        SizeHold::OverCap(bytes) => format!("fetch over {}, press f", size_label(bytes)),
    })
}

/// The detail pane's fetch line: that the last fetch failed, why, and when
/// the remote refs behind `behind` and `ahead` are from. Written the way the
/// header writes stale GitHub data (`as of 14:02`, `age unknown`). None
/// while the last fetch did not fail.
pub fn fetch_failure_line(
    facts: &crate::model::FetchFacts,
    now: chrono::DateTime<chrono::Local>,
) -> Option<String> {
    let failure = facts.failure()?;
    let phrase = fetch_failure_phrase(failure.kind);
    Some(match facts.last_success {
        Some(at) => format!("⚠ fetch failed: {phrase}, as of {}", swept_label(at, now)),
        None => format!("⚠ fetch failed: {phrase}, age unknown"),
    })
}

/// The status line after fetches someone asked for, when any failed: the
/// repo and why for a single fetch, a count for a batch.
pub fn fetch_report_label(report: &crate::app::FetchReport) -> String {
    match report.failed.as_slice() {
        [(name, kind)] if report.total == 1 => {
            format!("fetch failed: {name}, {}", fetch_failure_phrase(*kind))
        }
        [(name, _)] => format!("1 of {} fetches failed: {name}", report.total),
        failed => format!("{} of {} fetches failed", failed.len(), report.total),
    }
}

/// The header's GitHub freshness segment: the full text, the short form a
/// narrow header falls back to, and its colour. `None` when the data is
/// fresh, which is the ordinary case and says nothing.
///
/// Stale is `PEACH`, not `RED`: red already means CI failed, and this is the
/// same thing the overdue `swept` timestamp means, that a refresh is not
/// keeping up. Cached is the dim `OVERLAY` of the `swept` label: an attempt
/// is on its way and it is reassurance, not a warning. Ages are written the
/// way `swept_label` writes them.
pub fn github_freshness_segment(
    freshness: &crate::app::GithubFreshness,
    now: chrono::DateTime<chrono::Local>,
) -> Option<(String, String, Color)> {
    use crate::app::{DataTime, Outcome};
    match (&freshness.outcome, freshness.data_time) {
        (Outcome::Failed(failure), data_time) => Some(failed_segment(
            github_failure_label(failure),
            freshness.last_attempt,
            data_time,
            now,
        )),
        (Outcome::NotAttempted, DataTime::At(t)) => Some((
            format!(" github cached {} ", swept_label(t, now)),
            " github cached ".to_string(),
            colors::OVERLAY,
        )),
        (Outcome::NotAttempted, DataTime::Unknown) => Some((
            " github cached, age unknown ".to_string(),
            " github cached ".to_string(),
            colors::OVERLAY,
        )),
        (Outcome::NotAttempted, DataTime::NoData) | (Outcome::Success, _) => None,
    }
}

/// The stale form: the failure's words, when the failed attempt was
/// (`at 12:02`, always, however recent; #79), and the age of the data on
/// screen. Both times are written the way `swept_label` writes them.
fn failed_segment(
    label: &str,
    attempt: Option<chrono::DateTime<chrono::Local>>,
    data_time: crate::app::DataTime,
    now: chrono::DateTime<chrono::Local>,
) -> (String, String, Color) {
    use crate::app::DataTime;
    let stamp = attempt.map_or(String::new(), |t| format!(" at {}", swept_label(t, now)));
    let long = match data_time {
        DataTime::At(t) => format!(" ⚠ github {label}{stamp}, as of {} ", swept_label(t, now)),
        DataTime::Unknown => format!(" ⚠ github {label}{stamp}, age unknown "),
        DataTime::NoData => format!(" ⚠ github {label}{stamp} "),
    };
    (long, " ⚠ github ".to_string(), colors::PEACH)
}

/// The header's whole GitHub segment: `github_freshness_segment`, plus what
/// is holding polling back (`docs/api-budget-design.md` 4.6).
///
/// - Rate limited: the failure form with when polling resumes, while the
///   limit lasts, with watch on or off, since `r` and `R` wait for it too.
///   That form drops the attempt stamp; it returns once the limit passes.
/// - Paused at the reserve, or with too little left for one poll: until the
///   reset. `PEACH` like a failure, without its `⚠`: nothing failed, but
///   the data will age until then.
/// - Slowed: the pace spaces polls past the configured interval. Dim, like
///   `cached`: the dashboard still updates, only less often.
/// - Backing off after failures: nothing new, the failure form already says
///   it failed and how old the data is.
///
/// Paused and slowed describe automatic polls, so only under watch: with
/// watch off the eye already says nothing polls. No remaining-allowance
/// number: it says nothing actionable.
pub fn github_segment(
    app: &App,
    now: chrono::DateTime<chrono::Local>,
) -> Option<(String, String, Color)> {
    use crate::app::{Outcome, Pace};
    let freshness = &app.github_freshness;
    let (poll_secs, poll_cost) = (app.github_poll_secs, app.github_poll_cost);
    if let Outcome::Failed(failure) = &freshness.outcome {
        if failure.rate_limited
            && let Some(until) = app.github_limit_until(poll_secs, poll_cost, now)
        {
            let label = format!(
                "{} until {}",
                github_failure_label(failure),
                swept_label(until, now)
            );
            // No attempt stamp while the limit lasts: the resume time is
            // the one that matters, and two clock times in a row read badly.
            return Some(failed_segment(&label, None, freshness.data_time, now));
        }
        return github_freshness_segment(freshness, now);
    }
    if app.watch_mode {
        match app.github_pace(poll_cost) {
            Pace::Paused { until } if until > now => {
                return Some((
                    format!(
                        " github paused until {}, allowance nearly used ",
                        swept_label(until, now)
                    ),
                    " github paused ".to_string(),
                    colors::PEACH,
                ));
            }
            Pace::Every(secs) if secs > poll_secs => {
                return Some((
                    " github slowed, allowance low ".to_string(),
                    " github slowed ".to_string(),
                    colors::OVERLAY,
                ));
            }
            _ => {}
        }
    }
    github_freshness_segment(freshness, now)
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

/// The header's watch indicator: a Nerd Font eye while watching
/// (`nf-md-eye`), the same eye struck through when not (`nf-md-eye_off`).
const WATCH_ON_GLYPH: &str = "\u{f0208}";
const WATCH_OFF_GLYPH: &str = "\u{f0209}";

/// The plain-font watch marks: an open eye `o` and a shut one `-`. ASCII,
/// so one column in every monospace font, and the word `watch` beside them
/// says what they are, as it does for the Nerd Font eye.
const PLAIN_WATCH_ON_GLYPH: &str = "o";
const PLAIN_WATCH_OFF_GLYPH: &str = "-";

/// The header's watch mark for `on` in the active set
/// (`badges::active_icons`, the same one the rows draw from): the one place
/// that decides between the Nerd Font eye and its plain stand-in.
pub fn watch_icon(on: bool) -> &'static str {
    match (badges::active_icons(), on) {
        (crate::app::Icons::Nerd, true) => WATCH_ON_GLYPH,
        (crate::app::Icons::Nerd, false) => WATCH_OFF_GLYPH,
        (crate::app::Icons::Plain, true) => PLAIN_WATCH_ON_GLYPH,
        (crate::app::Icons::Plain, false) => PLAIN_WATCH_OFF_GLYPH,
    }
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
///
/// `den` must be non-zero. The spinner walks its palette with this over time;
/// the detail pane's headliner rule fades with it over distance.
pub(crate) fn blend(a: Color, b: Color, num: u32, den: u32) -> Color {
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
    /// The whole area the panels are laid out in, so a click can be tested
    /// for being inside it at all.
    pub main: Rect,
    /// The part of `main` the pane splits are actually computed in: `main`
    /// minus the contribution strip and the blank row above it.
    ///
    /// A drag converts back into a percentage of *this*, not of `main`. The
    /// two differ by eleven rows whenever the strip is open, and dividing by
    /// the wrong one sent the horizontal divider four to five rows away from
    /// the cursor the instant it was grabbed.
    pub body: Rect,
    pub repo_list: Rect,
    pub detail: Option<Rect>,
    pub inspector: Option<Rect>,
    /// Whether that inspector is folded, which leaves it nothing to resize.
    pub inspector_folded: bool,
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

/// Rows a folded pane keeps: its title row alone, a rule carrying the tag,
/// so it still reads as a pane (and shows focus) with its content collapsed.
/// A full frame spent two more rows on a blank line and a bottom border.
pub const FOLDED_HEIGHT: u16 = 1;

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
        // A folded inspector has no height to drag.
        if let Some(inspector) = self.inspector
            && !self.inspector_folded
            && col >= inspector.x
            && row.abs_diff(inspector.y) <= GRAB_SLOP
        {
            return Some(Divider::Horizontal);
        }
        None
    }

    /// Which foldable pane's `[N]` tag, if any, the given cell is on. Checked
    /// before `divider_at`, since the inspector's tag sits on the row its
    /// divider grabs.
    ///
    /// Every pane draws its title through `panel_title`, left-aligned just
    /// past the corner, so the tag's cells are `panel_tag`'s width from there.
    pub fn fold_tag_at(&self, col: u16, row: u16) -> Option<FocusedPanel> {
        let p = Position { x: col, y: row };
        [
            (self.inspector, FocusedPanel::Inspector, INSPECTOR_NUM),
            (
                self.contributions,
                FocusedPanel::Contributions,
                CONTRIBUTIONS_NUM,
            ),
        ]
        .into_iter()
        .find_map(|(rect, panel, num)| {
            let r = rect?;
            let width = panel_tag(num).chars().count() as u16;
            Rect::new(r.x + 1, r.y, width, 1)
                .intersection(r)
                .contains(p)
                .then_some(panel)
        })
    }

    /// The repo-list width the given column would produce, as a percentage of
    /// the main area. Unclamped: `App::set_repo_list_pct` owns the bounds.
    pub fn repo_list_pct_at(&self, col: u16) -> u16 {
        pct_for(col.saturating_sub(self.body.x), self.body.width)
    }

    /// The inspector height the given row would produce, as a percentage of
    /// the body. Measured from the bottom, since the inspector is docked
    /// there and the divider is its top edge.
    pub fn inspector_pct_at(&self, row: u16) -> u16 {
        let bottom = self.body.y + self.body.height;
        pct_for(bottom.saturating_sub(row), self.body.height)
    }
}

/// The percentage that makes a split of `total` come out at `cells`.
///
/// Rounded **up**, because the splits floor: `Layout` gives a pane
/// `total * pct / 100` cells, so the percentage that lands on `cells` is the
/// smallest one that does not fall short of it. Dividing down instead made
/// the round-trip lossy in the direction that matters -- a grab converted the
/// divider's own position into a percentage one point too small, which drew
/// it a row or column away from the cursor that had not moved yet.
fn pct_for(cells: u16, total: u16) -> u16 {
    if total == 0 {
        return 0;
    }
    let (cells, total) = (cells as u32, total as u32);
    (cells * 100).div_ceil(total).min(100) as u16
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
    let strip_folded = app.contributions_folded;

    match app.view {
        // Compact has no right column for the strip to dock under, so it
        // comes off the bottom of the whole area and the list shrinks by it.
        View::Compact => {
            let (body, contributions) = contributions::split_main_area(main, strip_folded);
            PanelRects {
                main: body,
                body,
                repo_list: body,
                detail: None,
                inspector: None,
                inspector_folded: false,
                contributions,
            }
        }
        // Detail stacks from the bottom: the strip comes off the whole area,
        // the inspector docks across the full width above it, and the repo
        // list and detail pane share the row that is left.
        //
        // The strip used to dock under the right column alone on a wide
        // terminal, buying the list a few rows. With the inspector spanning
        // the width there is no column for it to dock under, and the list
        // scrolls anyway, so the trade is gone.
        View::Detail => {
            let (body, contributions) = contributions::split_main_area(main, strip_folded);
            let (top, inspector) = detail::split_inspector(
                body,
                app.inspector_pct,
                app.shows_inspector(),
                app.inspector_folded,
            );
            let (repo_list, detail) = detail::split_main(top, app.repo_list_pct);
            PanelRects {
                main,
                body,
                repo_list,
                detail: Some(detail),
                inspector,
                inspector_folded: app.inspector_folded,
                contributions,
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

/// The panel numbers of the two foldable panes, which are also the keys that
/// focus them.
pub const INSPECTOR_NUM: u8 = 3;
pub const CONTRIBUTIONS_NUM: u8 = 4;

/// The number half of a panel title, spaces included: the cells a click has
/// to land on to fold the pane.
fn panel_tag(num: u8) -> String {
    format!(" [{num}] ")
}

/// A folded pane: its title on a top rule, with no corners, sides or bottom.
/// The title sits where an open pane's does, so `fold_tag_at` finds the tag
/// either way. It keeps focus styling, since folding the focused pane leaves
/// it focused.
pub fn render_folded(frame: &mut Frame, area: Rect, num: u8, label: &str, focused: bool) {
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let rule = Block::default()
        .borders(Borders::TOP)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));
    frame.render_widget(rule, area);
    // A block with no left side starts its title in the first column; an
    // open pane's corner pushes it one in, which is where the tag is clicked.
    let title = Rect::new(area.x + 1, area.y, area.width.saturating_sub(1), 1).intersection(area);
    frame.render_widget(panel_title(num, label, focused), title);
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
        Span::styled(panel_tag(num), Style::default().fg(num_color)),
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

    // Grab handles, drawn last of the main-area furniture so the horizontal
    // one lands on the inspector's border rather than under it. Not over the
    // load error, which has no panes to divide: the grip landed mid-sentence.
    if !app.shows_load_error() {
        grips::render(frame, &rects, app);
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
            "R reloads from GitHub, r re-checks local repos and polls GitHub",
            Style::default().fg(colors::OVERLAY),
        )),
    ];
    // Rate limits are the common way this happens and they clear on their own,
    // which is worth saying: the reflex otherwise is to go looking for a bug.
    // The typed flag catches a 403 or 429 limit, whose message names only the
    // status; the word match stays for a message with no attempt behind it.
    let typed_limit = matches!(
        &app.github_freshness.outcome,
        crate::app::Outcome::Failed(failure) if failure.rate_limited
    );
    if typed_limit || reason.contains("rate limit") || reason.contains("RATE_LIMIT") {
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
            format!(" - {}", app.header),
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
    let ts_span = if let Some(phase) = app.refresh_phase() {
        let label = match phase {
            crate::app::RefreshPhase::Local => "checking local changes…",
            crate::app::RefreshPhase::Remote => "checking remote…",
        };
        Some(spinner_group(app.spinner_frame, label))
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
    // Never dropped by the fit loop, for the same reason `loading` is not:
    // while it lasts it is the most important thing on the line. A narrow
    // header gets the short form instead. Wording and colours live in
    // `github_segment`.
    let gh_segment = github_segment(app, chrono::Local::now());
    // Persistent indicator so the `w` toggle's state is visible at a glance,
    // not just in the transient status message when it flips.
    //
    // Two spans on the same terms as `spinner_group`: only the lamp moves, and
    // the word beside it holds one steady colour. A label that pulsed along
    // with the glyph made the whole group flicker, which is motion where the
    // eye is trying to read a word rather than watch a light.
    //
    // The lamp pulses between two neighbouring reds, holding one shape. It
    // used to do the opposite -- one colour, alternating disks -- on the
    // reasoning that a changing shape separated it from the CI lamps, which
    // say what they mean by colour. In use the shape change was the harder of
    // the two to see at the edge of vision, which is the only place this
    // indicator is ever read. The word `watch` is what actually distinguishes
    // it from a CI lamp; the glyph never had to.
    //
    // Off is drawn too, as a crossed-out eye in a steady muted grey: an
    // indicator that only exists while on cannot tell "off" from "not drawn
    // at this width".
    let watch_span = if app.watch_mode {
        let lamp = if app.blink_on {
            colors::WATCH_ON
        } else {
            colors::WATCH_OFF
        };
        vec![
            Span::styled(format!(" {}", watch_icon(true)), Style::default().fg(lamp)),
            Span::styled(" watch ".to_string(), Style::default().fg(colors::WATCH_ON)),
        ]
    } else {
        let off = Style::default().fg(colors::OVERLAY);
        vec![
            Span::styled(format!(" {}", watch_icon(false)), off),
            Span::styled(" watch ".to_string(), off),
        ]
    };
    let mut include_version = true;
    let mut include_ts = true;
    let mut include_group = true;
    let mut include_header = true;
    let mut gh_long = true;

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
        if let Some((ref long, ref short, color)) = gh_segment {
            let text = if gh_long { long } else { short };
            right.push(Span::styled(text.clone(), Style::default().fg(color)));
        }
        right.extend(watch_span.iter().cloned());
        if include_ts && let Some(ref t) = ts_span {
            right.extend(t.iter().cloned());
        }

        // Characters, not bytes: every glyph here is one column wide, and a
        // byte count charged the separator, arrows and eye up to four each.
        let left_w: usize = left.iter().map(|s| s.content.chars().count()).sum();
        let right_w: usize = right.iter().map(|s| s.content.chars().count()).sum();
        let exhausted = !include_version
            && !include_ts
            && !include_group
            && !include_header
            && (!gh_long || gh_segment.is_none());

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
        } else if include_header {
            include_header = false;
        } else {
            gh_long = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::author_code;
    use super::*;

    fn rects() -> PanelRects {
        // 100x40 main area, repo list 35 wide, inspector 16 tall across the
        // full width at the bottom; the list and the detail pane share the
        // 24 rows above it.
        let main = Rect::new(0, 1, 100, 40);
        PanelRects {
            main,
            // No strip in this fixture, so the splits get the whole area.
            body: main,
            repo_list: Rect::new(0, 1, 35, 24),
            detail: Some(Rect::new(36, 1, 64, 24)),
            inspector: Some(Rect::new(0, 25, 100, 16)),
            inspector_folded: false,
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

    /// The foreground colour of the `watch` indicator's glyph at a given blink
    /// phase.
    fn watch_glyph_color(blink_on: bool) -> ratatui::style::Color {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let width = 80u16;
        let mut app = crate::app::App::new(Vec::new(), "t".to_string(), width, 40);
        app.watch_mode = true;
        app.blink_on = blink_on;
        let app = app;
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal.draw(|f| render_header(f, f.area(), &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let x = (0..width)
            .find(|x| buffer[(*x, 0)].symbol() == WATCH_ON_GLYPH)
            .expect("watch indicator not drawn");
        buffer[(x, 0)]
            .style()
            .fg
            .expect("watch glyph has no colour")
    }

    /// The foreground colour of the `watch` label's first letter, at a given
    /// blink phase.
    fn watch_label_color(blink_on: bool) -> ratatui::style::Color {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let width = 80u16;
        let mut app = crate::app::App::new(Vec::new(), "t".to_string(), width, 40);
        app.watch_mode = true;
        app.blink_on = blink_on;
        let app = app;
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal.draw(|f| render_header(f, f.area(), &app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let lamp = (0..width)
            .find(|x| buffer[(*x, 0)].symbol() == WATCH_ON_GLYPH)
            .expect("watch indicator not drawn");
        // The first cell of "watch", one space past the lamp.
        buffer[(lamp + 2, 0)]
            .style()
            .fg
            .expect("watch label has no colour")
    }

    /// The house rule bans em and en dashes, and the header drew an em dash
    /// between the version and the window label.
    #[test]
    fn header_draws_no_em_or_en_dash() {
        let row = header_row("last 7 days", 120);
        assert!(row.contains("last 7 days"), "the window label is drawn");
        assert!(
            !row.contains(['\u{2013}', '\u{2014}']),
            "header has a dash character: {row:?}"
        );
    }

    /// Only the lamp moves. Same rule as `spinner_group`: a word that pulses
    /// along with the glyph beside it is motion where the eye is trying to
    /// read, and the light is what the pulse is meant to draw attention to.
    #[test]
    fn only_the_watch_lamp_pulses_not_its_label() {
        assert_ne!(
            watch_glyph_color(true),
            watch_glyph_color(false),
            "the lamp is not pulsing"
        );
        assert_eq!(
            watch_label_color(true),
            watch_label_color(false),
            "the label pulsed along with the lamp"
        );
        assert_eq!(watch_label_color(true), colors::WATCH_ON);
    }

    /// The pulse is two neighbouring reds. It used to be one colour and two
    /// disk shapes; the shape change was the harder of the two to catch at the
    /// edge of vision, which is the only place this indicator is read.
    #[test]
    fn the_watch_indicator_pulses_between_two_reds() {
        assert_eq!(watch_glyph_color(true), colors::WATCH_ON);
        assert_eq!(watch_glyph_color(false), colors::WATCH_OFF);
        assert_ne!(
            colors::WATCH_ON,
            colors::WATCH_OFF,
            "a pulse needs two phases"
        );
    }

    /// Both phases have to still read as red, or the indicator changes hue
    /// rather than pulsing.
    #[test]
    fn both_watch_phases_are_red_dominant() {
        for (phase, color) in [("on", colors::WATCH_ON), ("off", colors::WATCH_OFF)] {
            let ratatui::style::Color::Rgb(r, g, b) = color else {
                panic!("{phase} phase is not an RGB colour");
            };
            assert!(r > g && r > b, "{phase} phase is not red-dominant");
        }
    }

    /// Colour only: the header's fit loop lays out spans by width, so a phase
    /// that drew a different glyph could shift what sits beside it.
    #[test]
    fn both_watch_phases_draw_the_same_text() {
        let lit = header_row_with("t", 80, |a| {
            a.watch_mode = true;
            a.blink_on = true;
        });
        let dim = header_row_with("t", 80, |a| {
            a.watch_mode = true;
            a.blink_on = false;
        });
        assert_eq!(lit, dim, "the phases differ in colour, not in text");
        assert!(
            lit.contains(&format!("{WATCH_ON_GLYPH} watch")),
            "got {lit:?}"
        );
    }

    /// Off is shown, not omitted: the struck-through eye, never the open one,
    /// and in a colour that does not pulse or read as the live red.
    #[test]
    fn watch_off_draws_a_crossed_out_eye() {
        let width = 80u16;
        let row = header_row_with("t", width, |a| a.watch_mode = false);
        assert!(
            row.contains(&format!("{WATCH_OFF_GLYPH} watch")),
            "got {row:?}"
        );
        assert!(
            !row.contains(WATCH_ON_GLYPH),
            "the open eye drawn while off"
        );

        for blink_on in [true, false] {
            use ratatui::Terminal;
            use ratatui::backend::TestBackend;
            let mut app = crate::app::App::new(Vec::new(), "t".to_string(), width, 40);
            app.blink_on = blink_on;
            let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
            terminal.draw(|f| render_header(f, f.area(), &app)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            let x = (0..width)
                .find(|x| buffer[(*x, 0)].symbol() == WATCH_OFF_GLYPH)
                .expect("watch-off indicator not drawn");
            assert_eq!(buffer[(x, 0)].style().fg, Some(colors::OVERLAY));
        }
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

    /// A manual `r` says which half it is on: the local re-read is quick, the
    /// GitHub poll after it takes seconds, and one word for both made the
    /// fast part look slow.
    #[test]
    fn a_manual_refresh_names_the_half_it_is_on() {
        let local = header_row_with("h", 120, |app| {
            app.sweep_started = Some(std::time::Instant::now());
            app.sweep_manual = true;
            app.refresh_awaits_poll = true;
        });
        assert!(local.contains("checking local changes"), "{local}");
        assert!(!local.contains("checking remote"), "{local}");

        let remote = header_row_with("h", 120, |app| app.refresh_awaits_poll = true);
        assert!(remote.contains("checking remote"), "{remote}");
        assert!(!remote.contains("checking local"), "{remote}");

        let fetching = header_row_with("h", 120, |app| {
            app.refresh_fetches.insert("~/repos/a".to_string());
        });
        assert!(fetching.contains("checking remote"), "{fetching}");
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
    fn horizontal_divider_is_grabbable_across_the_full_width() {
        let r = rects();
        assert_eq!(r.divider_at(60, 25), Some(Divider::Horizontal));
        // The inspector spans under the repo list too, so its top edge is
        // grabbable there as well.
        assert_eq!(r.divider_at(10, 25), Some(Divider::Horizontal));
        assert_eq!(r.divider_at(10, 12), None, "well inside the repo list");
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

        // The inspector is docked at the bottom of the main area, so its size
        // is measured from there: dragging its top edge upward makes it bigger.
        assert_eq!(r.inspector_pct_at(21), 50, "half the 40-row main area");
        assert_eq!(r.inspector_pct_at(31), 25);
    }

    #[test]
    fn a_zero_sized_area_does_not_divide_by_zero() {
        let empty = PanelRects {
            main: Rect::new(0, 0, 0, 0),
            body: Rect::new(0, 0, 0, 0),
            repo_list: Rect::new(0, 0, 0, 0),
            detail: None,
            inspector: None,
            inspector_folded: false,
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
    fn panel_rects_reports_the_strip_as_it_is_drawn() {
        let area = Rect::new(0, 0, 200, 44);
        let mut app = layout_app();
        app.view = crate::app::View::Detail;

        app.contributions_folded = true;
        let folded = panel_rects(area, &app);
        assert_eq!(
            folded.contributions.map(|r| r.height),
            Some(FOLDED_HEIGHT),
            "a folded strip is still laid out"
        );

        app.contributions_folded = false;
        let open = panel_rects(area, &app);
        let strip = open.contributions.expect("strip present");
        assert_eq!(strip.height, contributions::CONTRIB_HEIGHT);
        assert_eq!(
            open.at(strip.x + 2, strip.y + 2),
            Some(FocusedPanel::Contributions),
            "a click in the strip lands on the strip"
        );
        // The strip comes off the bottom of everything, under the inspector,
        // so the panes above all give up rows for it.
        assert!(open.repo_list.height < folded.repo_list.height);
        assert!(open.detail.unwrap().height < folded.detail.unwrap().height);
    }

    /// The strip spans the full width at the very bottom whatever the
    /// terminal size: the inspector sits directly above it and the list and
    /// detail pane above that.
    #[test]
    fn the_strip_spans_the_bottom_under_the_inspector() {
        let mut app = layout_app();
        app.view = crate::app::View::Detail;
        app.contributions = crate::app::ContributionsMode::Github;
        for width in [120u16, 200] {
            let rects = panel_rects(Rect::new(0, 0, width, 44), &app);
            let strip = rects.contributions.expect("strip present");
            let inspector = rects.inspector.expect("inspector present");
            assert_eq!(strip.x, rects.main.x, "strip starts at the left edge");
            assert_eq!(strip.width, rects.main.width, "strip spans the width");
            assert_eq!(
                strip.y + strip.height,
                rects.main.y + rects.main.height,
                "strip is the bottom of the main area"
            );
            // `split_main_area` keeps one blank row between the strip and
            // whatever is above it.
            assert_eq!(inspector.y + inspector.height + 1, strip.y);
            assert_eq!(inspector.width, rects.main.width);
        }
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

    /// The inspector is docked under both panes, not under the detail pane
    /// alone: the repo list is rarely tall and scrolls when it must, while a
    /// commit's files and message both want width. The list and the detail
    /// pane share the row above it.
    #[test]
    fn the_inspector_spans_the_full_width_under_both_panes() {
        let mut app = layout_app();
        app.view = crate::app::View::Detail;
        let rects = panel_rects(Rect::new(0, 0, 200, 44), &app);
        let inspector = rects.inspector.expect("inspector present");
        let detail = rects.detail.expect("detail present");
        assert_eq!(inspector.x, rects.main.x, "starts at the left edge");
        assert_eq!(inspector.width, rects.main.width, "spans the width");
        assert_eq!(
            rects.repo_list.height, detail.height,
            "the list and the detail pane share the row above"
        );
        assert_eq!(
            rects.repo_list.y + rects.repo_list.height,
            inspector.y,
            "the list stops where the inspector starts"
        );
        assert_eq!(
            rects.at(rects.repo_list.x + 2, inspector.y + 2),
            Some(FocusedPanel::Inspector),
            "a click under the list lands on the inspector"
        );
    }

    /// A GitHub poll that failed has to leave a mark that a keypress cannot
    /// clear. It used to set only a status message, so a rate limit -- an
    /// hour long -- was invisible within one keystroke, and the frozen CI
    /// lamps and contribution strip looked like current data.
    #[test]
    fn a_failing_github_poll_is_marked_in_the_header() {
        let row = header_row_with("", 80, fail_github);
        assert!(row.contains("⚠ github"), "no failure marker in: {row:?}");
    }

    /// A poll that came back an error, as the event loop records one.
    fn fail_github(app: &mut crate::app::App) {
        app.github_freshness.failed(
            crate::github::Failure::new(crate::github::FailureKind::Http, "HTTP 502"),
            chrono::Local::now(),
        );
    }

    fn failure(kind: crate::github::FailureKind, message: &str) -> crate::github::Failure {
        crate::github::Failure::new(kind, message)
    }

    fn failed_fetch(
        kind: crate::git::FetchFailureKind,
        last_success: Option<chrono::DateTime<chrono::Local>>,
    ) -> crate::model::FetchFacts {
        let mut facts = crate::model::FetchFacts {
            last_success,
            ..Default::default()
        };
        let failure = crate::git::FetchFailure { kind, detail: None };
        facts.record(
            &crate::git::FetchOutcome::Failed(failure),
            at(2026, 10, 1, 14, 30),
        );
        facts
    }

    /// Rule 5 in the detail pane: the failure, why, and how old the remote
    /// refs behind the counts are, written the way the header writes it.
    #[test]
    fn the_fetch_failure_line_names_the_kind_and_the_last_success() {
        let now = at(2026, 10, 1, 14, 30);
        let facts = failed_fetch(
            crate::git::FetchFailureKind::NoRemote,
            Some(at(2026, 10, 1, 14, 2)),
        );
        assert_eq!(
            fetch_failure_line(&facts, now).as_deref(),
            Some("⚠ fetch failed: remote not found, as of 14:02")
        );
    }

    /// Rule 3: unknown is said, never drawn as a time.
    #[test]
    fn an_unknown_last_success_is_said_to_be_unknown() {
        let now = at(2026, 10, 1, 14, 30);
        let facts = failed_fetch(crate::git::FetchFailureKind::Auth, None);
        assert_eq!(
            fetch_failure_line(&facts, now).as_deref(),
            Some("⚠ fetch failed: authentication failed, age unknown")
        );
    }

    /// Nothing extra while fetches work, or before any has run.
    #[test]
    fn a_fetch_that_did_not_fail_draws_nothing() {
        let now = at(2026, 10, 1, 14, 30);
        let mut ok = crate::model::FetchFacts::default();
        assert_eq!(fetch_failure_line(&ok, now), None);
        assert_eq!(fetch_marker(&ok), None);
        ok.record(&crate::git::FetchOutcome::Succeeded, now);
        assert_eq!(fetch_failure_line(&ok, now), None);
        assert_eq!(fetch_marker(&ok), None);
    }

    /// FZ-35a (#84): the received size in the unit a person would say it,
    /// and nothing before the first byte.
    #[test]
    fn a_running_fetch_is_labelled_with_its_received_size() {
        let label = |bytes: Option<u64>| {
            let facts = crate::model::FetchFacts {
                progress: bytes.map(|bytes| crate::git::FetchProgress {
                    phase: crate::git::FetchPhase::Receiving,
                    objects_done: 1,
                    objects_total: 2,
                    bytes,
                }),
                ..Default::default()
            };
            fetch_progress_label(&facts)
        };
        assert_eq!(label(None), None);
        assert_eq!(label(Some(0)), None);
        assert_eq!(label(Some(1)).as_deref(), Some("fetching 1 KB"));
        assert_eq!(label(Some(700 * 1024)).as_deref(), Some("fetching 700 KB"));
        assert_eq!(
            label(Some(312 * 1024 * 1024)).as_deref(),
            Some("fetching 312 MB")
        );
        assert_eq!(
            label(Some(3 * 1024 * 1024 * 1024 / 2)).as_deref(),
            Some("fetching 1.5 GB")
        );
    }

    /// Every kind has its own short word and phrase.
    #[test]
    fn every_fetch_failure_kind_has_its_own_words() {
        use crate::git::FetchFailureKind::*;
        let kinds = [TimedOut, Auth, Unreachable, NoRemote, Other];
        let shorts: std::collections::HashSet<_> =
            kinds.iter().map(|k| fetch_failure_short(*k)).collect();
        let phrases: std::collections::HashSet<_> =
            kinds.iter().map(|k| fetch_failure_phrase(*k)).collect();
        assert_eq!(shorts.len(), kinds.len());
        assert_eq!(phrases.len(), kinds.len());
    }

    /// FZ-35b: `TooBig` has words of its own too.
    #[test]
    fn a_fetch_cut_at_the_cap_has_its_own_words() {
        use crate::git::FetchFailureKind::*;
        let others = [TimedOut, Auth, Unreachable, NoRemote, Other];
        assert!(
            others
                .iter()
                .all(|k| fetch_failure_short(*k) != fetch_failure_short(TooBig)
                    && fetch_failure_phrase(*k) != fetch_failure_phrase(TooBig))
        );
    }

    /// FZ-35b (#85): the two hold labels, sized as the progress label is,
    /// and nothing for a repo nothing holds.
    #[test]
    fn a_held_repo_is_labelled_with_the_size_and_the_key() {
        const MIB: u64 = 1024 * 1024;
        let mut facts = crate::model::FetchFacts {
            disk_baseline: Some(10),
            ..Default::default()
        };
        assert_eq!(fetch_hold_label(&facts), None);
        facts.assess_growth(Some(10 + 3 * 1024 * 1024 / 2), Some(200 * MIB));
        assert_eq!(
            fetch_hold_label(&facts).as_deref(),
            Some("fetch ~1.5 GB, press f")
        );
        facts.record(
            &crate::git::FetchOutcome::Failed(crate::git::FetchFailure::too_big(201 * MIB)),
            at(2026, 10, 20, 14, 0),
        );
        assert_eq!(
            fetch_hold_label(&facts).as_deref(),
            Some("fetch over 201 MB, press f")
        );
    }

    /// Rule 9's status line, for each shape of result.
    #[test]
    fn the_fetch_report_says_how_many_failed() {
        use crate::git::FetchFailureKind::*;
        let report =
            |total, failed: &[(&str, crate::git::FetchFailureKind)]| crate::app::FetchReport {
                total,
                failed: failed.iter().map(|(n, k)| (n.to_string(), *k)).collect(),
            };
        assert_eq!(
            fetch_report_label(&report(1, &[("fleetz", TimedOut)])),
            "fetch failed: fleetz, timed out"
        );
        assert_eq!(
            fetch_report_label(&report(5, &[("fleetz", Auth)])),
            "1 of 5 fetches failed: fleetz"
        );
        assert_eq!(
            fetch_report_label(&report(5, &[("a", Auth), ("b", Unreachable)])),
            "2 of 5 fetches failed"
        );
    }

    /// The case that was invisible: a poll that failed offline with good data
    /// on screen names the failure and how old the data is.
    #[test]
    fn a_stale_header_names_the_failure_and_the_data_age() {
        let now = at(2026, 10, 1, 14, 30);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.succeeded(at(2026, 10, 1, 14, 2));
        freshness.failed(failure(crate::github::FailureKind::Offline, "dns"), now);
        let (long, short, color) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " ⚠ github offline at 14:30, as of 14:02 ");
        assert_eq!(short, " ⚠ github ");
        assert_eq!(color, colors::PEACH);
    }

    /// A rate limit is an API failure, never "offline", and with a cache of
    /// unreadable age the age is said to be unknown rather than made up.
    #[test]
    fn a_rate_limit_on_data_of_unknown_age_says_both() {
        let now = at(2026, 10, 1, 14, 30);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.loaded(&crate::app::DatasetSource::Cache { written: None }, now);
        freshness.failed(
            crate::github::Failure::rate_limit("GitHub API errors: API rate limit exceeded"),
            now,
        );
        let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " ⚠ github rate limited at 14:30, age unknown ");
    }

    /// #79. The failure is dated by the attempt that failed, not only by the
    /// data it left on screen: an hour-old offline verdict and one from this
    /// minute used to read the same.
    #[test]
    fn a_failed_header_says_when_the_attempt_failed() {
        let now = at(2026, 10, 1, 12, 30);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.succeeded(at(2026, 10, 1, 11, 2));
        freshness.failed(
            failure(crate::github::FailureKind::Offline, "dns"),
            at(2026, 10, 1, 12, 2),
        );
        let (long, short, color) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " ⚠ github offline at 12:02, as of 11:02 ");
        assert_eq!(short, " ⚠ github ");
        assert_eq!(color, colors::PEACH);
    }

    /// #79. No threshold: an attempt seconds old is stamped too.
    #[test]
    fn a_failed_attempt_seconds_old_is_stamped_too() {
        let now = at(2026, 10, 1, 12, 2) + chrono::Duration::seconds(5);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.succeeded(at(2026, 10, 1, 11, 2));
        freshness.failed(
            failure(crate::github::FailureKind::Offline, "dns"),
            at(2026, 10, 1, 12, 2),
        );
        let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " ⚠ github offline at 12:02, as of 11:02 ");
    }

    /// #79. Data of unknown age keeps saying so after the stamp.
    #[test]
    fn a_failed_attempt_on_data_of_unknown_age_is_stamped() {
        let now = at(2026, 10, 1, 12, 30);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.loaded(&crate::app::DatasetSource::Cache { written: None }, now);
        freshness.failed(
            failure(crate::github::FailureKind::Offline, "dns"),
            at(2026, 10, 1, 12, 2),
        );
        let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " ⚠ github offline at 12:02, age unknown ");
    }

    /// #79. With no data at all there is no data part, only the stamp.
    #[test]
    fn a_failed_attempt_with_no_data_is_stamped() {
        let now = at(2026, 10, 1, 12, 30);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.failed(
            failure(crate::github::FailureKind::Offline, "dns"),
            at(2026, 10, 1, 12, 2),
        );
        let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " ⚠ github offline at 12:02 ");
    }

    /// #79. A failure with no attempt recorded (built by hand: no real path
    /// sets one without the other) renders as before, with no stamp.
    #[test]
    fn a_failure_with_no_attempt_recorded_has_no_stamp() {
        let now = at(2026, 10, 1, 12, 30);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.data_time = crate::app::DataTime::At(at(2026, 10, 1, 11, 2));
        freshness.outcome =
            crate::app::Outcome::Failed(failure(crate::github::FailureKind::Offline, "dns"));
        assert_eq!(freshness.last_attempt, None);
        let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " ⚠ github offline, as of 11:02 ");
    }

    /// #79. An attempt on another day carries its date, as `swept` does.
    #[test]
    fn a_failed_attempt_from_yesterday_carries_its_date() {
        let now = at(2026, 10, 2, 9, 0);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.succeeded(at(2026, 10, 1, 11, 2));
        freshness.failed(
            failure(crate::github::FailureKind::Offline, "dns"),
            at(2026, 10, 1, 12, 2),
        );
        let (long, _, _) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " ⚠ github offline at Oct 1 12:02, as of Oct 1 11:02 ");
    }

    /// A jitter that draws nothing, so a resume is the reset to the second.
    struct NoJitter;

    impl crate::app::Jitter for NoJitter {
        fn backoff_factor(&mut self) -> f64 {
            1.0
        }
        fn resume_secs(&mut self) -> i64 {
            0
        }
    }

    /// An app watching GitHub whose last good data is from 14:02, with the
    /// default interval (30 s) and cost (3 points).
    fn polled_app() -> crate::app::App {
        let mut app = crate::app::App::new(Vec::new(), String::new(), 120, 40);
        app.jitter = Box::new(NoJitter);
        app.watch_mode = true;
        app.github_freshness.succeeded(at(2026, 10, 1, 14, 2));
        app
    }

    /// The poll at 14:10 answered with `remaining` of 5000 points left, the
    /// window resetting at 15:00.
    fn polled_with(app: &mut crate::app::App, remaining: u32) {
        let arrival = at(2026, 10, 1, 14, 10);
        let reading = crate::github::BudgetReading {
            limit: 5000,
            remaining,
            used: 5000 - remaining,
            reset: at(2026, 10, 1, 15, 0).timestamp(),
            date: Some(arrival.with_timezone(&chrono::Utc)),
            token_id: "t".to_string(),
        };
        app.observe_budget(Some(&reading), true, arrival);
    }

    /// FZ-10c, 4.6. Rate limited: today's segment, with when polling resumes.
    #[test]
    fn a_rate_limited_header_says_when_polling_resumes() {
        let mut app = polled_app();
        polled_with(&mut app, 0);
        let refused = crate::github::Failure {
            rate_limited: true,
            limit: Some(crate::github::RateLimit::Primary),
            ..crate::github::Failure::new(crate::github::FailureKind::Http, "HTTP 403")
        };
        app.github_attempt_failed(&refused, at(2026, 10, 1, 14, 10));
        app.github_freshness
            .failed(refused, at(2026, 10, 1, 14, 10));
        let (long, short, color) = github_segment(&app, at(2026, 10, 1, 14, 20)).unwrap();
        assert_eq!(long, " ⚠ github rate limited until 15:00, as of 14:02 ");
        assert_eq!(short, " ⚠ github ");
        assert_eq!(color, colors::PEACH);
        // Once the limit has passed the time goes; the failure stays until a
        // poll succeeds.
        let (long, _, _) = github_segment(&app, at(2026, 10, 1, 15, 1)).unwrap();
        assert_eq!(long, " ⚠ github rate limited at 14:10, as of 14:02 ");
    }

    /// FZ-10c, 4.6. Paused at the reserve: polls succeed, but none goes again
    /// before the reset. Watch off has no polling to pause, and says nothing.
    #[test]
    fn a_paused_header_says_until_when_and_why() {
        let mut app = polled_app();
        polled_with(&mut app, 400);
        app.github_freshness.succeeded(at(2026, 10, 1, 14, 10));
        let (long, short, color) = github_segment(&app, at(2026, 10, 1, 14, 20)).unwrap();
        assert_eq!(long, " github paused until 15:00, allowance nearly used ");
        assert_eq!(short, " github paused ");
        assert_eq!(color, colors::PEACH);
        assert_eq!(github_segment(&app, at(2026, 10, 1, 15, 0)), None);
        app.watch_mode = false;
        assert_eq!(github_segment(&app, at(2026, 10, 1, 14, 20)), None);
    }

    /// FZ-10c, 4.6. Slowed: 100 points above the reserve over 50 minutes
    /// spaces 3-point polls 90 s apart, past the 30 s interval. Plenty left
    /// says nothing.
    #[test]
    fn a_slowed_header_says_the_allowance_is_low() {
        let mut app = polled_app();
        polled_with(&mut app, 600);
        app.github_freshness.succeeded(at(2026, 10, 1, 14, 10));
        let (long, short, color) = github_segment(&app, at(2026, 10, 1, 14, 20)).unwrap();
        assert_eq!(long, " github slowed, allowance low ");
        assert_eq!(short, " github slowed ");
        assert_eq!(color, colors::OVERLAY);

        let mut healthy = polled_app();
        polled_with(&mut healthy, 4000);
        healthy.github_freshness.succeeded(at(2026, 10, 1, 14, 10));
        assert_eq!(github_segment(&healthy, at(2026, 10, 1, 14, 20)), None);
    }

    /// FZ-10c, 4.6. Backing off after failures in a row adds nothing: the
    /// segment already says it failed and how old the data is.
    #[test]
    fn a_backing_off_header_keeps_the_failure_and_its_age() {
        let mut app = polled_app();
        polled_with(&mut app, 4000);
        for minute in [10, 11, 13] {
            let now = at(2026, 10, 1, 14, minute);
            let failed = failure(crate::github::FailureKind::Server, "HTTP 502");
            app.github_attempt_failed(&failed, now);
            app.github_freshness.failed(failed, now);
        }
        let (long, _, color) = github_segment(&app, at(2026, 10, 1, 14, 14)).unwrap();
        assert_eq!(long, " ⚠ github server error at 14:13, as of 14:02 ");
        assert_eq!(color, colors::PEACH);
    }

    /// A cache-served launch, before its attempt lands: quiet, and dated the
    /// way the `swept` label is, so yesterday's cache carries its date.
    #[test]
    fn a_cache_served_launch_is_marked_cached_with_its_age() {
        let now = at(2026, 10, 1, 9, 0);
        let mut freshness = crate::app::GithubFreshness::default();
        freshness.loaded(
            &crate::app::DatasetSource::Cache {
                written: Some(at(2026, 9, 30, 18, 5)),
            },
            now,
        );
        let (long, short, color) = github_freshness_segment(&freshness, now).unwrap();
        assert_eq!(long, " github cached Sep 30 18:05 ");
        assert_eq!(short, " github cached ");
        assert_eq!(color, colors::OVERLAY);

        let mut unknown = crate::app::GithubFreshness::default();
        unknown.loaded(&crate::app::DatasetSource::Cache { written: None }, now);
        let (long, _, _) = github_freshness_segment(&unknown, now).unwrap();
        assert_eq!(long, " github cached, age unknown ");
    }

    /// Fresh says nothing, and neither does a dashboard with no GitHub data
    /// at all (`-d`, or before the first load lands).
    #[test]
    fn fresh_or_no_github_data_draws_no_segment() {
        let now = chrono::Local::now();
        let mut freshness = crate::app::GithubFreshness::default();
        assert!(github_freshness_segment(&freshness, now).is_none());
        freshness.succeeded(now);
        assert!(github_freshness_segment(&freshness, now).is_none());
    }

    /// The long form when there is room for it, the short form when there is
    /// not, and never neither.
    #[test]
    fn the_cached_segment_shortens_rather_than_disappears() {
        let cached = |app: &mut crate::app::App| {
            app.github_freshness.loaded(
                &crate::app::DatasetSource::Cache { written: None },
                chrono::Local::now(),
            )
        };
        let wide = header_row_with("", 100, cached);
        assert!(wide.contains("github cached, age unknown"), "{wide:?}");
        let narrow = header_row_with("last 7 days", 30, cached);
        assert!(narrow.contains("github cached"), "dropped: {narrow:?}");
    }

    /// The other half: a healthy poll must not leave a warning sitting there.
    #[test]
    fn a_healthy_github_poll_leaves_the_header_clean() {
        let row = header_row_with("", 80, |app| {
            app.github_freshness.succeeded(chrono::Local::now())
        });
        assert!(!row.contains("⚠"), "spurious failure marker in: {row:?}");
    }

    /// The fit loop drops the version, then the timestamp, then the group,
    /// then the header text. This marker is not on that list: a narrow
    /// terminal is not a reason to hide that the data stopped updating.
    #[test]
    fn the_failure_marker_survives_a_narrow_header() {
        let row = header_row_with("last 7 days", 30, fail_github);
        assert!(
            row.contains("⚠ github"),
            "marker dropped by the fit loop: {row:?}"
        );
    }

    /// A header carrying every piece the fit loop can drop: the version, a
    /// window label with an arrow, a group, a fixed sweep time, and the
    /// Nerd Font watch eye. Several of these are multi-byte but one column.
    fn full_header_app(app: &mut crate::app::App) {
        app.group_names = vec!["work".to_string()];
        app.active_group = Some(0);
        app.last_refreshed = Some(at(2026, 1, 2, 3, 4));
    }

    const ARROW_HEADER: &str = "since 2 weeks → before 2026-09-01";

    /// The columns a header row draws: everything but the border filler.
    fn drawn_columns(row: &str) -> usize {
        row.chars().filter(|c| *c != '─').count()
    }

    /// Regression: the fit was measured in bytes, so the arrow, the
    /// separator and the watch eye cost two to four columns each and the
    /// version went while there was room for it. At the width the pieces
    /// draw, plus the one column the loop keeps between them, everything is
    /// kept; one column less, the version goes first.
    #[test]
    fn the_header_keeps_every_piece_that_fits_as_drawn() {
        let wide = header_row_with(ARROW_HEADER, 200, full_header_app);
        let drawn = drawn_columns(&wide);
        let pieces = wide.replace('─', "");
        assert!(pieces.len() > drawn + 1, "no multi-byte glyph: {pieces:?}");
        let width = (drawn + 1) as u16;

        let row = header_row_with(ARROW_HEADER, width, full_header_app);
        assert!(row.contains(VERSION), "version dropped: {row:?}");
        assert!(row.contains(&format!("{ARROW_HEADER} [t]")), "{row:?}");
        assert!(row.contains("[work]"), "{row:?}");
        assert!(row.contains("swept Jan 2 03:04"), "{row:?}");
        assert!(
            row.contains(&format!("{} watch", watch_icon(false))),
            "{row:?}"
        );

        let tighter = header_row_with(ARROW_HEADER, width - 1, full_header_app);
        assert!(!tighter.contains(VERSION), "{tighter:?}");
        assert!(tighter.contains("swept Jan 2 03:04"), "{tighter:?}");
    }

    /// The drop order, read from the drawn header at every width from wide
    /// to narrow: version, then the sweep time, then the group, then the
    /// window label, then the GitHub segment's long form. A reordering, or a
    /// step that is skipped, fails.
    #[test]
    fn the_header_drops_its_pieces_in_order() {
        let setup = |app: &mut crate::app::App| {
            full_header_app(app);
            fail_github(app);
        };
        let state = |row: &str| {
            (
                row.contains(VERSION),
                row.contains("swept"),
                row.contains("[work]"),
                row.contains("since 2 weeks"),
                row.contains("http error"),
            )
        };
        let mut states = Vec::new();
        for width in (10u16..=200).rev() {
            let s = state(&header_row_with(ARROW_HEADER, width, setup));
            if states.last() != Some(&s) {
                states.push(s);
            }
            if s == (false, false, false, false, false) {
                break;
            }
        }
        assert_eq!(
            states,
            vec![
                (true, true, true, true, true),
                (false, true, true, true, true),
                (false, false, true, true, true),
                (false, false, false, true, true),
                (false, false, false, false, true),
                (false, false, false, false, false),
            ]
        );
    }

    /// Sizes worth checking a divider round-trip at: the strip open and shut,
    /// since the two give the inspector split different areas to work in.
    fn drag_cases() -> Vec<(u16, u16, crate::app::ContributionsMode)> {
        let mut cases = Vec::new();
        for mode in [
            crate::app::ContributionsMode::Off,
            crate::app::ContributionsMode::Github,
        ] {
            for (w, h) in [(120u16, 30u16), (120, 40), (120, 50), (133, 44), (157, 60)] {
                cases.push((w, h, mode));
            }
        }
        cases
    }

    /// The failure this closes: pressing on the horizontal divider and holding
    /// still moved it. `inspector_pct_at` measured the row against `main`,
    /// while `split_inspector` applies the percentage to `body` -- main minus
    /// the contribution strip -- so with the strip open (the default) a grab
    /// sent the divider four to five rows away from the cursor before the
    /// first drag event arrived.
    #[test]
    fn grabbing_the_horizontal_divider_leaves_it_where_it_was() {
        for (w, h, mode) in drag_cases() {
            let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
            app.view = View::Detail;
            app.contributions = mode;
            let area = Rect::new(0, 0, w, h);
            let rects = panel_rects(area, &app);
            let Some(before) = rects.inspector else {
                continue;
            };
            app.set_inspector_pct(rects.inspector_pct_at(before.y));
            let after = panel_rects(area, &app).inspector.expect("inspector kept");
            assert_eq!(
                after.y, before.y,
                "divider moved on a grab at {w}x{h}, strip {mode:?}"
            );
        }
    }

    /// The same round-trip for the vertical divider, over every position a
    /// drag can actually put it in. Both directions used to floor, so a grab
    /// could quantise the column away even though the mouse had not moved.
    ///
    /// Walked over the percentages rather than the columns because only a
    /// percentage-produced column is representable: see
    /// `grabbing_the_capped_default_divider_moves_it_at_most_one_column`.
    #[test]
    fn grabbing_the_vertical_divider_leaves_it_where_it_was() {
        for (w, h, mode) in drag_cases() {
            for pct in crate::app::REPO_LIST_PCT_MIN..=crate::app::REPO_LIST_PCT_MAX {
                if pct == crate::app::DEFAULT_REPO_LIST_PCT {
                    continue; // capped, and covered by its own test below
                }
                let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
                app.view = View::Detail;
                app.contributions = mode;
                app.set_repo_list_pct(pct);
                let area = Rect::new(0, 0, w, h);
                let rects = panel_rects(area, &app);
                let before = rects.repo_list.x + rects.repo_list.width;
                app.set_repo_list_pct(rects.repo_list_pct_at(before));
                let list = panel_rects(area, &app).repo_list;
                assert_eq!(
                    list.x + list.width,
                    before,
                    "divider moved on a grab at {w}x{h} pct {pct}, strip {mode:?}"
                );
            }
        }
    }

    /// The one position a grab cannot reproduce exactly.
    ///
    /// `split_main` caps the repo list at `REPO_LIST_MAX_COLS` *only* at the
    /// default percentage, so on a wide terminal the drawn width is one no
    /// percentage of that terminal produces: at 157 columns the list is capped
    /// to 52, and 52 sits between the widths 33% and 34% draw. Grabbing it
    /// therefore releases the cap and the divider shifts by a column, once.
    /// Bounded here so the shift can never grow into the multi-row jump the
    /// horizontal divider had.
    #[test]
    fn grabbing_the_capped_default_divider_moves_it_at_most_one_column() {
        for (w, h, mode) in drag_cases() {
            let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
            app.view = View::Detail;
            app.contributions = mode;
            let area = Rect::new(0, 0, w, h);
            let rects = panel_rects(area, &app);
            let before = rects.repo_list.x + rects.repo_list.width;
            app.set_repo_list_pct(rects.repo_list_pct_at(before));
            let list = panel_rects(area, &app).repo_list;
            let moved = (list.x + list.width) as i32 - before as i32;
            assert!(
                moved.abs() <= 1,
                "divider jumped {moved} columns on a grab at {w}x{h}, strip {mode:?}"
            );
        }
    }

    /// A drag tracks the pointer: the divider lands on the row grabbed, not
    /// merely stays put. Outside the range the clamps in `split_inspector`
    /// allow it parks at the limit, which is deliberate -- so the limits are
    /// measured first by dragging past them, and exact tracking is asserted
    /// between.
    #[test]
    fn dragging_the_horizontal_divider_lands_on_the_grabbed_row() {
        let (w, h) = (120u16, 50u16);
        let mut app = crate::app::App::new(Vec::new(), String::new(), w, h);
        app.view = View::Detail;
        app.contributions = crate::app::ContributionsMode::Github;
        let area = Rect::new(0, 0, w, h);
        let body = panel_rects(area, &app).body;

        let drag_to = |app: &mut crate::app::App, row: u16| {
            app.set_inspector_pct(panel_rects(area, app).inspector_pct_at(row));
            panel_rects(area, app).inspector.expect("inspector kept").y
        };
        // Drag hard against each stop to find where the clamps park it.
        let tallest = drag_to(&mut app, body.y);
        let shortest = drag_to(&mut app, body.y + body.height);

        for row in body.y..body.y + body.height {
            let landed = drag_to(&mut app, row);
            assert_eq!(
                landed,
                row.clamp(tallest, shortest),
                "drag to row {row} landed at {landed} (stops {tallest}..={shortest})"
            );
        }
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

    /// FZ-10c. A 403 or 429 limit is `Http`, and its message is a status line
    /// and a code with no "rate limit" in it, so the word match alone missed
    /// it. The typed flag the fetch layer set decides.
    #[test]
    fn an_http_rate_limit_load_says_it_clears_on_its_own() {
        use crate::github::{Failure, FailureKind, RateLimit};
        let message = "GitHub answered HTTP 403 Forbidden";
        let out = screen(80, 24, |app| {
            let limited = Failure {
                rate_limited: true,
                limit: Some(RateLimit::Primary),
                ..Failure::new(FailureKind::Http, message)
            };
            app.github_freshness.failed(limited, at(2026, 10, 9, 14, 2));
            app.load_error = Some(message.to_string());
        });
        assert!(out.contains("clears on its own"), "no reassurance:\n{out}");
        // The same message without the flag is an ordinary refusal.
        let other = screen(80, 24, |app| {
            app.github_freshness.failed(
                Failure::new(FailureKind::Refused, message),
                at(2026, 10, 9, 14, 2),
            );
            app.load_error = Some(message.to_string());
        });
        assert!(
            !other.contains("clears on its own"),
            "wrong advice:\n{other}"
        );
    }

    /// FZ-10c. The panel said `r` rebuilds from cache; `r` sweeps the local
    /// repos and polls GitHub, and `R` is the full reload.
    #[test]
    fn the_load_error_panel_names_what_each_key_does() {
        let out = screen(100, 24, |app| {
            app.load_error = Some("GitHub API request failed: dns error".to_string());
        });
        assert!(!out.contains("rebuilds from cache"), "stale hint:\n{out}");
        assert!(out.contains("R reloads"), "no R hint:\n{out}");
        assert!(out.contains("r re-checks"), "no r hint:\n{out}");
    }

    /// The clean case: nothing failed, so nothing is claimed.
    #[test]
    fn a_healthy_launch_shows_no_error_panel() {
        let out = screen(80, 24, |app| app.load_error = None);
        assert!(!out.contains("Could not load the fleet"));
    }

    /// `panel_rects` must report exactly what `render` draws, or clicks land
    /// on the wrong pane silently. Both call `split_inspector`, so
    /// suppressing the inspector in one suppresses it in the other.
    #[test]
    fn panel_rects_reports_no_inspector_in_the_graph_view() {
        let mut app = crate::app::App::new(Vec::new(), String::new(), 120, 44);
        app.view = View::Detail;
        app.history_view = crate::app::HistoryView::Graph;
        let rects = panel_rects(Rect::new(0, 1, 120, 40), &app);
        assert!(
            rects.inspector.is_none(),
            "the graph view still docks an inspector"
        );
        let tall = rects.detail.expect("detail is always drawn in Detail view");

        app.history_view = crate::app::HistoryView::Commits;
        let with = panel_rects(Rect::new(0, 1, 120, 40), &app);
        assert!(
            tall.height > with.detail.unwrap().height,
            "suppressing the inspector did not give its rows to the graph"
        );
    }

    /// The whole screen rendered, with the rects `panel_rects` reports for it.
    fn rendered(setup: impl FnOnce(&mut crate::app::App)) -> (PanelRects, ratatui::buffer::Buffer) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (width, height) = (120u16, 44u16);
        let mut app = crate::app::App::new(Vec::new(), String::new(), width, height);
        app.view = View::Detail;
        app.contributions = crate::app::ContributionsMode::Github;
        setup(&mut app);
        let app = app;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let rects = panel_rects(Rect::new(0, 0, width, height), &app);
        (rects, terminal.backend().buffer().clone())
    }

    /// The column of `needle` on `row`, as drawn.
    fn column_of(buffer: &ratatui::buffer::Buffer, row: u16, needle: &str) -> Option<u16> {
        let line: Vec<&str> = (0..buffer.area.width)
            .map(|x| buffer[(x, row)].symbol())
            .collect();
        let chars: Vec<&str> = needle.split("").filter(|c| !c.is_empty()).collect();
        (0..line.len().saturating_sub(chars.len()))
            .find(|&x| line[x..x + chars.len()] == chars[..])
            .map(|x| x as u16)
    }

    /// The click target has to be where the tag is drawn, folded or not, or
    /// a click on `[3]` folds nothing and a click beside it folds the pane.
    #[test]
    fn the_fold_tag_is_where_the_title_draws_it() {
        for folded in [false, true] {
            let (rects, buffer) = rendered(|a| {
                a.inspector_folded = folded;
                a.contributions_folded = folded;
            });
            for (rect, panel, tag, label) in [
                (rects.inspector, FocusedPanel::Inspector, "[3]", "inspector"),
                (
                    rects.contributions,
                    FocusedPanel::Contributions,
                    "[4]",
                    "contributions",
                ),
            ] {
                let r = rect.unwrap_or_else(|| panic!("{label} not laid out"));
                let x = column_of(&buffer, r.y, tag)
                    .unwrap_or_else(|| panic!("{tag} not drawn (folded: {folded})"));
                for dx in 0..3 {
                    assert_eq!(rects.fold_tag_at(x + dx, r.y), Some(panel), "{tag}");
                }
                let lx = column_of(&buffer, r.y, label).expect("label drawn");
                assert_eq!(rects.fold_tag_at(lx, r.y), None, "{label} is not the tag");
                assert_eq!(rects.fold_tag_at(x, r.y + 1), None, "below the title row");
            }
        }
    }

    /// Only the auxiliary panes fold: the list and detail tags do nothing.
    #[test]
    fn the_repo_list_and_detail_tags_do_not_fold() {
        let (rects, buffer) = rendered(|_| {});
        let row = rects.repo_list.y;
        for tag in ["[1]", "[2]"] {
            let x = column_of(&buffer, row, tag).expect("tag drawn");
            assert_eq!(rects.fold_tag_at(x + 1, row), None, "{tag}");
        }
    }

    /// Folded is the frame alone, and the rows it gave up go to the panes
    /// above.
    #[test]
    fn a_folded_pane_keeps_only_its_frame() {
        let (open, _) = rendered(|_| {});
        let (folded, buffer) = rendered(|a| {
            a.inspector_folded = true;
            a.contributions_folded = true;
        });
        let inspector = folded.inspector.expect("folded inspector still laid out");
        let strip = folded.contributions.expect("folded strip still laid out");
        assert_eq!(inspector.height, FOLDED_HEIGHT);
        assert_eq!(strip.height, FOLDED_HEIGHT);
        assert!(folded.detail.unwrap().height > open.detail.unwrap().height);
        assert!(column_of(&buffer, inspector.y, "inspector").is_some());
        assert!(column_of(&buffer, strip.y, "contributions").is_some());
        for r in [inspector, strip] {
            let bottom = r.y + r.height - 1;
            assert_eq!(buffer[(r.x, r.y)].symbol(), "─", "top frame");
            assert_eq!(bottom, r.y, "the title row is the whole pane");
            assert!(!r.contains(Position::new(r.x + 2, r.y + 1)), "no content");
        }
    }

    /// A folded pane is its title row alone: each fold hands the panes above
    /// the two rows a three-row frame spent on a blank row and a bottom
    /// border.
    #[test]
    fn a_folded_pane_is_one_row_and_the_detail_pane_gets_the_rest() {
        let area = Rect::new(0, 0, 120, 44);
        let mut app = layout_app();
        app.view = View::Detail;
        app.contributions = crate::app::ContributionsMode::Github;
        let open = panel_rects(area, &app);
        app.inspector_folded = true;
        app.contributions_folded = true;
        let folded = panel_rects(area, &app);

        let inspector = folded.inspector.expect("folded inspector laid out");
        let strip = folded.contributions.expect("folded strip laid out");
        assert_eq!(inspector.height, 1, "the inspector folds to its title row");
        assert_eq!(strip.height, 1, "the strip folds to its title row");

        let open_inspector = open.inspector.expect("open inspector").height;
        let open_strip = open.contributions.expect("open strip").height;
        assert_eq!(
            folded.detail.unwrap().height,
            open.detail.unwrap().height + (open_inspector - 1) + (open_strip - 1),
            "every row a fold gives up goes to the detail pane"
        );
    }

    /// The one row is a rule with the pane's tag on it, no corners, in the
    /// pane's focus styling, and the tag is still where a click folds it.
    #[test]
    fn a_folded_row_is_a_rule_carrying_its_tag_in_focus_colors() {
        let (rects, buffer) = rendered(|a| {
            a.inspector_folded = true;
            a.contributions_folded = true;
            a.focused_panel = FocusedPanel::Inspector;
        });
        for (rect, panel, tag, tag_color, rule_color) in [
            (
                rects.inspector,
                FocusedPanel::Inspector,
                "[3]",
                colors::LAVENDER,
                colors::LAVENDER,
            ),
            (
                rects.contributions,
                FocusedPanel::Contributions,
                "[4]",
                colors::OVERLAY,
                colors::SURFACE,
            ),
        ] {
            let r = rect.unwrap_or_else(|| panic!("{tag} pane not laid out"));
            let right = r.x + r.width - 1;
            for x in [r.x, right] {
                assert_eq!(buffer[(x, r.y)].symbol(), "─", "{tag}: no corner");
                assert_eq!(buffer[(x, r.y)].fg, rule_color, "{tag}: rule color");
            }
            let x = column_of(&buffer, r.y, tag).unwrap_or_else(|| panic!("{tag} not drawn"));
            assert_eq!(buffer[(x, r.y)].fg, tag_color, "{tag}: tag color");
            assert_eq!(
                rects.fold_tag_at(x, r.y),
                Some(panel),
                "{tag}: click target"
            );
        }
    }

    /// Below a folded inspector's one row there is no frame left: the next
    /// row is the gap above the strip, and the strip's title follows it.
    #[test]
    fn the_row_below_a_folded_inspector_belongs_to_the_next_pane() {
        let (rects, buffer) = rendered(|a| {
            a.inspector_folded = true;
            a.contributions_folded = true;
        });
        let inspector = rects.inspector.expect("folded inspector laid out");
        let strip = rects.contributions.expect("folded strip laid out");
        let below = inspector.y + inspector.height;
        assert_eq!(below, inspector.y + 1, "the fold is the title row alone");
        assert_ne!(
            rects.at(inspector.x + 2, below),
            Some(FocusedPanel::Inspector)
        );
        for x in [
            inspector.x,
            inspector.x + 2,
            inspector.x + inspector.width - 1,
        ] {
            assert_eq!(buffer[(x, below)].symbol(), " ", "no frame below at {x}");
        }
        assert_eq!(strip.y, below + 1, "the strip follows the one-row gap");
        assert!(column_of(&buffer, strip.y, "contributions").is_some());
    }

    /// A folded inspector has nothing to resize, and its top row is its tag.
    #[test]
    fn a_folded_inspector_is_not_a_divider() {
        let (open, _) = rendered(|_| {});
        let r = open.inspector.unwrap();
        assert_eq!(open.divider_at(r.x + 20, r.y), Some(Divider::Horizontal));

        let (folded, _) = rendered(|a| a.inspector_folded = true);
        let r = folded.inspector.unwrap();
        assert_eq!(folded.divider_at(r.x + 20, r.y), None);
    }
}
