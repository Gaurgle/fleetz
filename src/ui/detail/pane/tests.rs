use super::*;
use crate::model::{CiStatus, RepoLocation, RepoStatus};
use crate::ui::badges;
use crate::ui::detail::tests::{
    branch_fixture, busy_repo, changed, changes_view_app, cloned_repo, columns, commit_rows_at,
    drawn, drawn_rows, header_block, header_rows, held_changes, info_text_at_bottom_expanded,
    is_title_column, notes_pane_text, notes_view_app, pane_rows, pane_text, pane2_rows,
    repo_with_a_worktree, sections, spread_repo, token_end,
};

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

/// `pane_text` for the `ci` view, with the recorded runs fixture landed
/// for the fixture repo.
fn ci_view_text(width: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = crate::app::App::new(vec![busy_repo()], "t".to_string(), width, 44);
    app.history_view = HistoryView::Ci;
    let (key, owner, name) = crate::ci::target(&app.repos[0]).expect("a GitHub repo");
    let request = app.claim_ci_poll(key.clone(), owner, name);
    let runs =
        crate::ci::parse_runs(include_str!("../../../ci/fixtures/runs.json")).expect("fixture");
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
