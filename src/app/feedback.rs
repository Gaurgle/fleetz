//! What the last frame measured and drew, reported back to the input layer:
//! scroll limits, view sizes, hit maps and the follow flags (`app.drawn`).
//!
//! Written by `ui::render` through `&App`, hence the `Cell`s, until R2 turns
//! the render pass into a function that returns this struct. Read by the key
//! and mouse handlers to clamp and to resolve clicks. Design:
//! `docs/app-split-design.md`, "Owner types" (`RenderFeedback`) and D4.

use super::{CiLogWrap, HistoryView, JobFollow, ListHits};

/// The render cells, moved off `App` unchanged (FZ-117).
pub(crate) struct RenderFeedback {
    /// Scroll offset per history view, indexed by `HistoryView::index`.
    ///
    /// Per view rather than shared, because a shared offset is wrong the
    /// moment you switch back to something you had already scrolled.
    ///
    /// `Cell` for the same reason `contrib_scroll` is one: the render pass is
    /// the only place that knows which screen row the selection landed on, so
    /// it is the only place that can scroll to keep it visible. Walking a
    /// cursor through two hundred rows the pane never followed is what made
    /// the view look frozen.
    pub(crate) detail_scrolls: [std::cell::Cell<u16>; HistoryView::COUNT],
    /// Largest useful scroll offset for each pane, measured by the renderer,
    /// which is the only place content length and pane height are both known.
    /// The scroll handlers clamp against these instead of a fixed cap.
    ///
    /// `Cell` because rendering takes `&App`: this is the render pass reporting
    /// a measurement back, not state the input layer owns. Both start at 0 and
    /// are written on the first frame, which is always drawn before any input
    /// is processed.
    pub(crate) detail_max_scroll: std::cell::Cell<u16>,
    /// Whether this frame drew the fleet's summary and activity in pane 1's
    /// summary box (FZ-95; pane 2's fleet column in FZ-93). Reset
    /// at the start of every frame and read by the footer, drawn after the
    /// panes, which carries the summary and the message itself when nothing
    /// else did (a pane 1 too short for the box), so an action never ends
    /// in silence.
    pub(crate) fleet_drawn: std::cell::Cell<bool>,
    /// Set when the cursor moves, cleared once the renderer has scrolled to
    /// show it.
    ///
    /// Without it the renderer re-centred on the selection every frame, which
    /// fought free scrolling to a standstill: `k` decremented the offset and
    /// the next frame put it straight back, so the pane looked frozen. The
    /// pane should chase the cursor when the cursor moves, and otherwise go
    /// where it is told.
    pub(crate) follow_selection: std::cell::Cell<bool>,
    /// Body row of every selectable entry in the *active* history view, in
    /// list order, recorded by the renderer.
    ///
    /// The graph's entries are its commit blocks, whose height depends on the
    /// message, the pane width and the wrap; the other views' are their list
    /// rows, which sit under however many sections the repo happened to draw.
    /// Either way the render pass is the only place that knows, which is why
    /// this is a `RefCell` for the same reason the scroll offsets are `Cell`s:
    /// the renderer reporting a measurement back, not state the input layer
    /// owns. Only the active view is drawn, so only its rows are held.
    pub(crate) detail_row_tops: std::cell::RefCell<Vec<u16>>,
    /// Where the detail pane's last frame drew its rows and their hashes, so
    /// a click resolves against what is on screen (FZ-37). Recorded by the
    /// renderer for the reason `detail_row_tops` is.
    pub(crate) detail_hits: std::cell::RefCell<crate::ui::detail::DetailHits>,
    /// Height of the detail pane's scrolling body, measured by the renderer.
    /// The scrolloff arithmetic needs it, and the renderer is the only place
    /// the pane's height and the header's are both known.
    pub(crate) detail_view_height: std::cell::Cell<u16>,
    pub(crate) inspector_max_scroll: std::cell::Cell<u16>,
    /// Leftmost visible week. The grid is wider than any terminal at the
    /// square size we draw, so it scrolls instead of shrinking. Written by
    /// the renderer (which is the only place the visible width is known) as
    /// well as by input, hence the `Cell`.
    pub(crate) contrib_scroll: std::cell::Cell<usize>,
    /// Weeks the last frame could actually fit, measured by the renderer so
    /// the scroll keys and the cursor have something real to clamp against.
    pub(crate) contrib_visible_weeks: std::cell::Cell<usize>,
    /// `expand_remote_branches` as the detail pane last drew it, so the frame
    /// that sees it flip on is the one that reveals the opened rows (FZ-55).
    /// A `Cell` because the renderer records it, as `detail_view_height`.
    pub(crate) remotes_drawn_expanded: std::cell::Cell<bool>,
    /// Whether the `jobs` body's next frame brings the marked job into
    /// view, and the offset it chose (`JobFollow`). Written by the renderer,
    /// which alone knows where the job's rows fall.
    pub(crate) ci_job_follow: std::cell::Cell<JobFollow>,
    /// The `log` body's whole log as last wrapped, written by the renderer
    /// (`RefCell` for the reason `detail_row_tops` is one). A held log never
    /// changes, so its job id and the width are the whole key.
    pub(crate) ci_log_wrap: std::cell::RefCell<Option<CiLogWrap>>,
    /// Largest useful reader offset, measured by the renderer for the
    /// reason `inspector_max_scroll` is.
    pub(crate) reader_max_scroll: std::cell::Cell<u16>,
    /// Rows the reader's body drew on the last frame: one `H`/`L` page.
    pub(crate) reader_view_height: std::cell::Cell<u16>,
    /// Columns the reader's body drew on the last frame: the width a
    /// markdown body is rendered at (FZ-59), off the UI thread.
    pub(crate) reader_width: std::cell::Cell<u16>,
    /// Columns of the main area the repo list and the right column split,
    /// as the key handler last measured it; 0 before it has. `<` and `>`
    /// step from the list width this draws (FZ-69), which differs from
    /// `repo_list_pct` where the default yields to the cap.
    pub(crate) main_area_width: std::cell::Cell<u16>,
    /// Where the repo list's last frame drew its rows, so a click or a hover
    /// resolves against what is on screen. Written by the renderer for the
    /// reason `detail_hits` is.
    pub(crate) list_hits: std::cell::Cell<ListHits>,
}

impl RenderFeedback {
    /// The values `App::new` gave these cells; not `Default`, since the
    /// strip's scroll opens past the end and `ci_job_follow` at `Free`.
    pub(crate) fn new() -> Self {
        Self {
            detail_scrolls: std::array::from_fn(|_| std::cell::Cell::new(0)),
            detail_max_scroll: std::cell::Cell::new(0),
            fleet_drawn: std::cell::Cell::new(false),
            follow_selection: std::cell::Cell::new(false),
            detail_row_tops: std::cell::RefCell::new(Vec::new()),
            detail_hits: std::cell::RefCell::new(Default::default()),
            detail_view_height: std::cell::Cell::new(0),
            inspector_max_scroll: std::cell::Cell::new(0),
            // Out of range on purpose: the renderer clamps it down to
            // "newest week at the right edge", which is where the grid opens.
            contrib_scroll: std::cell::Cell::new(crate::contributions::WEEKS),
            contrib_visible_weeks: std::cell::Cell::new(crate::contributions::WEEKS),
            remotes_drawn_expanded: std::cell::Cell::new(false),
            ci_job_follow: std::cell::Cell::new(JobFollow::Free),
            ci_log_wrap: std::cell::RefCell::new(None),
            reader_max_scroll: std::cell::Cell::new(0),
            reader_view_height: std::cell::Cell::new(0),
            reader_width: std::cell::Cell::new(0),
            main_area_width: std::cell::Cell::new(0),
            list_hits: std::cell::Cell::new(ListHits::default()),
        }
    }
}
