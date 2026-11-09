pub mod badges;
pub mod branches;
pub mod ci;
pub mod colors;
pub mod compact;
pub mod contributions;
pub mod detail;
pub mod digest;
pub mod footer;
pub mod grips;
pub mod help;
pub mod highlight;
pub mod intro;
pub mod markdown;
pub mod scroll;
pub mod settings;
pub mod timewindow;
pub mod todo;

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
        FetchFailureKind::Ssh => "ssh",
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
        FetchFailureKind::Ssh => "ssh key refused (run ssh-add)",
        FetchFailureKind::Unreachable => "remote unreachable",
        FetchFailureKind::NoRemote => "remote not found",
        FetchFailureKind::Other => "git error",
        FetchFailureKind::TooBig => "over the size cap",
    }
}

/// The row marker for a repo whose last fetch failed, or None. The warning
/// glyph follows the active icon set (`badges::fetch_warning_icon`).
pub fn fetch_marker(facts: &crate::model::FetchFacts) -> Option<String> {
    let failure = facts.failure()?;
    Some(format!(
        "{} {}",
        badges::fetch_warning_icon(),
        fetch_failure_short(failure.kind)
    ))
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
/// The colour here is the segment's own; the status line draws it in
/// `GithubHealth`'s instead, which makes a failure red on Andreas's ask
/// (2026-11-05) where this keeps the older `PEACH`. Cached is the dim `OVERLAY` of the `swept` label: an attempt
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
    /// The `notes` layout's reader text, right of pane 2's divider inside
    /// the `detail` rect (FZ-62, FZ-68); None in every other layout.
    pub reader: Option<Rect>,
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
        // The reader before the detail pane: it sits inside pane 2 in the
        // `notes` layout (FZ-67).
        if self.reader.is_some_and(|r| r.contains(p)) {
            return Some(FocusedPanel::Reader);
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
        // A folded inspector has no height to drag. Bounded to the
        // inspector's own columns.
        if let Some(inspector) = self.inspector
            && !self.inspector_folded
            && col >= inspector.x
            && col < inspector.x + inspector.width
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

/// The frame's rows: header, the compact view's status line, main area,
/// footer. One function for `render` and `panel_rects`, so the main area
/// they hit-test is the one drawn.
pub struct FrameChunks {
    pub header: Rect,
    /// Only in the compact view (`uses_status_line`); the detail view puts
    /// the summary and the activity in pane 1's summary box and the window
    /// on pane 1's top border (`detail::summary_box_lines`,
    /// `detail::list_title_window`).
    pub status: Option<Rect>,
    pub main: Rect,
    pub footer: Rect,
}

/// Split the frame. The chrome is three rows either way: a pane that
/// shrank by a row would lose its inspector at the minimum height.
pub fn frame_chunks(area: Rect, app: &App) -> FrameChunks {
    if uses_status_line(app) {
        let c = Layout::vertical([
            Constraint::Length(1), // header: name and version
            Constraint::Length(1), // status line on its divider
            Constraint::Min(1),    // main content
            Constraint::Length(1), // footer: keys, under the grid
        ])
        .split(area);
        FrameChunks {
            header: c[0],
            status: Some(c[1]),
            main: c[2],
            footer: c[3],
        }
    } else {
        let c = Layout::vertical([
            Constraint::Length(1), // header: name and version on a divider
            Constraint::Min(1),    // main content
            Constraint::Length(2), // footer: divider and keys
        ])
        .split(area);
        FrameChunks {
            header: c[0],
            status: None,
            main: c[1],
            footer: c[2],
        }
    }
}

/// Recompute the panel layout for `area` -- must mirror `render` +
/// `detail::render` exactly so hit-testing lines up with what's drawn.
pub fn panel_rects(area: Rect, app: &App) -> PanelRects {
    let main = frame_chunks(area, app).main;
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
                reader: None,
                contributions,
            }
        }
        // The `notes` layout (FZ-62) splits columns first: pane 1 the full
        // height on the left, docking no inspector, as `graph` docks none
        // (the file's facts sit in pane 2 beside the header, FZ-68), and
        // pane 2 at full height on the right, split by a divider into the
        // tree's column and the preview's. No preview while pane 2 shows a
        // digest or no repo.
        View::Detail if app.notes_layout() => {
            let (body, contributions) = contributions::split_main_area(main, strip_folded);
            let (repo_list, pane2) = detail::split_main(body, app.repo_list_pct);
            let reader = detail::notes_pane_body(pane2, app).map(|notes| notes.reader);
            PanelRects {
                main,
                body,
                repo_list,
                detail: Some(pane2),
                inspector: None,
                inspector_folded: false,
                reader,
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
                reader: None,
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
    render_at(frame, app, std::time::Instant::now());
}

/// `render` with the copy flash (FZ-81) drawn as it stands at `now`, so a
/// test can pick the moment.
pub fn render_at(frame: &mut Frame, app: &App, now: std::time::Instant) {
    let area = frame.area();

    // Pane 1's summary box sets it when drawn; the footer, drawn after it,
    // reads it.
    app.fleet_drawn.set(false);
    let chunks = frame_chunks(area, app);
    render_header(frame, chunks.header, app);
    if let Some(status) = chunks.status {
        render_status_line(frame, status, app);
    }

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
                    detail::render(
                        frame,
                        rects.repo_list,
                        detail_area,
                        rects.inspector,
                        rects.reader,
                        app,
                    );
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
        paint_yank_flash(frame.buffer_mut(), &rects, app, now);
    }

    // Footer
    footer::render(frame, chunks.footer, app);

    // Overlay (rendered last, on top)
    match &app.overlay {
        Overlay::Help => help::render(frame, area),
        Overlay::Branches(bo) => branches::render(frame, area, bo),
        Overlay::TimeWindow(two) => timewindow::render(frame, area, two),
        Overlay::Settings(so) => settings::render(frame, area, so),
        Overlay::None => {}
    }
}

/// Paint the copy flash (FZ-81) at `now` over the frame just drawn.
///
/// A list pane's flash is its cursor row: the cells wearing the band
/// (`SELECTED_BG`), which every list draws its cursor with, so the sweep
/// crosses exactly what the band covers. The inspector and the reader have
/// no cursor row, so theirs is their title: the inspector's label on its top
/// border, the reader's `notes preview` above the rule in pane 2. Nothing once
/// the app no longer points at the row the copy came from.
fn paint_yank_flash(
    buf: &mut ratatui::buffer::Buffer,
    rects: &PanelRects,
    app: &App,
    now: std::time::Instant,
) {
    let Some(source) = app.yank_flash.source() else {
        return;
    };
    if app.view != View::Detail || source != app.flash_source() {
        return;
    }
    let cells = match source.pane {
        FocusedPanel::RepoList => band_cells(buf, rects.repo_list),
        FocusedPanel::Detail => rects.detail.and_then(|r| band_cells(buf, r)),
        FocusedPanel::Inspector => rects.inspector.and_then(|r| border_label_cells(buf, r)),
        FocusedPanel::Reader => rects
            .detail
            .and_then(|pane| detail::reader_label_rect(pane, app))
            .and_then(|label| text_cells(buf, label)),
        FocusedPanel::Contributions => None,
    };
    let Some((y, xs)) = cells else { return };
    let width = xs.len() as u16;
    for (col, x) in xs.into_iter().enumerate() {
        buf[(x, y)].bg = app.yank_flash.cell_bg(now, col as u16, width);
    }
}

/// The first row inside `pane`'s border that wears the band, and its band
/// cells' columns. Only the part of `pane` inside the buffer is read: on a
/// terminal too small for the layout a pane can lie past its edge.
fn band_cells(buf: &ratatui::buffer::Buffer, pane: Rect) -> Option<(u16, Vec<u16>)> {
    let inner = pane
        .inner(ratatui::layout::Margin::new(1, 1))
        .intersection(buf.area);
    (inner.top()..inner.bottom()).find_map(|y| {
        let xs: Vec<u16> = (inner.left()..inner.right())
            .filter(|&x| buf[(x, y)].bg == colors::SELECTED_BG)
            .collect();
        (!xs.is_empty()).then_some((y, xs))
    })
}

/// The label on `pane`'s top border: from the corner to the first rule
/// cell after it. `None` when that border is not inside the buffer.
fn border_label_cells(buf: &ratatui::buffer::Buffer, pane: Rect) -> Option<(u16, Vec<u16>)> {
    let border = Rect::new(pane.x, pane.y, pane.width, pane.height.min(1)).intersection(buf.area);
    if border.is_empty() {
        return None;
    }
    let y = border.top();
    let xs = (pane.left() + 1..pane.right().saturating_sub(1))
        .take_while(|&x| border.contains(Position { x, y }) && buf[(x, y)].symbol() != "─")
        .collect();
    Some((y, xs))
}

/// The cells of the one-row `label` that lie inside the buffer.
fn text_cells(buf: &ratatui::buffer::Buffer, label: Rect) -> Option<(u16, Vec<u16>)> {
    let label = label.intersection(buf.area);
    (!label.is_empty()).then(|| (label.y, (label.left()..label.right()).collect()))
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

/// Render the header: the name and the version on the left; outside the
/// compact view, loading, GitHub, watch and the sweep time on the right.
/// The summary and the activity sit in pane 1's summary box, the window in
/// its box at the right of pane 2's header (FZ-95), and in the compact view
/// all of it on the status line under this row.
fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let name = Span::styled(
        " fleetz",
        Style::default()
            .fg(colors::LAVENDER)
            .add_modifier(Modifier::BOLD),
    );
    // Dimmest colour: "which build am I looking at" is answerable without
    // quitting, and it is the least urgent thing on screen.
    let version = Span::styled(format!(" {VERSION}"), Style::default().fg(colors::SUBTEXT));
    let line = Line::from(vec![name.clone(), version]);
    // Under it the compact view's status line is the divider, and carries
    // watch and the sweep. Elsewhere the header draws its own divider, as it
    // always did, with watch and the sweep time flush right on it (his look,
    // 2026-11-05): both say how current everything below is, so they sit
    // over all of it rather than in the status box.
    let block = if uses_status_line(app) {
        Block::default().title_top(line.left_aligned())
    } else {
        // Flush right, so a piece appearing on the left of the group moves
        // nothing to its right: loading while the fleet lands, then GitHub's
        // connection, watch and the sweep time (his look, 2026-11-05: GitHub
        // "belongs there"). Loading is here, not in pane 2, because pane 2
        // has no header to draw until the first repo lands.
        let p = fleet_pieces(app, chrono::Local::now());
        let build = |github: &str, with_swept: bool| {
            let mut right: Vec<Span<'static>> = Vec::new();
            if let Some(loading) = p.loading.clone() {
                right.extend(loading);
            }
            right.push(Span::styled(
                github.to_string(),
                Style::default().fg(p.github_health.color()),
            ));
            right.extend(p.watch.iter().cloned());
            if with_swept && let Some(swept) = p.swept.clone() {
                right.extend(swept);
            }
            right.push(Span::raw(" "));
            right
        };
        // The two sides are placed independently, so on a narrow terminal
        // they would overprint: GitHub shortens first, then the sweep time
        // goes, then the version. Loading, GitHub and watch always stay.
        let columns = |spans: &[Span]| {
            spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>()
        };
        let width = area.width as usize;
        let candidates = [
            (line.clone(), build(&p.github_long, true)),
            (line.clone(), build(&p.github_short, true)),
            (line.clone(), build(&p.github_short, false)),
            (Line::from(name), build(&p.github_short, false)),
        ];
        let last = candidates.len() - 1;
        let (left, right) = candidates
            .into_iter()
            .enumerate()
            .find(|(i, (left, right))| *i == last || left.width() + columns(right) < width)
            .map(|(_, pair)| pair)
            .expect("the last candidate always matches");
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(colors::SURFACE))
            .title_bottom(left.left_aligned())
            .title_bottom(Line::from(right).right_aligned())
    };
    frame.render_widget(block, area);
}

/// Whether the fleet's state is drawn on a status line under the header
/// rather than in pane 1's summary box and pane 2's header: only the
/// compact view, which has neither pane.
pub fn uses_status_line(app: &App) -> bool {
    app.view == View::Compact
}

/// How GitHub is doing, as the status line's lamp says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubHealth {
    /// The last poll succeeded and nothing holds polling back.
    Connected,
    /// Data on screen, but aging: rate limited, paused or slowed.
    Issue,
    /// The last poll failed for a reason that will not clear on its own.
    Error,
    /// No poll has answered yet: first load, or running on the cache.
    Unknown,
}

impl GithubHealth {
    pub fn color(self) -> Color {
        match self {
            GithubHealth::Connected => colors::GREEN,
            GithubHealth::Issue => colors::PEACH,
            GithubHealth::Error => colors::RED,
            GithubHealth::Unknown => colors::OVERLAY,
        }
    }
}

/// The status line's GitHub piece: its health, the full text and the short
/// form a narrow line falls back to. Always drawn, unlike the old header
/// segment, so a healthy poll says `connected` instead of nothing. The
/// words are `github_segment`'s whenever it has any; the failure form keeps
/// its `⚠`, which stands in for the lamp.
pub fn github_status(
    app: &App,
    now: chrono::DateTime<chrono::Local>,
) -> (GithubHealth, String, String) {
    use crate::app::Outcome;
    // `-d` polls nothing by design: say so, not "waiting".
    if app.dirty_mode {
        return (
            GithubHealth::Unknown,
            " ○ github off ".to_string(),
            " ○ github off ".to_string(),
        );
    }
    let Some((long, short, _)) = github_segment(app, now) else {
        return match app.github_freshness.outcome {
            Outcome::Success => (
                GithubHealth::Connected,
                " ● github connected ".to_string(),
                " ● github ".to_string(),
            ),
            _ => (
                GithubHealth::Unknown,
                " ○ github ".to_string(),
                " ○ github ".to_string(),
            ),
        };
    };
    let health = match &app.github_freshness.outcome {
        Outcome::Failed(failure) if failure.rate_limited => GithubHealth::Issue,
        Outcome::Failed(_) => GithubHealth::Error,
        Outcome::Success => GithubHealth::Issue,
        Outcome::NotAttempted => GithubHealth::Unknown,
    };
    if matches!(app.github_freshness.outcome, Outcome::Failed(_)) {
        return (health, long, short);
    }
    let lamp = if health == GithubHealth::Unknown {
        "○"
    } else {
        "●"
    };
    (health, format!(" {lamp}{long}"), format!(" {lamp}{short}"))
}

/// The time window in bold teal, so the scope every list on screen is cut
/// to reads at a glance rather than as one more grey phrase. No ground
/// behind it: on his look a tinted chip read as a highlight, as if it were
/// selected (2026-11-05).
fn window_chip(label: &str) -> Span<'static> {
    Span::styled(
        format!("{label} "),
        Style::default()
            .fg(colors::TEAL)
            .add_modifier(Modifier::BOLD),
    )
}

/// The fleet's state as spans, built once and laid out where each view
/// shows it: on one row by `render_status_line` (compact), in pane 1's
/// summary box (`detail::summary_box_lines`), and on the footer where
/// neither was drawn (`footer::fallback_spans`).
pub struct FleetPieces {
    pub window: Option<Span<'static>>,
    /// The one key hint off the footer, and only because it sits on the
    /// thing it edits: `t` opens the picker for exactly the window beside
    /// it. Dropped with the window, since a bare `[t]` explains nothing.
    pub window_key: Span<'static>,
    pub group: Option<Span<'static>>,
    /// The fleet's counters.
    pub summary: Vec<Span<'static>>,
    /// The transient status message after an action, while it lasts.
    pub message: Option<Span<'static>>,
    /// While the initial load or an R reload is in flight.
    pub loading: Option<Vec<Span<'static>>>,
    /// A manual refresh's spinner, naming the half it is on.
    pub refreshing: Option<Vec<Span<'static>>>,
    /// `swept 11:42`.
    pub swept: Option<Vec<Span<'static>>>,
    pub github_health: GithubHealth,
    pub github_long: String,
    pub github_short: String,
    pub watch: Vec<Span<'static>>,
}

pub fn fleet_pieces(app: &App, now: chrono::DateTime<chrono::Local>) -> FleetPieces {
    let window = (!app.header.is_empty()).then(|| window_chip(&app.header));
    let window_key = Span::styled(
        "[t]",
        Style::default()
            .fg(colors::TEXT)
            .add_modifier(Modifier::BOLD),
    );
    let group = app
        .active_group_label()
        .map(|group| Span::styled(format!("[{group}]"), Style::default().fg(colors::MAUVE)));
    let summary = footer::summary_spans(&app.summary());
    let message = app.status_message.as_ref().map(|msg| {
        let color = if msg.is_error {
            colors::RED
        } else {
            colors::GREEN
        };
        Span::styled(msg.text.clone(), Style::default().fg(color))
    });
    let loading = app
        .loading
        .then(|| spinner_group(app.spinner_frame, "loading"));
    // Only a sweep the user asked for takes over this slot. A watch-timer
    // sweep runs silently and leaves the timestamp in place, so an interval
    // refresh cannot repaint a moving colored indicator over a dashboard
    // being read.
    let refreshing = app.refresh_phase().map(|phase| {
        let label = match phase {
            crate::app::RefreshPhase::Local => "checking local changes…",
            crate::app::RefreshPhase::Remote => "checking remote…",
        };
        spinner_group(app.spinner_frame, label)
    });
    let swept = app.last_refreshed.map(|ts| {
        // Dimmer than the rest on purpose: reassurance you glance at, not
        // information you read. Unless it has gone stale, which is the
        // one time it has something to say.
        let color = if sweep_overdue(app.watch_mode, app.refresh_interval, ts, now) {
            colors::PEACH
        } else {
            colors::OVERLAY
        };
        vec![Span::styled(
            format!(" swept {} ", swept_label(ts, now)),
            Style::default().fg(color),
        )]
    });
    // Always drawn, in the colour of its health: a failing poll has to say so
    // somewhere that survives a keypress, and a healthy one now says so too.
    let (github_health, github_long, github_short) = github_status(app, now);
    // The `w` toggle's state, at a glance. Only the lamp moves, the word
    // beside it holds one steady colour (see `spinner_group`); off is drawn
    // too, as a crossed-out eye, so "off" differs from "not drawn".
    let watch = if app.watch_mode {
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
    FleetPieces {
        window,
        window_key,
        group,
        summary,
        message,
        loading,
        refreshing,
        swept,
        github_health,
        github_long,
        github_short,
        watch,
    }
}

/// Render the compact view's status line: the time window, the group and
/// the fleet's summary on the left; loading, GitHub, watch and the sweep
/// time on the right.
///
/// The two sides are positioned independently by ratatui with no mutual
/// awareness, so on a narrow line they can overlap. To avoid that, drop the
/// least important piece until both fit: the sweep time, then the group,
/// then GitHub's long form, then the summary, then the window. Loading,
/// GitHub's short form and watch are never dropped.
fn render_status_line(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width as usize;
    let mut p = fleet_pieces(app, chrono::Local::now());
    let gh_style = Style::default().fg(p.github_health.color());
    // One row has no room for slots of their own: a message takes the
    // summary's place while it lasts, a manual refresh the sweep time's.
    if let Some(message) = p.message.take() {
        p.summary = vec![message];
    }
    if let Some(refreshing) = p.refreshing.take() {
        p.swept = Some(refreshing);
    }

    let mut include_ts = true;
    let mut include_group = true;
    let mut gh_long = true;
    let mut include_summary = true;
    let mut include_window = true;

    loop {
        let mut left: Vec<Span> = vec![Span::raw(" ")];
        if include_window && let Some(ref w) = p.window {
            left.push(w.clone());
            left.push(p.window_key.clone());
        }
        if include_group && let Some(ref g) = p.group {
            left.push(Span::raw("  "));
            left.push(g.clone());
        }
        if include_summary && !p.summary.is_empty() {
            left.push(Span::raw("  "));
            left.extend(p.summary.iter().cloned());
            left.push(Span::raw(" "));
        }

        let mut right: Vec<Span> = Vec::new();
        if let Some(ref l) = p.loading {
            right.extend(l.iter().cloned());
        }
        let gh_text = if gh_long {
            &p.github_long
        } else {
            &p.github_short
        };
        right.push(Span::styled(gh_text.clone(), gh_style));
        right.extend(p.watch.iter().cloned());
        if include_ts && let Some(ref t) = p.swept {
            right.extend(t.iter().cloned());
            right.push(Span::raw(" "));
        }

        // Characters, not bytes: every glyph here is one column wide, and a
        // byte count charged the arrows and the eye up to four each.
        let left_w: usize = left.iter().map(|s| s.content.chars().count()).sum();
        let right_w: usize = right.iter().map(|s| s.content.chars().count()).sum();
        let exhausted =
            !include_ts && !include_group && !gh_long && !include_summary && !include_window;

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
        } else if include_group {
            include_group = false;
        } else if gh_long {
            gh_long = false;
        } else if include_summary {
            include_summary = false;
        } else {
            include_window = false;
        }
    }
}

#[cfg(test)]
mod tests;
