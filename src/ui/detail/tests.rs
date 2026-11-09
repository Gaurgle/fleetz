use super::*;
use crate::graph::{COMMIT_NODE, MERGE_NODE};
use crate::model::{GitRef, RefKind, RepoKind, RepoLocation};

fn git_ref(kind: RefKind, name: &str, is_head: bool) -> GitRef {
    GitRef {
        kind,
        name: name.to_string(),
        is_head,
    }
}

fn span_text(spans: &[Span]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

/// `*` on every row is a heavy glyph that turns a long history into
/// noise. Ordinary commits get a light node; `*` is kept for merges,
/// which is the one row worth picking out of a column.
/// git draws every commit as `*` and its lanes as punctuation. Both are
/// translated: the lanes to box drawing, so the column reads as a drawn
/// line, and the node to a circle -- with merges keeping the asterisk,
/// circled, so they stay pickable out of the column.
#[test]
fn the_graph_is_drawn_with_box_characters_not_ascii() {
    // A merge and a collapse, so every shape the column can draw appears.
    let spec: [(&str, &[&str]); 4] = [
        ("aaaaaaa", &["bbbbbbb", "ccccccc"]),
        ("ccccccc", &["ddddddd"]),
        ("bbbbbbb", &["ddddddd"]),
        ("ddddddd", &[]),
    ];
    let parents: Vec<Vec<String>> = spec
        .iter()
        .map(|(_, p)| p.iter().map(|s| (*s).to_string()).collect())
        .collect();
    let commits: Vec<crate::graph::Commit<'_>> = spec
        .iter()
        .zip(&parents)
        .map(|((hash, _), parents)| crate::graph::Commit { hash, parents })
        .collect();

    let mut r = busy_repo();
    r.graph = crate::graph::layout(&commits)
        .into_iter()
        .zip(spec)
        .map(|(lanes, (hash, parents))| crate::model::GraphLine {
            lanes,
            hash: hash.to_string(),
            is_merge: parents.len() > 1,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: "subject".to_string(),
        })
        .collect();
    let mut selected_row = None;
    let text: String = graph_list_lines(&r, 96, 0, None, &mut selected_row)
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    for drawn in [COMMIT_NODE, MERGE_NODE, '│', '─', '╮', '╯'] {
        assert!(text.contains(drawn), "{drawn} missing from:\n{text}");
    }
    for ascii in ['*', '|', '\\', '_'] {
        assert!(!text.contains(ascii), "git's own {ascii} leaked through");
    }
}

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

/// A repo carrying something for every view, so a view showing the wrong
/// section is visible rather than merely absent.
/// Flatten a rendered row to its text, so a test can assert on what the
/// eye sees rather than on span structure.
fn row_text(row: &Line<'static>) -> String {
    row.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn branch_rows(b: &crate::model::BranchStatus) -> Vec<Line<'static>> {
    let mut shown = std::collections::HashSet::new();
    branch_row_lines(b, &[], &mut shown)
}

fn branch_fixture(
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

/// The failure this closes: `info` and `branches` would each grow their
/// own copy of this row, and two copies of a row vocabulary drift exactly
/// the way two copies of layout arithmetic do. One function, called by
/// both.
#[test]
fn a_branch_row_says_head_upstream_and_divergence_once() {
    let b = branch_fixture("feature/who-goes-first", Some("origin/wip"), 19, true);
    let text = row_text(&branch_rows(&b)[0]);
    assert!(text.starts_with("* "), "HEAD marker missing: {text:?}");
    assert!(text.contains("-> origin/wip"), "upstream missing: {text:?}");
    assert!(text.contains("19 behind"), "divergence missing: {text:?}");
}

/// The arrow is for the odd one out. Repeating `origin/x` after every `x`
/// down the whole list is noise that would bury the case worth seeing.
#[test]
fn a_branch_tracking_its_own_name_stays_bare() {
    let b = branch_fixture("main", Some("origin/main"), 0, false);
    let text = row_text(&branch_rows(&b)[0]);
    assert!(
        !text.contains("->"),
        "arrow drawn for a matching upstream: {text:?}"
    );
    assert!(text.contains('✓'), "level branch lost its tick: {text:?}");
}

/// An untracked branch rendering bare reads as "fine" when it means "not
/// on any remote" -- the same trap as a not-cloned repo whose counters are
/// all zero.
#[test]
fn a_branch_with_no_upstream_says_so() {
    let b = branch_fixture("beta", None, 0, false);
    let text = row_text(&branch_rows(&b)[0]);
    assert!(
        text.contains("no upstream"),
        "silence stood in for a state: {text:?}"
    );
}

fn busy_repo() -> RepoStatus {
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

/// `pane_text` for the `ci` view, with the recorded runs fixture landed
/// for the fixture repo.
fn ci_view_text(width: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), width, 44);
    app.history_view = HistoryView::Ci;
    let (key, owner, name) = crate::ci::target(&app.repos[0]).expect("a GitHub repo");
    let request = app.claim_ci_poll(key.clone(), owner, name);
    let runs = crate::ci::parse_runs(include_str!("../../ci/fixtures/runs.json")).expect("fixture");
    app.apply_ci(
        crate::ci::CiAnswer {
            key,
            attempt: request.attempt,
            runs: Some(crate::ci::Fetched::Fresh {
                data: runs,
                etag: None,
            }),
            jobs: None,
            log: None,
            full_log: None,
            failure: None,
            budget: None,
            tip: None,
        },
        chrono::Local::now(),
    );
    let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    (0..30u16)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn pane_text(view: HistoryView, width: u16) -> String {
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

/// The color the `2` of the detail pane's ` [2] ` title is drawn in.
fn detail_number_color(focused_panel: FocusedPanel) -> Option<ratatui::style::Color> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let width = 92;
    let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), width, 44);
    app.focused_panel = focused_panel;
    let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let row: String = (0..width).map(|x| buf[(x, 0)].symbol()).collect();
    let x = row.find("[2]").expect("pane number not drawn") as u16 + 1;
    buf[(x, 0)].style().fg
}

/// The detail pane builds its own title and drew its unfocused number in
/// SURFACE, a shade darker than every other pane's, which is exactly
/// when the number is the key you need to read.
#[test]
fn the_unfocused_detail_number_matches_the_other_panes() {
    let number_fg = |focused| {
        crate::ui::panel_title(2, "detail", focused).spans[0]
            .style
            .fg
    };
    assert_eq!(
        detail_number_color(FocusedPanel::RepoList),
        number_fg(false),
        "unfocused number differs from panel_title's"
    );
    assert_eq!(detail_number_color(FocusedPanel::Detail), number_fg(true));
}

/// The failure this closes: a remote-only branch's tip is not in the union
/// of local branches that `git::commits` walks, so the inspector had
/// nothing to draw. Reporting it as a *range* problem would send someone
/// looking for a limit to raise; it is a reachability problem.
#[test]
fn an_unreachable_branch_tip_says_it_is_not_on_a_local_branch() {
    let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
    app.history_view = HistoryView::Branches;
    app.repos[0].branches = vec![branch_fixture("gone", None, 0, false)];
    app.repos[0].branches[0].tip = "beefbee".to_string();
    app.set_detail_selected(0);
    assert_eq!(
        inspector_empty_text(&app),
        "beefbee is not on any local branch"
    );
}

/// The reflog keeps its own wording: there the commit really is outside a
/// range that could be widened.
#[test]
fn an_unloaded_reflog_commit_still_reports_a_range() {
    let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 92, 44);
    app.history_view = HistoryView::Reflog;
    app.repos[0].reflog = vec![crate::model::ReflogEntry {
        hash: "beefbee".to_string(),
        action: "commit".to_string(),
        description: "x".to_string(),
    }];
    app.set_detail_selected(0);
    assert_eq!(
        inspector_empty_text(&app),
        "beefbee is outside the loaded commit range"
    );
}

/// A repo with two local branches and three remotes, one of which nothing
/// local claims, rendered in the branches view.
fn branches_text(selected: usize) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut r = busy_repo();
    r.branches = vec![
        branch_fixture("main", Some("origin/main"), 0, true),
        branch_fixture("beta", None, 0, false),
    ];
    r.remote_branches = ["origin/main", "origin/beta", "origin/nobody"]
        .iter()
        .map(|n| crate::model::RemoteBranch {
            name: (*n).to_string(),
            tip: "a1b2c3d".to_string(),
        })
        .collect();
    r.remote_only_branches = vec!["origin/nobody".to_string()];

    let width = 92u16;
    let mut app = crate::app::App::new(vec![r], "t".to_string(), width, 44);
    app.history_view = HistoryView::Branches;
    app.set_detail_selected(selected);
    let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    (0..30u16)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The branches view lists every remote, not only the unfetched ones: that
/// distinction is a marker on a row, not a reason to omit the row.
#[test]
fn the_branches_view_lists_local_and_remote_sections() {
    let out = branches_text(0);
    assert!(out.contains("local branches"), "no local section:\n{out}");
    assert!(out.contains("remotes"), "no remotes section:\n{out}");
    assert!(out.contains("main"), "a local branch was dropped:\n{out}");
    assert!(
        out.contains("origin/beta"),
        "a fetched remote was dropped:\n{out}"
    );
    assert!(
        out.contains("origin/nobody"),
        "an unfetched remote was dropped:\n{out}"
    );
}

/// A remote nothing local claims is marked; one a local branch claims is
/// not. Silence must not stand in for either state.
#[test]
fn only_unclaimed_remotes_are_marked_not_fetched() {
    let out = branches_text(0);
    let marked: Vec<&str> = out.lines().filter(|l| l.contains("not fetched")).collect();
    assert_eq!(
        marked.len(),
        1,
        "expected exactly one marked row: {marked:?}"
    );
    assert!(
        marked[0].contains("origin/nobody"),
        "a claimed remote was marked not fetched: {marked:?}"
    );
}

/// The cursor walks past the local branches into the remotes: the two
/// sections are one selectable list, in the order they are drawn.
#[test]
fn the_cursor_walks_from_local_branches_into_remotes() {
    let out = branches_text(2);
    let carets: Vec<&str> = out.lines().filter(|l| l.contains("▸ ")).collect();
    assert_eq!(carets.len(), 1, "expected one caret: {carets:?}");
    assert!(
        carets[0].contains("origin/main"),
        "index 2 should be the first remote: {carets:?}"
    );
}

/// The commit rows of the `commits` view, border stripped, for a list with
/// the given per-commit churn.
fn commit_rows(churn: &[(u32, u32)]) -> Vec<String> {
    commit_rows_at(churn, 100, |i| format!("chore: an ordinary subject {i}"))
}

/// The same at a chosen width, with the subject under the caller's control
/// so a test can force the truncation case.
fn commit_rows_at(
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
fn columns(row: &str) -> Vec<char> {
    row.chars().collect()
}

/// The column a right-aligned `+N` / `-N` token ends at, which is the edge
/// that has to agree between rows. Their *starts* differ by design: that is
/// what right-aligning them means.
fn token_end(row: &str, sign: char) -> Option<usize> {
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

/// The bug the columns alone did not fix. The row budget reserves a space
/// between the subject and what follows it, but it used to be drawn only
/// alongside the leader dots -- so a subject that filled its whole budget,
/// leaving no dots, came out a column short and slid its churn left by one.
/// Whether a subject truncates depends on the pane width, which is why the
/// tail looked aligned at one size and ragged at another.
#[test]
fn a_truncated_subject_keeps_the_churn_columns() {
    // Long enough to truncate at this width, alternating with short ones
    // so both shapes sit in the same list.
    let subject = |i: usize| match i % 2 {
        0 => format!("chore: release v0.3.{i}"),
        _ => format!("fix: make the behind count explicable where it is shown {i}"),
    };
    let rows = commit_rows_at(&[(2, 2), (338, 21), (2, 2), (104, 8)], 64, subject);
    assert_eq!(rows.len(), 4, "fixture did not draw four commit rows");
    assert!(
        rows.iter().any(|r| r.contains('…')),
        "no subject truncated, so this width does not exercise the case"
    );

    let adds: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '+')).collect();
    let dels: Vec<usize> = rows.iter().filter_map(|r| token_end(r, '-')).collect();
    assert!(
        adds.windows(2).all(|w| w[0] == w[1]),
        "insertions ragged across truncated rows: {adds:?}"
    );
    assert!(
        dels.windows(2).all(|w| w[0] == w[1]),
        "deletions ragged across truncated rows: {dels:?}"
    );
}

/// The same thing stated as the invariant underneath it: every commit row
/// draws to exactly the pane's width, dots or no dots.
#[test]
fn every_commit_row_reaches_the_pane_edge() {
    let subject = |i: usize| match i % 2 {
        0 => format!("chore: release v0.3.{i}"),
        _ => format!("fix: make the behind count explicable where it is shown {i}"),
    };
    for width in [64u16, 72, 80, 92] {
        let rows = commit_rows_at(&[(2, 2), (338, 21), (2, 2), (104, 8)], width, subject);
        let ends: Vec<usize> = rows
            .iter()
            .map(|r| {
                columns(r)
                    .iter()
                    .rposition(|c| !c.is_whitespace() && *c != '│')
                    .expect("row drew nothing")
            })
            .collect();
        assert!(
            ends.windows(2).all(|w| w[0] == w[1]),
            "rows end at different columns at width {width}: {ends:?}"
        );
    }
}

#[test]
fn a_zero_count_draws_no_token_at_all() {
    assert_eq!(stat_part_width(0), 0);
    assert_eq!(stat_part_width(9), 2);
    assert_eq!(stat_part_width(338), 4);
}

/// The separator is only spent when both columns exist, so a list with
/// nothing but insertions does not reserve a blank for deletions.
#[test]
fn the_churn_tail_spends_a_separator_only_between_two_columns() {
    assert_eq!(stat_tail_width(0, 0), 0);
    assert_eq!(stat_tail_width(4, 0), 4);
    assert_eq!(stat_tail_width(0, 3), 3);
    assert_eq!(stat_tail_width(4, 3), 8);
}

/// A repo shaped like the one that exposed this: several local branches
/// behind by different amounts, one of them tracking a ref it is not named
/// after, a long remote-only list under them, and enough history to scroll.
fn spread_repo() -> RepoStatus {
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

fn info_text_at_bottom_expanded(repo: RepoStatus, width: u16, height: u16, expand: bool) -> String {
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

/// The remote-only list is bounded by what everyone else has pushed, not
/// by what you have, so it stays in the scrolling body. Pinning it too
/// would cost the pane its history on exactly the busy repos that need it.
///
/// Tested expanded, because collapsed there are no names for the pin to
/// hold and the assertion would pass for the wrong reason.
#[test]
fn the_remote_only_list_is_not_pinned() {
    let text = info_text_at_bottom_expanded(spread_repo(), 92, 20, true);
    assert!(
        text.contains("feat: commit"),
        "the history is unreachable at the bottom of the scroll"
    );
    assert!(
        !text.contains("spare-0 "),
        "the first remote-only row is still on screen at the bottom"
    );
}

/// The reported bug: twenty-plus "not fetched" rows sat between the pinned
/// local branches and the commit list, so `recent commits` started below
/// the fold and the selection scrolled the whole block away as soon as the
/// cursor moved.
#[test]
fn info_keeps_the_commit_list_on_the_first_screen_with_many_remotes() {
    let text = info_text_at_bottom_expanded(spread_repo(), 92, 30, false);
    assert!(
        text.contains("12 remote branches not fetched"),
        "the list did not collapse:\n{text}"
    );
    assert!(
        text.contains("recent commits"),
        "the commit list is still below the fold:\n{text}"
    );
}

/// Collapsed is not a dead end: the names are still reachable in place.
#[test]
fn expanding_the_summary_draws_every_remote_name() {
    let text = info_text_at_bottom_expanded(spread_repo(), 92, 44, true);
    assert!(
        !text.contains("remote branches not fetched"),
        "still collapsed:\n{text}"
    );
    assert!(
        text.contains("spare-11"),
        "names missing when expanded:\n{text}"
    );
}

/// One branch is not "1 remote branches".
#[test]
fn a_single_unfetched_remote_reads_as_one_branch() {
    let mut r = spread_repo();
    r.remote_only_branches = vec!["origin/only".to_string()];
    let text = info_text_at_bottom_expanded(r, 92, 30, false);
    assert!(
        text.contains("1 remote branch not fetched"),
        "plural leaked into the singular:\n{text}"
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
fn is_title_column(column: &str, name: &str) -> bool {
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

/// One CI indicator per pane. The repo row carries a lamp and the detail
/// header carries another; a third in the pane's body made three of them
/// blinking the same fact in step, which reads as three things happening.
///
/// The workflow name and duration went with it. They were the only thing
/// that line added, and a name and a stopwatch are not worth a third lamp.
#[test]
fn the_info_body_does_not_repeat_the_ci_state() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut repo = busy_repo();
    repo.ci_status = crate::model::CiStatus::Passed;
    repo.ci_workflow = Some("build-and-test".to_string());
    repo.ci_duration = Some("12s".to_string());

    let (w, h) = (100u16, 30u16);
    let mut app = crate::app::App::new(vec![repo], "t".to_string(), w, h);
    app.history_view = HistoryView::Info;
    let app = app;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let text: String = (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(
        text.matches("CI ◉").count(),
        1,
        "the pane draws CI more than once:\n{text}"
    );
    assert!(
        !text.contains("build-and-test"),
        "the workflow name is still drawn"
    );
}

/// The header's worktree count is the same concept as the list badge, so
/// it wears the badge's colour; a repo with no linked worktree draws none.
#[test]
fn the_header_worktree_count_wears_the_badge_colour() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (w, h) = (100u16, 30u16);
    let draw = |repo: RepoStatus| {
        let mut app = crate::app::App::new(vec![repo], "t".to_string(), w, h);
        app.history_view = HistoryView::Info;
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        term.backend().buffer().clone()
    };
    let row_text = |buf: &ratatui::buffer::Buffer, y: u16| -> Vec<String> {
        (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect()
    };

    let mut with_one = busy_repo();
    with_one.worktrees = vec![crate::model::Worktree {
        path: "/tmp/wt".to_string(),
        branch: "feature".to_string(),
        ..Default::default()
    }];
    let buf = draw(with_one);
    let found = (0..h).find_map(|y| {
        let cells = row_text(&buf, y);
        let line = cells.concat();
        line.find("1 worktree").map(|byte| {
            let x = line[..byte].chars().count() as u16;
            (x, y)
        })
    });
    let (x, y) = found.expect("the header has no `1 worktree` text");
    assert_eq!(buf[(x, y)].fg, colors::WORKTREE_CLR);

    let buf = draw(busy_repo());
    let text: String = (0..h).flat_map(|y| row_text(&buf, y)).collect();
    assert!(
        !text.contains("worktree"),
        "a repo with no worktrees draws a worktree count"
    );
}

/// `busy_repo` with a second local branch `feature`, two ahead of its
/// upstream, checked out in a linked worktree inside the repo with three
/// uncommitted changes, last committed to two days ago.
fn repo_with_a_worktree() -> RepoStatus {
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
fn drawn(repo: RepoStatus, view: HistoryView) -> (ratatui::buffer::Buffer, Vec<String>) {
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

/// The colour of the first cell of `needle` on row `y`.
fn fg_at(
    buf: &ratatui::buffer::Buffer,
    rows: &[String],
    y: usize,
    needle: &str,
) -> ratatui::style::Color {
    let byte = rows[y]
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not on row {y}: {}", rows[y]));
    let x = rows[y][..byte].chars().count() as u16;
    buf[(x, y as u16)].fg
}

/// Info's `worktrees` section answers the glance: which branch, dirty,
/// ahead or behind, how long since the last commit, and where, relative
/// to the repo when it lives inside it. The counters wear the repo list's
/// colours. The branch row above no longer repeats the path, and the old
/// `other worktrees` section is gone.
#[test]
fn info_draws_a_worktrees_section_with_per_worktree_state() {
    let (buf, rows) = drawn(repo_with_a_worktree(), HistoryView::Info);
    let text = rows.join("\n");

    let label = rows
        .iter()
        .position(|r| r.trim() == "worktrees")
        .unwrap_or_else(|| panic!("no `worktrees` label:\n{text}"));
    let branches = rows
        .iter()
        .position(|r| r.trim() == "branches")
        .expect("the branches block");
    let recent = rows
        .iter()
        .position(|r| r.trim() == "recent commits")
        .expect("the recent commits block");
    assert!(
        branches < label && label < recent,
        "after branches, before recent commits:\n{text}"
    );

    let row = label + 1;
    let line = &rows[row];
    assert!(line.contains("feature"), "the branch: {line}");
    assert!(line.contains("3!"), "the dirty count: {line}");
    assert!(line.contains("2↑"), "the ahead count: {line}");
    assert!(line.contains("2d ago"), "the age: {line}");
    assert!(
        line.contains(" .claude/worktrees/feat"),
        "the path, relative to the repo: {line}"
    );
    assert!(
        !line.contains("~/repos/fleetz"),
        "not the full path: {line}"
    );

    let mut dirty_only = busy_repo();
    dirty_only.dirty = 3;
    let mut ahead_only = busy_repo();
    ahead_only.ahead = 2;
    assert_eq!(
        fg_at(&buf, &rows, row, "3!"),
        badges::status_color(&dirty_only)
    );
    assert_eq!(
        fg_at(&buf, &rows, row, "2↑"),
        badges::status_color(&ahead_only)
    );
    assert_eq!(fg_at(&buf, &rows, row, "2d ago"), colors::OVERLAY);
    assert_eq!(fg_at(&buf, &rows, row, ".claude"), colors::OVERLAY);
    assert_eq!(fg_at(&buf, &rows, row, "feature"), colors::TEXT);

    assert!(
        !rows.iter().any(|r| r.trim_start().starts_with("worktree ")),
        "Info still draws the inline worktree row:\n{text}"
    );
    assert!(
        !text.contains("other worktrees"),
        "the old section is still drawn:\n{text}"
    );
}

/// A clean worktree level with its upstream reads `✓` in the synced
/// colour; one with no upstream says so in words, as a branch row does;
/// a dirty probe that failed is `?`, never clean.
#[test]
fn a_worktree_row_says_clean_unknown_and_no_upstream() {
    let mut r = repo_with_a_worktree();
    r.branches[1].ahead = 0;
    r.worktrees[0].dirty = Some(0);
    let (buf, rows) = drawn(r.clone(), HistoryView::Info);
    let row = rows
        .iter()
        .position(|l| l.trim() == "worktrees")
        .expect("label")
        + 1;
    assert!(rows[row].contains('✓'), "clean and level: {}", rows[row]);
    assert_eq!(
        fg_at(&buf, &rows, row, "✓"),
        badges::status_color(&busy_repo())
    );

    r.branches[1].upstream = None;
    r.worktrees[0].dirty = None;
    let (buf, rows) = drawn(r, HistoryView::Info);
    let row = rows
        .iter()
        .position(|l| l.trim() == "worktrees")
        .expect("label")
        + 1;
    assert!(rows[row].contains("no upstream"), "{}", rows[row]);
    assert!(
        !rows[row].contains('✓'),
        "unknown is not clean: {}",
        rows[row]
    );
    assert_eq!(fg_at(&buf, &rows, row, "no upstream"), colors::OVERLAY);
    assert_eq!(fg_at(&buf, &rows, row, "?"), colors::OVERLAY);
}

/// A detached worktree is named by its HEAD and has no branch to compare,
/// so its counter columns read `·`; a path outside the repo keeps `~`.
#[test]
fn a_detached_worktree_is_named_by_its_head() {
    let mut r = busy_repo();
    r.worktrees = vec![crate::model::Worktree {
        path: "~/scratch/bisect".to_string(),
        branch: String::new(),
        detached: true,
        head: "9a2eb79".to_string(),
        dirty: Some(0),
        head_at: None,
    }];
    let (_, rows) = drawn(r, HistoryView::Info);
    let row = rows
        .iter()
        .position(|l| l.trim() == "worktrees")
        .expect("label")
        + 1;
    let line = &rows[row];
    assert!(line.contains("detached @ 9a2eb79"), "{line}");
    assert!(line.contains("~/scratch/bisect"), "{line}");
    let after_name = &line[line.find("9a2eb79").unwrap() + 7..line.find("~/").unwrap()];
    assert_eq!(
        after_name.split_whitespace().collect::<Vec<_>>(),
        ["·", "·", "·"],
        "dirty, ahead/behind and age all read `·`: {line}"
    );
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

/// A branch longer than the column's cap is cut at `WORKTREE_NAME_MAX`
/// with the pane's ellipsis.
#[test]
fn a_long_worktree_branch_is_cut_at_the_cap() {
    let mut r = repo_with_a_worktree();
    let long = "b".repeat(WORKTREE_NAME_MAX + 10);
    r.branches[1].name = long.clone();
    r.worktrees[0].branch = long;
    let lines = worktree_rows(&r, 200);
    let row: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
    let expected = format!("{}…", "b".repeat(WORKTREE_NAME_MAX - 1));
    assert!(row.contains(&expected), "{row}");
    assert!(!row.contains(&"b".repeat(WORKTREE_NAME_MAX)), "{row}");
}

/// The text of `line`, every span joined.
fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// The `worktrees` view lists one row per worktree, the main checkout
/// first and marked `*` before the linked one (FZ-77).
#[test]
fn the_worktrees_view_lists_the_main_checkout_first_and_marked() {
    let (_, rows) = drawn(repo_with_a_worktree(), HistoryView::Worktrees);
    let text = rows.join("\n");
    let main = rows
        .iter()
        .position(|r| r.contains("▸ * main"))
        .unwrap_or_else(|| panic!("no marked main row under the cursor:\n{text}"));
    let linked = rows
        .iter()
        .position(|r| r.contains("feature") && r.contains(".claude/worktrees/feat"))
        .unwrap_or_else(|| panic!("no linked row:\n{text}"));
    assert_eq!(
        linked,
        main + 1,
        "the linked row follows the main one:\n{text}"
    );
    assert!(!rows[linked].contains('*'), "only main is marked:\n{text}");
    assert!(!text.contains("no worktrees"), "{text}");

    let mut app = crate::app::App::new(vec![repo_with_a_worktree()], "t".to_string(), 100, 40);
    app.history_view = HistoryView::Worktrees;
    assert_eq!(app.selected_commit_count(), 2, "one stop per worktree");
}

/// The view's rows are `info`'s worktree rows: the linked row is the
/// same text as `worktree_rows` draws for the same repo, behind the
/// cursor's column, so the two cannot drift.
///
/// The view's table is aligned over every row it lists, the main
/// checkout included, so a main branch wider than the linked ones pads
/// the linked rows differently from `info`, which aligns over the linked
/// rows alone. That is by design; this fixture's main row is no wider
/// in any column, which is what lets the texts compare equal.
#[test]
fn the_worktrees_view_rows_are_the_info_section_rows() {
    let repo = repo_with_a_worktree();
    let mut app = crate::app::App::new(vec![repo.clone()], "t".to_string(), 100, 40);
    app.history_view = HistoryView::Worktrees;
    let width = 90;
    let (rows, selected) = worktrees_view_rows(&app, &repo, width);
    assert_eq!(selected, Some(0));
    assert_eq!(rows.len(), 2);
    let info: Vec<String> = worktree_rows(&repo, width - 2)[1..]
        .iter()
        .map(line_text)
        .collect();
    let view: Vec<String> = rows[1..]
        .iter()
        .map(|l| line_text(l).chars().skip(2).collect())
        .collect();
    assert_eq!(view, info);
    let main = line_text(&rows[0]);
    assert!(main.starts_with("▸ * main"), "{main}");
}

/// A repo with no linked worktree lists its main checkout and says it
/// has no worktrees besides; a row with nothing on disk says `not
/// cloned`.
#[test]
fn the_worktrees_view_says_no_worktrees_and_not_cloned() {
    let (_, rows) = drawn(busy_repo(), HistoryView::Worktrees);
    let text = rows.join("\n");
    assert!(text.contains("no worktrees"), "{text}");
    assert!(text.contains("▸ * main"), "{text}");

    let mut gone = busy_repo();
    gone.location = RepoLocation::NotCloned;
    let (_, rows) = drawn(gone, HistoryView::Worktrees);
    let text = rows.join("\n");
    assert!(text.contains("not cloned"), "{text}");
    assert!(!text.contains("▸ "), "nothing to select:\n{text}");
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

/// The inspector shows the highlighted worktree's path and branch, and
/// once the log landed its HEAD subject and commits.
#[test]
fn the_worktrees_inspector_shows_the_highlighted_worktree() {
    let mut app = crate::app::App::new(vec![repo_with_a_worktree()], "t".to_string(), 100, 40);
    app.view = crate::app::View::Detail;
    app.history_view = HistoryView::Worktrees;
    app.detail_selections[HistoryView::Worktrees.index()] = 1;
    let text: Vec<String> = worktree_inspector_lines(&app)
        .iter()
        .map(line_text)
        .collect();
    assert_eq!(text[0], "~/repos/fleetz/.claude/worktrees/feat");
    assert_eq!(text[1], "branch  feature");
    assert!(text[4].contains("2↑ against origin/feature"), "{text:?}");
    assert!(text.iter().any(|l| l.contains("reading")), "{text:?}");

    app.inspector_folded = false;
    let start = std::time::Instant::now();
    assert_eq!(app.claim_worktree_log(start), None, "the dwell starts");
    let read = app
        .claim_worktree_log(start + crate::ci::DWELL)
        .expect("a read");
    assert_eq!(read.key, "~/repos/fleetz/.claude/worktrees/feat");
    let commit = crate::model::Commit {
        hash: "b2c3d4e".to_string(),
        message: "feat: the side work".to_string(),
        author: "Gaurgle".to_string(),
        date: Some(chrono::Utc::now()),
        body: String::new(),
        insertions: 0,
        deletions: 0,
        files: vec![],
    };
    app.apply_worktree_log(read, vec![commit]);
    let text: Vec<String> = worktree_inspector_lines(&app)
        .iter()
        .map(line_text)
        .collect();
    assert_eq!(text[2], "HEAD    b2c3d4e feat: the side work");
    assert!(
        text.iter()
            .any(|l| l.starts_with("  b2c3d4e") && l.contains("feat: the side work")),
        "{text:?}"
    );

    app.detail_selections[HistoryView::Worktrees.index()] = 0;
    let text: Vec<String> = worktree_inspector_lines(&app)
        .iter()
        .map(line_text)
        .collect();
    assert_eq!(text[1], "branch  main");
}

/// A detached main checkout names its HEAD in the readout by the same
/// 7 digits its row draws, not the full 40 of `rev-parse HEAD`.
#[test]
fn a_detached_main_checkout_reads_out_its_short_head() {
    let mut r = repo_with_a_worktree();
    r.branches[0].is_head = false;
    let full = "0123456789abcdef0123456789abcdef01234567";
    r.work_key = Some(crate::digest::tests::work_key(full));
    let (_, rows) = drawn(r, HistoryView::Worktrees);
    let text = rows.join("\n");
    assert!(text.contains("[1/2 · 0123456]"), "{text}");
    assert!(!text.contains(full), "{text}");
    assert!(text.contains("detached @ 0123456"), "{text}");
}

/// A click on a worktree row moves the cursor there.
#[test]
fn a_click_on_a_worktree_row_moves_the_cursor() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut app = crate::app::App::new(vec![repo_with_a_worktree()], "t".to_string(), 100, 40);
    app.history_view = HistoryView::Worktrees;
    let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let y = (0..40u16)
        .find(|&y| {
            let row: String = (0..100u16).map(|x| buf[(x, y)].symbol()).collect();
            row.contains(".claude/worktrees/feat")
        })
        .expect("the linked row");
    let click = app.detail_hits.borrow().at(10, y).expect("a hit");
    app.click_detail(&click);
    assert_eq!(app.detail_selected(), 1);
    assert_eq!(
        app.worktree_highlighted().map(|w| w.branch),
        Some("feature".to_string())
    );
}

#[test]
fn info_with_no_linked_worktree_draws_no_worktrees_label() {
    let (_, rows) = drawn(busy_repo(), HistoryView::Info);
    assert!(!rows.iter().any(|r| r.trim() == "worktrees"));
}

/// The Branches view has no worktrees section, so its inline row is still
/// the place it says "checked out elsewhere".
#[test]
fn the_branches_view_keeps_the_inline_worktree_row() {
    let (_, rows) = drawn(repo_with_a_worktree(), HistoryView::Branches);
    assert!(
        rows.iter()
            .any(|r| r.contains("worktree ~/repos/fleetz/.claude/worktrees/feat")),
        "{}",
        rows.join("\n")
    );
    assert!(!rows.iter().any(|r| r.trim() == "worktrees"));
}

#[test]
fn a_worktree_path_is_shortened_only_under_the_repo() {
    assert_eq!(
        path_within_repo("~/repos/fleetz/.claude/worktrees/x", "~/repos/fleetz"),
        ".claude/worktrees/x"
    );
    assert_eq!(
        path_within_repo("~/repos/fleetz-hotfix", "~/repos/fleetz"),
        "~/repos/fleetz-hotfix",
        "a sibling sharing the prefix is outside"
    );
    assert_eq!(path_within_repo("/tmp/wt", "~/repos/fleetz"), "/tmp/wt");
    assert_eq!(path_within_repo("~/repos/fleetz", "~/repos/fleetz"), ".");
    assert_eq!(
        path_within_repo("~/repos/fleetz/wt", "~/repos/fleetz/"),
        "wt",
        "a trailing separator on the repo path"
    );
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

/// `info` is a glance: the newest few with their diffstats, capped even
/// when the loaded list runs to hundreds. `commits` is the whole thing.
#[test]
fn info_shows_a_glance_and_commits_shows_everything() {
    let mut r = busy_repo();
    r.commits = (0..30)
        .map(|i| crate::model::Commit {
            hash: format!("c{i:06}"),
            author: "Gaurgle".to_string(),
            date: Some(chrono::Utc::now()),
            message: format!("subject {i}"),
            body: String::new(),
            insertions: 1,
            deletions: 0,
            files: vec![],
        })
        .collect();
    r.graph = (0..30)
        .map(|i| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: format!("g{i:06}"),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("graph {i}"),
        })
        .collect();

    let rendered = |view: HistoryView| {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut app = crate::app::App::new(vec![r.clone()], "t".to_string(), 96, 44);
        app.history_view = view;
        let mut term = Terminal::new(TestBackend::new(96, 60)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..60u16)
            .map(|y| (0..96u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Both draw the same list through one renderer; only the length
    // differs, which is the point of sharing it.
    let info = rendered(HistoryView::Info);
    assert!(info.contains("g000005"), "the 6th is shown");
    assert!(
        !info.contains("g000006"),
        "the 7th is not: {RECENT_COMMIT_COUNT} max"
    );

    let commits = rendered(HistoryView::Commits);
    assert!(commits.contains("g000000"), "commits starts at the newest");
    assert!(commits.contains("g000020"), "and runs past info's cap");
}

/// Regression: the cursor walked every row before the pane scrolled at
/// all. Survivable at six commits and the whole view at two hundred --
/// `j` moved a cursor that was already off screen, so the pane looked
/// frozen. It follows the selection now.
#[test]
fn the_pane_scrolls_to_keep_the_selection_visible() {
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

    let mut app = crate::app::App::new(vec![r], "t".to_string(), 96, 30);
    app.history_view = HistoryView::Commits;

    let draw = |app: &crate::app::App| {
        let mut term = Terminal::new(TestBackend::new(96, 20)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..20u16)
            .map(|y| (0..96u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };

    assert!(draw(&app).contains("h000000"), "opens at the newest");
    assert_eq!(app.detail_scroll(), 0);

    // A selection far below the fold must drag the pane with it.
    app.set_detail_selected(120);
    let shown = draw(&app);
    assert!(shown.contains("h000120"), "selection off screen: {shown:?}");
    assert!(app.detail_scroll() > 0, "the pane did not follow");

    // And back up again.
    app.set_detail_selected(2);
    let shown = draw(&app);
    assert!(shown.contains("h000002"), "did not come back: {shown:?}");
}

/// A row must not spill past the pane. The refs and the subject can each
/// eat the width on their own, so the dot fill has to be allowed to
/// vanish rather than being forced to one -- that single dot was enough
/// to push the diffstat onto a line of its own.
#[test]
fn no_commit_row_overflows_the_pane() {
    let mut r = busy_repo();
    r.commits = vec![crate::model::Commit {
        hash: "a1b2c3d".to_string(),
        author: "Gaurgle".to_string(),
        date: Some(chrono::Utc::now()),
        message: "x".repeat(200),
        body: String::new(),
        insertions: 4321,
        deletions: 8765,
        files: vec![],
    }];
    r.graph = vec![crate::model::GraphLine {
        lanes: vec![crate::graph::commit_cell()],
        hash: "a1b2c3d".to_string(),
        is_merge: false,
        refs: vec![
            crate::model::GitRef {
                kind: crate::model::RefKind::Local,
                name: "a-rather-long-branch-name".to_string(),
                is_head: true,
            },
            crate::model::GitRef {
                kind: crate::model::RefKind::Remote,
                name: "origin/a-rather-long-branch-name".to_string(),
                is_head: false,
            },
        ],
        author: "Gaurgle".to_string(),
        message: "x".repeat(200),
    }];

    for width in [60usize, 80, 100, 140] {
        let mut row_marker = None;
        let lines = graph_list_lines(&r, width, 0, None, &mut row_marker);
        for line in &lines {
            let drawn: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(
                drawn <= width,
                "row is {drawn} wide in a {width} pane: {:?}",
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            );
        }
    }
}

/// Every list bolds its selection, so the pane always says which commit
/// the inspector is showing. Only the commit list did before.
#[test]
fn every_list_view_bolds_its_selection() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    for (view, hash) in [
        (HistoryView::Info, "a1b2c3d"),
        (HistoryView::Commits, "a1b2c3d"),
        (HistoryView::Reflog, "a1b2c3d"),
        (HistoryView::Graph, "a1b2c3d"),
    ] {
        let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), 96, 44);
        app.history_view = view;
        app.set_detail_selected(0);
        let mut term = Terminal::new(TestBackend::new(96, 40)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();

        let mut found_bold = false;
        for y in 0..40u16 {
            let row: String = (0..96u16).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(col) = row.find(hash)
                && buf[(col as u16, y)].modifier.contains(Modifier::BOLD)
            {
                found_bold = true;
            }
        }
        assert!(found_bold, "{} does not bold its selection", view.label());
    }
}

/// The expanded view reproduces `git log --graph`, so its node stays `*`.
/// That format spells merges out on their own `Merge:` line, and swapping
/// the glyph would only stop it looking like the thing it is copying.
#[test]
fn the_expanded_graph_keeps_gits_own_node() {
    let text: String = expanded_graph_lines("* commit a1b2c3d", "", 80)
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect();
    assert!(text.contains("* commit"), "got {text:?}");
    assert!(!text.contains(COMMIT_NODE), "no substitution here");

    // The compact list is the opposite: it computes its own topology and
    // draws it in box characters. See
    // `the_graph_is_drawn_with_box_characters_not_ascii`.
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

/// One selection marker across all four views. Reflog and the expanded
/// graph bolded the hash and nothing else, which marks a single token and
/// leaves the eye to find which token it was -- in a two-hundred row list
/// that search is the work the caret exists to save.
#[test]
fn every_view_marks_its_selection_the_same_way() {
    for view in HistoryView::ALL {
        // `ci` lists runs, so it is drawn with the recorded runs held.
        let text = match view {
            HistoryView::Ci => ci_view_text(92),
            HistoryView::Notes => {
                notes_pane_text(&notes_view_app(&[("notez/a.md", Some("notez"))]))
            }
            HistoryView::Changes => notes_pane_text(&changes_view_app(held_changes())),
            _ => pane_text(view, 92),
        };
        assert!(
            text.contains("▸ "),
            "{} view has no selection caret:\n{text}",
            view.label()
        );
    }
}

/// A percentage of a very wide terminal is more room than a repo row can
/// use: past about fifty columns the list is only drawing longer dots.
/// The right column has the opposite appetite, so the surplus goes there.
#[test]
fn the_repo_list_stops_growing_once_the_rows_have_what_they_need() {
    use ratatui::layout::Rect;
    let at = |w: u16| {
        let (list, right) = split_main(Rect::new(0, 0, w, 40), crate::app::DEFAULT_REPO_LIST_PCT);
        (list.width, right.width)
    };

    // Below the cap nothing changes: the percentage is still the answer.
    let (list, _) = at(120);
    assert_eq!(list, 120 * crate::app::DEFAULT_REPO_LIST_PCT / 100);

    let (narrow_list, narrow_right) = at(150);
    let (wide_list, wide_right) = at(300);
    assert_eq!(wide_list, REPO_LIST_MAX_COLS, "the list kept growing");
    assert!(
        wide_right - narrow_right >= 150,
        "the surplus did not reach the right column: {narrow_right} -> {wide_right}"
    );
    assert!(narrow_list <= REPO_LIST_MAX_COLS);
}

/// Only the default yields to the cap. Once the divider has been dragged
/// the number is the user's answer to this question, and a cap that
/// overrode it would make the drag look broken.
#[test]
fn a_dragged_divider_is_honoured_past_the_cap() {
    use ratatui::layout::Rect;
    let dragged = crate::app::DEFAULT_REPO_LIST_PCT + 5;
    let (list, _) = split_main(Rect::new(0, 0, 300, 40), dragged);
    assert_eq!(list.width, 300 * dragged / 100);
    assert!(list.width > REPO_LIST_MAX_COLS);
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

/// The repo header identifies what you are looking at, and scrolling two
/// hundred rows of history is exactly when that question comes back. It
/// sits outside the scrolling body now, like the pinned readout above it.
#[test]
fn the_repo_header_survives_scrolling() {
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

    let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 30);
    app.history_view = HistoryView::Commits;
    app.set_detail_scroll(40);
    let mut term = Terminal::new(TestBackend::new(92, 24)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let text: String = (0..24u16)
        .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        text.contains("~/repos/fleetz"),
        "header scrolled away:\n{text}"
    );
    assert!(text.contains("synced"), "status scrolled away");
    assert!(
        text.contains("h000040"),
        "the body did not scroll with the header pinned"
    );
}

/// A short pane spends its rows on the content you came for. The header
/// repeats what the frame's own title already says, so it is the half that
/// gives way rather than taking half the height.
#[test]
fn a_short_pane_lets_the_header_scroll_rather_than_eat_the_body() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut r = busy_repo();
    r.graph = (0..40)
        .map(|i| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: format!("h{i:06}"),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("subject {i}"),
        })
        .collect();
    let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 10);
    app.history_view = HistoryView::Commits;
    app.set_detail_scroll(30);
    let mut term = Terminal::new(TestBackend::new(92, 10)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let text: String = (0..10u16)
        .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("h0000"),
        "a short pane showed no history at all:\n{text}"
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

/// A node is a commit, not a branch. Colouring it by lane made the node
/// column change hue whenever a lane was reused, which is a signal that
/// means nothing -- the lanes either side already say what the topology
/// is.
#[test]
fn graph_nodes_are_plain_text_not_lane_coloured() {
    let spec: [(&str, &[&str]); 4] = [
        ("aaaaaaa", &["bbbbbbb", "ccccccc"]),
        ("ccccccc", &["ddddddd"]),
        ("bbbbbbb", &["ddddddd"]),
        ("ddddddd", &[]),
    ];
    let parents: Vec<Vec<String>> = spec
        .iter()
        .map(|(_, p)| p.iter().map(|s| (*s).to_string()).collect())
        .collect();
    let commits: Vec<crate::graph::Commit<'_>> = spec
        .iter()
        .zip(&parents)
        .map(|((hash, _), parents)| crate::graph::Commit { hash, parents })
        .collect();

    let rows = crate::graph::layout(&commits);
    let mut nodes = 0usize;
    let mut lanes = 0usize;
    for cells in &rows {
        for span in lane_spans(cells) {
            let glyph = span.content.chars().next().unwrap_or(' ');
            if glyph == crate::graph::COMMIT_NODE || glyph == crate::graph::MERGE_NODE {
                nodes += 1;
                assert_eq!(span.style.fg, Some(colors::TEXT), "a node is lane-coloured");
            } else if glyph == '\u{2502}' {
                lanes += 1;
                assert_ne!(span.style.fg, Some(colors::TEXT), "a lane went plain");
            }
        }
    }
    assert!(nodes >= 4, "fixture drew no nodes");
    assert!(lanes >= 1, "fixture drew no lanes to contrast against");
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

/// The inspector below shows one commit; the list has to say which.    /// The inspector below shows one commit; the list has to say which. The
/// ▸ marker lives in its own fixed-width column, so on a long list the
/// eye has to travel to it -- bold puts the weight on the hash itself.
#[test]
fn the_selected_commits_hash_is_bold() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut r = cloned_repo();
    r.graph = (0..3)
        .map(|i| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: format!("abc123{i}"),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("commit {i}"),
        })
        .collect();
    let mut app = crate::app::App::new(vec![r], "t".to_string(), 100, 44);
    app.history_view = HistoryView::Info;
    app.set_detail_selected(1);

    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();

    // Only the list rows, which are the ones carrying the dot fill. The
    // pinned readout above the body repeats the selected hash, and
    // counting that as a list row makes the test pass for the wrong
    // reason -- the readout has always been styled separately.
    let mut weights = Vec::new();
    for y in 0..30u16 {
        let row: String = (0..100u16).map(|x| buf[(x, y)].symbol()).collect();
        for i in 0..3 {
            // A list row carries both the hash and its message. The
            // pinned readout above the body repeats the selected hash on
            // its own (`[2/3 · abc1231]`), and counting that as a list
            // row makes this pass for the wrong reason: the readout has
            // always been styled separately from the list.
            if !row.contains(&format!("commit {i}")) {
                continue;
            }
            if let Some(col) = row.find(&format!("abc123{i}")) {
                weights.push((i, buf[(col as u16, y)].modifier.contains(Modifier::BOLD)));
            }
        }
    }
    assert_eq!(weights.len(), 3, "one list row per commit: {weights:?}");
    for (i, bold) in weights {
        assert_eq!(bold, i == 1, "hash {i} boldness wrong");
    }
}

/// Stashes were drawn in OVERLAY, the colour used for paths and other
/// furniture, which made a real fact read as something already dismissed.
/// They also carry the repo list's glyph now, so the same symbol means
/// the same thing in both panes.
#[test]
fn the_stash_count_is_findable_in_the_repo_pane() {
    let mut r = cloned_repo();
    r.stash_count = 3;
    let (_, _, facts) = header_rows(r, 100);
    assert!(facts.contains("3 stashes"), "got {facts:?}");
    assert!(
        facts.contains(badges::STASH_GLYPH),
        "the repo list's glyph is missing: {facts:?}"
    );
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

/// CI was named in the pane frame and again on the header's remote row,
/// in the same words one line apart. The frame gave way: the row groups
/// it with the PRs, which is the other half of what GitHub knows.
#[test]
fn ci_is_named_once_not_in_the_frame_as_well() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut r = cloned_repo();
    r.ci_status = CiStatus::Passed;
    let app = crate::app::App::new(vec![r], "t".to_string(), 92, 44);
    let mut term = Terminal::new(TestBackend::new(92, 20)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let rows: Vec<String> = (0..20u16)
        .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect())
        .collect();

    assert!(
        !rows[0].contains("CI ◉"),
        "frame should not name CI: {:?}",
        rows[0]
    );
    assert!(
        rows.iter().any(|r| r.contains("CI ◉ passed")),
        "but the header row still should"
    );
}

/// This pane is the repo's main window, so an open PR belongs on its top
/// line beside the sync state, not only in a section three groups down
/// that a default run does not even render.
#[test]
fn the_status_line_says_when_a_repo_has_open_prs() {
    let facts = |n: u32, numbers: Vec<u32>| {
        let mut r = cloned_repo();
        r.pr_count = n;
        r.prs = numbers.into_iter().map(pr_numbered).collect();
        header_rows(r, 120).2
    };

    // Flush right, on the remote half of the fact row.
    let two = facts(2, vec![13, 9]);
    assert!(two.contains("2 open PRs"), "got {two:?}");
    assert!(
        two.contains("#13") && two.contains("#9"),
        "numbers: {two:?}"
    );

    let one = facts(1, vec![13]);
    assert!(one.contains("1 open PR #13"), "singular: {one:?}");
    assert!(!one.contains("1 open PRs"), "got {one:?}");

    // More than fit are summarised rather than pushing CI off the row.
    let many = facts(5, vec![13, 9, 7, 4, 2]);
    assert!(many.contains("5 open PRs #13 #9+"), "got {many:?}");

    let none = facts(0, vec![]);
    assert!(!none.contains("open PR"), "silent when there are none");
}

/// Every row of the detail pane under `-p`, for the PR section's heading.
fn pane_rows_with_pr_detail(total: u32, listed: Vec<u32>) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut repo = cloned_repo();
    repo.pr_count = total;
    repo.prs = listed.into_iter().map(pr_numbered).collect();
    let mut app = crate::app::App::new(vec![repo], "test".to_string(), 120, 60);
    app.show_pr_detail = true;
    let mut terminal = Terminal::new(TestBackend::new(120, 60)).unwrap();
    terminal
        .draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..60)
        .map(|y| {
            (0..120)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

/// CV14: five listed of eight open. Fails if the heading goes back to
/// counting the list alone ("5 open PRs").
#[test]
fn pr_section_says_five_of_eight_when_the_list_is_capped() {
    let rows = pane_rows_with_pr_detail(8, vec![13, 9, 7, 4, 2]);
    assert!(
        rows.iter().any(|r| r.contains("5 of 8 open PRs")),
        "got {rows:?}"
    );
    assert!(!rows.iter().any(|r| r.contains(" 5 open PRs")));
}

/// Fails if the equal case grows an "of" or loses the singular.
#[test]
fn pr_section_wording_is_unchanged_when_every_open_pr_is_listed() {
    let five = pane_rows_with_pr_detail(5, vec![13, 9, 7, 4, 2]);
    assert!(five.iter().any(|r| r.contains("5 open PRs")));
    assert!(!five.iter().any(|r| r.contains(" of 5 open")));

    let one = pane_rows_with_pr_detail(1, vec![13]);
    assert!(one.iter().any(|r| r.contains("1 open PR")));
    assert!(!one.iter().any(|r| r.contains("1 open PRs")));
    assert!(!one.iter().any(|r| r.contains("1 of 1")));
}

/// The noun follows the total. Fails if it follows the listed count
/// ("1 of 3 open PR").
#[test]
fn pr_section_pluralises_by_the_total_when_one_of_three_is_listed() {
    let rows = pane_rows_with_pr_detail(3, vec![13]);
    assert!(
        rows.iter().any(|r| r.contains("1 of 3 open PRs")),
        "got {rows:?}"
    );
}

/// A list longer than the count must not print "8 of 5". Fails if the
/// comparison is dropped.
#[test]
fn pr_section_never_prints_a_listed_count_above_the_total() {
    let rows = pane_rows_with_pr_detail(2, vec![13, 9, 7]);
    assert!(rows.iter().any(|r| r.contains("3 open PRs")));
    assert!(!rows.iter().any(|r| r.contains(" of 2 open")));
}

/// The header row already uses the total and must keep doing so beside
/// the section's new wording.
#[test]
fn header_row_keeps_the_total_when_the_pr_section_is_capped() {
    let mut r = cloned_repo();
    r.pr_count = 8;
    r.prs = vec![13, 9, 7, 4, 2].into_iter().map(pr_numbered).collect();
    let facts = header_rows(r, 120).2;
    assert!(facts.contains("8 open PRs #13 #9+"), "got {facts:?}");
    assert!(!facts.contains(" of 8"), "got {facts:?}");
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

/// `refs_width` budgets the message column and `ref_spans` draws the
/// refs. Two pieces of arithmetic over the same thing: when they drift,
/// the message is truncated to the wrong width and nothing fails.
#[test]
fn the_measured_ref_width_matches_what_gets_drawn() {
    let cases: Vec<Vec<GitRef>> = vec![
        vec![],
        vec![git_ref(RefKind::Local, "main", true)],
        vec![git_ref(RefKind::Head, "HEAD", false)],
        vec![
            git_ref(RefKind::Local, "main", true),
            git_ref(RefKind::Remote, "origin/main", false),
            git_ref(RefKind::Tag, "v1.0", false),
        ],
    ];
    for refs in cases {
        let line = crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: "a1b2c3d".to_string(),
            is_merge: false,
            refs: refs.clone(),
            author: "Alice".to_string(),
            message: "feat: thing".to_string(),
        };
        let drawn = span_text(&ref_spans(&refs));
        // refs_width excludes the leading space; ref_spans emits it.
        let measured = if refs.is_empty() {
            0
        } else {
            line.refs_width() + 1
        };
        assert_eq!(drawn.chars().count(), measured, "width drift on {drawn:?}");
    }
}

#[test]
fn an_attached_head_draws_the_arrow_and_a_detached_one_does_not() {
    let attached = span_text(&ref_spans(&[
        git_ref(RefKind::Local, "main", true),
        git_ref(RefKind::Remote, "origin/main", false),
    ]));
    assert_eq!(attached, " (HEAD -> main, origin/main)");

    // Detached HEAD sitting on a tag: git separates these with a comma,
    // not an arrow. HEAD does not point at the tag.
    let detached = span_text(&ref_spans(&[
        git_ref(RefKind::Head, "HEAD", false),
        git_ref(RefKind::Tag, "v1.0", false),
    ]));
    assert_eq!(detached, " (HEAD, v1.0)");
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

fn repo(name: &str, kind: RepoKind, owner: Option<&str>) -> RepoStatus {
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

/// The reported width must match what actually gets drawn, or the group
/// stops being flush right.
fn drawn_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

#[test]
fn identity_splits_owner_above_name() {
    let r = repo("fleetz", RepoKind::Public, Some("Gaurgle"));
    let (owner, owner_w) = owner_spans(&r);
    let (name, name_w) = name_spans(&r);

    let owner_text: String = owner.iter().map(|s| s.content.as_ref()).collect();
    let name_text: String = name.iter().map(|s| s.content.as_ref()).collect();
    assert!(owner_text.ends_with("Gaurgle"), "got {owner_text:?}");
    assert!(
        !owner_text.contains("fleetz"),
        "name belongs on its own row: {owner_text:?}"
    );
    assert_eq!(name_text, "fleetz");

    assert_eq!(owner_w, drawn_width(&owner));
    assert_eq!(name_w, drawn_width(&name));
}

#[test]
fn identity_is_empty_without_an_owner() {
    // Name alone would just repeat the panel title, so neither row
    // renders: the block is all or nothing.
    let r = repo("fleetz", RepoKind::Unknown, None);
    assert!(owner_spans(&r).0.is_empty());
    assert_eq!(owner_spans(&r).1, 0);
    assert!(name_spans(&r).0.is_empty());
    assert_eq!(name_spans(&r).1, 0);
}

#[test]
fn identity_falls_back_to_the_remote_url() {
    let mut r = repo("fleetz", RepoKind::Unknown, None);
    r.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
    let owner_text: String = owner_spans(&r)
        .0
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(owner_text, "Gaurgle");
}

#[test]
fn identity_kind_glyph_is_only_drawn_when_known() {
    let (org, org_w) = owner_spans(&repo("svc", RepoKind::Org, Some("acme")));
    let (unknown, unknown_w) = owner_spans(&repo("svc", RepoKind::Unknown, Some("acme")));
    // Same text, but the org row carries the leading building glyph.
    assert_eq!(org_w - unknown_w, badges::GLYPH_W);
    assert_eq!(org_w, drawn_width(&org));
    assert_eq!(unknown_w, drawn_width(&unknown));
}

#[test]
fn identity_gap_pushes_the_group_flush_right() {
    // 40 wide, 10 used on the left, 14 for the group: 16 blank between.
    assert_eq!(identity_gap(40, 10, 14), Some(16));
}

#[test]
fn identity_gap_drops_the_group_when_it_would_not_fit() {
    // Exactly filling the row leaves no separation, so it is dropped
    // rather than run on against the path.
    assert_eq!(identity_gap(24, 10, 14), None);
    assert_eq!(identity_gap(25, 10, 14), None, "one column is not a gap");
    assert_eq!(identity_gap(26, 10, 14), Some(2), "two columns is enough");
    // Overflowing must not underflow the subtraction.
    assert_eq!(identity_gap(20, 10, 14), None);
}

/// Renders the real pane and reads rows back out, so placement is
/// verified against actual output rather than the arithmetic behind it.
/// Row 2 is the status line, row 3 the remote-URL line. Rows 0 and 1 are
/// the border and the block's top padding; the selected-commit readout
/// rides the view's title row since FZ-93 (`readout_row`). The border and
/// padding are stripped so assertions are about content, not the frame
/// around it.
fn pane_rows(repo: RepoStatus, width: u16) -> (String, String) {
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
fn header_rows(repo: RepoStatus, width: u16) -> (String, String, String) {
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

fn pr_numbered(number: u32) -> crate::model::PullRequest {
    crate::model::PullRequest {
        number,
        title: format!("pr {number}"),
        author: "Gaurgle".to_string(),
        created_at: chrono::Utc::now(),
        url: format!("https://github.com/Gaurgle/fleetz/pull/{number}"),
        review_status: crate::model::ReviewStatus::Unknown,
    }
}

fn cloned_repo() -> RepoStatus {
    let mut r = repo("fleetz", RepoKind::Public, Some("Gaurgle"));
    r.path = Some("~/repos/fleetz".to_string());
    r.remote_url = Some("git@github.com:Gaurgle/fleetz.git".to_string());
    r
}

/// Every row of the detail pane as drawn, joined by newlines.
fn header_text(repo: RepoStatus, width: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
    terminal
        .draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..24)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rule 8: an HTTPS remote can carry a token in its userinfo, and the
/// remote row drew `remote_url` exactly as stored. Display only: the
/// owner is still read from the stored URL.
#[test]
fn a_remote_url_with_a_credential_is_drawn_without_it() {
    let mut r = cloned_repo();
    r.owner = None;
    r.remote_url = Some("https://user:secret@github.com/Gaurgle/fleetz.git".to_string());
    assert_eq!(r.owner_display().as_deref(), Some("Gaurgle"));
    let text = header_text(r, 100);
    assert!(!text.contains("secret"), "credential on screen:\n{text}");
    assert!(!text.contains("user"), "userinfo on screen:\n{text}");
    // Named as `owner/name` since FZ-95, the URL's prefix dropped.
    assert!(text.contains("Gaurgle/fleetz"), "{text}");
    assert!(!text.contains("https://"), "{text}");
}

/// Rule 5 and rule 8 in the pane: under the counts, that the fetch
/// failed, why and since when, then git's line with no credential in it.
/// Since FZ-95's fix cycle (Andreas, 2026-11-06) on one orange banner
/// row under the block, cut with `…`, and whole in `info`'s body.
#[test]
fn a_failed_fetch_is_spelled_out_under_the_counts() {
    let mut r = cloned_repo();
    r.behind = 2;
    let at = chrono::Local::now();
    r.fetch.last_success = Some(at);
    let failure = crate::git::FetchFailure {
        kind: crate::git::FetchFailureKind::Unreachable,
        detail: Some("fatal: unable to access 'https://host.invalid/o/r.git/'".to_string()),
    };
    r.fetch
        .record(&crate::git::FetchOutcome::Failed(failure), at);
    let text = header_text(r, 100);
    let rows: Vec<&str> = text.lines().collect();
    // The counts on the block's first row; the failure and git's reason
    // on one banner row under the block's two rows (this repo has no
    // third; three while the window box stood beside them), cut at the
    // pane's edge, with no divider crossed: the row holds the frame's
    // two borders and nothing else.
    assert!(rows[2].contains("2 behind"), "{text}");
    let failure = format!(
        "⚠ fetch failed: remote unreachable, as of {}",
        at.format("%H:%M")
    );
    let reason = "fatal: unable to access 'https://host.invalid/o/r.git/'";
    let banner = rows[4];
    assert!(banner.contains(&format!("{failure} · fatal")), "{text}");
    assert!(!banner.contains(reason), "not cut: {banner:?}");
    assert!(banner.trim_end_matches('│').trim_end().ends_with('…'));
    assert_eq!(banner.matches('│').count(), 2, "a divider on {banner:?}");
    assert!(rows[5].trim_matches('│').trim().is_empty(), "{text}");
    // Whole, each on its own row, in `info`'s body below the headliner.
    let body = rows[6..].join("\n");
    assert!(body.contains(&failure), "{text}");
    assert!(body.contains(reason), "{text}");
}

/// Nothing extra in the pane while fetches work.
#[test]
fn a_working_fetch_adds_no_row_to_the_pane() {
    let mut r = cloned_repo();
    r.fetch
        .record(&crate::git::FetchOutcome::Succeeded, chrono::Local::now());
    let text = header_text(r, 100);
    assert!(!text.contains("fetch"), "{text}");
}

/// git accepts a remote pasted with smart quotes and returns it as
/// written. Drawing it must neither panic nor show the credential.
#[test]
fn a_smart_quoted_remote_url_draws_without_panicking_or_its_credential() {
    let mut r = cloned_repo();
    r.remote_url = Some("\u{2018}https://u:secret@github.com/o/r.git\u{2019}".to_string());
    let text = header_text(r, 100);
    assert!(!text.contains("secret"), "credential on screen:\n{text}");
    // Named as `owner/name` since FZ-95; the quotes are the URL's own.
    assert!(text.contains("o/r"), "{text}");
}

/// The readout: what the view's pinned title row (`INFO`, the headliner's
/// first row) carries right of the title since FZ-93, border stripped.
fn readout_row(repo: RepoStatus, selected: usize, width: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = crate::app::App::new(vec![repo], "test".to_string(), width, 40);
    app.set_detail_selected(selected);
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
    terminal
        .draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let title = HistoryView::Info.label().to_uppercase();
    (0..24)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .find_map(|row| {
            row.trim_matches('│')
                .trim_start()
                .strip_prefix(&title)
                .map(|rest| rest.trim_end().to_string())
        })
        .expect("the view title row is drawn")
}

/// A repo whose two history lists agree, which is the ordinary case: every
/// local branch is merged into HEAD, so `git log HEAD` and `git log
/// <every branch>` return the same commits in the same order. The readout
/// tests below are about the row's contents, not about the lists
/// diverging; `readout_names_the_row_the_cursor_is_on` covers that.
fn repo_with_commits(n: usize) -> RepoStatus {
    let mut r = cloned_repo();
    r.commits = (0..n)
        .map(|i| crate::model::Commit {
            hash: format!("{i}abc123"),
            message: format!("commit {i}"),
            author: "Gaurgle".to_string(),
            date: None,
            body: String::new(),
            insertions: 0,
            deletions: 0,
            files: vec![],
        })
        .collect();
    r.graph = r
        .commits
        .iter()
        .map(|c| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: c.hash.clone(),
            is_merge: false,
            refs: vec![],
            author: c.author.clone(),
            message: c.message.clone(),
        })
        .collect();
    r
}

#[test]
fn readout_names_the_selected_commit_and_its_position() {
    // The point of the row: with the commit list scrolled off the top,
    // the hash alone says which commit but not where in the list it sits.
    // Counted against what the view actually lists: `info` shows the
    // newest few, so ten loaded commits still navigate as six.
    let row = readout_row(repo_with_commits(10), 3, 60);
    assert!(
        row.contains(&format!("4/{RECENT_COMMIT_COUNT}")),
        "ordinal missing: {row:?}"
    );
    assert!(row.contains("3abc123"), "hash missing: {row:?}");
}

/// FZ-95: the header's two sections depend on the pane's width alone,
/// so the divider stands still between repos and windows: the row is
/// split in the middle at every width, and below the local floor there
/// are no sections at all (his look, 2026-11-05: "static in size, only
/// depending on window width"; the window moved to pane 1's border on
/// 2026-11-09, so no label length enters them).
#[test]
fn the_sections_depend_on_the_pane_width_alone() {
    let mut last = 0;
    for width in [42, 44, 60, 81, 120, 140, 160] {
        let s = section_widths(width).unwrap_or_else(|| panic!("sections at {width}"));
        assert_eq!(s.local + 3 + s.remote, width, "rows fill the pane: {s:?}");
        assert!(
            s.local >= s.remote && s.local - s.remote <= 1,
            "the divider in the middle at {width}: {s:?}"
        );
        assert!(s.local > last, "they grow with the pane: {s:?}");
        last = s.local;
    }
    // The local half one column under its floor: plain rows.
    assert_eq!(section_widths(2 * (MIN_LOCAL_SECTION - 1) + 3), None);
    assert!(section_widths(2 * MIN_LOCAL_SECTION - 1 + 3).is_some());
}

#[test]
fn readout_ends_flush_against_the_panes_right_edge() {
    // The readout sits flush against the pane's inner right edge on the
    // view's title row (FZ-93). It used to share a right-hand column with
    // the owner and the repo name, which lean left in their own section
    // now (his look, 2026-11-05). A trailing space parks it one cell
    // short of the edge, which is exactly the bug this guards.
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let width = 60;
    let mut app = crate::app::App::new(vec![repo_with_commits(4)], "test".to_string(), width, 40);
    app.set_detail_selected(1);
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
    terminal
        .draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row = |y: u16| {
        (0..width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    let title_y = (0..24u16)
        .find(|y| {
            row(*y)
                .split('│')
                .any(|column| is_title_column(column, "INFO"))
        })
        .expect("the title row is drawn");
    let end = (0..width)
        .rfind(|x| {
            let cell = buffer[(*x, title_y)].symbol();
            cell.trim() != "" && cell != "│"
        })
        .expect("the readout is drawn");
    // The border's column, then the frame's one column of padding.
    assert_eq!(end, width - 3, "not flush right: {:?}", row(title_y));
    assert!(row(title_y).contains("[2/4 · "), "{:?}", row(title_y));
}

/// The regression: `info` draws the graph but the readout and the
/// inspector used to index `commits`, and the two are different `git log`
/// runs -- `commits` walks HEAD, the graph walks the union of the local
/// branches. One commit living only on a side branch therefore sat in the
/// drawn list and not in the indexed one, and every row below it reported
/// its neighbour's hash: the caret was on `36fc171d` while the readout and
/// the inspector below both said `0a7d35e6`.
#[test]
fn readout_names_the_row_the_cursor_is_on() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let width = 72;
    let mut r = repo_with_commits(4);
    // Reachable from a side branch but not from HEAD, so it is in the
    // graph and not in `commits`, exactly as `git log` reports the two.
    r.graph.insert(
        1,
        crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: "sidebr1".to_string(),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: "docs: on a branch".to_string(),
        },
    );

    let mut app = crate::app::App::new(vec![r], "test".to_string(), width, 40);
    app.history_view = HistoryView::Info;
    app.set_detail_selected(2);
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
    terminal
        .draw(|f| render_detail_pane(f, f.area(), &app))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let rows: Vec<String> = (0..24u16)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect();

    let caret_row = rows
        .iter()
        .find(|row| row.contains("\u{25b8} "))
        .expect("the cursor row is drawn");
    // The readout rides the view's title row (FZ-93).
    let readout = rows
        .iter()
        .find(|row| row.split('│').any(|column| is_title_column(column, "INFO")))
        .expect("the title row is drawn");
    // Which hash the caret is on, taken from the drawn row rather than
    // from the model: the whole failure was the two disagreeing.
    let hash = caret_row
        .split('\u{25b8}')
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("the cursor row names a hash");
    // Third graph row: the side-branch commit displaced it by one, which
    // is precisely what `commits` does not know about.
    assert_eq!(hash, "1abc123", "the caret sits on the third graph row");
    // 5 rows, not 4: the readout counts the list it draws, side branch
    // included, so the ordinal and the total come from the same place.
    assert!(
        readout.contains(&format!("3/5 \u{b7} {hash}")),
        "readout {readout:?} names a different commit than the caret row {caret_row:?}"
    );
}

#[test]
fn readout_is_blank_for_a_repo_with_no_commits() {
    // Blank, not collapsed: the body below must not shift up a row.
    let row = readout_row(cloned_repo(), 0, 60);
    assert_eq!(row, "");
}

#[test]
fn readout_survives_a_selection_past_the_end() {
    // detail_selected is reset when the repo changes, but a stale index
    // must clamp rather than panic on the unwrap path.
    let row = readout_row(repo_with_commits(2), 99, 60);
    assert!(row.contains("2/2"), "should clamp to the last: {row:?}");
}

#[test]
/// Local on the left of every header row, remote in its own section,
/// right-aligned against the pane's edge (FZ-95, on his look of
/// 2026-11-05, two sections since 2026-11-09; from the first divider
/// in FZ-93, flush right before). The path used to share row one with
/// the sync status, which read as "how it is doing" and "where it
/// lives" run together.
fn owner_and_url_lean_right_in_the_remote_section() {
    let (state, location, _) = header_rows(cloned_repo(), 120);
    // `header_rows` trims the rows' ends: a row ending at the pane's
    // last column before its padding is this long.
    let edge = 120 - 3;

    // Row one is state only, the owner ending the remote section.
    let (local, remote) = sections(&state);
    assert!(!local.contains("~/repos"), "path left row one: {state:?}");
    assert!(
        remote.ends_with(" Gaurgle") && state.chars().count() == edge,
        "owner not against the pane's edge: {state:?}"
    );

    // Row two is location: local path left, `owner/name` against the
    // edge, the URL's prefix dropped.
    let (local, remote) = sections(&location);
    assert!(
        local.trim_start().starts_with("~/repos/fleetz"),
        "local path not on the left: {location:?}"
    );
    assert!(
        remote.ends_with(" Gaurgle/fleetz") && location.chars().count() == edge,
        "`owner/name` not against the pane's edge: {location:?}"
    );
}

/// Pane 2's three header rows as drawn at `width`, borders kept, for an
/// app `setup` prepares.
fn header_block(width: u16, setup: impl FnOnce(&mut crate::app::App)) -> Vec<String> {
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

/// The columns of a row's section dividers: every `│` but the frame's.
fn divider_columns(row: &str) -> Vec<usize> {
    let last = row.chars().count().saturating_sub(1);
    row.chars()
        .enumerate()
        .filter(|(x, c)| *c == '│' && *x != 0 && *x != last)
        .map(|(x, _)| x)
        .collect()
}

/// FZ-95: the header is two sections, local and remote, with one
/// divider in the same column on every row, in the middle of the pane;
/// the window is on pane 1's border (2026-11-09), not here.
#[test]
fn the_header_draws_local_and_remote_with_one_divider() {
    let rows = header_block(140, |_| {});
    let dividers = divider_columns(&rows[0]);
    assert_eq!(dividers.len(), 1, "one divider: {:?}", rows[0]);
    // This repo's block is two rows: state and location.
    for row in &rows[..2] {
        assert_eq!(divider_columns(row), dividers, "row {row:?}");
    }
    for row in &rows {
        assert!(!row.contains('◷') && !row.contains("[t]"), "{row:?}");
    }
    let at = |row: &str, needle: &str| {
        row.find(needle)
            .map(|byte| row[..byte].chars().count())
            .unwrap_or_else(|| panic!("{needle} not on {row:?}"))
    };
    assert!(at(&rows[0], "synced") < dividers[0], "{:?}", rows[0]);
    assert!(dividers[0] < at(&rows[0], "Gaurgle"), "{:?}", rows[0]);
    assert!(rows[1].contains("~/repos/fleetz"), "{:?}", rows[1]);
    assert!(rows[1].contains("Gaurgle/fleetz"), "{:?}", rows[1]);
    // The divider halves the space between the frame's padding columns.
    let inner_right = rows[0].chars().count() - 3;
    assert!(
        (dividers[0] - 2).abs_diff(inner_right - dividers[0]) <= 2,
        "not in the middle: {dividers:?} in {:?}",
        rows[0]
    );
}

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
    let message = |app: &mut crate::app::App| app.set_status("copied 1abc123".to_string(), false);
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

/// The whole frame at `w` by `h`: every row as drawn, and the footer's
/// last row, the keys row.
fn frame_rows(app: &crate::app::App, w: u16, h: u16) -> (Vec<String>, String) {
    let rows = drawn_rows(w, h, |f| crate::ui::render(f, app));
    let last = rows.last().cloned().unwrap_or_default();
    (rows, last)
}

/// The fleet summary's text as the footer would draw it.
fn summary_text(app: &crate::app::App) -> String {
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

/// The same in the `notes` layout, whose pane 1 carries the summary box
/// as every detail layout's does (FZ-95).
#[test]
fn in_the_notes_layout_the_frame_draws_the_message_once() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    app.set_status("copied 1abc123".to_string(), false);
    let (w, h) = (140u16, 44u16);
    let pane2 = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app)
        .detail
        .expect("pane 2");
    let notes = notes_pane_body(pane2, &app).expect("a tree");
    assert!(notes.facts.is_some(), "the header pins at {h} rows");
    let (rows, last) = frame_rows(&app, w, h);
    assert!(
        rows.iter().any(|r| r.contains("copied 1abc123")),
        "pane 1 drew no message:\n{}",
        rows.join("\n")
    );
    assert!(!last.contains("copied"), "the footer repeats it: {last:?}");
    let summary = summary_text(&app);
    assert!(
        summary.is_empty() || !last.contains(&summary),
        "the footer repeats it: {last:?}"
    );
}

/// A header row's local and remote sections: the text before its first
/// divider, and between the first and the second (FZ-95).
fn sections(row: &str) -> (String, String) {
    let mut parts = row.split('│');
    let local = parts.next().unwrap_or_default().to_string();
    let remote = parts.next().unwrap_or_default().to_string();
    (local, remote)
}

#[test]
fn stacked_identity_shares_its_dividers_on_both_rows() {
    // Both rows put the divider in the same column and end the remote
    // half at the pane's edge, which is the whole point of stacking
    // them (FZ-95, two sections since 2026-11-09; from the first
    // divider in FZ-93, at the pane's edge before). The halves differ in
    // length, so this only holds if each row is padded to its sections
    // independently.
    let (status, url) = pane_rows(cloned_repo(), 120);
    let columns = |row: &str| -> Vec<usize> {
        row.chars()
            .enumerate()
            .filter(|(_, c)| *c == '│')
            .map(|(x, _)| x)
            .collect()
    };
    assert_eq!(columns(&status).len(), 1, "one divider: {status:?}");
    assert_eq!(
        columns(&status),
        columns(&url),
        "rows divide at different columns: {status:?} vs {url:?}"
    );
    // The rows are trimmed at the end, so equal lengths mean both remote
    // halves end in the same column, the pane's last before its padding.
    assert_eq!(
        status.chars().count(),
        url.chars().count(),
        "the remote halves end in different columns: {status:?} vs {url:?}"
    );
    assert_eq!(status.chars().count(), 120 - 3, "{status:?}");
}

#[test]
/// On a narrow pane the local half always survives: it is the half you
/// cannot look up anywhere else. Since FZ-95 the remote section has a
/// fixed width, so a remote longer than it is cut with `…` at the
/// pane's edge, never run past it (review 1, N1); where the sections
/// give way and the plain rows have no room, it is dropped whole.
fn identity_is_dropped_whole_on_a_narrow_pane() {
    let full = "Gaurgle/a-much-longer-repository-name";
    let long = || {
        let mut r = cloned_repo();
        r.remote_url = Some(format!("git@github.com:{full}.git"));
        r
    };
    let mut cut_seen = false;
    for width in [24u16, 30, 34, 48, 60, 80, 120] {
        let (_, location, _) = header_rows(long(), width);
        assert!(
            location.contains("~/repos/fleetz"),
            "w{width}: local path lost: {location:?}"
        );
        let app = crate::app::App::new(Vec::new(), "test".to_string(), width, 40);
        let sections = header_sections(&app, usize::from(width - 4)).filter(|s| s.remote > 0);
        let Some(s) = sections else {
            assert!(
                !location.contains("Gaurgle/"),
                "w{width}: remote should have been dropped: {location:?}"
            );
            continue;
        };
        // The row as drawn starts at the pane's padding column.
        let chars: Vec<char> = location.chars().collect();
        let start = 1 + s.local + SECTION_DIVIDER.chars().count();
        let remote: String = chars[start..start + s.remote].iter().collect();
        // `header_rows` trims the row's end, so the remote section's
        // last column is the row's last.
        assert_eq!(
            chars.len(),
            start + s.remote,
            "w{width}: the remote section moved: {location:?}"
        );
        let shown = remote.trim_start();
        if shown != full {
            let kept = shown.strip_suffix('…').expect("cut with …");
            assert!(full.starts_with(kept), "w{width}: {shown:?}");
            cut_seen = true;
        }
    }
    assert!(cut_seen, "no width cut the remote");
    // And at a width with no room for it, it really is gone.
    let (_, location, _) = header_rows(long(), 34);
    assert!(
        !location.contains("Gaurgle/"),
        "remote should have been dropped: {location:?}"
    );
}

/// FZ-95 review 1, B3 and N2: every GitHub form reads `owner/name`, any
/// other host keeps its host, a credential never shows, and a remote
/// that is not a host and exactly `owner/name` keeps its redacted URL.
#[test]
fn remote_display_names_owner_and_name_in_every_form() {
    for (url, want) in [
        ("https://github.com/o/r.git", "o/r"),
        ("https://www.github.com/o/r", "o/r"),
        ("git@github.com:o/r.git", "o/r"),
        ("ssh://git@github.com/o/r", "o/r"),
        ("ssh://git@github.com:22/o/r.git", "o/r"),
        ("https://gitlab.com/o/r", "gitlab.com/o/r"),
        ("git@gitlab.com:o/r.git", "gitlab.com/o/r"),
        ("https://user:tok3n@github.com/o/r.git", "o/r"),
        ("https://tok3n@www.github.com/o/r", "o/r"),
        ("ssh://git:tok3n@github.com/o/r", "o/r"),
        ("https://user:tok3n@gitlab.com/o/r.git", "gitlab.com/o/r"),
    ] {
        assert_eq!(remote_display(url), want, "{url}");
    }
    for (url, want) in [
        ("file:///srv/git/o/r.git", "file:///srv/git/o/r.git"),
        (
            "https://gitlab.com/group/sub/r.git",
            "https://gitlab.com/group/sub/r.git",
        ),
        ("myalias:o/r.git", "myalias:o/r.git"),
        (
            "https://user:tok3n@gitlab.com/group/sub/r",
            "https://gitlab.com/group/sub/r",
        ),
        (
            "https://user:tok3n@github.com/o/r/tree/main",
            "https://github.com/o/r/tree/main",
        ),
    ] {
        assert_eq!(remote_display(url), want, "{url}");
        assert!(!remote_display(url).contains("tok3n"), "{url}");
    }
}

/// Pane 2's rows from the top padding (row 1) down, as drawn at
/// `width`, for `repo`.
fn pane2_rows(
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

fn failing(mut r: RepoStatus) -> RepoStatus {
    let at = chrono::Local::now();
    let failure = crate::git::FetchFailure {
        kind: crate::git::FetchFailureKind::Unreachable,
        detail: Some("fatal: unable to access 'https://host.invalid/o/r.git/'".to_string()),
    };
    r.fetch
        .record(&crate::git::FetchOutcome::Failed(failure), at);
    r
}

/// FZ-95 review 1, B3: at one width the dividers stand in the same
/// columns for two repos of different path lengths, with and without a
/// failed fetch; every row of the block, the subtitles' included, draws
/// it; the banner under it draws none.
#[test]
fn the_dividers_stand_still_across_repos_and_failures() {
    let short = cloned_repo();
    let mut deep = cloned_repo();
    deep.name = "project".to_string();
    // Longer than the local section at 140, so only the cut keeps it in.
    deep.path = Some(
        "~/repos/some/very/deep/path/to/the/project/checkout/that/runs/past/its/section"
            .to_string(),
    );
    deep.remote_url = Some("git@github.com:Gaurgle/project.git".to_string());
    let mut columns: Vec<Vec<usize>> = Vec::new();
    for repo in [short.clone(), deep.clone(), failing(short), failing(deep)] {
        let failed = repo.fetch.failure().is_some();
        let rows = pane2_rows(repo, 140, |_| {});
        // The subtitles' row and the block's two (three while the window
        // box stood beside them).
        for row in &rows[..3] {
            columns.push(divider_columns(row));
        }
        let banner = &rows[3];
        if failed {
            assert!(banner.contains("fetch failed"), "{rows:#?}");
        }
        assert!(
            divider_columns(banner).is_empty(),
            "a divider on {banner:?}"
        );
    }
    assert_eq!(columns[0].len(), 1, "{columns:?}");
    assert!(columns.iter().all(|c| *c == columns[0]), "{columns:?}");
}

/// FZ-95, Andreas 2026-11-06: a small subtitle over each section,
/// `local` and `remote` on the pane's top padding; no `window`, which
/// is on pane 1's border since 2026-11-09.
#[test]
fn each_header_section_carries_its_subtitle() {
    let rows = pane2_rows(cloned_repo(), 140, |_| {});
    let dividers = divider_columns(&rows[0]);
    assert_eq!(dividers.len(), 1, "{:?}", rows[0]);
    let cols: Vec<char> = rows[0].chars().collect();
    let between = |a: usize, b: usize| cols[a..b].iter().collect::<String>();
    assert_eq!(between(1, dividers[0]).trim(), "local", "{:?}", rows[0]);
    let remote = between(dividers[0] + 1, cols.len() - 1);
    assert_eq!(remote.trim(), "remote");
    assert!(remote.ends_with("remote "), "not flush right: {remote:?}");
    assert!(!rows[1].contains("window"), "{:?}", rows[1]);
    // Not while the header scrolls with a short pane's body.
    let short = drawn_rows(140, 9, |f| {
        let app = crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), 140, 40);
        render_detail_pane(f, f.area(), &app)
    });
    assert!(!short[1].contains("local"), "{short:#?}");
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

/// FZ-95: on a narrow pane the header keeps local and remote, and the
/// window stays off it (it is on pane 1's border).
#[test]
fn a_narrow_header_keeps_its_sections() {
    let s = section_widths(60).expect("sections at 60");
    assert!(s.remote >= MIN_LOCAL_SECTION - 1, "{s:?}");
    let rows = pane2_rows(cloned_repo(), 64, |_| {});
    assert!(rows[2].contains("~/repos/fleetz"), "{rows:#?}");
    assert!(rows[2].contains("Gaurgle/fleetz"), "{rows:#?}");
    assert!(!rows.iter().any(|r| r.contains('◷')), "{rows:#?}");
}

/// FZ-95: the long notes join on one banner row, cut at the pane's
/// width.
#[test]
fn the_banner_is_one_row_cut_with_an_ellipsis() {
    assert_eq!(banner_line(&[], 40), None);
    let notes = vec!["⚠ fetch failed: timed out".to_string(), "x".repeat(60)];
    let line = banner_line(&notes, 40).expect("a banner");
    let text = line.to_string();
    assert_eq!(text.chars().count(), 40, "{text:?}");
    assert!(
        text.starts_with("  ⚠ fetch failed: timed out · x"),
        "{text:?}"
    );
    assert!(text.ends_with('…'), "{text:?}");
    assert!(line.spans.iter().all(|s| s.style.fg == Some(colors::PEACH)));
}

/// FZ-95 (Andreas, 2026-11-09): the window on pane 1's top border,
/// `[1] repos · ◷ since 2 weeks [t]`, and pane 2's header two sections
/// split in the middle, one divider.
#[test]
fn the_window_is_on_pane_ones_border_and_pane_two_has_two_sections() {
    let mut app = crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), 160, 30);
    app.view = crate::app::View::Detail;
    let (rows, _) = frame_rows(&app, 160, 30);
    assert!(
        rows[1].contains("[1] repos · ◷ since 2 weeks [t]"),
        "{:?}",
        rows[1]
    );
    let pane2 = rows
        .iter()
        .position(|r| r.contains("Gaurgle/fleetz"))
        .expect("pane 2's location row");
    // Pane 2's own section divider, once, in the middle of its row.
    let detail = crate::ui::panel_rects(Rect::new(0, 0, 160, 30), &app)
        .detail
        .expect("pane 2");
    let s2 = section_widths(usize::from(detail.width - 4)).expect("sections");
    assert!(s2.local.abs_diff(s2.remote) <= 1, "{s2:?}");
    assert_eq!(
        pane_two_dividers(&rows[pane2], detail),
        vec![usize::from(detail.x) + 2 + s2.local + 1],
        "{:?}",
        rows[pane2]
    );
}

/// The columns of the `│` inside pane 2's frame on a drawn frame row.
fn pane_two_dividers(row: &str, detail: Rect) -> Vec<usize> {
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
    let mut app = crate::app::App::new(vec![cloned_repo()], "since 2 weeks".to_string(), 120, 50);
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

/// Regression class this pane has been bitten by once already: a row that
/// budgets its width against something other than what it draws wraps, and
/// a wrapped row silently shifts every row beneath it.
///
/// The graph column is where that can happen now, because it is padded to a
/// cap rather than to the widest row. A history deep enough to blow past
/// the cap has to give up subject width, not spill.
#[test]
fn a_history_deeper_than_the_cap_still_fits_the_pane() {
    // Every commit a merge whose second parent is never listed, so lanes
    // pile up and none is ever reclaimed.
    let hashes: Vec<String> = (0..24).map(|i| format!("h{i:06}")).collect();
    let parents: Vec<Vec<String>> = (0..24)
        .map(|i| {
            let mut p: Vec<String> = hashes.get(i + 1).cloned().into_iter().collect();
            p.push(format!("never{i:03}"));
            p
        })
        .collect();
    let commits: Vec<crate::graph::Commit<'_>> = hashes
        .iter()
        .zip(&parents)
        .map(|(hash, parents)| crate::graph::Commit { hash, parents })
        .collect();

    let mut r = busy_repo();
    r.graph = crate::graph::layout(&commits)
        .into_iter()
        .zip(&hashes)
        .map(|(lanes, hash)| crate::model::GraphLine {
            lanes,
            hash: hash.clone(),
            is_merge: true,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: "a subject long enough to want every column it can get".to_string(),
        })
        .collect();

    let deepest = r.graph.last().map(|g| g.lanes.len()).unwrap_or(0);
    assert!(deepest > 12, "fixture is not deep enough: {deepest} lanes");

    for width in [72, 100, 140] {
        let mut selected_row = None;
        for line in graph_list_lines(&r, width, 0, None, &mut selected_row) {
            assert!(
                line.width() <= width,
                "a {}-lane row ran {} past a {width}-column pane: {:?}",
                deepest,
                line.width() - width,
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            );
        }
    }
}

/// `git log --graph` writes `*` for every commit, and the node used to take
/// the lane's hue, so the same commit changed colour depending on which
/// column its branch happened to occupy. The compact graph already paints
/// nodes in the plain text colour (`graph::Cell::is_node`): a commit belongs
/// to the history, not to whichever lane it sits in.
#[test]
fn graph_nodes_are_not_lane_coloured() {
    let spans = graph_prefix_spans_raw("| | * |");
    let colour_of = |i: usize| spans[i].style.fg.expect("every span is styled");

    assert_eq!(colour_of(4), colors::TEXT, "the node took a lane colour");
    assert_eq!(
        colour_of(0),
        colors::graph_lane_color(0),
        "pipes must keep their lane hue"
    );
    assert_ne!(
        colour_of(0),
        colour_of(6),
        "two pipes in different lanes must stay distinguishable"
    );
}

/// A node is a node in every column, which is the whole point: the old rule
/// made column 0 and column 4 different colours for the same kind of thing.
#[test]
fn a_node_is_the_same_colour_in_every_column() {
    let first = graph_prefix_spans_raw("* ");
    let fifth = graph_prefix_spans_raw("| | * ");
    assert_eq!(
        first[0].style.fg.unwrap(),
        fifth[4].style.fg.unwrap(),
        "node colour still depends on the column"
    );
}

/// git writes two columns per lane (glyph and horizontal run), so the
/// palette must be indexed by lane not by column. If this reverts to raw
/// column indices, adjacent lanes land two palette entries apart and six
/// lanes only use three of six colours. This guards against that regression.
#[test]
fn pipes_are_coloured_by_lane_not_by_column() {
    let spans = graph_prefix_spans_raw("| | | | ");
    let colour_of = |i: usize| spans[i].style.fg.expect("every span is styled");

    // Columns 0, 2, 4, 6 are lanes 0, 1, 2, 3 respectively. Each should
    // get a different palette entry if indexed by lane (i / 2), or land on
    // the same entry if mistakenly indexed by raw column (i).
    assert_eq!(
        colour_of(0),
        colors::graph_lane_color(0),
        "column 0 is lane 0"
    );
    assert_eq!(
        colour_of(2),
        colors::graph_lane_color(1),
        "column 2 is lane 1"
    );
    assert_eq!(
        colour_of(4),
        colors::graph_lane_color(2),
        "column 4 is lane 2"
    );
    assert_eq!(
        colour_of(6),
        colors::graph_lane_color(3),
        "column 6 is lane 3"
    );

    // Adjacent lanes must use different palette entries.
    assert_ne!(
        colour_of(0),
        colour_of(2),
        "lanes 0 and 1 must be distinguishable"
    );
}

/// The graph view truncated every line to the pane width, so a long
/// subject or a long decoration list was simply unreadable. Wrapping
/// happens in the builder rather than in the `Paragraph` so one source
/// line stays one rendered row, which the scroll arithmetic depends on.
#[test]
fn a_long_graph_line_wraps_instead_of_truncating() {
    let raw = "|     feat: rotate the contribution window's from date past GitHub's cache";
    let rows = expanded_graph_lines(raw, "", 40);
    assert!(rows.len() > 1, "the line was truncated, not wrapped");
    for row in &rows {
        let w: usize = row.spans.iter().map(|s| s.content.chars().count()).sum();
        assert!(w <= 40, "a wrapped row is {w} wide, past the pane's 40");
    }
}

/// Every row carries the lane prefix. A gap in the pipes reads as a
/// topology change that did not happen.
#[test]
fn wrapped_rows_repeat_the_lane_prefix() {
    let raw = "| | |     a subject long enough to need more than one row here";
    let rows = expanded_graph_lines(raw, "", 30);
    assert!(rows.len() > 1, "fixture did not wrap");
    for (i, row) in rows.iter().enumerate() {
        let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.starts_with("  | | |"),
            "row {i} lost the lane prefix: {text:?}"
        );
    }
}

/// "Dynamic" means it re-wraps to whatever the pane is now, so a narrower
/// pane must produce more rows from the same input.
#[test]
fn a_narrower_pane_produces_more_rows() {
    let raw = "|     a subject long enough to wrap at several different widths";
    let wide = expanded_graph_lines(raw, "", 60).len();
    let narrow = expanded_graph_lines(raw, "", 30).len();
    assert!(
        narrow > wide,
        "wrapping did not follow the width: {narrow} vs {wide}"
    );
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

/// The staleness bug: git rendered "3 minutes ago" once, the expanded
/// graph was only refetched when the repo moved, and an idle repo kept
/// showing it for hours while the commit list beside it said "16h ago".
#[test]
fn the_graph_block_date_is_aged_at_draw_time() {
    let then = chrono::Utc::now() - chrono::Duration::hours(16);
    let line = format!("Date:   {}", then.to_rfc3339());
    assert_eq!(
        refresh_date_line(&line).as_deref(),
        Some("Date:   16h ago"),
        "the frozen relative date survived to the screen"
    );
}

/// Only the `Date:` row is rewritten. Everything else is git's output and
/// is reproduced verbatim, which is the whole point of this view.
#[test]
fn other_graph_rows_are_left_exactly_as_git_wrote_them() {
    assert_eq!(refresh_date_line("Author: someone <a@b.c>"), None);
    assert_eq!(refresh_date_line("commit 1c51c6e"), None);
    assert_eq!(refresh_date_line("    a subject line"), None);
    assert_eq!(
        refresh_date_line("Date:   Mon Aug 25 09:12:33 2026 +0200"),
        None,
        "an unparseable stamp must pass through, not vanish"
    );
}

/// End to end: the rewritten row reaches the drawn line.
#[test]
fn the_drawn_graph_block_shows_a_current_age() {
    let then = chrono::Utc::now() - chrono::Duration::days(3);
    let raw = format!("| | Date:   {}", then.to_rfc3339());
    let rows = expanded_graph_lines(&raw, "", 80);
    let text: String = rows[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    assert!(text.contains("3d ago"), "drew {text:?}");
}

/// Phantom blocks: a substring test for "commit " matched message bodies,
/// so this repo's own log reported thirty block starts for twenty-five
/// commits and the marker indexed past the commit it was on.
#[test]
fn only_gits_commit_row_opens_a_block() {
    assert_eq!(commit_hash_at("* commit 7b1f067"), Some("7b1f067"));
    assert_eq!(
        commit_hash_at("| | commit 7b1f067 (HEAD -> main, origin/main)"),
        Some("7b1f067")
    );
    assert_eq!(
        commit_hash_at("|     notch jumped a whole commit block, which is"),
        None,
        "a message body opened a phantom block"
    );
    assert_eq!(
        commit_hash_at("|     is the same symptom. That commit had the mechanism"),
        None
    );
    assert_eq!(
        commit_hash_at("|     commit blocks are tall"),
        None,
        "a body starting with the word is still not a commit row"
    );
    assert_eq!(commit_hash_at("| Author: Gaurgle <a@b.c>"), None);
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

#[test]
fn fresh_app_inspector_opens_at_its_default_with_a_row_floor() {
    use ratatui::layout::Rect;
    let rects = |h: u16| {
        let mut app = crate::app::App::new(vec![list_repo(3)], "h".to_string(), 120, h);
        app.view = crate::app::View::Detail;
        app.contributions_folded = true;
        assert_eq!(app.inspector_pct, 25);
        crate::ui::panel_rects(Rect::new(0, 0, 120, h), &app)
    };
    // Tall terminal: 25 percent of the body, above the floor.
    let r = rects(80);
    let tall = r
        .inspector
        .expect("tall terminal docks an inspector")
        .height;
    assert!(tall > INSPECTOR_MIN_HEIGHT, "not above the floor: {tall}");
    assert_eq!(tall, r.body.height * 25 / 100, "not 25 percent: {tall}");
    // Short terminal: 25 percent is under the floor, so the floor holds.
    let r = rects(22);
    assert!(r.body.height * 25 / 100 < INSPECTOR_MIN_HEIGHT);
    let short = r
        .inspector
        .expect("short terminal keeps an inspector")
        .height;
    assert_eq!(short, INSPECTOR_MIN_HEIGHT, "floor lost: {short}");
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
fn list_repo(n: usize) -> RepoStatus {
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

/// FZ-56: the repo list drawn for a ranked owner and author search, as
/// screen rows, with the app that drew it.
fn draw_people_search(query: &str) -> (crate::app::App, Vec<String>) {
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

/// A local-only repo has no `pushedAt` to date it by -- that is what puts
/// it in "local changes" in the first place -- so `info` is the only
/// place its age is legible. It is also the number the time window uses
/// to decide whether the repo keeps its row, so seeing it explains an
/// eviction rather than leaving it a mystery.
#[test]
fn info_dates_a_local_only_repo() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let draw = |last_local: Option<chrono::DateTime<chrono::Utc>>| {
        let mut r = busy_repo();
        r.location = RepoLocation::LocalOnly;
        r.last_local = last_local;
        let mut app = crate::app::App::new(vec![r], "t".to_string(), 96, 44);
        app.history_view = HistoryView::Info;
        let mut term = Terminal::new(TestBackend::new(96, 60)).unwrap();
        term.draw(|f| render_detail_pane(f, f.area(), &app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..60u16)
            .map(|y| (0..96u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };

    let at = chrono::Utc::now() - chrono::Duration::days(95);
    let shown = draw(Some(at));
    assert!(shown.contains("last touched"), "no row drawn: {shown}");
    assert!(
        shown.contains(&at.format("%Y-%m-%d").to_string()),
        "the date itself must be there, not just an age: {shown}"
    );
    assert!(
        shown.contains("3mo ago"),
        "and the age beside it, as the commit rows read: {shown}"
    );

    // A repo git could not date at all says nothing rather than lying
    // about it with a zero or an epoch.
    assert!(!draw(None).contains("last touched"));
}

// --- FZ-09d: GitHub values from an earlier refresh (O4) ---

use ratatui::style::Color as TermColor;

/// Two `Pending` rows with two PRs each, `stale` and `fresh`, under
/// watch, with `stale` not in the last poll.
fn app_with_a_stale_row() -> crate::app::App {
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

// --- FZ-66: the cursor's row in pane 1 wears a band ---

/// Two repos with two PRs each, so every row carries a badge group.
fn two_repo_app() -> crate::app::App {
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

/// The detail pane of the selected repo, as text, and the colour of its
/// CI lamp.
fn detail_of(app: &crate::app::App) -> (String, TermColor) {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (w, h) = (100u16, 24u16);
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
    let buf = term.backend().buffer().clone();
    let text = (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let lamp = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == "◉")
        .map(|(x, y)| buf[(x, y)].fg)
        .expect("lamp drawn");
    (text, lamp)
}

#[test]
fn the_detail_pane_says_a_repo_not_in_the_last_poll_is_from_an_earlier_refresh() {
    let mut app = app_with_a_stale_row();
    app.blink_on = true;
    // The first row, `stale`, is selected.
    let (text, lamp) = detail_of(&app);
    assert!(text.contains(EARLIER_REFRESH_NOTE), "{text}");
    assert_eq!(lamp, badges::EARLIER_REFRESH);

    app.not_in_last_poll.clear();
    let (text, lamp) = detail_of(&app);
    assert!(!text.contains(EARLIER_REFRESH_NOTE), "{text}");
    assert_eq!(lamp, colors::PEACH);
}

/// FZ-37 review: a hit carries the entry's own list index and the hash
/// as drawn, so a top shed into the pinned header cannot shift the index
/// and a later list change cannot change what a click copies.
#[test]
fn a_hit_carries_the_drawn_hash_and_the_entry_index() {
    let lines = vec![
        Line::from("  aaa1111 first"),
        Line::from("  bbb2222 second"),
    ];
    // Entry 0 sat in the pinned header, so it has no body line.
    let tops = [None, Some(0), Some(1)];
    let hash_of = |i: usize| ["zzz9999", "aaa1111", "bbb2222"].get(i).copied();
    let body = Rect::new(10, 5, 40, 10);
    let hits = DetailHits::measure(&lines, &tops, hash_of, body, 0, false);

    let on_hash = hits.at(12, 6).expect("a row");
    assert_eq!(on_hash.entry, Some(2));
    assert_eq!(on_hash.hash.as_deref(), Some("bbb2222"));
    assert_eq!(on_hash.line, 1);

    let off_hash = hits.at(12 + 8, 6).expect("a row");
    assert_eq!((off_hash.entry, off_hash.hash), (Some(2), None));

    assert_eq!(hits.at(12, 7), None, "below the last line");
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

/// A repo whose commits, reflog and local branches each run to `n`
/// one-row entries, so every one of those views overflows the pane.
fn long_lists_repo(n: usize) -> RepoStatus {
    let mut r = busy_repo();
    r.graph = (0..n)
        .map(|i| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: format!("h{i:06}"),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("subject {i}"),
        })
        .collect();
    r.reflog = (0..n)
        .map(|i| crate::model::ReflogEntry {
            hash: format!("h{i:06}"),
            action: "commit".to_string(),
            description: format!("subject {i}"),
        })
        .collect();
    r.branches = (0..n)
        .map(|i| crate::model::BranchStatus {
            name: format!("branch-{i:02}"),
            upstream: Some(format!("origin/branch-{i:02}")),
            behind: 0,
            ahead: 0,
            is_head: i == 0,
            tip: String::new(),
        })
        .collect();
    r
}

/// Draws pane 2 `height` rows tall and returns the cursor's rows of
/// margin above and below it on screen, and the rows of content past it
/// either way, as the renderer measured them.
fn detail_cursor_margins(app: &crate::app::App, height: u16) -> (u16, u16, u16, u16) {
    let body = draw_detail(app, height);
    let top = app.detail_row_tops.borrow()[app.detail_selected()];
    let pos = top - app.detail_scroll();
    let content = app.detail_max_scroll.get() + body;
    (pos, body - 1 - pos, top, content - 1 - top)
}

/// Draws pane 2 `height` rows tall and returns its scrolling body's
/// height, as the renderer measured it.
fn draw_detail(app: &crate::app::App, height: u16) -> u16 {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut term = Terminal::new(TestBackend::new(96, height)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
    app.detail_view_height.get()
}

/// Steps pane 2's cursor down `steps` entries and back with `j` and `k`,
/// checking the margin after every draw.
fn walk_detail(view: HistoryView, height: u16, steps: usize) {
    let mut app = crate::app::App::new(vec![long_lists_repo(80)], "t".to_string(), 96, 40);
    app.history_view = view;
    app.focused_panel = crate::app::FocusedPanel::Detail;
    let margin = crate::app::scrolloff_for(draw_detail(&app, height));
    let mut parked = false;
    for _ in 0..steps {
        app.detail_nav_down();
        let (_, below, _, past) = detail_cursor_margins(&app, height);
        assert!(below >= margin.min(past), "{view:?}: {below} rows below");
        parked |= app.detail_scroll() > 0 && below == margin;
    }
    assert!(parked, "{view:?}: never scrolled with the margin held");
    for _ in 0..steps {
        app.detail_nav_up();
        let (above, _, before, _) = detail_cursor_margins(&app, height);
        assert!(above >= margin.min(before), "{view:?}: {above} rows above");
    }
}

/// `commits`, the longest list, through the cursor-follow path `j` and
/// `k` take: the full `SCROLLOFF` past the cursor in a tall pane, the
/// shrunken margin in a short one.
#[test]
fn the_commit_list_keeps_the_shared_margin_past_the_cursor() {
    let mut app = crate::app::App::new(vec![long_lists_repo(80)], "t".to_string(), 96, 40);
    app.history_view = HistoryView::Commits;
    let tall = draw_detail(&app, 24);
    assert_eq!(
        crate::app::scrolloff_for(tall),
        crate::app::SCROLLOFF,
        "the pane is too short for the full margin: body {tall}"
    );
    walk_detail(HistoryView::Commits, 24, 60);

    let short = draw_detail(&app, 10);
    assert!(
        crate::app::scrolloff_for(short) < crate::app::SCROLLOFF,
        "body {short}"
    );
    walk_detail(HistoryView::Commits, 10, 60);
}

/// One margin for every view the cursor walks: each parks its cursor the
/// same `scrolloff_for` rows off the bottom, so a change to that function
/// moves all of them.
#[test]
fn every_history_view_parks_its_cursor_on_the_shared_margin() {
    for view in [
        HistoryView::Commits,
        HistoryView::Reflog,
        HistoryView::Branches,
    ] {
        let mut app = crate::app::App::new(vec![long_lists_repo(80)], "t".to_string(), 96, 40);
        app.history_view = view;
        app.focused_panel = crate::app::FocusedPanel::Detail;
        draw_detail(&app, 24);
        for _ in 0..40 {
            app.detail_nav_down();
            draw_detail(&app, 24);
        }
        let (_, below, _, _) = detail_cursor_margins(&app, 24);
        let body = app.detail_view_height.get();
        assert_eq!(below, crate::app::scrolloff_for(body), "{view:?}");
        walk_detail(view, 24, 50);
    }
}

/// Draws `info` `height` rows tall and returns the screen text.
fn info_text(app: &crate::app::App, height: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut term = Terminal::new(TestBackend::new(92, height)).unwrap();
    term.draw(|f| render_detail_pane(f, f.area(), app)).unwrap();
    let buf = term.backend().buffer().clone();
    (0..height)
        .map(|y| (0..92u16).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Short enough that the branches block scrolls with the body (nine
/// rows, a margin of four), so the remote-only summary sits low in it
/// and the names `B` opens run past the bottom edge. 17 since FZ-93 gave
/// pane 2 back the readout's row: at 18 the names fit.
const REVEAL_HEIGHT: u16 = 17;

/// The body row, counted from the pane's top edge, holding `needle`.
fn text_row_of(text: &str, needle: &str) -> Option<usize> {
    text.lines().position(|l| l.contains(needle))
}

/// `B` on a remotes list that overflows the pane scrolls it into view:
/// five names open one row past the bottom edge, and the reveal brings
/// the last onto the bottom row.
#[test]
fn expanding_the_remotes_reveals_the_rows_it_opened() {
    let mut r = spread_repo();
    r.remote_only_branches.truncate(5);
    let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 40);
    app.history_view = HistoryView::Info;
    let before = info_text(&app, REVEAL_HEIGHT);
    assert!(before.contains("5 remote branches not fetched"), "{before}");
    assert_eq!(app.detail_scroll(), 0);

    app.expand_remote_branches = true;
    let after = info_text(&app, REVEAL_HEIGHT);
    assert_eq!(app.detail_scroll(), 1, "the pane did not move:\n{after}");
    // The last text row sits above the bottom border.
    assert_eq!(
        text_row_of(&after, "spare-4 "),
        Some(usize::from(REVEAL_HEIGHT) - 2),
        "the last opened row is not on the bottom row:\n{after}"
    );
    assert!(after.contains("spare-0 "), "{after}");
}

/// Twelve names cannot all show in a nine-row body, so the reveal stops
/// with the first of them on the top margin rather than scrolling it off.
#[test]
fn a_reveal_taller_than_the_pane_stops_at_the_top_margin() {
    let mut app = crate::app::App::new(vec![spread_repo()], "t".to_string(), 92, 40);
    app.history_view = HistoryView::Info;
    info_text(&app, REVEAL_HEIGHT);
    app.expand_remote_branches = true;
    let after = info_text(&app, REVEAL_HEIGHT);
    assert!(app.detail_scroll() > 0, "the pane did not move:\n{after}");
    let body_top = usize::from(REVEAL_HEIGHT - app.detail_view_height.get() - 1);
    let margin = usize::from(crate::app::scrolloff_for(app.detail_view_height.get()));
    assert_eq!(
        text_row_of(&after, "spare-0 "),
        Some(body_top + margin),
        "the expanded row is not on the top margin:\n{after}"
    );
}

/// Without the reveal the names open below the fold, which is the bug:
/// the same draw with the expand already recorded leaves the offset at 0.
/// Also covers "only on the frame it flips": scrolling back up after the
/// reveal is left alone.
#[test]
fn the_reveal_fires_once_per_expand() {
    let mut app = crate::app::App::new(vec![spread_repo()], "t".to_string(), 92, 40);
    app.history_view = HistoryView::Info;
    info_text(&app, REVEAL_HEIGHT);
    app.expand_remote_branches = true;
    info_text(&app, REVEAL_HEIGHT);
    assert!(app.detail_scroll() > 0);

    app.set_detail_scroll(0);
    let text = info_text(&app, REVEAL_HEIGHT);
    assert_eq!(app.detail_scroll(), 0, "the reveal ran again:\n{text}");

    // Collapsing never scrolls, and expanding again reveals again.
    app.expand_remote_branches = false;
    info_text(&app, REVEAL_HEIGHT);
    assert_eq!(app.detail_scroll(), 0);
    app.expand_remote_branches = true;
    info_text(&app, REVEAL_HEIGHT);
    assert!(app.detail_scroll() > 0, "a second expand did not reveal");
}

/// An expand that opens no rows (no remote-only branches, or a view that
/// does not draw them) leaves the pane where it was.
#[test]
fn an_expand_that_opens_nothing_does_not_scroll() {
    let mut r = spread_repo();
    r.remote_only_branches.clear();
    let mut app = crate::app::App::new(vec![r], "t".to_string(), 92, 40);
    app.history_view = HistoryView::Info;
    info_text(&app, REVEAL_HEIGHT);
    app.expand_remote_branches = true;
    info_text(&app, REVEAL_HEIGHT);
    assert_eq!(app.detail_scroll(), 0);

    // `B` pressed in `commits`, then back to `info`: nothing opened on
    // the frame it flipped, so nothing is revealed later.
    let mut app = crate::app::App::new(vec![spread_repo()], "t".to_string(), 92, 40);
    app.history_view = HistoryView::Commits;
    info_text(&app, REVEAL_HEIGHT);
    app.expand_remote_branches = true;
    info_text(&app, REVEAL_HEIGHT);
    app.history_view = HistoryView::Info;
    app.set_detail_scroll(0);
    info_text(&app, REVEAL_HEIGHT);
    assert_eq!(app.detail_scroll(), 0);
}

// --- The `notes` view (FZ-53) ---

/// An app on `busy_repo` showing `notes`, with a walk of `files` landed.
fn notes_view_app(files: &[(&str, Option<&str>)]) -> crate::app::App {
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
fn notes_pane_text(app: &crate::app::App) -> String {
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

fn changed(status: &str, path: &str, churn: Option<(u32, u32)>) -> crate::git::ChangedFile {
    crate::git::ChangedFile {
        status: status.to_string(),
        path: path.to_string(),
        from: None,
        churn,
    }
}

/// The design's example (FZ-60): three uncommitted kinds, two files
/// unpushed.
fn held_changes() -> crate::git::Changes {
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
fn changes_view_app(changes: crate::git::Changes) -> crate::app::App {
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

/// FZ-60: both sections under their headings, porcelain's two status
/// columns, the churn column, the caret on the cursor's file, and the
/// base switch pinned on top.
#[test]
fn the_changes_view_draws_both_sections_with_status_and_churn() {
    let mut app = changes_view_app(held_changes());
    let text = notes_pane_text(&app);
    for want in [
        "vs: [upstream] default  (u)",
        "uncommitted (3)",
        "▸  M  src/app.rs",
        "A   docs/foo.md",
        "??  scratch/",
        "unpushed (2 ahead of origin/main)",
        "M   src/ui/detail.rs",
        "D   src/old.rs",
        "+88 -12",
    ] {
        assert!(text.contains(want), "{want:?} missing:\n{text}");
    }
    app.set_detail_selected(4);
    let text = notes_pane_text(&app);
    assert!(text.contains("▸ D   src/old.rs"), "{text}");
}

/// The one-line states: no upstream, detached, clean and pushed, the
/// default-branch section, and a not-cloned row.
#[test]
fn the_changes_view_says_why_a_section_is_missing() {
    use crate::git::{Changes, Outgoing};
    let with = |outgoing| Changes {
        uncommitted: Vec::new(),
        outgoing,
    };
    let cases = [
        (
            with(Outgoing::NoUpstream {
                branch: Some("feat".to_string()),
            }),
            "no upstream: feat tracks no remote branch",
        ),
        (
            with(Outgoing::NoUpstream { branch: None }),
            "no upstream: HEAD is detached",
        ),
        (
            with(Outgoing::Unpushed {
                upstream: "origin/main".to_string(),
                ahead: 0,
                files: Vec::new(),
            }),
            "clean and pushed to origin/main",
        ),
        (
            with(Outgoing::VsDefault {
                base: "origin/main".to_string(),
                files: vec![changed("A", "f.txt", Some((1, 0)))],
            }),
            "on branch vs origin/main (1)",
        ),
    ];
    for (changes, want) in cases {
        let text = notes_pane_text(&changes_view_app(changes));
        assert!(text.contains(want), "{want:?} missing:\n{text}");
        assert!(!text.contains("unpushed ("), "{text}");
    }
    let mut app = changes_view_app(held_changes());
    app.repos[0].path = None;
    let text = notes_pane_text(&app);
    assert!(text.contains("not cloned: nothing to read"), "{text}");
}

/// The inspector draws the highlighted file's diff with added and
/// removed lines in the churn column's green and red, a failed read's
/// error in place, and scrolls with j/k when focused.
#[test]
fn the_changes_inspector_draws_the_diff_in_churn_colors_and_scrolls() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let draw = |app: &crate::app::App| {
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| render_inspector(f, f.area(), app)).unwrap();
        term.backend().buffer().clone()
    };
    let row_of = |buf: &ratatui::buffer::Buffer, needle: &str| {
        (0..12u16).find_map(|y| {
            let row: String = (0..80u16).map(|x| buf[(x, y)].symbol()).collect();
            row.find(needle)
                .map(|x| (row[..x].chars().count() as u16, y))
        })
    };
    let mut app = changes_view_app(held_changes());
    let read = app.claim_change_diff().expect("the highlighted file");
    let diff: String = "@@ -1 +1 @@\n-old line\n+new line\n"
        .chars()
        .chain((0..30).flat_map(|i| format!(" context {i}\n").chars().collect::<Vec<_>>()))
        .collect();
    app.apply_change_diff(read.clone(), Ok(diff));
    let buf = draw(&app);
    let (x, y) = row_of(&buf, "+new line").expect("added line drawn");
    assert_eq!(buf[(x, y)].fg, colors::DIFF_ADD);
    let (x, y) = row_of(&buf, "-old line").expect("removed line drawn");
    assert_eq!(buf[(x, y)].fg, colors::DIFF_DEL);

    app.focused_panel = FocusedPanel::Inspector;
    app.inspector_nav(5);
    let buf = draw(&app);
    assert!(row_of(&buf, "+new line").is_none(), "scrolled past it");

    app.change_diff = None;
    let read = app.claim_change_diff().expect("again");
    app.apply_change_diff(read, Err("boom".to_string()));
    let buf = draw(&app);
    assert!(row_of(&buf, "could not read: boom").is_some());
}

/// The tree draws each notes directory as a section, nested directories
/// indented, and the caret on the cursor's file; the cursor walks files
/// only, so its second stop is the nested file, not the directory.
#[test]
fn the_notes_view_draws_sections_and_marks_the_file() {
    let mut app = notes_view_app(&[
        ("notez/a.md", Some("notez")),
        ("notez/sub/b.md", Some("notez")),
        ("crates/x/notez/c.md", Some("crates/x/notez")),
        ("docs/guide.md", None),
    ]);
    app.select_commit_down();
    let text = notes_pane_text(&app);
    for want in [
        "notes: [notez] markdown  (m)",
        "notez/",
        "  sub/",
        "crates/x/notez/",
    ] {
        assert!(text.contains(want), "{want} missing:\n{text}");
    }
    let glyph = badges::note_kind_icon(crate::notes::NoteKind::Markdown);
    assert!(
        text.contains(&format!("▸   {glyph} b.md")),
        "caret on the nested file:\n{text}"
    );
    assert!(!text.contains("guide.md"), "docs only under the toggle");
    assert_eq!(app.detail_row_tops.borrow().len(), 3, "one stop per file");

    app.toggle_notes_markdown();
    let text = notes_pane_text(&app);
    assert!(
        text.contains("docs/") && text.contains("guide.md"),
        "{text}"
    );
}

/// Each empty case says what it is in one line.
#[test]
fn the_notes_view_says_why_it_lists_nothing() {
    let app = notes_view_app(&[("docs/guide.md", None)]);
    assert!(notes_pane_text(&app).contains("no notez/ or .notez/ directory"));

    let mut app = notes_view_app(&[]);
    app.notes.clear();
    assert!(notes_pane_text(&app).contains("reading notes…"));

    app.repos[0].path = None;
    assert!(notes_pane_text(&app).contains("not cloned: no notes to read"));
}

/// The reader (FZ-62, formerly the inspector) reads the highlighted file:
/// its name in the facts at the tree's foot (FZ-93), then its body, or
/// the read's error in place.
#[test]
fn the_notes_inspector_shows_the_body_or_the_error() {
    let draw = |app: &crate::app::App| notes_pane_rows(120, 12, app).join("\n");
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let read = app.claim_note_read().expect("a read");
    assert!(draw(&app).contains("reading…"));

    app.apply_note_read(read.clone(), Ok("# Plan\nship it".to_string()));
    let text = draw(&app);
    for want in ["# Plan", "ship it"] {
        assert!(text.contains(want), "{want} missing:\n{text}");
    }
    let facts = foot_text(&app);
    assert!(facts.starts_with("─ a.md · markdown"), "{facts:?}");

    app.note_body.as_mut().unwrap().text = None;
    app.apply_note_read(read, Err("permission denied".to_string()));
    assert!(draw(&app).contains("could not read: permission denied"));
}

// --- The `notes` reader and facts (FZ-62, FZ-68), the mode line (FZ-57) ---

/// Pane 2 of the `notes` layout, `w` by `h`, as `ui::render` draws it:
/// the frame, the tree's column and the reader at `notes_pane_body`'s
/// rect (FZ-68).
fn notes_pane_buffer(w: u16, h: u16, app: &crate::app::App) -> ratatui::buffer::Buffer {
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

fn notes_pane_rows(w: u16, h: u16, app: &crate::app::App) -> Vec<String> {
    let buf = notes_pane_buffer(w, h, app);
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// The tree's foot (FZ-93) as `notes_foot_spans` builds it at a width
/// that holds every fact, so a test reads each one; what a narrow foot
/// keeps is `the_foot_holds_its_row_when_narrow`'s.
fn foot_text(app: &crate::app::App) -> String {
    notes_foot_spans(app, 400)
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>()
        .trim_end_matches('─')
        .to_string()
}

/// Draw `paint` into a `w` by `h` buffer and return its rows.
fn drawn_rows(w: u16, h: u16, paint: impl FnOnce(&mut Frame)) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(paint).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// The first line names both modes, the active one bracketed, and `m`
/// moves the brackets; each empty state names the mode it belongs to.
#[test]
fn the_mode_line_marks_the_active_mode_and_m_moves_it() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let text = notes_pane_text(&app);
    assert!(text.contains("notes: [notez] markdown  (m)"), "{text}");

    app.toggle_notes_markdown();
    let text = notes_pane_text(&app);
    assert!(text.contains("notes: notez [markdown]  (m)"), "{text}");

    let mut app = notes_view_app(&[]);
    assert!(notes_pane_text(&app).contains("notez: no notez/ or .notez/ directory"));
    app.toggle_notes_markdown();
    assert!(notes_pane_text(&app).contains("markdown: no markdown files"));
}

/// Each file wears its kind's glyph before its name, the Nerd Font icon
/// or, under `icons = "plain"`, its one-letter stand-in.
#[test]
fn the_tree_draws_each_files_kind_glyph_in_both_sets() {
    use crate::notes::NoteKind;
    let files = [
        ("notez/TODO.md", NoteKind::Todo),
        ("notez/diagram.png", NoteKind::Other),
        ("notez/notes.txt", NoteKind::Text),
        ("notez/plan.md", NoteKind::Markdown),
    ];
    let walk: Vec<(&str, Option<&str>)> = files.iter().map(|(f, _)| (*f, Some("notez"))).collect();
    for icons in [crate::app::Icons::Nerd, crate::app::Icons::Plain] {
        badges::set_active_icons(icons);
        let app = notes_view_app(&walk);
        let text = notes_pane_text(&app);
        for (rel, kind) in files {
            let name = rel.trim_start_matches("notez/");
            let want = format!("{} {name}", badges::note_kind_icon(kind));
            assert!(text.contains(&want), "{icons:?}: {want} missing:\n{text}");
        }
    }
    assert!(notes_pane_text(&notes_view_app(&walk)).contains("T TODO.md"));
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

/// FZ-63: the tree opens on `todos`, each board under its own name and
/// its place, and a board is not listed again in its directory. The
/// facts row at the tree's foot names the highlighted file too, so the
/// count leaves it out.
#[test]
fn the_tree_pins_the_boards_first() {
    let app = notes_view_app(&[
        ("TODO.md", None),
        ("notez/TODO.md", Some("notez")),
        ("notez/plan.md", Some("notez")),
    ]);
    let text = notes_pane_text(&app);
    let todos = text.find("todos").expect("a todos section");
    assert!(todos < text.find("plan.md").unwrap(), "{text}");
    assert!(text.contains("TODO.md  ./"), "{text}");
    assert!(text.contains("TODO.md  notez/"), "{text}");
    let tree_rows = text
        .lines()
        .filter(|l| !l.contains("─ TODO.md ·"))
        .map(|l| l.matches("TODO.md").count())
        .sum::<usize>();
    assert_eq!(tree_rows, 2, "{text}");
}

/// The facts beside the header name the file, its kind and size (FZ-68
/// round 2, the reader's border before), and its body scrolls by its
/// own offset: the tree's cursor stays where it was, and a scrolled body
/// starts further down.
#[test]
fn the_reader_titles_the_file_and_scrolls_its_body() {
    let mut app = notes_view_app(&[("notez/README.md", Some("notez"))]);
    app.focused_panel = FocusedPanel::Reader;
    let read = app.claim_note_read().expect("a read");
    let body: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
    let facts = crate::notes::NoteFacts {
        size: Some(4300),
        ..Default::default()
    };
    app.apply_note_facts(&read, facts);
    app.apply_note_read(read, Ok(body.join("\n")));

    let facts = foot_text(&app);
    assert!(
        facts.starts_with("─ README.md · markdown · 4.2 KiB"),
        "{facts:?}"
    );
    // Too short to pin the header: the text from pane 2's first inner
    // row, under its top padding, to the row above its bottom border
    // (FZ-68).
    let draw = |app: &crate::app::App| notes_pane_rows(120, 12, app);
    let rows = draw(&app);
    assert!(rows[2].contains("line 1 "), "{}", rows[2]);

    assert_eq!(app.reader_view_height.get(), 9, "one H/L page measured");
    app.scroll_reader(10);
    let rows = draw(&app);
    assert!(rows[2].contains("line 11"), "{}", rows.join("\n"));
    assert!(!rows.join("\n").contains("line 1 "), "{}", rows.join("\n"));
    assert_eq!(app.detail_selected(), 0, "the tree's cursor did not move");

    // Past the end it parks on the last page, as the inspector does.
    app.scroll_reader(1000);
    let rows = draw(&app);
    assert!(rows[10].contains("line 40"), "{}", rows.join("\n"));
}

// --- Rendered markdown in the reader (FZ-59) ---

const MARKDOWN_FIXTURE: &str = "# Plan\n\n- ship *it* now\n- run `cargo test`\n\n\
                                See [the docs](https://x.io).";

/// Draw the reader into a 120 by 12 pane 2, its titles on the pane's
/// border (FZ-68).
fn reader_buffer(app: &crate::app::App) -> ratatui::buffer::Buffer {
    notes_pane_buffer(120, 12, app)
}

fn buffer_rows(buf: &ratatui::buffer::Buffer) -> Vec<String> {
    let area = buf.area;
    (0..area.height)
        .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

/// The style of the cell where `needle` starts on screen.
fn style_at(buf: &ratatui::buffer::Buffer, needle: &str) -> Style {
    for (y, row) in buffer_rows(buf).iter().enumerate() {
        let cells: Vec<char> = row.chars().collect();
        let want: Vec<char> = needle.chars().collect();
        if let Some(x) = cells.windows(want.len()).position(|w| w == want) {
            return buf[(x as u16, y as u16)].style();
        }
    }
    panic!("{needle} not on screen:\n{}", buffer_rows(buf).join("\n"));
}

/// Land `text` as the reader's body the way the event loop does: the
/// read claimed at the width the reader last drew, rendered with it.
fn land_rendered(app: &mut crate::app::App, text: &str) {
    reader_buffer(app);
    let read = app.claim_note_read().expect("a read");
    let width = read.render_width.expect("a markdown read renders");
    let lines = crate::ui::markdown::render_markdown(text, width);
    let rendered = crate::app::RenderedNote { width, lines };
    app.apply_note_rendered(&read.path, read.generation, rendered);
    app.apply_note_read(read, Ok(text.to_string()));
}

/// FZ-59: a markdown file draws rendered by default, through the
/// `colors.rs` tokens: the heading in the title colour, bold, without
/// its `#`; list bullets; emphasis italic; code in the code colour; a
/// link's text in the link colour beside its url. The facts say
/// `rendered`; `p` shows the source unchanged and the facts say `raw`.
#[test]
fn the_reader_renders_markdown_and_r_shows_the_source() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    land_rendered(&mut app, MARKDOWN_FIXTURE);
    let buf = reader_buffer(&app);
    let text = buffer_rows(&buf).join("\n");
    let facts = foot_text(&app);
    assert!(facts.contains("a.md · markdown · rendered"), "{facts:?}");
    for want in [
        "Plan",
        "• ship it now",
        "• run cargo test",
        "See the docs (https://x.io).",
    ] {
        assert!(text.contains(want), "{want} missing:\n{text}");
    }
    assert!(!text.contains("# Plan"), "the heading's # is gone:\n{text}");
    let heading = style_at(&buf, "Plan");
    assert_eq!(heading.fg, Some(colors::MAUVE), "the title colour");
    assert!(heading.add_modifier.contains(Modifier::BOLD));
    assert!(
        style_at(&buf, "it now")
            .add_modifier
            .contains(Modifier::ITALIC)
    );
    assert_eq!(style_at(&buf, "cargo test").fg, Some(colors::PEACH));
    assert_eq!(style_at(&buf, "the docs").fg, Some(colors::SAPPHIRE));
    assert_eq!(style_at(&buf, "(https").fg, Some(colors::OVERLAY));

    app.toggle_reader_raw();
    let text = buffer_rows(&reader_buffer(&app)).join("\n");
    let facts = foot_text(&app);
    assert!(facts.contains("a.md · markdown · raw"), "{facts:?}");
    for line in MARKDOWN_FIXTURE.lines().filter(|l| !l.is_empty()) {
        assert!(text.contains(line), "source line {line:?} missing:\n{text}");
    }
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

/// FZ-59: a text file is always raw: no render is asked for, its facts
/// name no mode, and `p` does not change what it shows.
#[test]
fn a_text_file_stays_raw_whatever_the_toggle() {
    let mut app = notes_view_app(&[("notez/notes.txt", Some("notez"))]);
    reader_buffer(&app);
    let read = app.claim_note_read().expect("a read");
    assert_eq!(read.render_width, None, "no render for text");
    app.apply_note_read(read, Ok("# not a heading".to_string()));
    let before = buffer_rows(&reader_buffer(&app)).join("\n");
    let facts = foot_text(&app);
    assert!(facts.starts_with("─ notes.txt · text · "), "{facts:?}");
    assert!(!facts.contains("rendered") && !facts.contains("raw"));
    assert!(before.contains("# not a heading"), "{before}");
    app.toggle_reader_raw();
    assert_eq!(buffer_rows(&reader_buffer(&app)).join("\n"), before);
    assert_eq!(foot_text(&app), facts);
}

/// The facts at the tree's foot (FZ-93; beside the header in FZ-68 round
/// 2, the `[3] file` card before) say every fact the accepted design
/// keeps for a file: name,
/// kind, size, where it lives, modified and the last commit, which is
/// "untracked" when git knew nothing of the file. Path, project and
/// created were the card's alone.
#[test]
fn the_facts_render_every_fact_and_untracked() {
    let mut app = notes_view_app(&[(".notez/scratch.md", Some(".notez"))]);
    let read = app.claim_note_read().expect("a read");
    let at = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
    app.apply_note_facts(
        &read,
        crate::notes::NoteFacts {
            size: Some(812),
            modified: Some(at),
            created: Some(at),
        },
    );
    let draw = |app: &crate::app::App| foot_text(app);
    // Before git answers the date says it is still being read.
    assert!(draw(&app).contains("last commit reading…"));
    app.apply_note_commit(&read, crate::notes::LastCommit::Untracked);
    let text = draw(&app);
    let stamp = card_time(at);
    for want in [
        "scratch.md",
        "markdown · 812 B",
        &format!(".notez/ · modified {stamp}"),
        "last commit untracked",
    ] {
        assert!(text.contains(want), "{want} missing:\n{text}");
    }

    app.apply_note_commit(
        &read,
        crate::notes::LastCommit::At("2026-10-29T10:50:00+01:00".to_string()),
    );
    let text = draw(&app);
    let committed = card_commit_time("2026-10-29T10:50:00+01:00");
    assert!(text.contains(&format!("last commit {committed}")), "{text}");
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

/// FZ-68 (was FZ-67's box): pane 2 keeps one outer border in the
/// `notes` layout, its top border whole, and the preview is its right
/// column, divided from the tree by a line from under the top border to
/// `┴` on the bottom, crossed by the rule (round 2), titled
/// `notes preview` right of it above the rule; the repo header is drawn
/// left of the divider only. Fails while the preview is a box of its own
/// inside pane 2.
#[test]
fn the_notes_preview_is_a_divided_column_of_pane_2() {
    let (w, h) = (160u16, 44u16);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    let read = app.claim_note_read().expect("a read");
    app.apply_note_read(read, Ok("hello".to_string()));
    let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
    let rects = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app);
    let pane2 = rects.detail.expect("pane 2");
    let reader = rects.reader.expect("preview");
    let notes = notes_pane_body(pane2, &app).expect("a tree");
    let divider = notes.divider;
    let span = |y: u16, from: u16, to: u16| -> String {
        rows[y as usize]
            .chars()
            .skip(from as usize)
            .take((to - from) as usize)
            .collect()
    };
    let cell = |x: u16, y: u16| rows[y as usize].chars().nth(x as usize).unwrap();

    // One border on pane 2's top row: its two corners and nothing
    // between them that opens a second box.
    let top = span(pane2.y, pane2.x, pane2.right());
    assert!(top.starts_with('╭') && top.ends_with('╮'), "{top}");
    assert_eq!(top.matches('╭').count(), 1, "one outer border: {top}");
    assert!(top.contains("[2]"), "the pane's title: {top}");
    assert!(top.contains("NOTES"), "the view tabs: {top}");
    let bottom = span(pane2.bottom() - 1, pane2.x, pane2.right());
    assert!(bottom.starts_with('╰') && bottom.ends_with('╯'), "{bottom}");
    assert_eq!(bottom.matches('╰').count(), 1, "no second box: {bottom}");

    // The divider: under the top border, which it does not cut, joined
    // to the bottom one, a line on every inner row but the rule's.
    let rule = notes.rule.expect("pinned at 44 rows");
    assert!(!top.contains('┬'), "the top border is whole: {top}");
    assert_eq!(cell(divider, pane2.bottom() - 1), '┴', "{bottom}");
    for y in (pane2.y + 1..pane2.bottom() - 1).filter(|y| *y != rule) {
        assert_eq!(cell(divider, y), '│', "row {y}:\n{}", rows.join("\n"));
    }

    // The preview's title right of the divider level with `NOTES`, and
    // its text right of the divider, inside pane 2.
    let preview_title = span(rule - 1, divider, pane2.right());
    assert!(
        preview_title.starts_with("│ notes preview "),
        "{preview_title}"
    );
    assert!(span(rule - 1, pane2.x, divider).contains("NOTES"));
    assert_eq!(reader.x, divider + 2, "one column in from the divider");
    assert!(
        reader.right() < pane2.right() && reader.bottom() < pane2.bottom(),
        "{reader:?} in {pane2:?}"
    );
    assert!(span(reader.y, reader.x, reader.right()).contains("hello"));

    // The header left of the divider only: its rows hold the repo's
    // state and path in the tree's column, and nothing of them crosses.
    let header = notes.header.expect("pinned at 44 rows");
    let repo = app.selected_repo().expect("a repo");
    let state = repo.long_status();
    let path = repo.path.clone().expect("cloned");
    let left = |y: u16| span(y, pane2.x, divider);
    assert!(left(header.y).contains(&state), "{}", left(header.y));
    assert!(
        (header.y..header.bottom()).any(|y| left(y).contains(&path)),
        "{}",
        rows.join("\n")
    );
    // Right of the divider on those rows nothing, row for row: the
    // window sits on pane 1's border (FZ-95, 2026-11-09). Then `notes
    // preview` on the row above the rule.
    let facts = notes.facts.expect("pinned at 44 rows");
    assert_eq!((facts.y, facts.bottom()), (header.y, rule));
    let mut want: Vec<String> = vec![String::new(); usize::from(facts.height - 1)];
    want.push(READER_LABEL.to_string());
    for (i, y) in (header.y..rule).enumerate() {
        let right = span(y, divider + 1, pane2.right() - 1);
        // The path only: `synced`, the state of this fixture, is also
        // a word of the fleet's summary that sits here (FZ-93), and the
        // row-for-row match below pins everything right of the divider.
        assert!(
            !right.contains(&path),
            "header row {y} crosses the divider: {right}"
        );
        assert_eq!(right.trim(), want[i], "row {y} right of the divider");
    }

    // The file's facts on the tree's last row, left of the divider.
    let foot = notes.foot.expect("a foot at 44 rows");
    assert_eq!(foot.bottom(), notes.body.bottom() + 1, "under the tree");
    assert!(foot.right() <= divider, "left of the divider: {foot:?}");
    assert!(left(foot.y).contains("─ a.md · "), "{}", left(foot.y));

    // The tree on the left of the same body: its mode line on the
    // tree's first row, before the divider.
    let mode = left(notes.body.y);
    assert!(mode.contains("notes: [notez]"), "the tree: {mode}");
}

/// FZ-67, moved to pane 2's border by FZ-68, beside the header by round
/// 2 and to the tree's foot by FZ-93: the file's facts hold their row on
/// a narrow pane. The facts give way before the name, and a name wider
/// than the row is cut at it, never crossing the divider or wrapping
/// onto the bottom border. Fails while a fact can overprint another.
#[test]
fn the_foot_holds_its_row_when_narrow() {
    let long = "notez/a-file-name-far-too-long-for-the-facts-block-at-eighty.md";
    let mut app = notes_view_app(&[(long, Some("notez"))]);
    let read = app.claim_note_read().expect("a read");
    let facts = crate::notes::NoteFacts {
        size: Some(4300),
        ..Default::default()
    };
    app.apply_note_facts(&read, facts);
    app.apply_note_read(read, Ok("body".to_string()));
    let (w, h) = (80u16, 24u16);
    let notes = notes_pane_body(Rect::new(0, 0, w, h), &app).expect("a tree");
    let foot = notes.foot.expect("a foot at 24 rows");
    let name = long.trim_start_matches("notez/");
    assert!(name.len() > usize::from(foot.width), "the name overflows");
    let rows = notes_pane_rows(w, h, &app);
    let row: String = rows[foot.y as usize]
        .chars()
        .skip(foot.x as usize)
        .take(foot.width as usize)
        .collect();
    assert!(row.starts_with("─ a-file-name"), "{row}");
    assert!(row.ends_with('…'), "the name is cut at the row: {row}");
    assert!(!row.contains("4.2 KiB"), "the facts gave way first: {row}");
    let divider: String = rows[foot.y as usize]
        .chars()
        .nth(notes.divider as usize)
        .into_iter()
        .collect();
    assert_eq!(divider, "│", "the divider is not overprinted");

    // Wider, the facts come back in their order of rank: the size before
    // the kind, the kind before where it lives.
    let short = notes_view_app(&[("notez/plan.md", Some("notez"))]);
    let at = |width: usize| -> String {
        notes_foot_spans(&short, width)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    };
    let full = at(400);
    assert!(
        full.contains("plan.md · markdown · rendered · notez/"),
        "{full}"
    );
    let narrow = at(30);
    assert!(narrow.starts_with("─ plan.md"), "{narrow}");
    assert!(
        !narrow.contains("notez/"),
        "where it lives goes first: {narrow}"
    );
    assert_eq!(narrow.chars().count(), 30, "the rule fills the row");
}

/// FZ-68: the divider and the outline take focus by side. The top
/// border is one segment, lit with either side (round 2); below it the
/// tree focused lights the left outline and the divider, the preview's
/// outline dim, and the reader focused lights its outline and the
/// divider, the left dim.
#[test]
fn the_divider_lights_with_either_side_and_each_segment_with_its_own() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let (w, h) = (140u16, 24u16);
    let notes = notes_pane_body(Rect::new(0, 0, w, h), &app).expect("a tree");
    let divider = notes.divider;
    let (mid, bottom) = (h / 2, h - 1);
    // The top border, the left border and the bottom left of the
    // divider, the divider and the rule's crossing, the right border
    // and the bottom right of the divider.
    let colours = |app: &crate::app::App| {
        let buf = notes_pane_buffer(w, h, app);
        let rule = notes.rule.expect("pinned at 24 rows");
        // The top border's plain cells nearest each corner, past the
        // titles.
        let plain = |x: &u16| buf[(*x, 0)].symbol() == "─";
        let first = (1..w - 1).find(plain).expect("a border cell");
        let last = (1..w - 1).rev().find(plain).expect("a border cell");
        assert!(first < divider && last > divider, "{first} {last}");
        [
            (first, 0),
            (last, 0),
            (0, mid),
            (1, bottom),
            (divider, mid),
            (divider, rule),
            (w - 1, mid),
            (w - 2, bottom),
        ]
        .map(|(x, y)| buf[(x, y)].fg)
    };
    let (on, off) = (colors::LAVENDER, colors::SURFACE);

    app.focused_panel = FocusedPanel::Detail;
    let want = [on, on, on, on, on, on, off, off];
    assert_eq!(colours(&app), want, "the tree focused");
    app.focused_panel = FocusedPanel::Reader;
    let want = [on, on, off, off, on, on, on, on];
    assert_eq!(colours(&app), want, "the reader focused");
    app.focused_panel = FocusedPanel::RepoList;
    assert_eq!(colours(&app), [off; 8], "neither");
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

/// FZ-68: `panel_rects` puts the reader right of the divider, one
/// column in, on the tree's rows under the rule (round 2), down to the
/// bottom border, up to the right padding. `at` answers `Reader` there,
/// and `Detail` on the divider, on a tree row and on the facts block.
#[test]
fn panel_rects_put_the_reader_right_of_the_divider_under_the_rule() {
    let area = Rect::new(0, 0, 140, 44);
    let app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let rects = crate::ui::panel_rects(area, &app);
    let pane2 = rects.detail.expect("pane 2");
    let reader = rects.reader.expect("preview");
    let notes = notes_pane_body(pane2, &app).expect("a tree");
    assert_eq!(reader, notes.reader, "one geometry");
    assert_eq!(reader.x, notes.divider + 2);
    assert_eq!(reader.y, notes.rule.expect("pinned") + 1);
    assert_eq!(reader.y, notes.body.y, "level with the tree");
    assert_eq!(reader.bottom(), pane2.bottom() - 1);
    assert_eq!(reader.right(), pane2.right() - 2);
    let facts = notes.facts.expect("pinned");
    for (x, y) in [
        (facts.x, facts.y),
        (facts.x + facts.width / 2, facts.bottom() - 1),
        (facts.x, notes.rule.expect("pinned")),
    ] {
        assert_eq!(
            rects.at(x, y),
            Some(FocusedPanel::Detail),
            "facts ({x}, {y})"
        );
    }
    for y in [reader.y, reader.y + reader.height / 2, reader.bottom() - 1] {
        assert_eq!(rects.at(reader.x, y), Some(FocusedPanel::Reader));
        assert_eq!(rects.at(notes.divider, y), Some(FocusedPanel::Detail));
    }
    let tree_row = notes.body.y;
    assert_eq!(
        rects.at(notes.body.x + 1, tree_row),
        Some(FocusedPanel::Detail),
        "a tree row"
    );
    assert!(notes.body.right() < notes.divider, "the tree is left of it");
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

/// FZ-68 round 2, the facts moved to the tree's foot by FZ-93: the file's
/// name in the tree's colour for a file name, its kind word in the
/// colour of the tree's kind glyph, the size dim, and the mode in the
/// mode line's mark; `notes preview` level with `NOTES` right of the
/// divider, in the notes view's colour and bold like the headliner word.
#[test]
fn the_foot_shows_name_kind_size_and_mode_in_their_colours() {
    let mut app = notes_view_app(&[("notez/plan.md", Some("notez"))]);
    let read = app.claim_note_read().expect("a read");
    let facts = crate::notes::NoteFacts {
        size: Some(4300),
        ..Default::default()
    };
    app.apply_note_facts(&read, facts);
    app.apply_note_read(read, Ok("body".to_string()));
    let (w, h) = (120u16, 24u16);
    let notes = notes_pane_body(Rect::new(0, 0, w, h), &app).expect("a tree");
    let block = notes.facts.expect("pinned at 24 rows");
    let buf = notes_pane_buffer(w, h, &app);
    let rows = buffer_rows(&buf);
    // The style of the cell where `needle` starts on row `y`, right of
    // the divider or left of it.
    let at = |y: u16, needle: &str, right: bool| -> Style {
        let cells: Vec<char> = rows[y as usize].chars().collect();
        let want: Vec<char> = needle.chars().collect();
        let x = cells
            .windows(want.len())
            .enumerate()
            .position(|(x, w)| w == want && (x as u16 > notes.divider) == right)
            .unwrap_or_else(|| panic!("{needle} not on row {y}:\n{}", rows.join("\n")));
        buf[(x as u16, y)].style()
    };
    let label = block.bottom() - 1;
    assert_eq!(label, notes.rule.expect("pinned") - 1, "level with NOTES");

    // The tree's own row for the file, left of the divider.
    let tree_row = (notes.body.y..notes.body.bottom())
        .find(|y| rows[*y as usize].contains("plan.md"))
        .expect("the tree draws plan.md");
    let glyph = badges::note_kind_icon(crate::notes::NoteKind::Markdown);
    let tree_name = at(tree_row, "plan.md", false);
    let tree_glyph = at(tree_row, glyph, false);

    // The foot's facts (FZ-93), built where every one fits: the style of
    // the span holding `needle`.
    let foot = notes_foot_spans(&app, 400);
    let style_of = |needle: &str| -> Style {
        foot.iter()
            .find(|s| s.content.contains(needle))
            .unwrap_or_else(|| panic!("{needle} not in the foot: {foot:?}"))
            .style
    };
    assert_eq!(style_of("plan.md").fg, tree_name.fg, "the name");
    assert_eq!(style_of("markdown").fg, tree_glyph.fg, "the kind");
    assert_eq!(style_of("4.2 KiB").fg, Some(colors::OVERLAY), "the size");
    let mark = at(notes.body.y, "[notez]", false);
    let mode = style_of("rendered");
    assert_eq!((mode.fg, mode.add_modifier), (mark.fg, mark.add_modifier));
    assert_eq!(style_of("notez/").fg, Some(colors::TEXT), "where");
    let title = at(label, "notes preview", true);
    let notes_colour = colors::history_view_color(HistoryView::Notes);
    assert_eq!(title.fg, Some(notes_colour), "the notes view's colour");
    assert!(title.add_modifier.contains(Modifier::BOLD));
    let headliner = at(label, "NOTES", false);
    assert_eq!(
        (title.fg, title.add_modifier),
        (headliner.fg, headliner.add_modifier)
    );
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
