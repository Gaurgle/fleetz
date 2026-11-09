use super::header::header_lines;
use super::layout::REPO_LIST_MAX_COLS;
use super::worktrees::worktree_rows;
use super::*;
use crate::app::FocusedPanel;
use crate::graph::{COMMIT_NODE, MERGE_NODE};
use crate::model::{CiStatus, RefKind, RepoKind, RepoLocation, RepoStatus};

/// The node is the one glyph here we are free to pick, so it stays Narrow
/// even though the lanes cannot be. The lanes are safe for a different
/// reason: every pane border in the app is drawn with the same box
/// characters already.
#[test]
fn both_nodes_occupy_one_column() {
    for node in [COMMIT_NODE, MERGE_NODE] {
        assert_eq!(Span::from(node.to_string()).width(), 1, "{node}");
    }
    for ambiguous in ['●', '○', '•', '◆', '⊙'] {
        assert_ne!(COMMIT_NODE, ambiguous);
        assert_ne!(MERGE_NODE, ambiguous);
    }
}

/// The graph column is padded to the widest prefix and drawn per cell, so
/// an Ambiguous-width node would be one column in some terminals and two
/// in others, and every row after it would misalign. Narrow is a
/// requirement here, not a preference.
#[test]
fn the_commit_node_occupies_one_column() {
    // Ratatui's own measurement, which is the one the layout uses, so a
    // mismatch here is a mismatch on screen. U+26AC is East Asian Narrow;
    // the obvious alternatives (● ○ • ◆ ⊙) are all Ambiguous, meaning
    // their width is a terminal setting rather than a property.
    assert_eq!(Span::from(COMMIT_NODE.to_string()).width(), 1);
    for ambiguous in ['●', '○', '•', '◆', '⊙'] {
        assert_ne!(
            COMMIT_NODE, ambiguous,
            "{ambiguous} is Ambiguous-width and would misalign the column"
        );
    }
}

pub(super) fn branch_fixture(
    name: &str,
    upstream: Option<&str>,
    behind: u32,
    is_head: bool,
) -> crate::model::BranchStatus {
    crate::model::BranchStatus {
        name: name.to_string(),
        upstream: upstream.map(|u| u.to_string()),
        behind,
        ahead: 0,
        is_head,
        tip: "a1b2c3d".to_string(),
    }
}

pub(super) fn busy_repo() -> RepoStatus {
    let mut r = cloned_repo();
    r.stash_count = 2;
    r.branches = vec![crate::model::BranchStatus {
        name: "main".to_string(),
        upstream: Some("origin/main".to_string()),
        behind: 0,
        ahead: 0,
        is_head: true,
        tip: "a1b2c3d".to_string(),
    }];
    r.commits = vec![crate::model::Commit {
        hash: "a1b2c3d".to_string(),
        author: "Gaurgle".to_string(),
        date: Some(chrono::Utc::now()),
        message: "feat: a thing".to_string(),
        body: String::new(),
        insertions: 3,
        deletions: 1,
        files: vec![],
    }];
    r.reflog = vec![crate::model::ReflogEntry {
        hash: "a1b2c3d".to_string(),
        action: "commit".to_string(),
        description: "feat: a thing".to_string(),
    }];
    r.graph = vec![crate::model::GraphLine {
        lanes: vec![crate::graph::commit_cell()],
        hash: "a1b2c3d".to_string(),
        is_merge: false,
        refs: vec![],
        author: "Gaurgle".to_string(),
        message: "feat: a thing".to_string(),
    }];
    // Real `git log --graph` shape, so the expanded view has its own
    // content rather than borrowing the compact list's.
    r.graph_expanded = vec![
        "* commit a1b2c3d (HEAD -> main)".to_string(),
        "| Author: Gaurgle <g@example.com>".to_string(),
        "| Date:   2 hours ago".to_string(),
        "|".to_string(),
        "|     feat: a thing".to_string(),
    ];
    r
}

pub(super) fn pane_text(view: HistoryView, width: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), width, 44);
    app.history_view = view;
    let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    (0..30u16)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The commit rows of the `commits` view, border stripped, for a list with
/// the given per-commit churn.
fn commit_rows(churn: &[(u32, u32)]) -> Vec<String> {
    commit_rows_at(churn, 100, |i| format!("chore: an ordinary subject {i}"))
}

/// The same at a chosen width, with the subject under the caller's control
/// so a test can force the truncation case.
pub(super) fn commit_rows_at(
    churn: &[(u32, u32)],
    width: u16,
    subject: impl Fn(usize) -> String,
) -> Vec<String> {
    let mut r = cloned_repo();
    r.graph = (0..churn.len())
        .map(|i| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: format!("c{i:06}"),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: subject(i),
        })
        .collect();
    r.commits = churn
        .iter()
        .enumerate()
        .map(|(i, (insertions, deletions))| crate::model::Commit {
            hash: format!("c{i:06}"),
            author: "Gaurgle".to_string(),
            date: Some(chrono::Utc::now()),
            message: subject(i),
            body: String::new(),
            insertions: *insertions,
            deletions: *deletions,
            files: vec![],
        })
        .collect();

    let height = 24u16;
    let mut app = crate::app::App::new(vec![r], "t".to_string(), width, height);
    app.history_view = HistoryView::Commits;
    let app = app;
    let mut term =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    (0..height)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .filter(|row| row.contains(" Gau "))
        .collect()
}

/// Column positions, counted in characters. The pane is full of multibyte
/// box-drawing and the leader dot, so `str::find`'s byte offsets do not
/// answer "which column", which is the only question these tests ask.
pub(super) fn columns(row: &str) -> Vec<char> {
    row.chars().collect()
}

/// The column a right-aligned `+N` / `-N` token ends at, which is the edge
/// that has to agree between rows. Their *starts* differ by design: that is
/// what right-aligning them means.
pub(super) fn token_end(row: &str, sign: char) -> Option<usize> {
    let cols = columns(row);
    let start = cols.iter().position(|c| *c == sign)?;
    let len = cols[start + 1..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    Some(start + 1 + len)
}

/// `+2` and `+338` share a right edge, and so do their deletions. Aligning
/// the pair as one right-aligned blob -- which is what it did -- put every
/// row's `+` in a different column, leaving the one part of the row that is
/// read by scanning down the page the one part left ragged.
#[test]
fn the_churn_columns_share_a_right_edge() {
    let rows = commit_rows(&[(2, 2), (338, 21), (104, 8), (7, 3)]);
    assert_eq!(rows.len(), 4, "fixture did not draw four commit rows");

    let adds: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '+')).collect();
    let dels: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '-')).collect();
    assert_eq!(adds.len(), 4, "a row lost its insertions");
    assert_eq!(dels.len(), 4, "a row lost its deletions");
    assert!(
        adds.windows(2).all(|w| w[0] == w[1]),
        "insertions ragged across rows: {adds:?}"
    );
    assert!(
        dels.windows(2).all(|w| w[0] == w[1]),
        "deletions ragged across rows: {dels:?}"
    );
}

/// A row with no churn still spends the columns, so the leader dots stop
/// where the `+` column begins rather than running under it on whichever
/// rows happen to have nothing to show.
#[test]
fn a_row_without_churn_still_holds_the_columns() {
    let rows = commit_rows(&[(338, 21), (0, 0)]);
    let dots_end = |r: &str| {
        columns(r)
            .iter()
            .rposition(|c| *c == '·')
            .expect("row drew no leader dots")
    };
    assert_eq!(
        dots_end(&rows[0]),
        dots_end(&rows[1]),
        "the churn-less row let its dots run into the columns"
    );
}

/// One-sided churn keeps the other column's blanks, which is what holds
/// the alignment when a delete-only commit sits under an add-only one.
#[test]
fn one_sided_churn_keeps_the_other_column() {
    let rows = commit_rows(&[(338, 21), (7, 0), (0, 12)]);
    let adds: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '+')).collect();
    let dels: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '-')).collect();
    assert_eq!(adds.len(), 2, "expected two rows with insertions");
    assert_eq!(dels.len(), 2, "expected two rows with deletions");
    assert_eq!(adds[0], adds[1], "insertions moved: {adds:?}");
    assert_eq!(dels[0], dels[1], "deletions moved: {dels:?}");
}

/// A repo shaped like the one that exposed this: several local branches
/// behind by different amounts, one of them tracking a ref it is not named
/// after, a long remote-only list under them, and enough history to scroll.
pub(super) fn spread_repo() -> RepoStatus {
    let mut r = busy_repo();
    r.behind = 40;
    r.branches = vec![
        crate::model::BranchStatus {
            name: "main".to_string(),
            upstream: Some("origin/main".to_string()),
            behind: 0,
            ahead: 0,
            is_head: true,
            tip: String::new(),
        },
        crate::model::BranchStatus {
            name: "beta".to_string(),
            upstream: Some("origin/beta".to_string()),
            behind: 21,
            ahead: 0,
            is_head: false,
            tip: String::new(),
        },
        crate::model::BranchStatus {
            name: "feature/who-goes-first".to_string(),
            upstream: Some("origin/wip".to_string()),
            behind: 19,
            ahead: 0,
            is_head: false,
            tip: String::new(),
        },
    ];
    r.remote_only_branches = (0..12)
        .map(|i| format!("origin/feature/spare-{i}"))
        .collect();
    // `info` draws its commit list from the graph rather than from
    // `commits`, so this is the field that decides whether the pane has
    // anything to scroll.
    r.graph = (0..30)
        .map(|i| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: format!("c{i:06}"),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("feat: commit {i}"),
        })
        .collect();
    r
}

/// The `info` pane's visible text at a given size, scrolled as far as its
/// content allows. The offset is clamped during render, so a large value
/// parks it at the bottom whatever the content turns out to measure.
fn info_text_at_bottom(repo: RepoStatus, width: u16, height: u16) -> String {
    info_text_at_bottom_expanded(repo, width, height, false)
}

pub(super) fn info_text_at_bottom_expanded(
    repo: RepoStatus,
    width: u16,
    height: u16,
    expand: bool,
) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = crate::app::App::new(vec![repo], "t".to_string(), width, height);
    app.history_view = HistoryView::Info;
    app.expand_remote_branches = expand;
    app.set_detail_scroll(u16::MAX);
    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    (0..height)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The branches block is the only thing that explains the repo's behind
/// count, so it has to survive the scrolling that reading the history
/// requires. Unpinned, stepping the commit cursor pushed it off the top
/// and left the "not fetched" list reading like the whole story.
#[test]
fn the_branches_block_survives_a_scroll_to_the_bottom() {
    // 20 rows is the shortest pane that still pins the branches;
    // `the_branches_pin_holds_down_to_a_twenty_row_pane` pins that
    // threshold directly.
    let text = info_text_at_bottom(spread_repo(), 92, 20);
    assert!(
        text.contains("branches"),
        "the branches label scrolled away"
    );
    assert!(text.contains("21 behind"), "beta's count scrolled away");
    assert!(
        text.contains("19 behind"),
        "the feature branch's count scrolled away"
    );
}

/// Where the branches block stops being pinned, stated so the next row
/// added to the pinned region is a deliberate decision rather than a
/// surprise. The headliner's divider is one such row and it landed inside
/// the slack `MIN_BODY_ROWS` already had, so this threshold is where it was
/// before the headliner grew. A boxed version, briefly tried, was three
/// rows and did push it to 21.
///
/// Below the threshold the ladder still keeps the identity header pinned;
/// it is the branches block alone that goes.
#[test]
fn the_branches_pin_holds_down_to_a_nineteen_row_pane() {
    // 19 and 18 since FZ-93 moved the readout onto the view's title row,
    // giving pane 2 a row back; 20 and 19 before.
    let pinned = |height| info_text_at_bottom(spread_repo(), 92, height).contains("21 behind");
    assert!(pinned(19), "19 rows should hold the branches");
    assert!(!pinned(18), "18 rows should not");
    assert!(
        info_text_at_bottom(spread_repo(), 92, 18).contains("~/repos/fleetz"),
        "the identity header lost its pin along with the branches block"
    );
}

/// On a pane too short for both, the header keeps its pin rather than the
/// pair losing it together -- the fallback is the whole reason the pinned
/// count is chosen from a ladder instead of a single test.
#[test]
fn a_short_pane_falls_back_to_pinning_the_header_alone() {
    // 17 rows is the window where the header alone clears MIN_BODY_ROWS
    // and the header plus the branches block does not.
    let text = info_text_at_bottom(spread_repo(), 92, 17);
    assert!(
        text.contains("~/repos/fleetz"),
        "the identity header lost its pin along with the branches block"
    );
}

/// A branch tracking a ref it is not named after says so. Left implicit,
/// its row is indistinguishable from one tracking its own name, and the
/// count against it cannot be reconciled with the remote.
#[test]
fn a_branch_names_an_upstream_that_differs_from_its_own_name() {
    let text = info_text_at_bottom(spread_repo(), 120, 22);
    assert!(
        text.contains("feature/who-goes-first -> origin/wip"),
        "the differing upstream is not shown"
    );
    // The ordinary case stays bare: repeating `origin/beta` after `beta`
    // on every row would bury the one row worth reading.
    assert!(
        !text.contains("beta -> origin/beta"),
        "a same-named upstream is being repeated as noise"
    );
}

/// Whether a rendered pane shows `name` as its headliner. The headliner
/// row trims to exactly the view's name; the tab strip in the border
/// carries the same string but never trims to it alone, so this is what
/// tells the two apart.
fn has_headliner(text: &str, name: &str) -> bool {
    text.lines()
        .any(|l| l.split('│').any(|column| is_title_column(column, name)))
}

/// Whether a bordered column is the view's title row: the name alone,
/// or the name and the commit counter flush right of it (FZ-93). The tab
/// strip draws the same name between other names and `·`, never this.
pub(super) fn is_title_column(column: &str, name: &str) -> bool {
    let column = column.trim();
    column == name
        || column
            .strip_prefix(name)
            .is_some_and(|rest| rest.starts_with(' ') && rest.trim_start().starts_with('['))
}

/// The colours of the headliner's divider: its first cell and its last.
fn rule_ends(view: HistoryView) -> (ratatui::style::Color, ratatui::style::Color) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let width = 92u16;
    let mut app = crate::app::App::new(vec![spread_repo()], "t".to_string(), width, 24);
    app.history_view = view;
    let app = app;
    let mut term = Terminal::new(TestBackend::new(width, 24)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();

    // The divider is the row directly under the name.
    let name = view_name(view);
    let title_y = (1..24u16)
        .find(|y| {
            let row: String = (0..width).map(|x| buf[(x, *y)].symbol()).collect();
            row.split('│').any(|column| is_title_column(column, &name))
        })
        .expect("no headliner drawn");
    let y = title_y + 1;
    let cells: Vec<u16> = (0..width)
        .filter(|x| buf[(*x, y)].symbol() == "─")
        .collect();
    assert!(cells.len() > 2, "no divider under the headliner");
    (
        buf[(cells[0], y)].fg,
        buf[(*cells.last().expect("last cell"), y)].fg,
    )
}

/// The divider fades out of the name's own colour rather than starting at
/// structure-grey. Starting there made it read as a separate dim line that
/// happened to sit under the name, instead of as the name's own underline
/// running out.
#[test]
fn the_headliner_divider_fades_from_the_name_colour_into_surface() {
    for view in HistoryView::ALL {
        let on = colors::history_view_color(view);
        let (near, far) = rule_ends(view);
        assert_ne!(
            near,
            colors::SURFACE,
            "{} divider starts at structure-grey, so it is not fading",
            view.label()
        );
        assert_eq!(
            far,
            colors::SURFACE,
            "{} divider does not arrive at structure-grey",
            view.label()
        );
        assert_ne!(near, on, "{} divider starts un-faded", view.label());
    }
}

/// The near end is nearer the name's colour than the far end is, in every
/// channel
/// that separates the two colours. States the direction of the fade, which
/// the endpoints alone do not.
#[test]
fn the_headliner_divider_fades_towards_the_far_end() {
    let channels = |c| match c {
        ratatui::style::Color::Rgb(r, g, b) => (r as i32, g as i32, b as i32),
        other => panic!("expected an RGB colour, got {other:?}"),
    };
    for view in HistoryView::ALL {
        let on = channels(colors::history_view_color(view));
        let surface = channels(colors::SURFACE);
        let (near, far) = rule_ends(view);
        let (near, far) = (channels(near), channels(far));
        for (i, (o, s)) in [(on.0, surface.0), (on.1, surface.1), (on.2, surface.2)]
            .into_iter()
            .enumerate()
        {
            if o == s {
                continue;
            }
            let (n, f) = match i {
                0 => (near.0, far.0),
                1 => (near.1, far.1),
                _ => (near.2, far.2),
            };
            assert!(
                (n - o).abs() <= (f - o).abs(),
                "{} channel {i}: near end is not closer to the name colour",
                view.label()
            );
        }
    }
}

/// The formatting rule itself. The render tests ask the helper what to
/// expect, so without this nothing states what it should produce.
#[test]
fn a_view_name_is_drawn_in_capitals() {
    assert_eq!(view_name(HistoryView::Info), "INFO");
    assert_eq!(view_name(HistoryView::Commits), "COMMITS");
    assert_eq!(view_name(HistoryView::Graph), "GRAPH");
}

/// Capitals cost the strip nothing, which is what lets the tab and the
/// headliner share one form: the switcher runs on a fit budget that drops
/// the inactive tabs when the labels outgrow the border it shares with the
/// pane title, and a wider name would collapse it to the active tab alone
/// on panes where all four fit today.
#[test]
fn capitalising_a_tab_does_not_change_its_width() {
    for view in HistoryView::ALL {
        assert_eq!(
            view_name(view).chars().count(),
            view.label().chars().count(),
            "{} changed width in the strip",
            view.label()
        );
    }
}

/// `busy_repo` with a second local branch `feature`, two ahead of its
/// upstream, checked out in a linked worktree inside the repo with three
/// uncommitted changes, last committed to two days ago.
pub(super) fn repo_with_a_worktree() -> RepoStatus {
    let mut r = busy_repo();
    r.branches.push(crate::model::BranchStatus {
        name: "feature".to_string(),
        upstream: Some("origin/feature".to_string()),
        behind: 0,
        ahead: 2,
        is_head: false,
        tip: "b2c3d4e".to_string(),
    });
    r.worktrees = vec![crate::model::Worktree {
        path: "~/repos/fleetz/.claude/worktrees/feat".to_string(),
        branch: "feature".to_string(),
        detached: false,
        head: "b2c3d4e".to_string(),
        dirty: Some(3),
        head_at: Some(chrono::Utc::now() - chrono::Duration::days(2)),
    }];
    r
}

/// The detail pane drawn in `view`, as the buffer and its rows of text,
/// the side borders blanked so a row trims to its content. One cell per
/// character, so a column in the text is a column in the buffer.
pub(super) fn drawn(repo: RepoStatus, view: HistoryView) -> (ratatui::buffer::Buffer, Vec<String>) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let (w, h) = (100u16, 40u16);
    let mut app = crate::app::App::new(vec![repo], "t".to_string(), w, h);
    app.history_view = view;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let rows = (0..h)
        .map(|y| {
            (0..w)
                .map(|x| match buf[(x, y)].symbol() {
                    "│" => " ",
                    s => s,
                })
                .collect::<String>()
        })
        .collect();
    (buf, rows)
}

/// The body wraps, while the commit list's row tops are counted in source
/// lines. A worktree row wider than the pane wrapped and put every commit
/// below it a row lower than recorded, so the cursor follow misjudged the
/// selection. A row is bounded to the pane, the path cut to what is left.
#[test]
fn a_wide_worktree_row_stays_one_row_and_the_commit_tops_hold() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut r = repo_with_a_worktree();
    r.branches[1].name = "feat/a-very-long-branch-name-for-the-worktree".to_string();
    r.branches[1].upstream = None;
    r.worktrees[0].branch = r.branches[1].name.clone();
    r.worktrees[0].path = "~/repos/fleetz-worktrees/a/deeply/nested/checkout/path".to_string();

    let (w, h) = (50u16, 40u16);
    let mut app = crate::app::App::new(vec![r], "t".to_string(), w, h);
    app.history_view = HistoryView::Info;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let rows: Vec<String> = (0..h)
        .map(|y| {
            (0..w)
                .map(|x| match buf[(x, y)].symbol() {
                    "│" => " ",
                    s => s,
                })
                .collect()
        })
        .collect();
    let text = rows.join("\n");

    let label = rows
        .iter()
        .position(|l| l.trim() == "worktrees")
        .unwrap_or_else(|| panic!("no label:\n{text}"));
    assert!(rows[label + 1].contains("feat/a-very"), "{text}");
    assert!(
        rows[label + 1].contains('…'),
        "the row is cut at the pane's edge:\n{text}"
    );
    assert_eq!(
        rows[label + 2].trim(),
        "",
        "the worktree row wrapped onto a second row:\n{text}"
    );

    // The pinned block ends with the last local branch; the body starts
    // on the blank row below it, and the first commit must sit where its
    // recorded top says.
    let branches = rows
        .iter()
        .position(|l| l.trim() == "branches")
        .unwrap_or_else(|| panic!("no branches block:\n{text}"));
    let body_top = branches
        + rows[branches..]
            .iter()
            .position(|l| l.trim().is_empty())
            .expect("a blank row closes the branches block");
    let first_top = *app
        .detail_row_tops
        .borrow()
        .first()
        .expect("the commit list recorded its tops") as usize;
    let commit = rows
        .iter()
        .position(|l| l.contains("a1b2c3d") && !l.contains('['))
        .unwrap_or_else(|| panic!("no commit row:\n{text}"));
    assert_eq!(
        commit,
        body_top + first_top,
        "the commit is not where its top says:\n{text}"
    );

    // Where the columns leave room, it is the path that gives way.
    let r = app.selected_repo().expect("the repo").clone();
    let row: String = worktree_rows(&r, 70)[1]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(row.chars().count(), 70, "{row}");
    assert!(row.ends_with('…'), "{row}");
    assert!(row.contains("2d ago  ~/repos/"), "{row}");
}

/// The tab strip names the view and lights it in its own colour.
#[test]
fn the_tab_strip_names_the_worktrees_view() {
    let line = view_switcher(HistoryView::Worktrees, true, 200);
    let tab = line
        .spans
        .iter()
        .find(|s| s.content == "WORKTREES")
        .expect("a WORKTREES tab");
    assert_eq!(tab.style.fg, Some(colors::PINK));
}

/// Each view draws its own section and none of the others'. The whole
/// point is that they take turns rather than queueing for one scroll
/// offset, so a section leaking across views defeats it.
#[test]
fn each_view_shows_only_its_own_section() {
    // `info` draws a named section of its own; the other three are named
    // by the pane's headliner, matched in its boxed form. The bare name
    // will not do: the tab strip carries all four in every view, and the
    // strip and the headliner draw the same string, so `contains("GRAPH")`
    // is true in every view and would assert nothing.
    // `info` draws a named section of its own; the other three are named
    // by the pane's headliner. The bare name will not do for those: the
    // tab strip carries all four in every view and draws the same string,
    // so `contains("GRAPH")` is true everywhere and asserts nothing.
    for view in HistoryView::ALL {
        let text = pane_text(view, 92);
        match view {
            HistoryView::Info => assert!(
                text.contains("recent commits"),
                "info is missing its own section"
            ),
            _ => assert!(
                has_headliner(&text, &view_name(view)),
                "{} is missing its headliner",
                view.label()
            ),
        }
        for other in HistoryView::ALL {
            if other == view {
                continue;
            }
            assert!(
                !has_headliner(&text, &view_name(other)),
                "{} leaked the {} headliner",
                view.label(),
                other.label()
            );
        }
        if view != HistoryView::Info {
            assert!(
                !text.contains("recent commits"),
                "{} leaked info's section",
                view.label()
            );
        }
    }
}

/// The header and the switcher are what tell you where you are, so they
/// must survive every view and every scroll.
#[test]
fn the_header_and_tab_strip_are_in_every_view() {
    for view in HistoryView::ALL {
        let text = pane_text(view, 104);
        assert!(text.contains("~/repos/fleetz"), "{}", view.label());
        assert!(text.contains("synced"), "{}", view.label());
        for tab in HistoryView::ALL {
            assert!(
                text.contains(&view_name(tab)),
                "{} is missing the {} tab",
                view.label(),
                tab.label()
            );
        }
    }
}

/// The switcher moved into the border because the body scrolls. The doc
/// comment on the old one claimed it was pinned above the scrolling
/// content; it was the scrolling content's first row, so paging down took
/// it off screen -- exactly when "what am I looking at" gets asked.
#[test]
fn the_view_switcher_is_in_the_border_and_says_which_keys_move_it() {
    for view in HistoryView::ALL {
        let border = pane_text(view, 104)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        for tab in HistoryView::ALL {
            assert!(
                border.contains(&view_name(tab)),
                "{} view: the border is missing the {} tab: {border:?}",
                view.label(),
                tab.label()
            );
        }
        assert!(
            border.contains("[/]"),
            "{} view: the border does not say which keys switch: {border:?}",
            view.label()
        );
    }
}

/// The regression itself: scroll the body to the bottom and the switcher
/// is still there, because it is not in the body.
#[test]
fn scrolling_to_the_bottom_does_not_lose_the_view_switcher() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut r = busy_repo();
    r.graph = (0..200)
        .map(|i| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: format!("h{i:06}"),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("subject {i}"),
        })
        .collect();
    r.reflog = (0..200)
        .map(|i| crate::model::ReflogEntry {
            hash: format!("r{i:06}"),
            action: "commit".to_string(),
            description: format!("entry {i}"),
        })
        .collect();

    for view in HistoryView::ALL {
        let mut app = crate::app::App::new(vec![r.clone()], "t".to_string(), 104, 30);
        app.history_view = view;
        app.set_detail_scroll(150);
        let mut term = Terminal::new(TestBackend::new(104, 30)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let border: String = (0..104u16).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(
            border.contains(&view_name(view)),
            "{} view: scrolled away: {border:?}",
            view.label()
        );
    }
}

/// The standing invariant: `panel_rects` must report the rectangles
/// `render` draws, and both reach the split through `split_main`. A cap
/// added to one side only is exactly the drift that function exists to
/// prevent, and it fails silently -- clicks land on the wrong pane.
#[test]
fn the_capped_split_is_the_one_hit_testing_uses() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 300, 40);
    let mut term = Terminal::new(TestBackend::new(300, 40)).unwrap();
    let mut drawn = None;
    term.draw(|f| {
        drawn = Some(crate::ui::panel_rects(f.area(), &app));
        crate::ui::render(f, &app);
    })
    .unwrap();
    let rects = drawn.expect("panel_rects");
    assert_eq!(
        rects.repo_list.width, REPO_LIST_MAX_COLS,
        "hit testing still thinks the list is the uncapped width"
    );
}

/// Only the active tab is coloured, and it wears the colour its own view's
/// content uses. One lit tab reads instantly; four lit ones are a palette.
#[test]
fn only_the_active_tab_wears_its_view_colour() {
    for view in HistoryView::ALL {
        let line = view_switcher(view, true, 200);
        let label = |v: HistoryView| {
            line.spans
                .iter()
                .find(|s| s.content == view_name(v))
                .unwrap_or_else(|| panic!("{} tab missing", v.label()))
        };
        assert_eq!(
            label(view).style.fg,
            Some(colors::history_view_color(view)),
            "the {} tab is not lit while it is active",
            view.label()
        );
        for other in HistoryView::ALL.iter().filter(|v| **v != view) {
            assert_eq!(
                label(*other).style.fg,
                Some(colors::SUBTEXT),
                "the {} tab is lit while {} is active",
                other.label(),
                view.label()
            );
        }
    }
}

/// A view's label cut to the first three letters, as the shortened
/// strip draws it.
fn short_name(view: HistoryView) -> String {
    view_name(view).chars().take(3).collect()
}

/// The cells a strip of these labels takes, key hint and spacing included.
fn switcher_width(labels: &[String]) -> usize {
    let cells: usize = labels.iter().map(|l| l.chars().count()).sum();
    5 + cells + 3 * (labels.len() - 1) + 1
}

fn full_width() -> usize {
    switcher_width(&HistoryView::ALL.map(view_name))
}

fn short_width() -> usize {
    switcher_width(&HistoryView::ALL.map(short_name))
}

/// A budget of exactly the full width still draws every full name: the
/// threshold is inclusive, so the strip never shortens a tab it had room for.
#[test]
fn a_budget_of_exactly_the_full_width_draws_every_full_name() {
    let names = HistoryView::ALL.map(view_name);
    for view in HistoryView::ALL {
        let line = view_switcher(view, true, full_width());
        let tabs: Vec<String> = line
            .spans
            .iter()
            .filter(|s| names.contains(&s.content.to_string()))
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(tabs, names.to_vec(), "{}", view.label());
        assert!(
            line.spans.iter().any(|s| s.content == "/"),
            "{} dropped the [/] hint",
            view.label()
        );
    }
}

/// FZ-60 regression: adding `CHANGES` widened the full strip by ten
/// columns, and in that gap the switcher dropped to the active name
/// alone. There it now keeps every tab, each cut to three letters, in
/// ALL's order, the active one lit and the key hint kept.
#[test]
fn a_strip_too_narrow_for_full_names_shows_every_tab_shortened() {
    assert!(short_width() < full_width());
    for budget in [short_width(), full_width() - 1] {
        for view in HistoryView::ALL {
            let line = view_switcher(view, true, budget);
            let shorts = HistoryView::ALL.map(short_name);
            let tabs: Vec<_> = line
                .spans
                .iter()
                .filter(|s| shorts.contains(&s.content.to_string()))
                .map(|s| (s.content.to_string(), s.style.fg))
                .collect();
            let want: Vec<_> = HistoryView::ALL
                .iter()
                .map(|v| {
                    let colour = match *v == view {
                        true => colors::history_view_color(view),
                        false => colors::SUBTEXT,
                    };
                    (short_name(*v), Some(colour))
                })
                .collect();
            assert_eq!(tabs, want, "{} at budget {budget}", view.label());
            assert!(
                line.spans.iter().any(|s| s.content == "/"),
                "the key hint was dropped at budget {budget}"
            );
        }
    }
}

/// Too narrow even for the short labels, the strip is `[X/N]`: the active
/// view's place in the cycle, digits in its colour, no label text.
#[test]
fn a_strip_too_narrow_for_short_names_shows_the_position_alone() {
    let budget = short_width() - 1;
    let n = HistoryView::COUNT;
    for (i, view) in [(1, HistoryView::ALL[0]), (n, HistoryView::ALL[n - 1])] {
        let line = view_switcher(view, true, budget);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, format!(" [{i}/{n}] "), "{}", view.label());
        let digits = line
            .spans
            .iter()
            .find(|s| s.content == format!("{i}/{n}"))
            .expect("the position is one span");
        assert_eq!(digits.style.fg, Some(colors::history_view_color(view)));
        for bracket in ["[", "]"] {
            let span = line.spans.iter().find(|s| s.content == bracket).unwrap();
            assert_eq!(span.style.fg, Some(colors::TEXT), "{bracket} is not a key");
        }
        for other in HistoryView::ALL {
            assert!(
                !text.contains(&short_name(other)),
                "{} label drawn",
                other.label()
            );
        }
    }
}

/// The position form drops the `[/]` hint: its own brackets already
/// stand there, and a second pair beside them reads as noise.
#[test]
fn the_position_form_has_no_key_hint() {
    for view in HistoryView::ALL {
        let line = view_switcher(view, true, short_width() - 1);
        assert!(
            !line.spans.iter().any(|s| s.content == "/"),
            "{} kept the [/] hint",
            view.label()
        );
        let brackets = line
            .spans
            .iter()
            .filter(|s| s.content == "[" || s.content == "]")
            .count();
        assert_eq!(brackets, 2, "{}", view.label());
    }
}

/// The colours have to be four, not one repeated: the point is telling
/// them apart at a glance from across the screen.
#[test]
fn the_four_view_colours_are_distinct() {
    let mut seen = Vec::new();
    for view in HistoryView::ALL {
        let c = colors::history_view_color(view);
        assert!(!seen.contains(&c), "{} shares a colour", view.label());
        seen.push(c);
    }
}

/// Arriving in a view shows its page, not one of its commits. Switching
/// used to follow the selection, and the expanded graph anchors its
/// selection to the top of the pane -- so asking for the tab scrolled its
/// own header off and landed you on a commit.
#[test]
fn switching_views_does_not_scroll_the_one_you_arrive_in() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut r = busy_repo();
    r.graph_expanded = (0..30)
        .flat_map(|i| {
            vec![
                format!("* commit e{i:06} (HEAD -> main)"),
                "| Author: Gaurgle <g@example.com>".to_string(),
                "|".to_string(),
                format!("|     expanded subject {i}"),
            ]
        })
        .collect();

    let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 30);
    while app.history_view != HistoryView::Graph {
        app.cycle_history_view(false);
    }
    assert_eq!(app.detail_scroll(), 0, "arriving already moved the offset");

    let mut term = Terminal::new(TestBackend::new(92, 24)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    assert_eq!(
        app.detail_scroll(),
        0,
        "the first frame after arriving scrolled the page"
    );
    let buf = term.backend().buffer().clone();
    let text: String = (0..24u16)
        .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        has_headliner(&text, &view_name(HistoryView::Graph)),
        "the view title scrolled off"
    );
    assert!(
        text.contains("e000000"),
        "the first commit is not on screen"
    );
}

/// The tab in the frame is what you aim at; the title inside the pane is
/// what confirms you landed. Same word, same colour, one source -- said
/// twice in two colours they read as two different things.
#[test]
fn the_pinned_title_matches_the_tab_it_belongs_to() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    for view in HistoryView::ALL {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 30);
        app.history_view = view;
        let mut term = Terminal::new(TestBackend::new(92, 24)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();

        // The title row is the one inside the pane whose text is exactly
        // the view's headliner -- the frame's tab strip carries all four
        // on row 0, so start below it.
        let title = view_name(view);
        let first_letter = title[0..1].to_string();
        let want = colors::history_view_color(view);
        let mut found = false;
        for y in 1..24u16 {
            let row: String = (0..92u16).map(|x| buf[(x, y)].symbol()).collect();
            // The row has to *be* the name: the strip draws the same
            // string, so a substring match would find the border row as
            // readily as the headliner.
            if !row.split('│').any(|column| is_title_column(column, &title)) {
                continue;
            }
            let x = (0..92u16)
                .find(|x| buf[(*x, y)].symbol() == first_letter)
                .expect("first letter");
            assert_eq!(
                buf[(x, y)].fg,
                want,
                "the {} title is not in its tab's colour",
                view.label()
            );
            found = true;
            break;
        }
        assert!(found, "{} has no title row of its own", view.label());
    }
}

/// The inspector follows whichever list is active, resolving the hash
/// rather than an index -- the reflog and the commit list are different
/// lists of different lengths, so an index into one means nothing in the
/// other.
#[test]
fn the_inspector_follows_the_active_view() {
    for view in [
        HistoryView::Commits,
        HistoryView::Reflog,
        HistoryView::Graph,
    ] {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
        app.history_view = view;
        assert_eq!(
            app.inspected_hash(),
            Some("a1b2c3d"),
            "{} did not reach the commit",
            view.label()
        );
        assert_eq!(
            app.inspected_commit().map(|c| c.message.as_str()),
            Some("feat: a thing"),
            "{}",
            view.label()
        );
    }
}

/// Every view has a selection of its own now, `info` included -- it lists
/// the newest commits rather than nothing.
#[test]
fn every_view_including_info_resolves_its_own_selection() {
    for view in HistoryView::ALL {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
        app.history_view = view;
        // A run is not a commit; FZ-30f details runs.
        let expected = match view {
            HistoryView::Ci | HistoryView::Notes | HistoryView::Changes => None,
            _ => Some("a1b2c3d"),
        };
        assert_eq!(
            app.inspected_hash(),
            expected,
            "{} resolved nothing",
            view.label()
        );
    }
}

/// A reflog entry can name a commit older than the loaded list. Saying
/// "no commit selected" there would be a lie: something is selected, it
/// just is not in the window.
#[test]
fn a_selection_outside_the_loaded_range_is_not_reported_as_none() {
    let mut r = busy_repo();
    r.reflog[0].hash = "deadbee".to_string();
    let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 44);
    app.history_view = HistoryView::Reflog;
    assert_eq!(app.inspected_hash(), Some("deadbee"), "still a selection");
    assert!(app.inspected_commit().is_none(), "but not a loaded commit");
}

/// Grey said "shown because we must, not because you want it", which is
/// right for a path and wrong for a stash. The badge and the pane have to
/// pick the same colour, or the two disagree about what a stash looks
/// like the moment either is touched.
#[test]
fn stashes_are_not_drawn_as_furniture() {
    for grey in [colors::OVERLAY, colors::SUBTEXT, colors::SURFACE] {
        assert_ne!(colors::STASH_CLR, grey, "stash is news, not furniture");
    }
    assert_ne!(colors::STASH_CLR, colors::RED, "nor a failing build");
    assert_ne!(colors::STASH_CLR, colors::YELLOW, "nor uncommitted work");
}

/// One colour per concept: the badge, the footer count, the PR number in
/// the list, and the `#prs` filter tag must not drift apart. They used to
/// share MAUVE with section labels, which made a PR count read as
/// furniture instead of as news.
#[test]
fn every_pr_surface_uses_the_same_colour() {
    assert_eq!(
        crate::ui::filter_color(crate::app::FilterKind::HasPrs),
        colors::PR_CLR,
        "the filter tag must match the badge"
    );
    assert_ne!(colors::PR_CLR, colors::MAUVE, "PRs are not a section label");
    assert_ne!(colors::PR_CLR, colors::PEACH, "nor a sync counter");
}

/// Regression: the whole `(...)` blob was drawn in one flat mauve, so a
/// local branch and its remote counterpart were indistinguishable.
#[test]
fn each_ref_kind_gets_its_own_colour() {
    let kinds = [RefKind::Head, RefKind::Local, RefKind::Remote, RefKind::Tag];
    let seen: std::collections::HashSet<_> = kinds.iter().map(|k| colors::ref_color(*k)).collect();
    assert_eq!(seen.len(), kinds.len(), "two kinds share a colour");
}

/// A hash is a hash in every pane, but `YELLOW` already means
/// "uncommitted" in the badges and the footer. The gold has to be its own
/// colour, not a second meaning stacked on that one.
#[test]
fn the_hash_gold_is_not_the_uncommitted_yellow() {
    assert_ne!(colors::HASH_CLR, colors::YELLOW);
}

pub(super) fn repo(name: &str, kind: RepoKind, owner: Option<&str>) -> RepoStatus {
    RepoStatus {
        daily: None,
        daily_key: None,
        last_local: None,
        name: name.to_string(),
        path: Some("~/repos/x".to_string()),
        location: RepoLocation::Primary,
        branches: vec![],
        commits: vec![],
        behind: 0,
        ahead: 0,
        dirty: 0,
        pr_count: 0,
        prs: vec![],
        ci_status: CiStatus::Unknown,
        ci_workflow: None,
        ci_duration: None,
        groups: vec![],
        stash_count: 0,
        stash_entries: vec![],
        worktrees: vec![],
        reflog: vec![],
        graph: vec![],
        graph_expanded: Vec::new(),
        repo_kind: kind,
        owner: owner.map(|s| s.to_string()),
        remote_url: None,
        remote_only_branches: vec![],
        remote_branches: vec![],
        work_key: None,
        fetch: crate::model::FetchFacts::default(),
        authors: Default::default(),
    }
}

/// Renders the real pane and reads rows back out, so placement is
/// verified against actual output rather than the arithmetic behind it.
/// Row 2 is the status line, row 3 the remote-URL line. Rows 0 and 1 are
/// the border and the block's top padding; the selected-commit readout
/// rides the view's title row since FZ-93 (`readout_row`). The border and
/// padding are stripped so assertions are about content, not the frame
/// around it.
pub(super) fn pane_rows(repo: RepoStatus, width: u16) -> (String, String) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            render_detail_pane(f, area, &app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    // Leading whitespace is kept deliberately: the URL row is indented,
    // so trimming the front would make two flush-right rows measure
    // different lengths and hide a real misalignment.
    let row = |y: u16| {
        (0..width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .trim_matches('│')
            .trim_end()
            .to_string()
    };
    (row(2), row(3))
}

/// The header's three rows: state, location, and the local/remote fact
/// row beneath them.
pub(super) fn header_rows(repo: RepoStatus, width: u16) -> (String, String, String) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
    let mut terminal = Terminal::new(TestBackend::new(width, 14)).unwrap();
    terminal
        .draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row = |y: u16| {
        (0..width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .trim_matches('│')
            .trim_end()
            .to_string()
    };
    (row(2), row(3), row(4))
}

pub(super) fn cloned_repo() -> RepoStatus {
    let mut r = repo("fleetz", RepoKind::Public, Some("Gaurgle"));
    r.path = Some("~/repos/fleetz".to_string());
    r.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
    r
}

/// Pane 2's three header rows as drawn at `width`, borders kept, for an
/// app `setup` prepares.
pub(super) fn header_block(width: u16, setup: impl FnOnce(&mut crate::app::App)) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut app = crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), width, 40);
    setup(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(width, 16)).unwrap();
    terminal
        .draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (2..5u16)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

/// The whole frame at `w` by `h`: every row as drawn, and the footer's
/// last row, the keys row.
pub(super) fn frame_rows(app: &crate::app::App, w: u16, h: u16) -> (Vec<String>, String) {
    let rows = drawn_rows(w, h, |f| crate::ui::render(f, app));
    let last = rows.last().cloned().unwrap_or_default();
    (rows, last)
}

/// The fleet summary's text as the footer would draw it.
pub(super) fn summary_text(app: &crate::app::App) -> String {
    crate::ui::footer::summary_spans(&app.summary())
        .iter()
        .map(|s| s.content.as_ref())
        .collect()
}

/// Where pane 1 draws its summary box, the footer is keys only: the
/// message and the summary are drawn once, in pane 1 (FZ-95; pane 2's
/// fleet column in FZ-93, review round 2). Fails while the box stops
/// marking the frame as having drawn them.
#[test]
fn beside_the_summary_box_the_frame_draws_the_message_once() {
    let mut app = crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), 160, 30);
    app.view = crate::app::View::Detail;
    app.contributions_folded = true;
    app.set_status("copied 1abc123".to_string(), false);
    let (rows, last) = frame_rows(&app, 160, 30);
    let summary = summary_text(&app);
    assert!(!summary.is_empty());
    assert!(
        rows.iter().any(|r| r.contains("copied 1abc123")),
        "pane 1 drew no message:\n{}",
        rows.join("\n")
    );
    assert!(!last.contains("copied"), "the footer repeats it: {last:?}");
    assert!(!last.contains(&summary), "the footer repeats it: {last:?}");
}

/// A header row's local and remote sections: the text before its first
/// divider, and between the first and the second (FZ-95).
pub(super) fn sections(row: &str) -> (String, String) {
    let mut parts = row.split('│');
    let local = parts.next().unwrap_or_default().to_string();
    let remote = parts.next().unwrap_or_default().to_string();
    (local, remote)
}

/// Pane 2's rows from the top padding (row 1) down, as drawn at
/// `width`, for `repo`.
pub(super) fn pane2_rows(
    repo: RepoStatus,
    width: u16,
    setup: impl FnOnce(&mut crate::app::App),
) -> Vec<String> {
    let mut app = crate::app::App::new(vec![repo], "since 2 weeks".to_string(), width, 40);
    setup(&mut app);
    drawn_rows(width, 20, |f| render_detail_pane(f, f.area(), &app))
        .into_iter()
        .skip(1)
        .collect()
}

/// The columns of the `│` inside pane 2's frame on a drawn frame row.
pub(super) fn pane_two_dividers(row: &str, detail: Rect) -> Vec<usize> {
    let row: Vec<char> = row.chars().collect();
    (usize::from(detail.x) + 1..usize::from(detail.right()) - 1)
        .filter(|x| row[*x] == '│')
        .collect()
}

/// FZ-95: the window box pane 2's header carried during the look is
/// gone at every width: the frame draws the window once, on pane 1's
/// top border, and never a framed box.
#[test]
fn pane_two_draws_no_window_box_at_any_width() {
    for width in [60u16, 80, 100, 120, 140, 160, 200] {
        let mut app =
            crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), width, 30);
        app.view = crate::app::View::Detail;
        let (rows, _) = frame_rows(&app, width, 30);
        let detail = crate::ui::panel_rects(Rect::new(0, 0, width, 30), &app)
            .detail
            .expect("pane 2");
        let pane_rows = usize::from(detail.y)..usize::from(detail.bottom());
        for (y, row) in rows
            .iter()
            .enumerate()
            .filter(|(y, _)| pane_rows.contains(y))
        {
            let pane2: String = row
                .chars()
                .skip(usize::from(detail.x))
                .take(usize::from(detail.width))
                .collect();
            assert!(!pane2.contains("window"), "w{width} row {y}: {pane2:?}");
            assert!(!pane2.contains('◷'), "w{width} row {y}: {pane2:?}");
            assert!(!pane2.contains("[t]"), "w{width} row {y}: {pane2:?}");
        }
    }
}

/// FZ-95 review 3: the window's label, however long, and its shedding
/// on a narrow pane 1 border never move pane 2's divider.
#[test]
fn the_window_label_never_moves_pane_twos_divider() {
    for width in [100u16, 160] {
        let dividers = |header: &str| {
            let mut app = crate::app::App::new(vec![cloned_repo()], header.to_string(), width, 30);
            app.view = crate::app::View::Detail;
            let (rows, _) = frame_rows(&app, width, 30);
            let detail = crate::ui::panel_rects(Rect::new(0, 0, width, 30), &app)
                .detail
                .expect("pane 2");
            let at = rows
                .iter()
                .position(|r| r.contains("Gaurgle/fleetz"))
                .expect("pane 2's location row");
            (rows[1].clone(), pane_two_dividers(&rows[at], detail))
        };
        let (_, plain) = dividers("since 2 weeks");
        assert_eq!(plain.len(), 1, "w{width}: {plain:?}");
        for header in [
            "",
            "since 2026-11-09  Work (09-17) (50 of 95 repos)",
            "since 2 weeks (50 of 95 repos and a long coverage note)",
        ] {
            let (border, cols) = dividers(header);
            assert_eq!(cols, plain, "w{width} {header:?}: {border:?}");
        }
    }
}

/// A single token longer than the pane must still render, and must
/// terminate. Breaking only at spaces loops forever on a long URL.
#[test]
fn a_token_longer_than_the_pane_still_renders() {
    let long = "x".repeat(120);
    let rows = wrap_text(&long, 20);
    assert_eq!(rows.len(), 6, "expected 120 chars to fill six 20-wide rows");
    for row in &rows {
        assert!(row.chars().count() <= 20);
    }
    assert_eq!(rows.concat(), long, "characters were dropped");
}

/// Empty input is one empty row, not zero: the blank lines between git's
/// blocks are load-bearing spacing.
#[test]
fn empty_text_wraps_to_a_single_empty_row() {
    assert_eq!(wrap_text("", 20), vec![String::new()]);
}

/// The graph view pinned the selected commit to the top of the pane on
/// every selection change, so stepping one commit moved the whole view.
/// A block already clear of both margins must not move it at all.
///
/// All four cases below run at height 20, where `scrolloff_for` is 4.
#[test]
fn a_visible_block_does_not_move_the_view() {
    assert_eq!(bring_into_view((16, 21), 10, 20), 10);
}

/// A block above the viewport scrolls up until it clears the top margin,
/// not until it touches the top edge: an entry brought hard against the
/// edge is one you cannot see what precedes.
#[test]
fn a_block_above_the_viewport_stops_at_the_margin() {
    assert_eq!(bring_into_view((14, 19), 20, 20), 10);
}

/// A block just inside the margin still moves. This is the case that made
/// the margin worth having: the old rule called it visible and left the
/// selection pinned to the pane's first row.
#[test]
fn a_block_inside_the_top_margin_still_scrolls() {
    assert_eq!(bring_into_view((12, 17), 10, 20), 8);
}

/// A block below the viewport scrolls down by the least that clears the
/// bottom margin.
#[test]
fn a_block_below_the_viewport_scrolls_the_minimum() {
    assert_eq!(bring_into_view((30, 34), 10, 20), 19);
}

/// A block taller than the pane cannot clear both margins, so its top
/// wins: scrolling to the bottom of an oversized block leaves you looking
/// at the tail of a message with nothing to say whose it is. This is the
/// one thing the old top-pinning got right.
#[test]
fn an_oversized_block_shows_its_top() {
    assert_eq!(bring_into_view((30, 60), 10, 20), 26);
}

/// The margin never eats more than half a short pane, or the two overlap
/// and the cursor is pinned to the middle instead of walking.
#[test]
fn a_short_pane_shrinks_the_margin_rather_than_seizing_up() {
    // height 5 -> pad 2, so a block at row 10 sits two rows down.
    assert_eq!(bring_into_view((10, 10), 0, 5), 8);
    // height 1 -> pad 0, and the row simply becomes the pane.
    assert_eq!(bring_into_view((10, 10), 0, 1), 10);
}

/// The highlight is a row, and it must be a *drawn* row at every offset:
/// a commit-granular marker could only sit on a `commit <hash>` row, so it
/// spent whole blocks off screen and the graph looked like it had no
/// cursor at all.
#[test]
fn the_graph_highlight_is_a_drawn_row_at_every_offset() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (w, h) = (120u16, 40u16);
    let mut repo = busy_repo();
    let hashes: Vec<String> = (0..12).map(|i| format!("a1b2c{i:02x}")).collect();
    repo.graph_expanded = hashes
        .iter()
        .flat_map(|hash| {
            let mut block = vec![
                format!("* commit {hash}"),
                "| Author: Gaurgle <a@b.c>".to_string(),
                "| Date:   2026-08-24T16:52:11+02:00".to_string(),
                "|".to_string(),
            ];
            // A tall body, which is the case that broke: the commit row
            // leaves the top of the pane and does not come back.
            block.extend((0..16).map(|n| format!("|     body line {n}")));
            block.push("|".to_string());
            block
        })
        .collect();
    repo.graph = hashes
        .iter()
        .map(|hash| crate::model::GraphLine {
            hash: hash.clone(),
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: "m".to_string(),
            lanes: vec![],
            is_merge: false,
        })
        .collect();

    let mut app = crate::app::App::new(vec![repo], "h".to_string(), w, h);
    app.view = crate::app::View::Detail;
    app.history_view = crate::app::HistoryView::Graph;
    app.focused_panel = crate::app::FocusedPanel::Detail;

    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    assert!(app.detail_max_scroll.get() > 0, "nothing to scroll");
    let pane = crate::ui::panel_rects(term.get_frame().area(), &app)
        .detail
        .expect("the graph view draws a detail pane");

    let mut rows_seen = std::collections::BTreeSet::new();
    for _ in 0..app.detail_max_scroll.get() {
        app.scroll_detail(1);
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        let buf = term.backend().buffer().clone();
        // Only inside the detail pane: the repo list draws a caret too.
        let marked: Vec<u16> = (pane.y..pane.y + pane.height)
            .filter(|y| (pane.x..pane.x + pane.width).any(|x| buf[(x, *y)].symbol() == "\u{25b8}"))
            .collect();
        assert_eq!(
            marked.len(),
            1,
            "expected one highlighted row at offset {}, got {marked:?}",
            app.detail_scroll()
        );
        rows_seen.insert(marked[0]);
    }
    assert!(
        rows_seen.len() > 10,
        "the highlight only ever sat on {} rows, so it is not walking",
        rows_seen.len()
    );
}

/// The wrap breaks at spaces and keeps every span's styling, so a wrapped
/// paragraph is not one span per character and no word is cut in half.
#[test]
fn wrapping_a_line_keeps_its_words_and_its_styling() {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let plain = Style::default().fg(colors::TEXT);
    let line = Line::from(vec![
        Span::styled("hash1234 ".to_string(), bold),
        Span::styled("the quick brown fox jumps".to_string(), plain),
    ]);

    let rows = wrap_line(&line, 14);
    assert!(rows.len() > 1, "nothing wrapped");
    for row in &rows {
        let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.chars().count() <= 14,
            "row ran past the pane: {text:?}"
        );
        assert!(
            row.spans.len() <= 2,
            "one span per character: {:?}",
            row.spans.len()
        );
    }
    let rejoined: String = rows
        .iter()
        .flat_map(|r| r.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(rejoined, "hash1234 the quick brown fox jumps");
    assert_eq!(rows[0].spans[0].style, bold, "the hash lost its weight");
}

/// A word wider than the pane still renders rather than looping, and its
/// characters are not lost.
#[test]
fn wrapping_a_line_survives_a_word_wider_than_the_pane() {
    let line = Line::from(Span::raw("a".repeat(30)));
    let rows = wrap_line(&line, 8);
    assert_eq!(rows.len(), 4);
    let rejoined: String = rows
        .iter()
        .flat_map(|r| r.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(rejoined.chars().count(), 30);
}

/// The inspector's scroll must reach the last row of a long body.
///
/// It clamped against `wrapped_row_count`, which counts `ceil(len/width)`
/// while `Paragraph` broke at spaces: every wrapped paragraph reported
/// fewer rows than it drew, so the tail of a commit message was
/// unreachable. The lines are wrapped in the builder now, so the count is
/// the count.
#[test]
fn the_inspector_scrolls_to_the_end_of_a_long_body() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (w, h) = (120u16, 30u16);
    let mut repo = list_repo(3);
    // One long paragraph of long words. Broken at spaces every word takes a
    // row of its own and most of that row's width goes unused, so a
    // `ceil(len / width)` count reports far fewer rows than are drawn --
    // and it is that shortfall that stranded the tail.
    let mut body = "z".repeat(50);
    for _ in 0..40 {
        body.push(' ');
        body.push_str(&"z".repeat(50));
    }
    body.push_str(" TAILMARKER");
    repo.commits[0].body = body;

    let mut app = crate::app::App::new(vec![repo], "h".to_string(), w, h);
    app.view = crate::app::View::Detail;
    app.history_view = crate::app::HistoryView::Commits;

    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    let max = app.inspector_max_scroll.get();
    assert!(max > 0, "the body did not overflow the inspector");

    app.scroll_inspector(max as i32);
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    let buf = term.backend().buffer().clone();
    let screen: String = (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect();
    assert!(
        screen.contains("TAILMARKER"),
        "the end of the body was past the end of the scroll"
    );
}

fn inspector_screen_at(w: u16, h: u16, inspector_pct: u16) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut repo = list_repo(3);
    repo.commits[0].body = "why it changed".to_string();
    repo.commits[0].files = vec![crate::model::FileStat {
        path: "src/lib.rs".to_string(),
        added: 3,
        removed: 1,
    }];
    let mut app = crate::app::App::new(vec![repo], "h".to_string(), w, h);
    app.view = crate::app::View::Detail;
    app.history_view = crate::app::HistoryView::Commits;
    app.contributions_folded = true;
    app.inspector_pct = inspector_pct;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

fn row_of(screen: &[String], needle: &str) -> usize {
    screen
        .iter()
        .position(|r| r.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{}", screen.join("\n")))
}

/// A wide inspector puts the changed files in a lane of their own to the
/// right of the message, so neither has to wait for the other to scroll
/// past. The files header lands level with the masthead: the body sits
/// five rows under the hash (hash, author, blank, subject, blank), so it
/// is the one line only the inspector draws that fixes where that is.
#[test]
fn a_wide_inspector_lays_the_files_beside_the_message() {
    let screen = inspector_screen_at(160, 40, 30);
    let files_row = row_of(&screen, "changed files");
    assert_eq!(
        files_row,
        row_of(&screen, "why it changed") - 5,
        "files lane should start level with the masthead"
    );
    assert!(
        screen[files_row].find("changed files").unwrap() > 80,
        "files on the right half: {:?}",
        screen[files_row]
    );
}

/// Below the width threshold the lanes stack as one column, as before.
#[test]
fn a_narrow_inspector_stacks_the_files_under_the_message() {
    // Draws at the pre-FZ-88 inspector size (40%) it was written for.
    let screen = inspector_screen_at(90, 40, 40);
    assert!(
        row_of(&screen, "changed files") > row_of(&screen, "why it changed"),
        "files should follow the body"
    );
}

/// Stepping the cursor with `j` must keep it on the pane in every view.
///
/// `graph_list_lines` reported the selected row in its own coordinates
/// while the reflog reported the pane's, so the follow step scrolled to a
/// row `base` above the selection and the highlight sat that far below the
/// bottom edge -- and a commit whose index fell inside the pinned header
/// had its block nulled out by the header subtraction, so the pane did not
/// follow at all. Only the lists drifted, which is why the reflog looked
/// fine.
#[test]
fn stepping_the_cursor_never_pushes_it_off_the_pane() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (w, h) = (120u16, 30u16);
    let repo = list_repo(40);

    for view in crate::app::HistoryView::ALL {
        let mut app = crate::app::App::new(vec![repo.clone()], "h".to_string(), w, h);
        app.view = crate::app::View::Detail;
        app.history_view = view;
        app.focused_panel = crate::app::FocusedPanel::Detail;
        app.contributions_folded = true;

        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        let pane = crate::ui::panel_rects(term.get_frame().area(), &app)
            .detail
            .expect("a detail pane");
        let caret_row = |term: &Terminal<TestBackend>| {
            let buf = term.backend().buffer().clone();
            (pane.y..pane.y + pane.height).find(|y| {
                (pane.x..pane.x + pane.width).any(|x| buf[(x, *y)].symbol() == "\u{25b8}")
            })
        };

        for step in 0..app.selected_commit_count() {
            app.detail_nav_down();
            term.draw(|f| crate::ui::render(f, &app)).unwrap();
            assert!(
                caret_row(&term).is_some(),
                "{} pushed its cursor off the pane stepping down to {step}",
                view.label()
            );
        }
        for step in 0..app.selected_commit_count() {
            app.detail_nav_up();
            term.draw(|f| crate::ui::render(f, &app)).unwrap();
            assert!(
                caret_row(&term).is_some(),
                "{} pushed its cursor off the pane stepping up to {step}",
                view.label()
            );
        }
    }
}

/// The other views step commit to commit, so scrolling moves the view out
/// from under their cursor rather than moving the cursor. It used to leave
/// the highlight off the pane entirely, and `j` then stepped a commit you
/// could not see. Asserted end to end, at the pane, in every view that
/// draws a list.
/// A repo whose every history view has a list long enough to scroll.
pub(super) fn list_repo(n: usize) -> RepoStatus {
    let mut repo = busy_repo();
    repo.commits = (0..n)
        .map(|i| crate::model::Commit {
            hash: format!("c0ffee{i:02}"),
            author: "Gaurgle".to_string(),
            date: Some(chrono::Utc::now()),
            message: format!("feat: thing {i}"),
            body: String::new(),
            insertions: 1,
            deletions: 0,
            files: vec![],
        })
        .collect();
    repo.graph = repo
        .commits
        .iter()
        .map(|c| crate::model::GraphLine {
            hash: c.hash.clone(),
            refs: vec![],
            author: c.author.clone(),
            message: c.message.clone(),
            lanes: vec![],
            is_merge: false,
        })
        .collect();
    repo.reflog = repo
        .commits
        .iter()
        .map(|c| crate::model::ReflogEntry {
            hash: c.hash.clone(),
            action: "commit".to_string(),
            description: c.message.clone(),
        })
        .collect();
    repo
}

/// Scrolling leaves the selection where it is, so the marker is free to
/// travel off the pane with the content it belongs to -- that is the
/// natural behaviour and the whole point of the change. What has to hold
/// is that a single step brings it back, at the edge being looked at.
///
/// The old contract was the opposite: every wheel notch dragged the
/// selection to the nearest visible entry, so the wheel silently
/// reassigned which commit the inspector was reading out.
#[test]
fn a_step_after_scrolling_brings_the_selection_back_onto_the_pane() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (w, h) = (120u16, 30u16);
    let repo = list_repo(40);

    // `info` is excluded: its commit list is one section among several, and
    // scrolling past it into the branches and stashes below is the point.
    for view in [
        crate::app::HistoryView::Commits,
        crate::app::HistoryView::Reflog,
    ] {
        let mut app = crate::app::App::new(vec![repo.clone()], "h".to_string(), w, h);
        app.view = crate::app::View::Detail;
        app.history_view = view;
        app.focused_panel = crate::app::FocusedPanel::Detail;

        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        assert!(
            app.detail_max_scroll.get() > 0,
            "{} has nothing to scroll",
            view.label()
        );
        let pane = crate::ui::panel_rects(term.get_frame().area(), &app)
            .detail
            .expect("a detail pane");
        let marker_on_pane = |term: &Terminal<TestBackend>| {
            let buf = term.backend().buffer().clone();
            (pane.y..pane.y + pane.height)
                .any(|y| (pane.x..pane.x + pane.width).any(|x| buf[(x, y)].symbol() == "\u{25b8}"))
        };

        let before = app.detail_selected();
        for _ in 0..app.detail_max_scroll.get() {
            app.scroll_detail(1);
        }
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        assert_eq!(
            app.detail_selected(),
            before,
            "{} let the wheel move its selection",
            view.label()
        );

        app.detail_nav_down();
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        assert!(
            marker_on_pane(&term),
            "{}: one step did not bring the selection back onto the pane",
            view.label()
        );
        assert_ne!(
            app.detail_selected(),
            before,
            "{}: the step did not re-enter at all",
            view.label()
        );
    }
}

/// The end-to-end version of the marker drag: render the graph view, then
/// scroll past the second block's first row and check the selection
/// followed. The app-side tests hand-feed the block tops; this one proves
/// the renderer actually reports them, which is the half that cannot be
/// faked.
#[test]
fn rendering_the_graph_view_reports_where_each_block_starts() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut repo = busy_repo();
    repo.graph_expanded = (0..20)
        .flat_map(|i| {
            [
                format!("* commit {i:07}"),
                "| Author: someone <a@b.c>".to_string(),
                "|".to_string(),
                format!("|     subject {i}"),
                "|".to_string(),
            ]
        })
        .collect();

    let mut app = crate::app::App::new(vec![repo], "h".to_string(), 100, 40);
    app.view = crate::app::View::Detail;
    app.history_view = crate::app::HistoryView::Graph;
    let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();
    term.draw(|f| crate::ui::render(f, &app)).unwrap();

    let tops = app.detail_row_tops.borrow().clone();
    assert_eq!(tops.len(), 20, "one top per commit block");
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "tops must ascend");

    // Walk the cursor onto the second block's first row.
    app.scroll_detail(tops[1] as i32);
    assert_eq!(app.graph_cursor, tops[1]);
    assert_eq!(
        app.detail_selected(),
        1,
        "the selection did not follow the cursor"
    );
}

/// Scrolling the graph view must not panic at any pane size or offset.
/// The wrap and the bring-into-view arithmetic both do width and height
/// subtraction, and a deep lane prefix in a narrow pane is where those
/// run out of room.
#[test]
fn the_graph_view_survives_every_scroll_offset() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut repo = busy_repo();
    repo.graph_expanded = (0..40)
        .map(|i| format!("| | | | * commit {i:030} a very long trailing subject line indeed"))
        .collect();

    for (w, h) in [(40u16, 10u16), (80, 30), (24, 6), (200, 50), (16, 5)] {
        for view in crate::app::HistoryView::ALL {
            for focus in [
                crate::app::FocusedPanel::Detail,
                crate::app::FocusedPanel::Inspector,
            ] {
                let mut app = crate::app::App::new(vec![repo.clone()], "h".to_string(), w, h);
                app.view = crate::app::View::Detail;
                app.history_view = view;
                app.focused_panel = focus;
                for off in [0i32, 1, 5, 50, 500, 5000, -5000] {
                    app.scroll_detail(off);
                    app.detail_nav_down();
                    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                    term.draw(|f| crate::ui::render(f, &app)).unwrap();
                }
            }
        }
    }
}

/// FZ-56: the repo list drawn for a ranked owner and author search, as
/// screen rows, with the app that drew it.
pub(super) fn draw_people_search(query: &str) -> (crate::app::App, Vec<String>) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut tools = repo("tools-kestrel", RepoKind::Public, Some("Gaurgle"));
    tools.commits = vec![crate::model::Commit {
        hash: "a1".to_string(),
        message: "subject".to_string(),
        body: String::new(),
        author: "Andreas Roos".to_string(),
        date: None,
        insertions: 0,
        deletions: 0,
        files: vec![],
    }];
    let web = repo("tools-web", RepoKind::Org, Some("kestrel-labs"));
    let mut scratch = repo("tools-scratch", RepoKind::Unknown, None);
    scratch.location = RepoLocation::LocalOnly;
    scratch.commits = tools.commits.clone();
    scratch.commits[0].author = "Kestrel Hansen".to_string();

    let mut app = crate::app::App::new(vec![scratch, web, tools], "t".to_string(), 120, 40);
    app.search_query = query.to_string();
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| render_repo_list(f, f.area(), &app)).unwrap();
    let buf = term.backend().buffer().clone();
    let screen = (0..40u16)
        .map(|y| (0..120u16).map(|x| buf[(x, y)].symbol()).collect())
        .collect();
    (app, screen)
}

/// The ranked order drawn for an owner query is the order the cursor
/// walks, with the name hit first, then the organisation, then the
/// contributor: one list, so a click lands on the row it names.
#[test]
fn an_owner_search_draws_the_order_the_cursor_walks() {
    let (app, screen) = draw_people_search("kestrel");
    let names = ["tools-kestrel", "tools-web", "tools-scratch"];
    let drawn: Vec<&str> = screen
        .iter()
        .filter_map(|row| names.into_iter().find(|n| row.contains(n)))
        .collect();
    let walked: Vec<&str> = app
        .visible_repos()
        .iter()
        .map(|(_, r)| r.name.as_str())
        .collect();
    assert_eq!(drawn, names, "drew: {screen:#?}");
    assert_eq!(walked, drawn, "cursor order disagrees with drawn order");
}

// --- FZ-09d: GitHub values from an earlier refresh (O4) ---

/// Two `Pending` rows with two PRs each, `stale` and `fresh`, under
/// watch, with `stale` not in the last poll.
pub(super) fn app_with_a_stale_row() -> crate::app::App {
    let mut repos = vec![
        repo("stale", RepoKind::Public, Some("alice")),
        repo("fresh", RepoKind::Public, Some("alice")),
    ];
    for r in &mut repos {
        r.ci_status = CiStatus::Pending;
        r.pr_count = 2;
    }
    let mut app = crate::app::App::new(repos, "t".to_string(), 120, 40);
    app.watch_mode = true;
    app.not_in_last_poll
        .insert((Some("alice".to_string()), "stale".to_string()));
    app
}

// --- FZ-66: the cursor's row in pane 1 wears a band ---

/// Two repos with two PRs each, so every row carries a badge group.
pub(super) fn two_repo_app() -> crate::app::App {
    let mut repos = vec![
        repo("alpha", RepoKind::Public, Some("alice")),
        repo("beta", RepoKind::Public, Some("alice")),
    ];
    for r in &mut repos {
        r.pr_count = 2;
    }
    crate::app::App::new(repos, "t".to_string(), 120, 40)
}

#[test]
fn the_band_and_bold_dots_move_no_column() {
    use ratatui::buffer::Buffer;
    use ratatui::widgets::Widget;

    let app = two_repo_app();
    let r = &app.repos[0];
    let width = 40u16;
    let drawn = |selected: bool| {
        let line = Line::from(repo_row_spans(
            r,
            selected,
            width as usize,
            false,
            false,
            None,
        ));
        let cells = line.width();
        let area = Rect::new(0, 0, width + 10, 1);
        let mut buf = Buffer::empty(area);
        line.render(area, &mut buf);
        let symbols: Vec<String> = (0..area.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        (cells, symbols)
    };
    let (on_w, on) = drawn(true);
    let (off_w, off) = drawn(false);
    assert_eq!(on_w, width as usize, "the cursor's row fills its width");
    assert_eq!(on_w, off_w, "same number of cells");
    let badge_at = |s: &[String]| s.iter().position(|c| c == "P").expect("PR badge");
    assert_eq!(badge_at(&on), badge_at(&off), "badges in the same column");
    assert_eq!(on[2..], off[2..], "only the marker differs");
}

// --- FZ-55: a scroll margin on every list, reveal on expand ---

/// Pane 1 drawn `height` rows tall: the `visible_repos` index of each repo
/// row on screen, top to bottom, and the index of the selected one.
fn drawn_repo_window(app: &crate::app::App, height: u16) -> (Vec<usize>, usize) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let names: Vec<String> = app
        .visible_repos()
        .iter()
        .map(|(_, r)| format!(" {} ", r.name))
        .collect();
    let mut term = Terminal::new(TestBackend::new(60, height)).unwrap();
    term.draw(|f| render_repo_list(f, f.area(), app)).unwrap();
    let buf = term.backend().buffer().clone();
    let drawn = (0..height)
        .filter_map(|y| {
            let row: String = (0..60u16).map(|x| buf[(x, y)].symbol()).collect();
            names.iter().position(|n| row.contains(n.as_str()))
        })
        .collect();
    let selected = format!(" {} ", app.selected_repo().expect("a selection").name);
    let selected = names.iter().position(|n| *n == selected).expect("listed");
    (drawn, selected)
}

/// Walks pane 1 down to the last repo and back, checking the drawn
/// window after every step: `margin` rows stay past the cursor on the
/// side it heads for, fewer only where the list itself runs out.
fn walk_repo_list(height: u16, margin: usize) {
    let repos = (0..40)
        .map(|i| repo(&format!("list-{i:02}"), RepoKind::Public, None))
        .collect();
    let mut app = crate::app::App::new(repos, "t".to_string(), 60, height);
    let last = app.visible_repos().len() - 1;
    let mut parked = false;
    let check = |app: &crate::app::App, down: bool, parked: &mut bool| {
        let (drawn, selected) = drawn_repo_window(app, height);
        let pos = drawn
            .iter()
            .position(|&i| i == selected)
            .expect("on screen");
        let (above, below) = (pos, drawn.len() - 1 - pos);
        if down {
            assert!(
                below >= margin.min(last - selected),
                "cursor {selected} has {below} rows below it: {drawn:?}"
            );
            *parked |= drawn[0] > 0 && below == margin;
        } else {
            assert!(
                above >= margin.min(selected),
                "cursor {selected} has {above} rows above it: {drawn:?}"
            );
        }
    };
    for _ in 0..last {
        app.move_down();
        check(&app, true, &mut parked);
    }
    assert!(parked, "the list never scrolled with the margin held");
    for _ in 0..last {
        app.move_up();
        check(&app, false, &mut parked);
    }
}

/// Twenty rows less the border and the filter strip: sixteen list rows,
/// room for the full margin pane 2 keeps.
#[test]
fn the_repo_list_keeps_the_shared_margin_past_the_cursor() {
    assert_eq!(crate::app::scrolloff_for(16), crate::app::SCROLLOFF);
    walk_repo_list(20, usize::from(crate::app::SCROLLOFF));
}

/// Eight list rows cannot hold the full margin on each side of the
/// cursor, so it shrinks to (8 - 1) / 2 = 3 rather than pinning it.
#[test]
fn a_short_repo_list_shrinks_the_margin() {
    assert_eq!(crate::app::scrolloff_for(8), 3);
    walk_repo_list(12, 3);
}

// --- The `notes` view (FZ-53) ---

/// An app on `busy_repo` showing `notes`, with a walk of `files` landed.
pub(super) fn notes_view_app(files: &[(&str, Option<&str>)]) -> crate::app::App {
    let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 100, 44);
    app.view = crate::app::View::Detail;
    app.history_view = HistoryView::Notes;
    app.inspector_folded = false;
    let key = app.repos[0].path.clone().expect("cloned");
    let notes = crate::notes::Notes {
        files: files
            .iter()
            .map(|(rel, notez)| crate::notes::NoteFile {
                rel: (*rel).into(),
                notez: notez.map(Into::into),
            })
            .collect(),
    };
    let walk = crate::app::NotesWalk {
        root: key.clone().into(),
        key,
        generation: 0,
    };
    app.apply_notes_walk(walk, Ok(notes));
    app
}

/// Pane 2 in the `notes` layout, 120 columns wide: the tree takes its
/// share beside the preview (FZ-67), and at this width that share holds
/// its longest message, `notez: no notez/ or .notez/ directory`, on one
/// row.
pub(super) fn notes_pane_text(app: &crate::app::App) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..30u16)
        .map(|y| {
            (0..120u16)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn changed(
    status: &str,
    path: &str,
    churn: Option<(u32, u32)>,
) -> crate::git::ChangedFile {
    crate::git::ChangedFile {
        status: status.to_string(),
        path: path.to_string(),
        from: None,
        churn,
    }
}

/// The design's example (FZ-60): three uncommitted kinds, two files
/// unpushed.
pub(super) fn held_changes() -> crate::git::Changes {
    crate::git::Changes {
        uncommitted: vec![
            changed(" M", "src/app.rs", Some((12, 3))),
            changed("A ", "docs/foo.md", Some((40, 0))),
            changed("??", "scratch/", None),
        ],
        outgoing: crate::git::Outgoing::Unpushed {
            upstream: "origin/main".to_string(),
            ahead: 2,
            files: vec![
                changed("M", "src/ui/detail.rs", Some((88, 12))),
                changed("D", "src/old.rs", Some((0, 5))),
            ],
        },
    }
}

/// The `changes` view on the busy repo with `changes` held for it.
pub(super) fn changes_view_app(changes: crate::git::Changes) -> crate::app::App {
    let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 100, 44);
    app.view = crate::app::View::Detail;
    app.history_view = HistoryView::Changes;
    app.inspector_folded = false;
    let key = app.repos[0].path.clone().expect("cloned");
    let read = crate::app::ChangesRead {
        root: key.clone().into(),
        key,
        generation: 0,
        base: app.changes_base,
    };
    app.apply_changes_read(read, Ok(changes));
    app
}

// --- The `notes` reader and facts (FZ-62, FZ-68), the mode line (FZ-57) ---

/// Pane 2 of the `notes` layout, `w` by `h`, as `ui::render` draws it:
/// the frame, the tree's column and the reader at `notes_pane_body`'s
/// rect (FZ-68).
pub(super) fn notes_pane_buffer(w: u16, h: u16, app: &crate::app::App) -> ratatui::buffer::Buffer {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| {
        render_detail_pane(f, f.area(), app);
        let notes = notes_pane_body(f.area(), app).expect("a tree");
        render_reader(f, notes.reader, app);
    })
    .unwrap();
    term.backend().buffer().clone()
}

/// Draw `paint` into a `w` by `h` buffer and return its rows.
pub(super) fn drawn_rows(w: u16, h: u16, paint: impl FnOnce(&mut Frame)) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(paint).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// FZ-63: a `TODO.md` draws in the reader as a todoz board, sections,
/// states, subtasks, tag dots and due dates, and the facts row counts
/// its unchecked and half tasks (FZ-68 moved the count there from the
/// reader's border).
#[test]
fn the_reader_draws_a_board_and_counts_what_is_open() {
    let mut app = notes_view_app(&[("TODO.md", None)]);
    let read = app.claim_note_read().expect("a read");
    let draw = |app: &crate::app::App| drawn_rows(60, 12, |f| render_reader(f, f.area(), app));
    let facts = |app: &crate::app::App| {
        notes_pane_text(app)
            .lines()
            .find(|l| l.contains("─ TODO.md · todo"))
            .map(str::to_string)
            .expect("a facts row")
    };
    assert!(draw(&app)[0].contains("reading…"), "before the read");
    assert!(!facts(&app).contains("open"), "before the read");

    app.apply_note_read(read, Ok(crate::ui::todo::FIXTURE.to_string()));
    let facts = facts(&app);
    assert!(facts.contains("─ TODO.md · todo · 4 open ·"), "{facts}");
    // A `TODO.md` is markdown, so a render lands for it too; the board
    // still draws over it.
    draw(&app);
    let render = app.claim_note_render().expect("a render of the board");
    let lines = crate::ui::markdown::render_markdown(&render.text, render.width);
    let rendered = crate::app::RenderedNote {
        width: render.width,
        lines,
    };
    app.apply_note_rendered(&render.path, render.generation, rendered);
    let body: Vec<String> = draw(&app)[..10]
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect();
    assert_eq!(
        body,
        [
            "       ▼ Board",
            "Loose prose line.",
            "       ▼ Now",
            " ·●··· ├─▾ [ ] ship the reader @2026-11-01",
            " ●···· │ ├─▾ [/] parse it",
            " ···●· │ │ └─  [x] split lines @2026-10-30",
            " ··●·● │ └─  [ ] draw it",
            " ····· └─  [x] done thing",
            "       ▼ Later",
            " ····· └─  [/] half way",
        ]
    );
}

// --- Rendered markdown in the reader (FZ-59) ---

/// Draw the reader into a 120 by 12 pane 2, its titles on the pane's
/// border (FZ-68).
pub(super) fn reader_buffer(app: &crate::app::App) -> ratatui::buffer::Buffer {
    notes_pane_buffer(120, 12, app)
}

/// Land `text` as the reader's body the way the event loop does: the
/// read claimed at the width the reader last drew, rendered with it.
pub(super) fn land_rendered(app: &mut crate::app::App, text: &str) {
    reader_buffer(app);
    let read = app.claim_note_read().expect("a read");
    let width = read.render_width.expect("a markdown read renders");
    let lines = crate::ui::markdown::render_markdown(text, width);
    let rendered = crate::app::RenderedNote { width, lines };
    app.apply_note_rendered(&read.path, read.generation, rendered);
    app.apply_note_read(read, Ok(text.to_string()));
}

/// FZ-59: the scroll bound counts rendered rows, not source lines: 40
/// source lines that render as one paragraph of a few rows do not
/// scroll, and raw they scroll by the source's 40 rows.
#[test]
fn the_rendered_reader_scrolls_by_rendered_rows() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let body: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
    let body = body.join("\n");
    land_rendered(&mut app, &body);
    let rows = crate::ui::markdown::render_markdown(&body, app.reader_width.get()).len();
    reader_buffer(&app);
    let height = app.reader_view_height.get();
    let want = u16::try_from(rows).unwrap().saturating_sub(height);
    assert_eq!(app.reader_max_scroll.get(), want, "{rows} rendered rows");
    assert!(rows < 40, "one paragraph, {rows} rows");

    app.toggle_reader_raw();
    reader_buffer(&app);
    assert_eq!(app.reader_max_scroll.get(), 40 - height, "raw rows");
}

/// The `notes` layout as painted and as hit-tested agree: a click inside
/// the preview and on a tree row lands on each, each pane's border is
/// drawn where its rectangle says, pane 2 runs the full height beside
/// pane 1, which runs it too with no card under it (FZ-68 round 2), and
/// the preview sits inside pane 2 under the rule.
#[test]
fn notes_clicks_land_on_the_tree_and_the_reader() {
    let (w, h) = (140u16, 44u16);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    let read = app.claim_note_read().expect("a read");
    app.apply_note_read(read, Ok("hello".to_string()));
    let area = Rect::new(0, 0, w, h);
    let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
    let rects = crate::ui::panel_rects(area, &app);
    let pane2 = rects.detail.expect("pane 2");
    let reader = rects.reader.expect("reader");
    let notes = notes_pane_body(pane2, &app).expect("a tree");

    assert_eq!(rects.inspector, None, "no card in the notes layout");
    assert_eq!(pane2.y, rects.repo_list.y, "pane 2 starts at the top");
    assert_eq!(
        pane2.bottom(),
        rects.repo_list.bottom(),
        "and runs the full height, as pane 1 does"
    );
    // Inside pane 2's border, under its title, readout and pinned
    // header, and in its right part, which leaves the tree the left.
    assert!(
        reader.x > pane2.x + 1 && reader.right() < pane2.right(),
        "the reader is inside pane 2's width: {reader:?} in {pane2:?}"
    );
    // Under the rule, down to the bottom border.
    let rule = notes.rule.expect("pinned at 44 rows");
    assert!(
        reader.y == rule + 1 && reader.bottom() == pane2.bottom() - 1,
        "the reader runs from the rule to the bottom: {reader:?} in {pane2:?}"
    );
    assert!(
        reader.x > pane2.x + pane2.width / 4,
        "the tree keeps the left of pane 2"
    );

    let cell = |x: u16, y: u16| rows[y as usize].chars().nth(x as usize).unwrap();
    // The preview's divider is drawn where its rect says, two columns
    // left of its text, under the top border, and its headliner word
    // right of it above the rule (FZ-68 round 2).
    let divider = reader.x - 2;
    assert_eq!(cell(divider, pane2.y + 1), '│', "Reader divider top");
    assert_eq!(cell(divider, pane2.bottom() - 1), '┴', "Reader divider");
    let over_reader: String = rows[rule as usize - 1]
        .chars()
        .skip(divider as usize)
        .take((pane2.right() - divider) as usize)
        .collect();
    assert!(
        over_reader.contains("notes preview"),
        "Reader title: {over_reader}"
    );
    assert_eq!(
        rects.at(reader.x, reader.y),
        Some(FocusedPanel::Reader),
        "click corner Reader"
    );
    for (rect, panel, title) in [
        (pane2, FocusedPanel::Detail, "[2]"),
        (rects.repo_list, FocusedPanel::RepoList, "[1]"),
    ] {
        assert_eq!(cell(rect.x, rect.y), '╭', "{panel:?} corner");
        assert_eq!(cell(rect.right() - 1, rect.bottom() - 1), '╯', "{panel:?}");
        let top: String = rows[rect.y as usize]
            .chars()
            .skip(rect.x as usize)
            .take(rect.width as usize)
            .collect();
        assert!(top.contains(title), "{panel:?} title: {top}");
        assert_eq!(
            rects.at(rect.x, rect.y),
            Some(panel),
            "click corner {panel:?}"
        );
    }
    let (mx, my) = (reader.x + reader.width / 2, reader.y + reader.height / 2);
    assert_eq!(rects.at(mx, my), Some(FocusedPanel::Reader), "click mid");
    // Pane 2's middle is the preview's now; the tree's file row is
    // pane 2's own.
    let (fy, fx) = rows
        .iter()
        .enumerate()
        .find_map(|(y, r)| {
            let x = r
                .chars()
                .take(reader.x as usize)
                .collect::<String>()
                .find("a.md")?;
            Some((y as u16, r[..x].chars().count() as u16))
        })
        .expect("the tree draws a.md left of the preview");
    assert!(pane2.contains(ratatui::layout::Position { x: fx, y: fy }));
    assert_eq!(
        rects.at(fx, fy),
        Some(FocusedPanel::Detail),
        "click a tree row"
    );
    assert!(
        rows.iter().any(|r| r.contains("hello")),
        "the body is drawn"
    );
    // No card, so no horizontal divider to grab on either column, and
    // pane 2's own divider is not draggable.
    for x in [rects.repo_list.x + 2, reader.x + 2, divider] {
        for y in [reader.y, rule, pane2.bottom() - 1] {
            assert_eq!(rects.divider_at(x, y), None, "({x}, {y})");
        }
    }
}

/// FZ-68 review: a pane 2 squeezed to a few columns (a tiny terminal, a
/// tmux pane mid-resize) puts the divider off the buffer; drawing the
/// `notes` frame there must skip it, not panic.
#[test]
fn a_tiny_notes_frame_draws_without_panicking() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    for w in [5u16, 1] {
        drawn_rows(w, 40, |f| crate::ui::render(f, &app));
    }
    for w in 1..=6u16 {
        drawn_rows(w, 12, |f| render_detail_pane(f, f.area(), &app));
    }
}

/// FZ-67: in the `notes` layout `detail` is all of pane 2 and `reader`
/// the preview inside it, so `at` answers the preview inside it and
/// pane 2 on a tree row. A section's digest and an empty fleet draw no
/// preview, and `panel_rects` says so.
#[test]
fn panel_rects_nest_the_preview_inside_pane_2() {
    let area = Rect::new(0, 0, 140, 44);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let rects = crate::ui::panel_rects(area, &app);
    let pane2 = rects.detail.expect("pane 2");
    let reader = rects.reader.expect("preview");
    assert_eq!(pane2.intersection(reader), reader, "reader inside detail");
    assert!(pane2.width > reader.width && pane2.height > reader.height);
    let (cx, cy) = (reader.x + reader.width / 2, reader.y + reader.height / 2);
    assert_eq!(rects.at(cx, cy), Some(FocusedPanel::Reader));
    assert_eq!(
        rects.at(reader.right() - 1, reader.bottom() - 1),
        Some(FocusedPanel::Reader)
    );
    // A tree row: left of the preview, on its rows.
    assert_eq!(
        rects.at(pane2.x + 3, reader.y + 2),
        Some(FocusedPanel::Detail)
    );
    // Pane 2's header, above the preview, is pane 2's too.
    assert_eq!(rects.at(cx, pane2.y + 1), Some(FocusedPanel::Detail));

    // A local-only repo's section always draws its header.
    app.repos[0].location = crate::model::RepoLocation::LocalOnly;
    let section = crate::digest::Section::of(&app.repos[0]);
    app.land_on(crate::app::ListRow::Header(section));
    assert!(app.digest(chrono::Local::now()).is_some(), "on the digest");
    let rects = crate::ui::panel_rects(area, &app);
    assert_eq!(rects.reader, None, "the digest draws no preview");
    let rows = drawn_rows(140, 44, |f| crate::ui::render(f, &app));
    assert!(!rows.join("\n").contains("notes preview"));

    let mut empty = crate::app::App::new(Vec::new(), "t".to_string(), 140, 44);
    empty.view = crate::app::View::Detail;
    empty.history_view = HistoryView::Notes;
    assert_eq!(crate::ui::panel_rects(area, &empty).reader, None);
    let rows = drawn_rows(140, 44, |f| crate::ui::render(f, &empty));
    assert!(!rows.join("\n").contains("notes preview"));
}

/// FZ-67: a click on the tree's second file selects it with the tree
/// inside pane 2's body: the hit map is built from the tree's rect, so
/// the row under the pointer is the file drawn there.
#[test]
fn a_click_on_the_second_file_selects_it_inside_pane_2() {
    let (w, h) = (140u16, 44u16);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez")), ("notez/b.md", Some("notez"))]);
    app.contributions_folded = true;
    let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
    let reader = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app)
        .reader
        .expect("preview");
    let (y, x) = rows
        .iter()
        .enumerate()
        .find_map(|(y, r)| {
            let left: String = r.chars().take(reader.x as usize).collect();
            let x = left.find("b.md")?;
            Some((y as u16, left[..x].chars().count() as u16))
        })
        .expect("b.md drawn in the tree");
    assert_eq!(app.detail_selected(), 0);
    let body = app.detail_hits.borrow().body;
    assert!(
        body.right() < reader.x,
        "the hit map is the tree's, left of the preview: {body:?}, {reader:?}"
    );
    let click = app.detail_hits.borrow().at(x, y).expect("a hit on b.md");
    app.click_detail(&click);
    assert_eq!(app.detail_selected(), 1, "the second file is selected");
    assert_eq!(
        app.notes_highlighted_file().map(|f| f.rel.clone()),
        Some("notez/b.md".into())
    );
}

/// FZ-67: on a pane too short to pin the header over the tree and the
/// preview, the header scrolls with the tree in the tree's column, laid
/// out at the tree's width: its rows do not wrap, so the mode line sits
/// right under them, and the preview keeps its column beside it, right
/// of the divider (FZ-68). Fails while the fallback keeps the header
/// laid out for the whole pane.
#[test]
fn a_short_notes_pane_scrolls_the_header_with_the_tree() {
    let (w, h) = (140u16, 18u16);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    let pane2 = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app)
        .detail
        .expect("pane 2");
    let notes = notes_pane_body(pane2, &app).expect("a tree");
    assert_eq!(notes.header, None, "too short to pin the header");
    let tree = notes.body;

    let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
    let reader = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app)
        .reader
        .expect("preview");
    let cell = |x: u16, y: u16| rows[y as usize].chars().nth(x as usize).unwrap();
    assert_eq!(
        cell(notes.divider, pane2.y + 1),
        '│',
        "the preview's divider"
    );
    assert_eq!(notes.rule, None, "no rule pinned to cross");
    assert_eq!(cell(notes.divider, reader.y), '│', "{}", rows.join("\n"));
    let left = |y: u16| -> String {
        rows[y as usize]
            .chars()
            .skip(tree.x as usize)
            .take((notes.divider - tree.x) as usize)
            .collect()
    };

    // The header's first row, the repo's state, opens the tree column.
    let repo = app.selected_repo().expect("a repo");
    let state = repo.long_status();
    assert!(left(tree.y).contains(&state), "{}", left(tree.y));
    // Laid out at the tree's width, one row per line: the mode line is
    // the row after the last header line.
    let header_rows = header_lines(&app, repo, tree.width as usize).len() as u16;
    let mode = left(tree.y + header_rows);
    assert!(mode.contains("notes: [notez]"), "{}", rows.join("\n"));

    let body = app.detail_hits.borrow().body;
    assert!(body.right() < reader.x, "{body:?}, {reader:?}");
}

/// The tree is wide enough for its mode line whole, `(m)` included, in
/// both modes on a whole rendered frame: at 140 columns the 30-column
/// floor left 27 text columns and drew `(m` (FZ-62 review F3).
#[test]
fn the_tree_holds_its_mode_line_whole_on_a_140_column_frame() {
    for w in [120u16, 140, 150] {
        let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
        for want in [
            "notes: [notez] markdown  (m)",
            "notes: notez [markdown]  (m)",
        ] {
            let rows = drawn_rows(w, 44, |f| crate::ui::render(f, &app));
            assert!(
                rows.iter().any(|r| r.contains(want)),
                "{w} columns: {want} cut:\n{}",
                rows.join("\n")
            );
            app.toggle_notes_markdown();
        }
    }
}

/// Off the `notes` view the usual layout holds: no reader, the
/// inspector across the full width.
#[test]
fn the_notes_layout_is_the_notes_views_alone() {
    let area = Rect::new(0, 0, 140, 44);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    assert!(crate::ui::panel_rects(area, &app).reader.is_some());
    app.history_view = HistoryView::Info;
    let rects = crate::ui::panel_rects(area, &app);
    assert_eq!(rects.reader, None);
    assert_eq!(rects.inspector.map(|r| r.width), Some(140));
}

/// FZ-68 round 2: the `NOTES` rule runs pane 2's whole inner width,
/// from the left padding to the right one, and crosses the divider with
/// `┼` in the divider's colour; its fade runs the whole width, so the
/// cell right of the crossing is still on its way to `SURFACE`. Fails
/// while the rule stops at the tree's column.
#[test]
fn the_notes_rule_runs_edge_to_edge_and_crosses_the_divider() {
    let (w, h) = (140u16, 44u16);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    app.focused_panel = FocusedPanel::Detail;
    let area = Rect::new(0, 0, w, h);
    let pane2 = crate::ui::panel_rects(area, &app).detail.expect("pane 2");
    let notes = notes_pane_body(pane2, &app).expect("a tree");
    let rule = notes.rule.expect("pinned at 44 rows");
    let inner = pane2_block(colors::SURFACE).inner(pane2);
    let buf = {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| crate::ui::render(f, &app)).unwrap();
        term.backend().buffer().clone()
    };
    let symbol = |x: u16| buf[(x, rule)].symbol().to_string();
    for x in inner.x..inner.right() {
        let want = match x == notes.divider {
            true => "┼",
            false => "─",
        };
        assert_eq!(symbol(x), want, "column {x} of the rule");
    }
    assert_eq!(symbol(pane2.x), "│", "the left border");
    assert_eq!(symbol(pane2.right() - 1), "│", "the right border");
    assert_eq!(buf[(notes.divider, rule)].fg, colors::LAVENDER);
    let on = colors::history_view_color(HistoryView::Notes);
    assert_eq!(
        buf[(inner.x, rule)].fg,
        crate::ui::blend(on, colors::SURFACE, 1, u32::from(inner.width)),
        "the fade starts at the pane's left"
    );
    let past = buf[(notes.divider + 1, rule)].fg;
    assert_ne!(past, colors::SURFACE, "still fading past the divider");
    assert_eq!(buf[(inner.right() - 1, rule)].fg, colors::SURFACE);
}

/// FZ-68 round 2: the `notes` layout docks no `[3] file` card, as
/// `graph` docks no inspector: pane 1 takes the card's rows and runs the
/// body's full height, whatever the inspector's size or fold, and
/// nothing on screen is titled `[3]`. Fails while the card sits under
/// pane 1.
#[test]
fn the_notes_layout_docks_no_card_and_pane_1_takes_its_rows() {
    let (w, h) = (140u16, 44u16);
    let area = Rect::new(0, 0, w, h);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    app.history_view = HistoryView::Info;
    let info = crate::ui::panel_rects(area, &app);
    let card = info.inspector.expect("info docks the inspector").height;
    app.history_view = HistoryView::Notes;
    for (pct, folded) in [(30, false), (60, false), (30, true)] {
        app.inspector_pct = pct;
        app.inspector_folded = folded;
        let rects = crate::ui::panel_rects(area, &app);
        assert_eq!(rects.inspector, None, "no card at {pct}% folded {folded}");
        assert_eq!(rects.repo_list.height, rects.body.height, "full height");
        let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
        assert!(!rows.join("\n").contains("[3]"), "{}", rows.join("\n"));
    }
    app.inspector_pct = 30;
    app.inspector_folded = false;
    let notes = crate::ui::panel_rects(area, &app);
    assert_eq!(
        notes.repo_list.height,
        info.repo_list.height + card,
        "pane 1 is taller by the card's rows"
    );
}
