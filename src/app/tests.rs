use super::*;

fn commit(hash: &str) -> crate::model::Commit {
    crate::model::Commit {
        hash: hash.to_string(),
        author: "Gaurgle".to_string(),
        date: None,
        message: format!("subject {hash}"),
        body: String::new(),
        insertions: 0,
        deletions: 0,
        files: vec![],
    }
}

fn make_app(n: usize) -> App {
    let repos: Vec<RepoStatus> = (0..n)
        .map(|i| RepoStatus {
            daily: None,
            daily_key: None,
            last_local: None,
            name: format!("repo-{i}"),
            path: Some(format!("~/repos/repo-{i}")),
            location: RepoLocation::Primary,
            branches: vec![],
            commits: vec![],
            behind: 0,
            ahead: 0,
            dirty: 0,
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
        })
        .collect();
    App::new(repos, "test".to_string(), 120, 40)
}

/// Regression: the `r` spinner keyed on the local sweep alone, so it
/// stopped while the GitHub poll and the fetches it set off were still
/// landing rows one at a time.
#[test]
fn a_manual_refresh_spins_until_its_poll_and_fetches_land() {
    let mut app = make_app(3);
    app.sweep_started = Some(std::time::Instant::now());
    app.sweep_manual = true;
    app.refresh_awaits_poll = true;
    assert!(app.is_refreshing());

    // The local sweep finishing is not the end of it.
    app.sweep_started = None;
    assert!(app.is_refreshing(), "stopped before the poll landed");

    app.settle_refresh_poll(["~/repos/repo-0".to_string(), "~/repos/repo-2".to_string()]);
    assert!(app.is_refreshing(), "stopped before the fetches landed");

    app.settle_refresh_fetch("~/repos/repo-0", &crate::git::FetchOutcome::Succeeded);
    assert!(app.is_refreshing());
    app.settle_refresh_fetch("~/repos/repo-2", &crate::git::FetchOutcome::Succeeded);
    assert!(!app.is_refreshing());
    assert!(app.refresh_fetches_started.is_none());
}

#[test]
fn a_manual_refresh_is_local_until_its_sweep_ends_then_remote() {
    let mut app = make_app(1);
    assert_eq!(app.refresh_phase(), None);
    app.sweep_started = Some(std::time::Instant::now());
    app.sweep_manual = true;
    app.refresh_awaits_poll = true;
    assert_eq!(app.refresh_phase(), Some(RefreshPhase::Local));
    app.sweep_started = None;
    assert_eq!(app.refresh_phase(), Some(RefreshPhase::Remote));
    app.settle_refresh_poll(std::iter::empty());
    assert_eq!(app.refresh_phase(), None);
}

#[test]
fn a_manual_refresh_ends_with_its_poll_when_nothing_moved() {
    let mut app = make_app(1);
    app.refresh_awaits_poll = true;
    app.settle_refresh_poll(std::iter::empty());
    assert!(!app.is_refreshing());
    assert!(app.refresh_fetches_started.is_none());
}

/// The local sweep can report a repo before that repo's fetch has run.
#[test]
fn an_unfetched_update_does_not_settle_a_refresh_fetch() {
    let mut app = make_app(1);
    app.refresh_awaits_poll = true;
    app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    app.settle_refresh_fetch("~/repos/repo-0", &crate::git::FetchOutcome::NotAttempted);
    assert!(app.is_refreshing());
}

pub(crate) fn fetch_failed(kind: crate::git::FetchFailureKind) -> crate::git::FetchOutcome {
    crate::git::FetchOutcome::Failed(crate::git::FetchFailure { kind, detail: None })
}

/// Rule 4: a fetch that failed is still a fetch that finished, so it
/// settles the refresh waiting on it.
#[test]
fn a_failed_fetch_settles_a_refresh_fetch() {
    let mut app = make_app(1);
    app.refresh_awaits_poll = true;
    app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    app.settle_refresh_fetch(
        "~/repos/repo-0",
        &fetch_failed(crate::git::FetchFailureKind::Unreachable),
    );
    assert!(!app.is_refreshing());
    assert!(app.refresh_fetches_started.is_none());
}

/// The fetches `r`'s poll sets off are the ones its report counts.
#[test]
fn a_manual_refresh_tallies_the_fetches_its_poll_set_off() {
    let mut app = make_app(2);
    app.refresh_awaits_poll = true;
    app.settle_refresh_poll(["~/repos/repo-0".to_string(), "~/repos/repo-1".to_string()]);
    let ok = crate::git::FetchOutcome::Succeeded;
    assert_eq!(
        app.manual_fetches.land("~/repos/repo-0", "repo-0", &ok),
        None
    );
    let report = app.manual_fetches.land(
        "~/repos/repo-1",
        "repo-1",
        &fetch_failed(crate::git::FetchFailureKind::Auth),
    );
    assert_eq!(
        report,
        Some(FetchReport {
            total: 2,
            failed: vec![("repo-1".to_string(), crate::git::FetchFailureKind::Auth)],
        })
    );
}

/// A watch timer's poll is not a keypress: its fetches report nothing.
#[test]
fn a_poll_nobody_pressed_r_for_tallies_nothing() {
    let mut app = make_app(1);
    app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    let report = app.manual_fetches.land(
        "~/repos/repo-0",
        "repo-0",
        &fetch_failed(crate::git::FetchFailureKind::Auth),
    );
    assert_eq!(report, None);
}

/// The watch timer's poll fetches silently: no spinner, nothing to wait on.
#[test]
fn a_poll_nobody_pressed_r_for_adds_no_refresh_fetches() {
    let mut app = make_app(1);
    app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    assert!(!app.is_refreshing());
    assert!(app.refresh_fetches.is_empty());
}

#[test]
fn a_watch_timer_sweep_is_not_a_refresh() {
    let mut app = make_app(1);
    app.sweep_started = Some(std::time::Instant::now());
    app.sweep_manual = false;
    assert!(!app.is_refreshing());
}

#[test]
fn a_refresh_fetch_that_never_returns_is_given_up_on() {
    let mut app = make_app(1);
    app.refresh_awaits_poll = true;
    app.settle_refresh_poll(["~/repos/repo-0".to_string()]);
    app.expire_refresh_fetches(std::time::Duration::from_secs(60));
    assert!(
        app.is_refreshing(),
        "expired a fetch still inside its limit"
    );
    app.refresh_fetches_started =
        Some(std::time::Instant::now() - std::time::Duration::from_secs(61));
    app.expire_refresh_fetches(std::time::Duration::from_secs(60));
    assert!(!app.is_refreshing());
}

/// Regression: the footer folded `self.repos` while the repo list drew
/// `visible_repos`, so with a filter on it reported repos that were not
/// on screen. The two walk the same list now.
/// Regression: the wheel could only ever move the grid one way. The strip
/// opens with `contrib_scroll` pinned to the newest week, so scrolling
/// "newer" was already clamped and did nothing, and the arm that should
/// have scrolled older had been wired to a cursor move instead. The
/// symptom was a strip where the only thing the mouse could do was walk
/// the selected day upward.
#[test]
fn the_wheel_scrolls_the_grid_both_ways() {
    let mut app = make_app(1);
    app.contrib_visible_weeks.set(20);
    let max = crate::contributions::WEEKS - 20;
    app.contrib_scroll.set(max);

    // Newer, from the right edge: already there, nothing to give.
    app.scroll_contributions(3);
    assert_eq!(app.contrib_scroll.get(), max, "clamped at the newest week");

    // Older must actually move, which is the half that was missing.
    app.scroll_contributions(-3);
    assert_eq!(app.contrib_scroll.get(), max - 3);
    app.scroll_contributions(3);
    assert_eq!(app.contrib_scroll.get(), max, "and back again");

    // Neither direction disturbs the day cursor.
    assert_eq!(app.contrib_cursor, None);
}

/// Dragging pulls the grid along with the pointer, and a drag too short
/// to be worth a whole week banks its columns instead of discarding
/// them -- otherwise a slow drag rounds to zero forever and never moves.
#[test]
fn dragging_pans_the_grid_and_banks_the_remainder() {
    let mut app = make_app(1);
    app.contrib_visible_weeks.set(20);
    let start = crate::contributions::WEEKS - 20;
    app.contrib_scroll.set(start);

    // One column is less than a week: nothing moves, anchor unchanged.
    let anchor = app.pan_contributions(50, 51);
    assert_eq!(anchor, 50, "sub-week drag keeps its anchor");
    assert_eq!(app.contrib_scroll.get(), start);

    // Dragging right pulls older weeks in from the left.
    let anchor = app.pan_contributions(50, 58);
    assert_eq!(app.contrib_scroll.get(), start - 4, "8 columns = 4 weeks");
    assert_eq!(anchor, 58, "anchor advances by what was consumed");

    // And back the other way.
    app.pan_contributions(58, 50);
    assert_eq!(app.contrib_scroll.get(), start);
}

/// One key steps through every view and comes back where it started.
#[test]
fn cycling_history_views_wraps_through_all_of_them() {
    let mut app = make_app(1);
    assert_eq!(app.history_view, HistoryView::Info, "opens on the facts");

    let mut seen = vec![app.history_view];
    for _ in 1..HistoryView::COUNT {
        app.cycle_history_view(false);
        seen.push(app.history_view);
    }
    assert_eq!(seen, HistoryView::ALL.to_vec(), "in tab-strip order");

    app.cycle_history_view(false);
    assert_eq!(app.history_view, HistoryView::Info, "wraps forward");

    // And backwards, which a shifted letter could not have given us.
    app.cycle_history_view(true);
    assert_eq!(app.history_view, HistoryView::Changes, "wraps backward");
    app.cycle_history_view(true);
    assert_eq!(app.history_view, HistoryView::Notes);
    app.cycle_history_view(true);
    assert_eq!(app.history_view, HistoryView::Ci);
}

/// Scroll and selection are per view. A shared offset is wrong the moment
/// you switch back to something you had already scrolled -- you would
/// land wherever the other list happened to leave you.
#[test]
fn each_view_keeps_its_own_place() {
    let mut app = make_app(1);

    app.history_view = HistoryView::Commits;
    app.set_detail_scroll(12);
    app.set_detail_selected(3);

    app.history_view = HistoryView::Reflog;
    assert_eq!(app.detail_scroll(), 0, "a fresh view starts at the top");
    assert_eq!(app.detail_selected(), 0);
    app.set_detail_scroll(40);
    app.set_detail_selected(7);

    app.history_view = HistoryView::Commits;
    assert_eq!(app.detail_scroll(), 12, "commits kept its place");
    assert_eq!(app.detail_selected(), 3);

    app.history_view = HistoryView::Reflog;
    assert_eq!(app.detail_scroll(), 40, "and so did the reflog");
    assert_eq!(app.detail_selected(), 7);
}

/// The cursor walks whichever list is on screen, so its bound is that
/// list's length. `info` and `commits` both draw the graph; only the cap
/// differs.
#[test]
fn the_cursor_is_bounded_by_the_active_views_list() {
    let mut app = make_app(1);
    app.repos[0].commits = vec![commit("aaa"), commit("bbb")];
    app.repos[0].reflog = vec![
        crate::model::ReflogEntry {
            hash: "aaa".to_string(),
            action: "commit".to_string(),
            description: "one".to_string(),
        },
        crate::model::ReflogEntry {
            hash: "bbb".to_string(),
            action: "checkout".to_string(),
            description: "two".to_string(),
        },
        crate::model::ReflogEntry {
            hash: "ccc".to_string(),
            action: "pull".to_string(),
            description: "three".to_string(),
        },
    ];

    // Both history lists walk the graph rows, not the diffstat list: the
    // diffstat list is `git log HEAD` and the graph is the union of the
    // local branches, so they are not the same commits in the same order.
    app.repos[0].graph = vec![crate::model::GraphLine {
        lanes: vec![crate::graph::commit_cell()],
        hash: "aaa".to_string(),
        is_merge: false,
        refs: vec![],
        author: "Gaurgle".to_string(),
        message: "one".to_string(),
    }];
    app.history_view = HistoryView::Info;
    assert_eq!(app.selected_commit_count(), 1, "graph rows, not commits");

    app.history_view = HistoryView::Commits;
    assert_eq!(app.selected_commit_count(), 1, "graph rows, not commits");

    app.history_view = HistoryView::Reflog;
    assert_eq!(app.selected_commit_count(), 3, "a longer list of its own");
}

#[test]
fn the_summary_describes_only_what_the_list_is_drawing() {
    let mut app = make_app(4);
    app.repos[0].dirty = 3;
    app.repos[1].dirty = 1;
    app.repos[2].behind = 2;
    app.repos[3].location = RepoLocation::NotCloned;

    let all = app.summary();
    assert_eq!(all.dirty_count, 2, "unfiltered, the whole fleet counts");
    assert_eq!(all.behind_count, 1);
    assert_eq!(all.not_cloned_count, 1);

    // Narrow to a single repo by name; the footer must follow the list.
    app.search_query = "repo-0".to_string();
    let filtered = app.summary();
    assert_eq!(app.visible_repos().len(), 1, "test premise");
    assert_eq!(filtered.dirty_count, 1);
    assert_eq!(filtered.behind_count, 0, "repo-2 is filtered out of view");
    assert_eq!(filtered.not_cloned_count, 0, "repo-3 is filtered out too");
}

#[test]
fn visible_repos_are_grouped_in_display_order() {
    // Assembly order deliberately interleaves the three sections; the
    // repo list always draws them grouped, so navigation must too.
    let mut app = make_app(5);
    app.repos[0].location = RepoLocation::LocalOnly;
    app.repos[1].location = RepoLocation::Primary;
    app.repos[2].location = RepoLocation::NotCloned;
    app.repos[3].location = RepoLocation::LocalOnly;
    app.repos[4].location = RepoLocation::Primary;

    let order: Vec<RepoLocation> = app
        .visible_repos()
        .iter()
        .map(|(_, r)| r.location.clone())
        .collect();
    assert_eq!(
        order,
        vec![
            RepoLocation::Primary,
            RepoLocation::Primary,
            RepoLocation::LocalOnly,
            RepoLocation::LocalOnly,
            RepoLocation::NotCloned,
        ]
    );

    // Stability: within a section, assembly order is preserved.
    let names: Vec<&str> = app
        .visible_repos()
        .iter()
        .map(|(_, r)| r.name.as_str())
        .collect();
    assert_eq!(names[0], "repo-1");
    assert_eq!(names[1], "repo-4");
}

fn visible_names(app: &App) -> Vec<String> {
    app.visible_repos()
        .iter()
        .map(|(_, r)| r.name.clone())
        .collect()
}

/// FZ-86: rows move only when the order changes. An unchanged order
/// asks for no redraw, so a poll that moved nothing does not flicker.
#[test]
fn an_unchanged_order_moves_nothing_and_asks_no_redraw() {
    let mut app = make_app(3);
    app.selected = 1;
    app.needs_redraw = false;
    assert!(!app.reorder_repos(vec![0, 1, 1]));
    assert!(!app.needs_redraw);
    assert_eq!(visible_names(&app), ["repo-0", "repo-1", "repo-2"]);
    assert_eq!(app.selected, 1);

    assert!(app.reorder_repos(vec![2, 1, 0]));
    assert!(app.needs_redraw);
    assert_eq!(visible_names(&app), ["repo-2", "repo-1", "repo-0"]);
}

/// FZ-86: a cursor on a section header stays on the header when the
/// section's rows move under it; the repo it stands over is the
/// section's new first, as landing on the header would choose.
#[test]
fn a_cursor_on_a_header_stays_on_it_when_its_rows_move() {
    use crate::digest::Section;
    let mut app = make_app(3);
    app.view = View::Detail;
    app.repos[1].location = RepoLocation::LocalOnly;
    app.repos[2].location = RepoLocation::LocalOnly;
    app.land_on(ListRow::Header(Section::LocalChanges));
    assert_eq!(app.on_header(), Some(Section::LocalChanges), "premise");
    assert_eq!(app.repos[app.selected].name, "repo-1", "premise");

    assert!(app.reorder_repos(vec![0, 2, 1]));
    assert_eq!(app.on_header(), Some(Section::LocalChanges));
    assert_eq!(visible_names(&app), ["repo-0", "repo-2", "repo-1"]);
    assert_eq!(app.visible_repos()[app.selected].1.name, "repo-2");
}

#[test]
fn navigation_up_down() {
    let mut app = make_app(5);
    assert_eq!(app.selected, 0);
    app.move_down();
    assert_eq!(app.selected, 1);
    app.move_down();
    assert_eq!(app.selected, 2);
    app.move_up();
    assert_eq!(app.selected, 1);
    // Can't go above 0
    app.move_up();
    app.move_up();
    assert_eq!(app.selected, 0);
}

#[test]
fn navigation_clamps_at_end() {
    let mut app = make_app(3);
    for _ in 0..10 {
        app.move_down();
    }
    assert_eq!(app.selected, 2);
}

#[test]
fn initial_view_small_terminal_is_compact() {
    assert_eq!(initial_view(SMALL_WIDTH - 1, 40), View::Compact);
    assert_eq!(initial_view(120, SMALL_HEIGHT - 1), View::Compact);
}

#[test]
fn initial_view_roomy_terminal_is_detail() {
    assert_eq!(initial_view(SMALL_WIDTH, SMALL_HEIGHT), View::Detail);
}

#[test]
fn toggle_view() {
    let mut app = make_app(1);
    assert_eq!(app.view, View::Detail);
    app.toggle_view();
    assert_eq!(app.view, View::Compact);
    app.toggle_view();
    assert_eq!(app.view, View::Detail);
}

#[test]
fn search_filters_repos() {
    let mut app = make_app(5);
    app.search_push_char('1');
    let visible = app.visible_repos();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].1.name, "repo-1");
}

#[test]
fn search_resets_selection() {
    let mut app = make_app(5);
    app.move_down();
    app.move_down();
    assert_eq!(app.selected, 2);
    app.search_push_char('0');
    assert_eq!(app.selected, 0);
}

#[test]
fn clear_search_shows_all() {
    let mut app = make_app(5);
    app.search_push_char('1');
    assert_eq!(app.visible_repos().len(), 1);
    app.clear_search();
    assert_eq!(app.visible_repos().len(), 5);
}

#[test]
fn selected_repo_returns_correct_repo() {
    let mut app = make_app(3);
    app.move_down();
    let r = app.selected_repo().unwrap();
    assert_eq!(r.name, "repo-1");
}

#[test]
fn dirty_filter_shows_only_dirty_repos() {
    let mut app = make_app(3);
    app.repos[1].dirty = 2;
    app.search_query = "#dirty".to_string();
    let visible = app.visible_repos();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].1.name, "repo-1");
}

#[test]
fn quick_filters_combine_with_and() {
    let mut app = make_app(3);
    app.repos[0].dirty = 1; // dirty only
    app.repos[1].dirty = 1;
    app.repos[1].behind = 1; // dirty AND behind
    app.repos[2].behind = 1; // behind only

    app.search_query = "#dirty #behind".to_string();

    let visible = app.visible_repos();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].1.name, "repo-1");
}

#[test]
fn filter_tag_prefix_matches() {
    let mut app = make_app(2);
    app.repos[1].dirty = 1;
    app.search_query = "#d".to_string(); // prefix of "dirty"
    let visible = app.visible_repos();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].1.name, "repo-1");
}

#[test]
fn clear_filters_strips_tags_but_keeps_text() {
    let mut app = make_app(3);
    app.repos[1].dirty = 1;
    app.repos[1].name = "repo-1-target".to_string();
    app.search_query = "target #dirty".to_string();
    assert_eq!(app.visible_repos().len(), 1);
    app.clear_filters();
    assert_eq!(app.search_query, "target");
    assert_eq!(app.visible_repos().len(), 1); // text query alone still narrows it
}

#[test]
fn ci_failed_filter_matches_only_failed_ci() {
    let mut app = make_app(2);
    app.repos[0].ci_status = crate::model::CiStatus::Passed;
    app.repos[1].ci_status = crate::model::CiStatus::Failed;
    app.search_query = "#ci".to_string();
    let visible = app.visible_repos();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].1.name, "repo-1");
}

#[test]
fn parse_query_extracts_tag_and_text() {
    let parsed = parse_query("repo #dirty");
    assert_eq!(parsed.text, "repo");
    assert!(parsed.filters.dirty);
    assert!(!parsed.filters.behind);
    assert!(parsed.unknown_tags.is_empty());
}

#[test]
fn parse_query_bare_hash_is_inert() {
    // It used to activate all six filters, which AND'd to an empty list --
    // and since "#" is the first keystroke of every tag, the list blanked
    // on the way to typing any of them.
    let parsed = parse_query("#");
    assert_eq!(parsed.text, "");
    assert_eq!(parsed.filters, QuickFilters::default());
    assert!(parsed.unknown_tags.is_empty(), "incomplete, not wrong");
}

#[test]
fn parse_query_reports_an_unknown_tag_instead_of_swallowing_it() {
    let parsed = parse_query("#nonsense hello");
    assert_eq!(parsed.text, "hello", "still not treated as literal text");
    assert_eq!(parsed.filters, QuickFilters::default());
    assert_eq!(parsed.unknown_tags, vec!["nonsense".to_string()]);
}

#[test]
fn fuzzy_search_matches_a_non_substring_query() {
    let mut app = make_app(2);
    app.repos[0].name = "fleetz".to_string();
    app.repos[1].name = "notez2".to_string();
    app.search_query = "flz".to_string();

    let visible = app.visible_repos();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].1.name, "fleetz");
}

#[test]
fn search_results_are_ranked_best_match_first() {
    // The weaker match sits earlier in `repos` and in an earlier section,
    // so only scoring can put the exact hit on top.
    let mut app = make_app(2);
    app.repos[0].name = "pinz-board-archive".to_string();
    app.repos[1].name = "pinz".to_string();
    app.repos[1].location = RepoLocation::LocalOnly;
    app.search_query = "pinz".to_string();

    let visible = app.visible_repos();
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].1.name, "pinz", "exact match ranks first");
}

#[test]
fn section_order_returns_once_the_query_is_cleared() {
    let mut app = make_app(2);
    app.repos[0].location = RepoLocation::LocalOnly;
    app.repos[1].location = RepoLocation::Primary;

    assert!(!app.has_text_query());
    let visible = app.visible_repos();
    assert_eq!(
        visible[0].1.location,
        RepoLocation::Primary,
        "github activity leads when nothing is typed"
    );
}

/// "local changes" is a worklist -- uncommitted work on this machine --
/// while "also active" is other people's repos you happen to have. The
/// worklist reads first.
#[test]
fn local_changes_sorts_above_also_active() {
    let mut app = make_app(3);
    app.repos[0].location = RepoLocation::Elsewhere;
    app.repos[1].location = RepoLocation::LocalOnly;
    app.repos[2].location = RepoLocation::Primary;

    let order: Vec<RepoLocation> = app
        .visible_repos()
        .iter()
        .map(|(_, r)| r.location.clone())
        .collect();
    assert_eq!(
        order,
        vec![
            RepoLocation::Primary,
            RepoLocation::LocalOnly,
            RepoLocation::Elsewhere,
        ]
    );
}

/// A not-cloned repo shares "also active" with `Elsewhere`, so it has to
/// move with it. Ranking them apart would split one drawn section in two.
#[test]
fn not_cloned_stays_with_also_active() {
    let mut app = make_app(2);
    app.repos[0].location = RepoLocation::NotCloned;
    app.repos[1].location = RepoLocation::LocalOnly;

    let order: Vec<RepoLocation> = app
        .visible_repos()
        .iter()
        .map(|(_, r)| r.location.clone())
        .collect();
    assert_eq!(
        order,
        vec![RepoLocation::LocalOnly, RepoLocation::NotCloned]
    );
}

#[test]
fn tag_only_query_keeps_section_order() {
    // "#dirty" has no free text to rank by, so the list must not switch
    // into ranked mode (which would also drop the section headers).
    let mut app = make_app(2);
    app.repos[0].dirty = 1;
    app.repos[1].dirty = 1;
    app.search_query = "#dirty".to_string();
    assert!(!app.has_text_query());
    assert_eq!(app.visible_repos().len(), 2);
}

/// A commit by `author`, for the FZ-56 search fixtures.
fn commit_by(hash: &str, author: &str) -> crate::model::Commit {
    let mut c = commit(hash);
    c.author = author.to_string();
    c
}

/// FZ-56's four rows: one name prefix, two owners, one organisation and
/// one local-only clone with an author nobody else has.
fn people_app() -> App {
    use crate::model::RepoKind;
    let mut app = make_app(4);
    let r = &mut app.repos[0];
    r.name = "tools".to_string();
    r.owner = Some("Gaurgle".to_string());
    r.repo_kind = RepoKind::Public;
    r.commits = vec![
        commit_by("a1", "Andreas Roos"),
        commit_by("a2", "Andreas Roos"),
    ];
    let r = &mut app.repos[1];
    r.name = "tools-kestrel".to_string();
    r.owner = Some("Gaurgle".to_string());
    r.repo_kind = RepoKind::Private;
    r.location = RepoLocation::NotCloned;
    r.path = None;
    let r = &mut app.repos[2];
    r.name = "tools-web".to_string();
    r.owner = Some("kestrel-labs".to_string());
    r.repo_kind = RepoKind::Org;
    r.commits = vec![commit_by("b1", "Pia Holm")];
    let r = &mut app.repos[3];
    r.name = "tools-scratch".to_string();
    r.location = RepoLocation::LocalOnly;
    r.commits = vec![commit_by("c1", "Mira Lindqvist")];
    app
}

fn names_for(app: &mut App, query: &str) -> Vec<String> {
    app.search_query = query.to_string();
    app.visible_repos()
        .iter()
        .map(|(_, r)| r.name.clone())
        .collect()
}

#[test]
fn an_owner_login_finds_the_owners_rows_and_not_the_organisations() {
    let mut app = people_app();
    let mut found = names_for(&mut app, "Gaurgle");
    found.sort();
    assert_eq!(found, vec!["tools", "tools-kestrel"]);
}

#[test]
fn an_organisation_name_finds_its_row() {
    let mut app = people_app();
    assert_eq!(names_for(&mut app, "kestrel-labs"), vec!["tools-web"]);
}

#[test]
fn a_contributor_finds_the_rows_whose_commits_carry_them() {
    let mut app = people_app();
    assert_eq!(names_for(&mut app, "Andreas Roos"), vec!["tools"]);
    assert_eq!(names_for(&mut app, "Roos"), vec!["tools"]);
    // Word by word as well as the phrase: the order typed does not matter.
    assert_eq!(names_for(&mut app, "roos andreas"), vec!["tools"]);
}

#[test]
fn a_local_only_row_matches_on_its_authors() {
    let mut app = people_app();
    assert_eq!(names_for(&mut app, "Lindqvist"), vec!["tools-scratch"]);
}

#[test]
fn a_not_cloned_row_matches_on_its_owner_and_has_no_authors() {
    let mut app = people_app();
    app.repos.retain(|r| r.is_not_cloned());
    assert_eq!(names_for(&mut app, "gaurgle"), vec!["tools-kestrel"]);
    assert!(names_for(&mut app, "Roos").is_empty());
}

/// The tiers outrank the score: the organisation's login is a better
/// fuzzy hit for "kestrel" than `tools-kestrel` is, and still ranks
/// below it, since a name match is what a search usually means.
#[test]
fn a_name_match_ranks_above_an_owner_match() {
    let mut app = people_app();
    assert!(
        crate::fuzzy::score("kestrel", "kestrel-labs")
            > crate::fuzzy::score("kestrel", "tools-kestrel")
    );
    assert_eq!(
        names_for(&mut app, "kestrel"),
        vec!["tools-kestrel", "tools-web"]
    );
}

/// All three tiers at once: name, then organisation, then contributor.
#[test]
fn an_owner_match_ranks_above_a_contributor_match() {
    let mut app = people_app();
    app.repos[3].commits = vec![commit_by("c2", "Kestrel Hansen")];
    assert_eq!(
        names_for(&mut app, "kestrel"),
        vec!["tools-kestrel", "tools-web", "tools-scratch"]
    );
}

/// Owners and authors match on word starts, not as loose subsequences:
/// a short name query would otherwise pull in every row a prolific
/// author touched ("dr" is in "Andreas"), burying the names it meant.
#[test]
fn an_owner_or_author_matches_on_word_starts_only() {
    let mut app = people_app();
    assert!(names_for(&mut app, "dr").is_empty());
    assert!(names_for(&mut app, "urgle").is_empty());
    assert_eq!(names_for(&mut app, "and ro"), vec!["tools"]);
}

/// A text of separators alone splits into no words, and zero words
/// must not match: "-" or "." against an owner or author matched every
/// row. A name that holds the separator still matches it as a name.
#[test]
fn a_separator_only_query_matches_no_owner_or_author() {
    for text in ["-", ".", "/", "_", "- .", " "] {
        assert_eq!(person_score(text, "Gaurgle"), None, "{text:?}");
        assert_eq!(person_score(text, "Andreas Roos"), None, "{text:?}");
    }
    let mut app = people_app();
    assert!(names_for(&mut app, ".").is_empty());
    let mut found = names_for(&mut app, "-");
    found.sort();
    assert_eq!(found, vec!["tools-kestrel", "tools-scratch", "tools-web"]);
}

#[test]
fn a_pull_request_author_counts_as_a_contributor() {
    let mut app = people_app();
    app.repos[1].prs = vec![crate::model::PullRequest {
        number: 7,
        title: "fix".to_string(),
        author: "octo-reviewer".to_string(),
        created_at: chrono::Utc::now(),
        review_status: crate::model::ReviewStatus::Unknown,
        url: String::new(),
    }];
    assert_eq!(names_for(&mut app, "octo"), vec!["tools-kestrel"]);
}

#[test]
fn the_matched_field_is_reported_only_for_a_non_name_hit() {
    let mut app = people_app();
    let field = |app: &App, i: usize| app.search_match_field(&app.repos[i]);
    assert_eq!(field(&app, 0), None, "nothing typed");
    app.search_query = "kestrel".to_string();
    assert_eq!(field(&app, 1), None, "a name hit needs no mark");
    assert_eq!(field(&app, 2), Some(MatchField::Org));
    app.search_query = "gaurgle".to_string();
    assert_eq!(field(&app, 0), Some(MatchField::Owner));
    app.search_query = "roos".to_string();
    assert_eq!(field(&app, 0), Some(MatchField::Author));
    assert_eq!(field(&app, 3), None, "not a match at all");
}

/// The author set is cached on the row, so it has to follow a sweep
/// that lands a new commit list rather than keep the old authors.
#[test]
fn a_sweep_landing_new_commits_refreshes_who_the_row_matches() {
    let mut app = people_app();
    assert_eq!(names_for(&mut app, "Lindqvist"), vec!["tools-scratch"]);
    let mut fresh = app.repos[3].clone();
    fresh.commits = vec![commit_by("c9", "Ola Berg")];
    app.repos[3].apply_local_sweep(fresh);
    assert!(names_for(&mut app, "Lindqvist").is_empty());
    assert_eq!(names_for(&mut app, "Berg"), vec!["tools-scratch"]);
}

/// FZ-56's performance criterion: a 200-row fleet, every row with a
/// 200-commit history, filters in well under the input-to-paint budget
/// in `docs/performance-baseline.md` (median about 4 to 6 ms for a whole
/// keypress frame). The bound here is that budget's p90, 9 ms, for the
/// filter alone in a debug build, a deliberately loose ceiling: the
/// release measurement is in the FZ-56 report. The first query builds
/// the author sets; the timed ones reuse them.
#[test]
fn a_two_hundred_row_fleet_filters_within_the_input_budget() {
    let mut app = make_app(200);
    for (i, r) in app.repos.iter_mut().enumerate() {
        r.owner = Some(format!("owner-{}", i % 7));
        r.commits = (0..200)
            .map(|n| {
                commit_by(
                    &format!("{i}-{n}"),
                    ["Andreas Roos", "Pia Holm", "Ola Berg"][n % 3],
                )
            })
            .collect();
    }
    names_for(&mut app, "warm");
    let first = app.repos[0].commit_authors().as_ptr();
    let started = std::time::Instant::now();
    for query in ["repo-1", "owner-3", "holm", "nothing at all"] {
        names_for(&mut app, query);
    }
    let per_query = started.elapsed() / 4;
    assert_eq!(
        app.repos[0].commit_authors().as_ptr(),
        first,
        "the author set is built once, not per keystroke"
    );
    assert!(
        per_query < std::time::Duration::from_millis(9),
        "filtering took {per_query:?} per query"
    );
}

/// Tag filters still narrow an owner search, as they narrow a name one.
#[test]
fn tag_filters_combine_with_an_owner_query() {
    let mut app = people_app();
    app.repos[0].dirty = 1;
    assert_eq!(names_for(&mut app, "Gaurgle #dirty"), vec!["tools"]);
}

fn settings() -> SettingsOverlay {
    SettingsOverlay::new(
        60,
        30,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Off,
        CellSize::Medium,
        true,
        Icons::Nerd,
    )
}

/// Put the cursor on a field by name rather than by number, so inserting a
/// row does not silently retarget every test that follows it.
fn focus(s: &mut SettingsOverlay, field: SettingField) {
    s.cursor = SETTING_FIELDS
        .iter()
        .position(|f| *f == field)
        .expect("field is in SETTING_FIELDS");
    assert_eq!(s.field(), field);
}

#[test]
fn settings_cursor_wraps_both_ways() {
    // The last row is derived rather than named: this test is about the
    // wrap, and adding a settings row should not break it.
    let last = *SETTING_FIELDS.last().expect("at least one setting");
    let mut s = settings();
    assert_eq!(s.field(), SettingField::RefreshInterval);
    s.move_cursor(-1);
    assert_eq!(s.field(), last, "wraps to the end");
    s.move_cursor(1);
    assert_eq!(s.field(), SettingField::RefreshInterval);
}

#[test]
fn settings_interval_clamps_to_its_range() {
    let mut s = settings();
    for _ in 0..100 {
        s.step(-1);
    }
    assert_eq!(s.refresh_interval, 5, "never reaches zero");
    for _ in 0..2000 {
        s.step(1);
    }
    assert_eq!(s.refresh_interval, 3600);
}

#[test]
fn settings_view_cycles_through_auto() {
    let mut s = settings();
    assert_eq!(s.view, None);
    focus(&mut s, SettingField::View);
    s.step(1);
    assert_eq!(s.view, Some(View::Detail));
    s.step(1);
    assert_eq!(s.view, Some(View::Compact));
    s.step(1);
    assert_eq!(s.view, None, "wraps back to auto");
    s.step(-1);
    assert_eq!(s.view, Some(View::Compact), "and cycles backwards");
}

#[test]
fn settings_work_hours_stay_ordered() {
    // The pair must never invert or collapse: an empty work slot would
    // make the time-slot labels nonsense.
    let mut s = settings();
    focus(&mut s, SettingField::WorkStart);
    for _ in 0..40 {
        s.step(1);
    }
    assert!(s.work_start < s.work_end);
    assert_eq!(s.work_start, 17, "stops one below work_end");

    focus(&mut s, SettingField::WorkEnd);
    for _ in 0..40 {
        s.step(-1);
    }
    assert!(s.work_start < s.work_end);
    assert_eq!(s.work_end, 18, "stops one above work_start");
}

#[test]
fn settings_digits_type_into_numeric_fields_only() {
    let mut s = settings();
    s.push_digit(3);
    s.push_digit(0);
    assert_eq!(s.refresh_interval, 30, "'3' then '0' reads as 30");

    focus(&mut s, SettingField::View); // not numeric
    let before = s.view;
    s.push_digit(7);
    assert_eq!(
        s.view, before,
        "digits are ignored, not swallowed as a step"
    );
}

#[test]
fn settings_normalize_lifts_a_half_typed_value_to_its_floor() {
    // Typing "30" passes through "3", which is below INTERVAL_MIN. The
    // intermediate state is allowed; applying it is not.
    let mut s = settings();
    s.push_digit(3);
    assert_eq!(s.refresh_interval, 3);
    s.normalize();
    assert_eq!(s.refresh_interval, 5);
}

#[test]
fn settings_poll_interval_has_its_own_floor() {
    // Lower than the sweep floor on purpose: a poll is one GraphQL call,
    // a sweep is a fetch against every remote.
    let mut s = settings();
    focus(&mut s, SettingField::GithubPollInterval);
    for _ in 0..100 {
        s.step(-1);
    }
    assert_eq!(s.github_poll_interval, POLL_MIN);
    // And the sweep floor is untouched by it: two separate limits, because
    // the two operations cost very different things.
    assert_eq!(s.refresh_interval, 60);
}

#[test]
fn settings_watch_is_a_restart_field() {
    // `w` changes the live state; this row only sets what the next launch
    // starts with, and the overlay says so rather than looking inert.
    assert_eq!(SettingField::Watch.applies(), Applies::Restart);
    let mut s = settings();
    focus(&mut s, SettingField::Watch);
    let before = s.watch;
    s.step(1);
    assert_ne!(s.watch, before, "toggles");
}

#[test]
fn settings_write_every_field_back() {
    use crate::config::SettingValue;
    let mut s = settings();
    s.refresh_interval = 90;
    s.view = Some(View::Compact);
    s.persist_panes = true;

    let updates = s.config_updates();
    let by_key = |k: &str| {
        updates
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(by_key("refresh_interval"), Some(SettingValue::Int(90)));
    assert_eq!(by_key("github_poll_interval"), Some(SettingValue::Int(30)));
    assert_eq!(by_key("watch"), Some(SettingValue::Bool(true)));
    assert_eq!(by_key("view"), Some(SettingValue::Str("compact")));
    assert_eq!(by_key("work_start"), Some(SettingValue::Int(9)));
    assert_eq!(by_key("persist_pane_sizes"), Some(SettingValue::Bool(true)));
}

#[test]
fn the_blink_actually_flips_between_frames() {
    // Guards a parity bug rather than the flip itself: repaints for
    // watch-mode animation happen every SLOW_REPAINT_TICKS, and if that is
    // an even multiple of BLINK_TICKS the blink is drawn in the same phase
    // every time and never appears to blink. Anything odd-multiple, or
    // equal, is fine.
    let repaint_every = crate::SLOW_REPAINT_TICKS;
    let flips_per_repaint = repaint_every / BLINK_TICKS;
    assert_eq!(
        repaint_every % BLINK_TICKS,
        0,
        "repaints should land on a blink boundary"
    );
    assert!(
        flips_per_repaint % 2 == 1,
        "an even number of flips between repaints draws the same phase forever \
         (repaint every {repaint_every} ticks, blink every {BLINK_TICKS})"
    );
}

#[test]
fn on_tick_flips_the_blink_on_its_period() {
    let mut app = make_app(1);
    let start = app.blink_on;
    for _ in 0..BLINK_TICKS {
        app.on_tick();
    }
    assert_ne!(app.blink_on, start, "flips after one period");
    for _ in 0..BLINK_TICKS {
        app.on_tick();
    }
    assert_eq!(app.blink_on, start, "and back after two");
}

#[test]
fn scrolling_stops_at_the_measured_end_of_content() {
    // The overshoot bug: the offset used to climb to a fixed cap of 300
    // regardless of content, so scrolling back needed one dead keypress
    // per step of overshoot before the view moved at all.
    let mut app = make_app(1);
    app.detail_max_scroll.set(12);

    for _ in 0..100 {
        app.scroll_detail(1);
    }
    assert_eq!(app.detail_scroll(), 12, "never runs past the content");

    // The very next scroll back must move the view, not burn off phantom
    // offset.
    app.scroll_detail(-1);
    assert_eq!(app.detail_scroll(), 11);
}

#[test]
fn scrolling_up_stops_at_the_top() {
    let mut app = make_app(1);
    app.inspector_max_scroll.set(5);
    app.scroll_inspector(3);
    assert_eq!(app.inspector_scroll, 3);

    for _ in 0..50 {
        app.scroll_inspector(-1);
    }
    assert_eq!(app.inspector_scroll, 0);
    app.scroll_inspector(1);
    assert_eq!(app.inspector_scroll, 1, "moves again immediately");
}

#[test]
fn a_pane_with_no_overflow_does_not_scroll() {
    // Content shorter than the pane: max is 0, so every scroll is a no-op
    // rather than silently accumulating offset.
    let mut app = make_app(1);
    app.detail_max_scroll.set(0);
    app.scroll_detail(10);
    assert_eq!(app.detail_scroll(), 0);
}

#[test]
fn page_scrolling_clamps_the_same_way() {
    let mut app = make_app(1);
    app.detail_max_scroll.set(4);
    app.scroll_detail(10); // one PageDown
    assert_eq!(app.detail_scroll(), 4);
    app.scroll_detail(-10); // one PageUp
    assert_eq!(app.detail_scroll(), 0);
}

#[test]
fn pane_resize_clamps_and_resets() {
    let mut app = make_app(1);
    assert_eq!(app.repo_list_pct, DEFAULT_REPO_LIST_PCT);

    for _ in 0..50 {
        app.resize_repo_list(-1);
    }
    assert_eq!(app.repo_list_pct, REPO_LIST_PCT_MIN, "never collapses");
    for _ in 0..50 {
        app.resize_repo_list(1);
    }
    assert_eq!(app.repo_list_pct, REPO_LIST_PCT_MAX);

    for _ in 0..50 {
        app.resize_inspector(1);
    }
    assert_eq!(app.inspector_pct, INSPECTOR_PCT_MAX);

    app.reset_pane_sizes();
    assert_eq!(app.repo_list_pct, DEFAULT_REPO_LIST_PCT);
    assert_eq!(app.inspector_pct, DEFAULT_INSPECTOR_PCT);
}

/// The list width `split_main` draws for `app` in a main area `w` wide.
fn drawn_list_width(app: &App, w: u16) -> u16 {
    let area = ratatui::layout::Rect::new(0, 0, w, 40);
    crate::ui::detail::split_main(area, app.repo_list_pct)
        .0
        .width
}

/// FZ-69: at 180 columns the default draws 52 (the cap), and stepping
/// the percentage blind gave `>` 72 but `<` 54, wider than the 52 on
/// screen. Every step now starts from the drawn width.
#[test]
fn resize_keys_step_from_the_capped_width_on_screen() {
    let w = 180;
    let at_default = || {
        let app = make_app(1);
        app.main_area_width.set(w);
        app
    };

    let mut app = at_default();
    let default = drawn_list_width(&app, w);
    assert!(default < w * DEFAULT_REPO_LIST_PCT / 100, "the cap bites");
    app.resize_repo_list(-1);
    let after_lt = drawn_list_width(&app, w);
    assert!(after_lt < default, "< widened: {default} -> {after_lt}");
    assert_eq!(after_lt, 43, "< steps 5% of 180 from 52");

    let mut app = at_default();
    let mut widths = vec![default];
    for _ in 0..3 {
        app.resize_repo_list(1);
        widths.push(drawn_list_width(&app, w));
    }
    assert_eq!(widths[1], 61, "> steps 5% of 180 from 52");
    assert!(
        widths.windows(2).all(|p| p[0] < p[1]),
        "> did not widen every press: {widths:?}"
    );
}

/// At every width a key press moves the list the way it says or, at a
/// bound, not at all: never back onto the capped default (which would
/// stall `>`), never wider under `<` where the floor exceeds the cap.
#[test]
fn resize_keys_never_move_the_list_the_wrong_way_at_any_width() {
    for w in 60..=400 {
        for delta in [-1i16, 1] {
            let mut app = make_app(1);
            app.main_area_width.set(w);
            for press in 0..10 {
                let before = drawn_list_width(&app, w);
                let pct = app.repo_list_pct;
                app.resize_repo_list(delta);
                let after = drawn_list_width(&app, w);
                if delta < 0 {
                    assert!(
                        after <= before,
                        "w {w} press {press}: < {before} -> {after}"
                    );
                } else if pct < REPO_LIST_PCT_MAX {
                    assert!(after > before, "w {w} press {press}: > {before} -> {after}");
                }
            }
        }
    }
}

/// Where the cap does not bite the step is the plain 5% it always was.
#[test]
fn resize_keys_step_five_percent_where_the_cap_does_not_bite() {
    let w = 100;
    let mut app = make_app(1);
    app.main_area_width.set(w);
    assert_eq!(drawn_list_width(&app, w), 35);
    app.resize_repo_list(-1);
    assert_eq!(app.repo_list_pct, DEFAULT_REPO_LIST_PCT - 5);
    assert_eq!(drawn_list_width(&app, w), 30);
    app.reset_pane_sizes();
    app.resize_repo_list(1);
    assert_eq!(app.repo_list_pct, DEFAULT_REPO_LIST_PCT + 5);
    assert_eq!(drawn_list_width(&app, w), 40);
}

#[test]
fn window_overlay_opens_on_number_field() {
    let o = TimeWindowOverlay::new(TimeWindow {
        n: 2,
        unit: WindowUnit::Weeks,
    });
    assert_eq!(o.field, WindowField::Number);
    assert_eq!(o.n, 2);
}

#[test]
fn window_overlay_slot_forces_unit_field() {
    let o = TimeWindowOverlay::new(TimeWindow::default());
    assert_eq!(o.field, WindowField::Unit);
    let mut o = o;
    o.toggle_field();
    assert_eq!(o.field, WindowField::Unit); // can't focus the dimmed number
}

#[test]
fn window_overlay_toggle_field_flips() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Days,
    });
    assert_eq!(o.field, WindowField::Number);
    o.toggle_field();
    assert_eq!(o.field, WindowField::Unit);
    o.toggle_field();
    assert_eq!(o.field, WindowField::Number);
}

#[test]
fn window_overlay_number_wraps_both_ways() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 31,
        unit: WindowUnit::Days,
    });
    o.step_up();
    assert_eq!(o.n, 1);
    o.step_down();
    assert_eq!(o.n, 31);
}

#[test]
fn window_overlay_unit_cycles_forward_and_back() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Days,
    });
    o.toggle_field(); // focus unit
    o.step_down(); // Days -> Weeks
    assert_eq!(o.unit, WindowUnit::Weeks);
    o.step_up(); // Weeks -> Days
    assert_eq!(o.unit, WindowUnit::Days);
}

#[test]
fn window_overlay_unit_wraps_and_slot_grabs_focus() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Years,
    });
    o.toggle_field(); // focus unit
    o.step_down(); // Years wraps to Slot
    assert_eq!(o.unit, WindowUnit::Slot);
    assert_eq!(o.field, WindowField::Unit);
}

#[test]
fn window_overlay_digit_entry_two_digit_and_clamp() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Days,
    });
    o.push_digit(2);
    assert_eq!(o.n, 2);
    o.push_digit(9);
    assert_eq!(o.n, 29); // "2" then "9" = 29
    o.push_digit(5);
    assert_eq!(o.n, 5); // third digit starts a fresh entry
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Days,
    });
    o.push_digit(3);
    o.push_digit(5);
    assert_eq!(o.n, 31); // 35 clamps to max
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 5,
        unit: WindowUnit::Days,
    });
    o.push_digit(0);
    assert_eq!(o.n, 1); // 0 clamps to min
}

#[test]
fn window_overlay_digits_ignored_on_unit_field() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 4,
        unit: WindowUnit::Days,
    });
    o.toggle_field();
    o.push_digit(9);
    assert_eq!(o.n, 4);
}

#[test]
fn window_overlay_window_returns_clamped() {
    let o = TimeWindowOverlay::new(TimeWindow {
        n: 3,
        unit: WindowUnit::Months,
    });
    assert_eq!(
        o.window(),
        TimeWindow {
            n: 3,
            unit: WindowUnit::Months
        }
    );
}

#[test]
fn open_time_window_prefills_from_app_window() {
    let mut app = make_app(1);
    app.window = Some(TimeWindow {
        n: 2,
        unit: WindowUnit::Weeks,
    });
    app.open_time_window();
    match &app.overlay {
        Overlay::TimeWindow(o) => {
            assert_eq!(o.n, 2);
            assert_eq!(o.unit, WindowUnit::Weeks);
        }
        _ => panic!("expected time window overlay"),
    }
}

#[test]
fn window_overlay_number_wraps_at_unit_max() {
    // Hours cap is 48, so 48 -> step_up wraps to 1.
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 48,
        unit: WindowUnit::Hours,
    });
    o.step_up();
    assert_eq!(o.n, 1);
    o.step_down();
    assert_eq!(o.n, 48);
}

#[test]
fn window_overlay_digit_clamps_to_unit_max() {
    // Years cap is 10: typing 4 then 0 (=40) clamps to 10.
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Years,
    });
    o.push_digit(4);
    o.push_digit(0);
    assert_eq!(o.n, 10);
}

#[test]
fn window_overlay_unit_change_clamps_n_down() {
    // 52 weeks, switch forward to Years (max 10): n must clamp to 10.
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 52,
        unit: WindowUnit::Weeks,
    });
    o.toggle_field(); // focus unit
    o.step_down(); // Weeks -> Months (max 24): 52 clamps to 24
    assert_eq!(o.unit, WindowUnit::Months);
    assert_eq!(o.n, 24);
    o.step_down(); // Months -> Years (max 10): 24 clamps to 10
    assert_eq!(o.unit, WindowUnit::Years);
    assert_eq!(o.n, 10);
}

#[test]
fn window_overlay_cycling_through_slot_preserves_count() {
    // Slot ignores the count, so passing through it must not squash n.
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 14,
        unit: WindowUnit::Days,
    });
    o.toggle_field(); // focus unit
    o.step_up(); // Days -> Hours (max 48): 14 fits
    assert_eq!(o.unit, WindowUnit::Hours);
    assert_eq!(o.n, 14);
    o.step_up(); // Hours -> Slot: count must be preserved, not clamped to 1
    assert_eq!(o.unit, WindowUnit::Slot);
    assert_eq!(o.n, 14);
    o.step_up(); // Slot -> Years (max 10): NOW it clamps, to 10
    assert_eq!(o.unit, WindowUnit::Years);
    assert_eq!(o.n, 10);
}

#[test]
fn apply_progress_ratio_and_label() {
    assert_eq!(ApplyProgress::Discovering.ratio(), 0.0);
    assert_eq!(ApplyProgress::Discovering.label(), "discovering…");
    let s = ApplyProgress::Statting {
        done: 12,
        total: 48,
    };
    assert!((s.ratio() - 0.25).abs() < 1e-9);
    assert_eq!(s.label(), "12/48 repos");
    // total 0 must not divide by zero.
    let z = ApplyProgress::Statting { done: 0, total: 0 };
    assert_eq!(z.ratio(), 0.0);
    assert_eq!(ApplyProgress::Finishing.ratio(), 1.0);
    assert_eq!(ApplyProgress::Finishing.label(), "finishing…");
}

fn today() -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str("2026-08-18", "%Y-%m-%d").unwrap()
}

#[test]
fn c_switches_the_source_while_the_strip_is_up() {
    let mut app = make_app(1);
    assert_eq!(
        app.contributions,
        ContributionsMode::Github,
        "defaults to github"
    );
    app.cycle_contributions_source();
    assert_eq!(app.contributions, ContributionsMode::Local);
    app.cycle_contributions_source();
    assert_eq!(app.contributions, ContributionsMode::Github);
}

#[test]
fn c_does_nothing_while_the_strip_is_folded() {
    let mut app = make_app(1);
    app.contributions_folded = true;
    app.cycle_contributions_source();
    assert_eq!(app.contributions, ContributionsMode::Github);
    assert!(app.contributions_folded, "c unfolded the strip");
}

/// The setting's "off" predates folding and meant "not on screen". The
/// strip is always on screen now, so it folds, and keeps its source.
#[test]
fn the_off_setting_folds_the_strip_and_keeps_its_source() {
    let mut app = make_app(1);
    app.apply_contributions_setting(ContributionsMode::Local);
    assert!(!app.contributions_folded);
    app.apply_contributions_setting(ContributionsMode::Off);
    assert!(app.contributions_folded);
    assert_eq!(app.contributions, ContributionsMode::Local);
    app.apply_contributions_setting(ContributionsMode::Github);
    assert!(!app.contributions_folded, "a source setting unfolds it");
    assert_eq!(app.contributions, ContributionsMode::Github);
}

#[test]
fn the_strip_is_always_focusable() {
    let mut app = make_app(1);
    app.contributions_folded = true;
    app.focus_panel(FocusedPanel::Contributions);
    assert_eq!(app.focused_panel, FocusedPanel::Contributions);
}

/// Unlike the other three panels, this one exists in Compact view, so the
/// Detail-only guard on `focus_panel` must not apply to it.
#[test]
fn the_pane_is_focusable_in_compact_view() {
    let mut app = make_app(1);
    app.view = View::Compact;
    app.focus_panel(FocusedPanel::Contributions);
    assert_eq!(app.focused_panel, FocusedPanel::Contributions);
    app.focus_panel(FocusedPanel::Detail);
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Contributions,
        "detail is still unreachable in compact"
    );
}

#[test]
fn tab_walks_all_four_panes() {
    let mut app = make_app(1);
    app.view = View::Detail;
    for next in [
        FocusedPanel::Detail,
        FocusedPanel::Inspector,
        FocusedPanel::Contributions,
        FocusedPanel::RepoList,
    ] {
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, next);
    }
    app.history_view = HistoryView::Graph;
    app.focused_panel = FocusedPanel::Detail;
    app.toggle_panel_focus();
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Contributions,
        "no inspector"
    );
}

#[test]
fn tab_in_compact_toggles_between_the_list_and_the_pane() {
    let mut app = make_app(1);
    app.view = View::Compact;
    app.toggle_panel_focus();
    assert_eq!(app.focused_panel, FocusedPanel::Contributions);
    app.toggle_panel_focus();
    assert_eq!(app.focused_panel, FocusedPanel::RepoList);
}

/// Shift-Tab used to call the forward cycle, so it walked the same way
/// as Tab and "back one pane" took three presses.
#[test]
fn shift_tab_walks_all_four_panes_in_reverse() {
    let mut app = make_app(1);
    app.view = View::Detail;
    for next in [
        FocusedPanel::Contributions,
        FocusedPanel::Inspector,
        FocusedPanel::Detail,
        FocusedPanel::RepoList,
    ] {
        app.toggle_panel_focus_back();
        assert_eq!(app.focused_panel, next);
    }
    for start in [
        FocusedPanel::RepoList,
        FocusedPanel::Detail,
        FocusedPanel::Inspector,
        FocusedPanel::Contributions,
    ] {
        app.focused_panel = start;
        app.toggle_panel_focus();
        app.toggle_panel_focus_back();
        assert_eq!(app.focused_panel, start, "Shift-Tab did not undo Tab");
    }
}

#[test]
fn shift_tab_in_compact_toggles_between_the_list_and_the_pane() {
    let mut app = make_app(1);
    app.view = View::Compact;
    app.toggle_panel_focus_back();
    assert_eq!(app.focused_panel, FocusedPanel::Contributions);
    app.toggle_panel_focus_back();
    assert_eq!(app.focused_panel, FocusedPanel::RepoList);
}

/// #78: the tick advances the GitHub calendar to the local date. A
/// weekday leaves the cursor on its date, and a Sunday shifts the grid a
/// week, so the cursor moves seven slots back to stay on the same day.
#[test]
fn observe_day_keeps_the_cursor_on_its_date() {
    use crate::contributions::{Calendar, Source};
    let d = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();

    let mut app = make_app(1);
    // Scrolled back from the newest edge, so the scroll has to follow.
    app.contrib_visible_weeks.set(20);
    let tuesday = d("2026-08-18");
    app.github_calendar = Some(Box::new(Calendar::new(tuesday, Source::Github)));
    let old_today = app.github_calendar.as_ref().unwrap().today_slot();
    app.contrib_cursor = Some(old_today);
    app.contrib_scroll.set(40);
    app.observe_day(tuesday + chrono::Duration::days(1));
    assert_eq!(app.contrib_cursor, Some(old_today), "weekday: unchanged");
    assert_eq!(app.contrib_scroll.get(), 40, "weekday: same weeks shown");
    assert_eq!(
        app.github_calendar.as_ref().unwrap().today_slot(),
        old_today + 1
    );

    let saturday = d("2026-08-22");
    app.github_calendar = Some(Box::new(Calendar::new(saturday, Source::Github)));
    let old_today = app.github_calendar.as_ref().unwrap().today_slot();
    app.contrib_cursor = Some(old_today);
    app.observe_day(saturday + chrono::Duration::days(1));
    assert_eq!(
        app.contrib_cursor,
        Some(old_today - 7),
        "sunday: a week back"
    );
    assert_eq!(app.contrib_scroll.get(), 39, "sunday: same weeks shown");

    app.contrib_cursor = Some(3);
    app.contrib_scroll.set(0);
    app.observe_day(saturday + chrono::Duration::days(8));
    assert_eq!(app.contrib_cursor, Some(0), "clamped at the first slot");
    assert_eq!(app.contrib_scroll.get(), 0);
}

#[test]
fn observe_day_without_a_github_calendar_changes_nothing() {
    let mut app = make_app(1);
    app.contrib_cursor = Some(12);
    app.contrib_scroll.set(40);
    app.observe_day(today() + chrono::Duration::days(1));
    assert!(app.github_calendar.is_none());
    assert_eq!(app.contrib_cursor, Some(12));
    assert_eq!(app.contrib_scroll.get(), 40);
}

/// A strip showing the newest weeks keeps showing them across a Sunday:
/// shifting its scroll back with the grid would put the new day's column
/// just off the right edge.
#[test]
fn observe_day_keeps_a_strip_pinned_to_the_newest_week() {
    use crate::contributions::{Calendar, Source, WEEKS};
    let saturday = chrono::NaiveDate::from_ymd_opt(2026, 8, 22).unwrap();
    let mut app = make_app(1);
    app.github_calendar = Some(Box::new(Calendar::new(saturday, Source::Github)));
    app.contrib_visible_weeks.set(20);
    app.contrib_scroll.set(WEEKS - 20);
    app.observe_day(saturday + chrono::Duration::days(1));
    assert_eq!(app.contrib_scroll.get(), WEEKS - 20);
}

/// The grid reads as columns of weeks and rows of weekdays, so the keys
/// must move that way: left/right change the week, up/down change the
/// weekday. They were wired the other way round at first, which moved the
/// cursor at right angles to what the eye expected.
#[test]
fn left_right_walks_weeks_and_up_down_walks_weekdays() {
    let mut app = make_app(1);
    let last = app.contrib_last_slot(today());
    app.contrib_cursor = Some(last);

    app.move_contrib_cursor(-1, 0, today());
    assert_eq!(
        app.contrib_cursor,
        Some(last - 7),
        "left is one column back, same weekday"
    );

    app.move_contrib_cursor(0, -1, today());
    assert_eq!(
        app.contrib_cursor,
        Some(last - 8),
        "up is one row, same column"
    );

    app.move_contrib_cursor(1, 0, today());
    assert_eq!(app.contrib_cursor, Some(last - 1), "right returns");
}

/// Opening the strip stands on today (#66). A strip that was never opened
/// (launched with the setting off) has no cursor, and focusing it does not
/// invent one: its first move marks today. A source change reopens the
/// strip, so it returns to today.
#[test]
fn an_unopened_strip_marks_today_on_the_first_move_and_a_source_change_returns_to_it() {
    let mut app = make_app(1);
    assert_eq!(app.contrib_cursor, None, "starting marks nothing");
    app.focus_panel(FocusedPanel::Contributions);
    assert_eq!(app.contrib_cursor, None, "focusing marks nothing");

    // The first keypress lands on today rather than jumping from nowhere.
    app.move_contrib_cursor(-1, 0, today());
    assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today())));

    // And it goes back to today when the source changes.
    app.cycle_contributions_source();
    let now = chrono::Local::now().date_naive();
    assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(now)));
}

/// Opening the strip stands on today, so the first look answers "what
/// did I do today" without a throwaway keypress (#66). A cursor left
/// somewhere else, and a grid scrolled away, both come back to today.
#[test]
fn opening_the_strip_stands_on_today() {
    let mut app = make_app(1);
    app.contrib_cursor = Some(0);
    app.contrib_scroll.set(0);
    app.open_contributions(ContributionsMode::Github, today());
    assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today())));
    assert!(
        app.contrib_scroll.get() >= crate::contributions::WEEKS,
        "still asks for the newest weeks, which holds today's column"
    );
}

/// The first arrow after opening is a real step from today, not the
/// marking-only keypress it used to be.
#[test]
fn the_first_arrow_after_opening_moves_one_step_from_today() {
    let mut app = make_app(1);
    app.focus_panel(FocusedPanel::Contributions);
    let last = app.contrib_last_slot(today());

    app.open_contributions(ContributionsMode::Github, today());
    app.move_contrib_cursor(-1, 0, today());
    assert_eq!(app.contrib_cursor, Some(last - 7), "one week back");

    app.open_contributions(ContributionsMode::Github, today());
    app.move_contrib_cursor(0, -1, today());
    assert_eq!(app.contrib_cursor, Some(last - 1), "one weekday up");
}

/// A source change reopens the strip, so it lands on today as well, even
/// after the user travelled away.
#[test]
fn a_source_change_leaves_the_cursor_on_today() {
    let mut app = make_app(1);
    app.contrib_cursor = Some(0);
    app.cycle_contributions_source();
    assert_eq!(app.contributions, ContributionsMode::Local);
    let today = chrono::Local::now().date_naive();
    assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today)));

    app.contrib_cursor = Some(0);
    app.cycle_contributions_source();
    assert_eq!(app.contributions, ContributionsMode::Github);
    assert_eq!(app.contrib_cursor, Some(app.contrib_last_slot(today)));
}

/// Walking up a column must not spill into the neighbouring week, and
/// walking right must not walk into the future.
#[test]
fn each_axis_clamps_without_spilling_into_the_other() {
    let mut app = make_app(1);
    let last = app.contrib_last_slot(today());
    let (last_week, last_weekday) = (last / 7, last % 7);

    // Top of a column: further up is a no-op, not a jump to last week.
    app.contrib_cursor = Some(last_week * 7);
    app.move_contrib_cursor(0, -1, today());
    assert_eq!(app.contrib_cursor, Some(last_week * 7));

    // 2026-08-18 is a Tuesday, so the newest column ends there.
    app.contrib_cursor = Some(last);
    app.move_contrib_cursor(0, 1, today());
    assert_eq!(
        app.contrib_cursor,
        Some(last),
        "cannot walk into the future"
    );

    app.move_contrib_cursor(1, 0, today());
    assert_eq!(app.contrib_cursor, Some(last), "no column after this one");

    // Entering the short final column from a later weekday slides up to
    // today rather than landing on a cell that does not exist.
    app.contrib_cursor = Some((last_week - 1) * 7 + 6);
    app.move_contrib_cursor(1, 0, today());
    assert_eq!(app.contrib_cursor, Some(last_week * 7 + last_weekday));

    // The oldest column is a hard left edge.
    app.contrib_cursor = Some(3);
    app.move_contrib_cursor(-1, 0, today());
    assert_eq!(app.contrib_cursor, Some(3));
}

#[test]
fn home_and_end_jump_to_the_ends_keeping_the_weekday() {
    let mut app = make_app(1);
    let last = app.contrib_last_slot(today());
    app.contrib_cursor = Some(20 * 7 + 4); // a Thursday mid-year

    app.jump_contrib_cursor(false, today());
    assert_eq!(app.contrib_cursor, Some(4), "oldest column, same weekday");

    app.jump_contrib_cursor(true, today());
    assert_eq!(
        app.contrib_cursor,
        Some(last),
        "newest column clamps to today"
    );
}

/// The grid is wider than the terminal, so moving off the visible window
/// must bring it back rather than losing the cursor off-screen.
#[test]
fn moving_off_the_window_scrolls_the_grid() {
    let mut app = make_app(1);
    app.contrib_visible_weeks.set(10);
    app.contrib_scroll.set(40);
    app.contrib_cursor = Some(40 * 7);

    app.move_contrib_cursor(-1, 0, today());
    assert_eq!(app.contrib_scroll.get(), 39, "scrolled one column left");

    app.contrib_scroll.set(0);
    app.contrib_cursor = Some(9 * 7);
    app.move_contrib_cursor(1, 0, today());
    assert_eq!(app.contrib_scroll.get(), 1, "scrolled one column right");
}

#[test]
fn scrolling_clamps_to_the_ends_of_the_year() {
    let mut app = make_app(1);
    app.contrib_visible_weeks.set(10);
    app.contrib_scroll.set(0);

    app.scroll_contributions(-5);
    assert_eq!(app.contrib_scroll.get(), 0, "cannot scroll before the year");

    app.scroll_contributions(1000);
    assert_eq!(
        app.contrib_scroll.get(),
        crate::contributions::WEEKS - 10,
        "cannot scroll past the newest week"
    );
}

/// Switching the source pins the newest weeks to the right edge: the grid
/// should show now, not last August. The request is deliberately
/// past the end, because only the renderer knows how many weeks fit; it
/// clamps this down on the next frame.
#[test]
fn switching_the_source_asks_for_the_newest_weeks() {
    let mut app = make_app(1);
    app.contrib_scroll.set(0);
    app.cycle_contributions_source();
    assert!(
        app.contrib_scroll.get() >= crate::contributions::WEEKS,
        "an out-of-range request is what anchors the grid right"
    );
}

#[test]
fn the_local_calendar_sums_only_the_visible_repos() {
    let mut app = make_app(0);
    let mut a = crate::contributions::DailyCounts::default();
    a.0[0] = 3;
    let mut b = crate::contributions::DailyCounts::default();
    b.0[0] = 4;
    let mut alpha = make_app(1).repos.remove(0);
    alpha.name = "alpha".into();
    alpha.daily = Some(Box::new(a));
    let mut beta = make_app(1).repos.remove(0);
    beta.name = "beta".into();
    beta.daily = Some(Box::new(b));
    app.repos = vec![alpha, beta];
    assert_eq!(app.local_calendar(today()).total(), 7, "both visible");

    app.search_query = "alpha".to_string();
    assert_eq!(
        app.local_calendar(today()).total(),
        3,
        "a search narrows the heatmap the same way it narrows the list"
    );
}

#[test]
fn a_repo_with_no_histogram_contributes_nothing() {
    let app = make_app(1);
    assert_eq!(app.local_calendar(today()).total(), 0);
    assert_eq!(
        app.local_calendar(today()).source(),
        crate::contributions::Source::Local
    );
}

/// The row cycles the three sizes both ways, starts from what the app
/// holds, and writes the key `Config` reads back. A row whose key did not
/// match would save happily and change nothing.
#[test]
fn the_cell_size_row_cycles_and_writes_its_key() {
    let mut overlay = SettingsOverlay::new(
        60,
        60,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Off,
        CellSize::Medium,
        true,
        Icons::Nerd,
    );
    overlay.cursor = SETTING_FIELDS
        .iter()
        .position(|f| *f == SettingField::ContribCellSize)
        .unwrap();
    assert_eq!(SettingField::ContribCellSize.applies(), Applies::Live);
    assert_eq!(overlay.value_label(SettingField::ContribCellSize), "medium");
    overlay.step(1);
    assert_eq!(overlay.value_label(SettingField::ContribCellSize), "large");
    overlay.step(1);
    assert_eq!(overlay.value_label(SettingField::ContribCellSize), "small");
    overlay.step(-1);
    assert_eq!(overlay.value_label(SettingField::ContribCellSize), "large");

    let updates = overlay.config_updates();
    let written = updates
        .iter()
        .find(|(key, _)| *key == "contributions_cell_size")
        .map(|(_, v)| v.clone());
    assert_eq!(written, Some(crate::config::SettingValue::Str("large")));
    let cfg: crate::config::Config =
        toml::from_str(r#"contributions_cell_size = "large""#).unwrap();
    assert_eq!(
        CellSize::from_config(cfg.contributions_cell_size.as_deref().unwrap()),
        CellSize::Large
    );
}

#[test]
fn the_settings_row_cycles_the_three_modes() {
    let mut overlay = SettingsOverlay::new(
        60,
        60,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Off,
        CellSize::Medium,
        true,
        Icons::Nerd,
    );
    overlay.cursor = SETTING_FIELDS
        .iter()
        .position(|f| *f == SettingField::Contributions)
        .unwrap();
    assert_eq!(overlay.value_label(SettingField::Contributions), "off");
    overlay.step(1);
    assert_eq!(overlay.value_label(SettingField::Contributions), "github");
    overlay.step(1);
    assert_eq!(overlay.value_label(SettingField::Contributions), "local");
    overlay.step(1);
    assert_eq!(
        overlay.value_label(SettingField::Contributions),
        "off",
        "wraps"
    );
    overlay.step(-1);
    assert_eq!(
        overlay.value_label(SettingField::Contributions),
        "local",
        "wraps back"
    );
}

#[test]
fn the_settings_row_writes_the_contributions_key() {
    assert_eq!(SettingField::Contributions.config_key(), "contributions");
    assert_eq!(SettingField::Contributions.applies(), Applies::Live);
    let overlay = SettingsOverlay::new(
        60,
        60,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Local,
        CellSize::Medium,
        true,
        Icons::Nerd,
    );
    let updates = overlay.config_updates();
    assert!(
        updates.iter().any(|(k, v)| *k == "contributions"
            && matches!(v, crate::config::SettingValue::Str("local")))
    );
}

/// A failed initial load used to set only a status message, which the
/// next keypress clears. The repo list is derived from the GitHub dataset,
/// so the result was a blank dashboard with no stated cause -- identical
/// to a fleet with nothing in the window.
#[test]
fn an_empty_fleet_with_a_failed_load_explains_itself() {
    let mut app = make_app(0);
    assert!(!app.shows_load_error(), "nothing has failed yet");
    app.load_error = Some("rate limited".to_string());
    assert!(app.shows_load_error());
}

/// The guard's other half: a reload that fails while rows are on screen
/// must keep them. Replacing live data with an error panel loses more
/// than it explains.
#[test]
fn a_failed_reload_keeps_a_populated_fleet_on_screen() {
    let mut app = make_app(3);
    app.load_error = Some("rate limited".to_string());
    assert!(
        !app.shows_load_error(),
        "the error panel would have replaced three good rows"
    );
}

/// Scrolling and the cursor are independent: the wheel used to call
/// `detail_nav_down`, so it moved the selection instead of the view and
/// the pane could only jump from commit to commit.
///
/// Asserted against `commits` rather than `graph`: the graph view now
/// drags its marker with the view deliberately (see below), and it is the
/// only view that does.
#[test]
fn scrolling_the_detail_pane_leaves_the_selection_alone() {
    let mut app = make_app(3);
    app.history_view = HistoryView::Commits;
    app.detail_max_scroll.set(20);
    app.set_detail_selected(2);
    let before = app.detail_selected();
    app.scroll_detail(5);
    app.scroll_detail(-2);
    assert_eq!(app.detail_selected(), before, "scrolling moved the cursor");
}

/// The graph view is the exception, and its cursor is a *row*. Blocks run
/// past twenty rows, so a commit-granular marker stood still through a
/// whole commit and then jumped -- the highlight was nowhere while you
/// read one. The cursor walks the pane a row at a time instead.
fn graph_app() -> App {
    let mut app = make_app(1);
    app.history_view = HistoryView::Graph;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    *app.detail_row_tops.borrow_mut() = vec![0, 10, 25, 60];
    app
}

/// Scrolloff: the highlight travels down the pane while the text holds
/// still, and once it is `SCROLLOFF` from the bottom the text starts
/// moving under it instead.
#[test]
fn the_graph_cursor_walks_the_pane_before_the_pane_moves() {
    let mut app = graph_app();

    for _ in 0..15 {
        app.scroll_detail(1);
    }
    assert_eq!(app.graph_cursor, 15, "one row per press");
    assert_eq!(
        app.detail_scroll(),
        0,
        "the text moved while the highlight still had room"
    );

    app.scroll_detail(1);
    assert_eq!(app.graph_cursor, 16);
    assert_eq!(app.detail_scroll(), 1, "now the text moves under it");

    for _ in 0..5 {
        app.scroll_detail(1);
    }
    assert_eq!((app.graph_cursor, app.detail_scroll()), (21, 6));
    assert_eq!(
        app.graph_cursor - app.detail_scroll(),
        15,
        "the highlight is parked four rows off the bottom"
    );
}

/// The same margin at the other edge, walking back up.
#[test]
fn the_graph_cursor_keeps_its_margin_going_up() {
    let mut app = graph_app();
    for _ in 0..21 {
        app.scroll_detail(1);
    }
    assert_eq!((app.graph_cursor, app.detail_scroll()), (21, 6));

    for _ in 0..11 {
        app.scroll_detail(-1);
    }
    assert_eq!(app.graph_cursor, 10);
    assert_eq!(
        app.detail_scroll(),
        6,
        "the highlight came up, not the text"
    );

    app.scroll_detail(-1);
    assert_eq!((app.graph_cursor, app.detail_scroll()), (9, 5));
}

/// The cursor cannot walk off the end of the content, and does not bank
/// phantom offset there either -- one press back must move it.
#[test]
fn the_graph_cursor_stops_at_the_last_row() {
    let mut app = graph_app();
    app.scroll_detail(10_000);
    // max_scroll + height - 1: the last row the pane can show.
    assert_eq!(app.graph_cursor, 119);
    app.scroll_detail(-1);
    assert_eq!(app.graph_cursor, 118, "moves again immediately");
}

/// The commit the cursor is inside is what the view calls selected, so the
/// hash emphasis and `inspected_hash` follow the highlight through a block
/// rather than waiting for its last row.
#[test]
fn the_cursor_selects_the_commit_it_is_inside() {
    let mut app = graph_app();
    app.scroll_detail(9);
    assert_eq!(app.detail_selected(), 0);
    app.scroll_detail(1);
    assert_eq!(app.detail_selected(), 1, "row 10 opens the second commit");
    app.scroll_detail(20);
    assert_eq!(app.detail_selected(), 2);
}

/// Stepping a commit shows its block from the top. Bringing the row into
/// view by the smallest nudge would leave it in the *bottom* margin: four
/// rows of the commit you stepped to, and the rest of it below the fold.
#[test]
fn stepping_a_commit_shows_its_block_from_the_top() {
    let mut app = graph_app();

    app.set_detail_selected(2);
    assert_eq!(app.graph_cursor, 25, "the cursor moved to the commit's row");
    assert_eq!(
        app.detail_scroll(),
        21,
        "which sits four rows down the pane"
    );

    // Already comfortably on screen: stepping must not jerk the view.
    let before = app.detail_scroll();
    app.set_detail_selected(2);
    assert_eq!(app.detail_scroll(), before);
}

/// Moving the cursor must not ask the pane to follow it: the next frame
/// would bring the whole block into view and undo the scrolloff the cursor
/// just established.
#[test]
fn moving_the_graph_cursor_does_not_request_follow() {
    let mut app = graph_app();
    app.follow_selection.set(false);
    app.scroll_detail(12);
    assert_eq!(app.detail_selected(), 1);
    assert!(
        !app.follow_selection.get(),
        "the cursor asked the renderer to follow, which would undo it"
    );
}

/// From the repo list, Shift+J/K drives window 2's cursor rather than its
/// scroll offset: `j`/`k` there belong to the repo list, so this is the
/// only way to step commits without leaving window 1, and stepping the
/// cursor is what makes the inspector read each one out. It used to scroll
/// the pane, which moved the text past a cursor that stayed put and told
/// the inspector nothing.
#[test]
fn shift_scroll_from_the_repo_list_steps_the_detail_cursor() {
    let mut app = make_app(1);
    // The graph, because that is the list `info` draws and therefore the
    // one its cursor is bounded by.
    app.repos[0].graph = (0..6)
        .map(|i| crate::model::GraphLine {
            lanes: vec![crate::graph::commit_cell()],
            hash: format!("c0ffee{i}"),
            is_merge: false,
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("feat: thing {i}"),
        })
        .collect();
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::RepoList;
    app.history_view = HistoryView::Info;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    // Six commits, as `info` shows.
    *app.detail_row_tops.borrow_mut() = (0..6).collect();

    app.scroll_next_panel(1);
    assert_eq!(app.detail_selected(), 1, "the inspector was told nothing");
    app.scroll_next_panel(1);
    assert_eq!(app.detail_selected(), 2);
    app.scroll_next_panel(-1);
    assert_eq!(app.detail_selected(), 1);
}

/// Past the last commit it falls through to scrolling, so the branches and
/// stashes below a short list stay reachable from the repo list too.
#[test]
fn shift_scroll_past_the_last_commit_scrolls_instead() {
    let mut app = make_app(1);
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::RepoList;
    app.history_view = HistoryView::Info;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    *app.detail_row_tops.borrow_mut() = (0..6).collect();

    // `make_app` builds repos with no commits, so the cursor is already at
    // the end of the list and every step is a scroll.
    assert_eq!(app.selected_commit_count(), 0);
    app.scroll_next_panel(1);
    assert_eq!(app.detail_scroll(), 1);
}

/// From the detail pane Shift+scroll reaches past it into the inspector,
/// so the commit under the cursor can be read further down without leaving
/// the pane that chose it.
///
/// It used to scroll the detail pane itself. `PageUp`/`PageDown` still do
/// that, so the gesture was not lost, only moved off a key that now has
/// somewhere better to point.
#[test]
fn shift_scroll_from_the_detail_pane_drives_the_inspector() {
    let mut app = make_app(1);
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::Detail;
    app.history_view = HistoryView::Commits;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    *app.detail_row_tops.borrow_mut() = (0..40).collect();
    assert!(app.shows_inspector(), "no inspector to aim at");
    assert_eq!(
        app.shift_scroll_target(),
        Some(FocusedPanel::Inspector),
        "the detail pane does not aim at the inspector"
    );

    // The inspector's own offset is clamped against its content, so give
    // it something to scroll before asking whether it moved.
    app.inspector_max_scroll.set(10);
    let scroll_before = app.detail_scroll();
    app.scroll_next_panel(1);
    assert_eq!(app.inspector_scroll, 1, "the inspector did not scroll");
    assert_eq!(
        app.detail_scroll(),
        scroll_before,
        "the detail pane scrolled itself instead of the pane after it"
    );
}

/// In the graph the shifted keys stay on the pane and walk its *rows*,
/// one line at a time, where plain `j`/`k` jumps whole commit blocks. The
/// graph is the only view whose rows are finer than its commits, so it is
/// the only one with a line-by-line step to offer.
#[test]
fn shift_scroll_walks_graph_rows_one_line_at_a_time() {
    let mut app = make_app(1);
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::Detail;
    app.history_view = HistoryView::Graph;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);

    assert_eq!(
        app.shift_scroll_target(),
        Some(FocusedPanel::Detail),
        "the graph aimed somewhere other than itself"
    );

    assert_eq!(app.graph_cursor, 0);
    app.scroll_next_panel(1);
    assert_eq!(app.graph_cursor, 1, "did not step a single row");
    app.scroll_next_panel(1);
    assert_eq!(app.graph_cursor, 2);
    app.scroll_next_panel(-1);
    assert_eq!(app.graph_cursor, 1, "did not step back a single row");
}

/// ...and it does not reach pane 1 to do it. The graph has no inspector to
/// pass the gesture on to, and wrapping to the repo list would have spent
/// the only line-by-line step the graph has on changing repo.
#[test]
fn shift_scroll_in_the_graph_leaves_the_repo_list_alone() {
    let mut app = make_app(3);
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::Detail;
    app.history_view = HistoryView::Graph;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);

    let repo_before = app.selected;
    app.scroll_next_panel(1);
    app.scroll_next_panel(1);
    assert_eq!(
        app.selected, repo_before,
        "the graph's shifted step changed which repo is selected"
    );
}

/// The strip is never a target: it is a fixed-height horizontal timeline
/// with nothing below the fold, so it is skipped rather than left in the
/// cycle as a stop where the key would do nothing.
#[test]
fn shift_scroll_never_aims_at_the_contributions_strip() {
    let mut app = make_app(1);
    app.view = View::Detail;
    app.open_contributions(ContributionsMode::Github, today());
    for focused in [
        FocusedPanel::RepoList,
        FocusedPanel::Detail,
        FocusedPanel::Inspector,
        FocusedPanel::Contributions,
    ] {
        app.focused_panel = focused;
        assert_ne!(
            app.shift_scroll_target(),
            Some(FocusedPanel::Contributions),
            "{focused:?} aimed at the strip"
        );
    }
}

/// Compact has one flat grid and no pane after the list, so the key has
/// nothing to aim at rather than quietly driving a pane that is not drawn.
#[test]
fn shift_scroll_has_no_target_in_compact() {
    let mut app = make_app(1);
    app.view = View::Compact;
    app.focused_panel = FocusedPanel::RepoList;
    assert_eq!(app.shift_scroll_target(), None);
}

/// Scrolling leaves the selection with the content it is attached to.
///
/// It used to drag the selection to the nearest visible entry on every
/// wheel notch, which meant scrolling silently reassigned which commit the
/// inspector was reading out: the highlight crept down the pane on its own
/// and the panel below it changed under you. Deliberately reversed --
/// scrolling is a view gesture, not a selection one.
#[test]
fn scrolling_does_not_move_the_selection() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Commits;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    // One row per commit, as the list draws them.
    *app.detail_row_tops.borrow_mut() = (0..40).collect();

    let before = app.detail_selected();
    app.scroll_detail(10);
    assert_eq!(
        app.detail_selected(),
        before,
        "the wheel moved the selection"
    );
    app.scroll_detail(20);
    assert_eq!(
        app.detail_selected(),
        before,
        "the wheel moved the selection"
    );
    app.scroll_detail(-5);
    assert_eq!(
        app.detail_selected(),
        before,
        "the wheel moved the selection"
    );
}

/// ...and the first step afterwards re-enters at the edge you are looking
/// at, rather than carrying on from a row that is no longer on screen.
/// That step is spent on the re-entry, so the highlight appears where the
/// eye already is before it starts moving again.
#[test]
fn the_first_step_after_a_scroll_re_enters_the_pane() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Commits;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    *app.detail_row_tops.borrow_mut() = (0..40).collect();

    app.scroll_detail(10);
    assert_eq!(app.detail_selected(), 0, "the wheel moved the selection");

    app.detail_nav_down();
    assert_eq!(
        app.detail_selected(),
        14,
        "did not re-enter at the top edge of the view"
    );
}

/// Scrolling back up re-enters from the bottom edge instead, so the
/// re-entry always lands on the side the selection went out of.
#[test]
fn a_step_after_scrolling_up_re_enters_from_the_bottom() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Commits;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    *app.detail_row_tops.borrow_mut() = (0..40).collect();

    app.set_detail_selected(30);
    app.scroll_detail(0_i32.saturating_sub(0));
    app.set_detail_scroll(0);
    app.detail_nav_up();
    assert_eq!(
        app.detail_selected(),
        15,
        "did not re-enter at the bottom edge of the view"
    );
}

/// A selection already on screen is left alone, so stepping through
/// commits that share a screen does not stutter on a phantom re-entry.
#[test]
fn a_visible_selection_steps_without_re_entering() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Commits;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    *app.detail_row_tops.borrow_mut() = (0..40).collect();

    // `Commits` counts graph rows, and a step past the last one falls
    // through to scrolling, so the list has to be long enough to step in.
    app.repos[0].graph = (0..40)
        .map(|i| crate::model::GraphLine {
            hash: format!("c{i:06}"),
            refs: vec![],
            author: "Gaurgle".to_string(),
            message: format!("feat: thing {i}"),
            lanes: vec![],
            is_merge: false,
        })
        .collect();
    app.set_detail_scroll(10);
    app.set_detail_selected(16);
    app.detail_nav_down();
    assert_eq!(
        app.detail_selected(),
        17,
        "a visible selection did not step"
    );
}

/// The margin collapses at the ends of the content: held there, the first
/// and last entries could never be selected.
#[test]
fn the_first_entry_is_reachable_at_the_top_of_the_content() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Commits;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    *app.detail_row_tops.borrow_mut() = (0..40).collect();

    app.set_detail_selected(0);
    app.scroll_detail(0);
    assert_eq!(app.detail_selected(), 0, "row 0 was pushed off its own row");
}

/// `info` draws branches, worktrees and stashes below its commit list, and
/// scrolling past the last commit to read them is the point. With no entry
/// left inside the pane the selection stays where it is rather than being
/// dragged to an edge.
#[test]
fn scrolling_past_a_list_leaves_its_selection_alone() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Info;
    app.detail_view_height.set(20);
    app.detail_max_scroll.set(100);
    // Six commits at the top, then sections that own no commit.
    *app.detail_row_tops.borrow_mut() = (0..6).collect();
    app.set_detail_selected(5);

    app.scroll_detail(60);
    assert_eq!(
        app.detail_scroll(),
        60,
        "the sections below stayed reachable"
    );
    assert_eq!(app.detail_selected(), 5, "and the cursor was not dragged");
}

/// The graph block already carries hash, author, date and message, so an
/// inspector there repeats it while taking the rows the block needs.
#[test]
fn the_graph_view_hides_the_inspector() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Commits;
    assert!(app.shows_inspector());
    app.history_view = HistoryView::Graph;
    assert!(!app.shows_inspector());
}

/// Tab must not park focus on a pane that is not drawn: every later
/// keypress would go somewhere invisible.
#[test]
fn focus_cycling_skips_a_hidden_inspector() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Graph;
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::Detail;
    app.toggle_panel_focus();
    assert_ne!(
        app.focused_panel,
        FocusedPanel::Inspector,
        "focus landed on a pane the graph view does not draw"
    );
}

/// The reverse walk has the same trap from the other side: back from the
/// strip is the inspector, unless the graph view hides it.
#[test]
fn reverse_focus_cycling_skips_a_hidden_inspector() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Graph;
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::Contributions;
    app.toggle_panel_focus_back();
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Detail,
        "focus landed on a pane the graph view does not draw"
    );
}

/// The same trap by another door: `3` jumps straight to the inspector.
#[test]
fn jumping_to_a_hidden_inspector_is_refused() {
    let mut app = make_app(1);
    app.history_view = HistoryView::Graph;
    app.view = View::Detail;
    app.focused_panel = FocusedPanel::Detail;
    app.focus_panel(FocusedPanel::Inspector);
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Detail,
        "focus went to a hidden pane"
    );
}

/// And by the third: switching into the graph view while the inspector
/// holds focus has to hand it back.
#[test]
fn entering_the_graph_view_moves_focus_off_the_inspector() {
    let mut app = make_app(1);
    app.view = View::Detail;
    app.history_view = HistoryView::Reflog;
    app.focused_panel = FocusedPanel::Inspector;
    // HistoryView::ALL is [Info, Commits, Reflog, Graph], so Reflog steps
    // forward into Graph.
    app.cycle_history_view(false);
    assert_eq!(app.history_view, HistoryView::Graph, "fixture assumption");
    assert_eq!(
        app.focused_panel,
        FocusedPanel::Detail,
        "focus left on a hidden pane"
    );
}

fn fold_app() -> App {
    let mut app = App::new(Vec::new(), String::new(), 120, 44);
    app.view = View::Detail;
    app.contributions = ContributionsMode::Github;
    app
}

fn is_folded(app: &App, panel: FocusedPanel) -> bool {
    match panel {
        FocusedPanel::Inspector => app.inspector_folded,
        _ => app.contributions_folded,
    }
}

const FOLDABLE: [FocusedPanel; 2] = [FocusedPanel::Inspector, FocusedPanel::Contributions];

#[test]
fn a_pane_number_on_an_unfocused_pane_focuses_it_without_folding() {
    for panel in FOLDABLE {
        let mut app = fold_app();
        app.press_panel_key(panel);
        assert_eq!(app.focused_panel, panel);
        assert!(!is_folded(&app, panel), "{panel:?} folded on first press");
    }
}

#[test]
fn the_focused_panes_number_folds_it_and_keeps_focus() {
    for panel in FOLDABLE {
        let mut app = fold_app();
        app.press_panel_key(panel);
        app.press_panel_key(panel);
        assert!(is_folded(&app, panel), "{panel:?}");
        assert_eq!(app.focused_panel, panel, "focus left the folded pane");
        app.press_panel_key(panel);
        assert!(!is_folded(&app, panel), "{panel:?} did not unfold");
        assert_eq!(app.focused_panel, panel);
    }
}

#[test]
fn a_folded_panes_number_from_elsewhere_opens_and_focuses_it() {
    for panel in FOLDABLE {
        let mut app = fold_app();
        app.toggle_fold(panel);
        app.focus_panel(FocusedPanel::RepoList);
        app.press_panel_key(panel);
        assert!(!is_folded(&app, panel), "{panel:?}");
        assert_eq!(app.focused_panel, panel);
    }
}

/// The graph view draws no inspector, so `3` there neither focuses nor
/// folds anything.
#[test]
fn three_does_nothing_where_there_is_no_inspector() {
    let mut app = fold_app();
    app.history_view = HistoryView::Graph;
    app.press_panel_key(FocusedPanel::Inspector);
    assert_eq!(app.focused_panel, FocusedPanel::RepoList);
    assert!(!app.inspector_folded);
}

#[test]
fn the_list_and_detail_numbers_only_focus() {
    let mut app = fold_app();
    for panel in [FocusedPanel::RepoList, FocusedPanel::Detail] {
        app.press_panel_key(panel);
        app.press_panel_key(panel);
        assert_eq!(app.focused_panel, panel);
    }
    app.toggle_fold(FocusedPanel::RepoList);
    app.toggle_fold(FocusedPanel::Detail);
    assert!(!app.inspector_folded && !app.contributions_folded);
}

#[test]
fn clicking_the_tag_folds_in_place_and_unfolds_into_focus() {
    for panel in FOLDABLE {
        let mut app = fold_app();
        app.focus_panel(FocusedPanel::Detail);
        app.toggle_fold(panel);
        assert!(is_folded(&app, panel));
        assert_eq!(app.focused_panel, FocusedPanel::Detail, "fold moved focus");
        app.toggle_fold(panel);
        assert!(!is_folded(&app, panel));
        assert_eq!(app.focused_panel, panel, "unfold did not focus");
    }
}

/// Shift-scroll reads a pane whose rows it can see; a folded inspector
/// has none, so the detail pane walks its own rows instead.
#[test]
fn shift_scroll_skips_a_folded_inspector() {
    let mut app = fold_app();
    app.inspector_folded = true;
    app.focus_panel(FocusedPanel::Detail);
    assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Detail));
}

/// Saving a source in settings is asking to see it.
#[test]
fn saving_a_source_setting_unfolds_the_strip() {
    let mut app = fold_app();
    app.toggle_fold(FocusedPanel::Contributions);
    app.apply_contributions_setting(ContributionsMode::Local);
    assert!(!app.contributions_folded);
}

fn at(h: u32, m: u32) -> DateTime<Local> {
    use chrono::TimeZone;
    Local
        .with_ymd_and_hms(2026, 10, 1, h, m, 0)
        .single()
        .expect("unambiguous local time")
}

fn offline() -> crate::github::Failure {
    crate::github::Failure::new(crate::github::FailureKind::Offline, "no route")
}

/// Rule 1: reading the cache sets the data time from the file and is not
/// an attempt, so the retry clock stays unset and the poll is due.
#[test]
fn a_cache_read_is_not_an_attempt() {
    let mut f = GithubFreshness::default();
    f.loaded(
        &DatasetSource::Cache {
            written: Some(at(6, 0)),
        },
        at(9, 0),
    );
    assert_eq!(f.data_time, DataTime::At(at(6, 0)));
    assert_eq!(f.last_attempt, None);
    assert_eq!(f.outcome, Outcome::NotAttempted);
    assert!(f.retry_due(at(9, 0), 90));
}

/// An unreadable write time stays unknown, never the time it was read.
#[test]
fn an_unreadable_cache_age_stays_unknown() {
    let mut f = GithubFreshness::default();
    f.loaded(&DatasetSource::Cache { written: None }, at(9, 0));
    assert_eq!(f.data_time, DataTime::Unknown);
    f.failed(offline(), at(9, 1));
    assert_eq!(f.data_time, DataTime::Unknown, "a failure made the age up");
}

/// Rule 3: only a success advances the data time. Every kind of failure,
/// the give-up among them, leaves it where it was.
#[test]
fn a_failure_of_any_kind_leaves_the_data_time() {
    use crate::github::{Failure, FailureKind};
    for kind in [
        FailureKind::Offline,
        FailureKind::Http,
        FailureKind::Api,
        FailureKind::Auth,
        FailureKind::Malformed,
    ] {
        let mut f = GithubFreshness::default();
        f.succeeded(at(9, 0));
        f.failed(Failure::new(kind, "x"), at(9, 5));
        assert_eq!(f.data_time, DataTime::At(at(9, 0)), "{kind:?}");
        assert_eq!(f.outcome, Outcome::Failed(Failure::new(kind, "x")));
    }
    let mut f = GithubFreshness::default();
    f.succeeded(at(9, 0));
    f.gave_up(std::time::Duration::from_secs(60), at(9, 5));
    assert_eq!(f.data_time, DataTime::At(at(9, 0)));
}

/// Rule 6: every finished attempt advances the retry clock, whatever it
/// ended in, so a failure waits out an interval like a success does.
#[test]
fn every_finished_attempt_advances_the_retry_clock() {
    let mut f = GithubFreshness::default();
    f.failed(offline(), at(9, 0));
    assert_eq!(f.last_attempt, Some(at(9, 0)));
    assert!(!f.retry_due(at(9, 1), 90));
    assert!(f.retry_due(at(9, 2), 90));
    f.gave_up(std::time::Duration::from_secs(60), at(9, 10));
    assert_eq!(f.last_attempt, Some(at(9, 10)));
    assert!(matches!(
        &f.outcome,
        Outcome::Failed(x) if x.kind == crate::github::FailureKind::TimedOut
    ));
}

/// Rule 7: a success clears the failure and makes the data current.
#[test]
fn a_success_clears_the_failure() {
    let mut f = GithubFreshness::default();
    f.failed(offline(), at(9, 0));
    assert!(f.is_failing());
    f.succeeded(at(9, 2));
    assert!(!f.is_failing());
    assert_eq!(f.outcome, Outcome::Success);
    assert_eq!(f.data_time, DataTime::At(at(9, 2)));
    assert_eq!(f.last_attempt, Some(at(9, 2)));
}

/// The `R` fallback row of the transition table: the rows are rebuilt
/// from the cache, so the data time is the file's, and the attempt is a
/// failure.
#[test]
fn a_reload_fallback_takes_the_files_time_and_records_the_failure() {
    let mut f = GithubFreshness::default();
    f.succeeded(at(9, 0));
    f.loaded(
        &DatasetSource::Fallback {
            failure: offline(),
            written: Some(at(9, 0)),
        },
        at(10, 0),
    );
    assert_eq!(f.data_time, DataTime::At(at(9, 0)));
    assert_eq!(f.last_attempt, Some(at(10, 0)));
    assert_eq!(f.outcome, Outcome::Failed(offline()));
}

/// A load with no GitHub fetch behind it (`-d`, a window apply) says
/// nothing about freshness.
#[test]
fn a_local_load_changes_no_freshness_fact() {
    let mut f = GithubFreshness::default();
    f.failed(offline(), at(9, 0));
    let before = f.clone();
    f.loaded(&DatasetSource::Local, at(10, 0));
    assert_eq!(f, before);
}

/// A load that went to GitHub supersedes every poll started before it;
/// a cache read or a local load supersedes none.
#[test]
fn only_a_load_from_github_supersedes_earlier_polls() {
    for (source, supersedes) in [
        (DatasetSource::Network, true),
        (
            DatasetSource::Fallback {
                failure: offline(),
                written: None,
            },
            true,
        ),
        (DatasetSource::Cache { written: None }, false),
        (DatasetSource::Local, false),
    ] {
        let mut f = GithubFreshness::default();
        let poll = f.begin_attempt();
        f.loaded(&source, at(9, 0));
        assert_eq!(!f.accept_answer(poll), supersedes, "{source:?}");
    }
}

/// Rule 10: an answer is applied only when no newer attempt's answer
/// has been, in either completion order.
#[test]
fn an_answer_older_than_the_one_applied_is_discarded() {
    let mut f = GithubFreshness::default();
    let first = f.begin_attempt();
    let second = f.begin_attempt();
    assert!(f.is_latest(second) && !f.is_latest(first));
    assert!(f.accept_answer(second));
    assert!(!f.accept_answer(first), "an older answer was applied");

    let mut g = GithubFreshness::default();
    let first = g.begin_attempt();
    let second = g.begin_attempt();
    assert!(g.accept_answer(first), "nothing newer had landed");
    assert!(g.accept_answer(second));
}

use crate::ui::badges::{active_icons, set_active_icons};

/// One repo of each kind, every one carrying a stash, a linked worktree
/// and an owner, so each glyph the `icons` key covers is on screen.
/// `extra` more repos cycle through the kinds, for a grid of several rows.
fn icon_fleet(extra: usize) -> App {
    use crate::model::RepoKind;
    let kinds = [
        RepoKind::Org,
        RepoKind::Private,
        RepoKind::Public,
        RepoKind::Unknown,
    ];
    let mut app = make_app(kinds.len() + extra);
    for (i, repo) in app.repos.iter_mut().enumerate() {
        repo.repo_kind = kinds[i % kinds.len()];
        repo.owner = Some("acme".to_string());
        repo.stash_count = 2;
        repo.worktrees = vec![crate::model::Worktree {
            path: format!("~/repos/repo-{i}-wt"),
            branch: "wt".to_string(),
            ..Default::default()
        }];
        // Uneven badge groups, so a right edge that lines up is evidence.
        repo.pr_count = i as u32 % 2;
    }
    app
}

const SCREEN_W: u16 = 120;
const SCREEN_H: u16 = 40;

fn draw_screen(app: &App, width: u16) -> ratatui::buffer::Buffer {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut terminal = Terminal::new(TestBackend::new(width, SCREEN_H)).unwrap();
    terminal.draw(|f| crate::ui::render(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn is_private_use(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..)
}

/// Every cell holding a private-use codepoint, as `(x, y, symbol)`,
/// outside the contribution strip: its day squares are FZ-13h, not part
/// of the `icons` key. The strip's rectangle comes from `panel_rects`, the
/// same function the renderer lays out with.
fn private_use_cells(app: &App, buffer: &ratatui::buffer::Buffer) -> Vec<(u16, u16, String)> {
    let area = buffer.area;
    let strip = crate::ui::panel_rects(area, app).contributions;
    let mut found = Vec::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if strip.is_some_and(|s| s.contains(ratatui::layout::Position { x, y })) {
                continue;
            }
            let symbol = buffer[(x, y)].symbol();
            if symbol.chars().any(is_private_use) {
                found.push((x, y, symbol.to_string()));
            }
        }
    }
    found
}

/// Where `text` starts on row `y` inside `area`, matched cell by cell.
fn find_in_row(
    buffer: &ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    y: u16,
    text: &str,
) -> Option<u16> {
    let wanted: Vec<String> = text.chars().map(String::from).collect();
    (area.left()..area.right()).find(|&x| {
        wanted.iter().enumerate().all(|(i, c)| {
            let cx = x + i as u16;
            cx < area.right() && buffer[(cx, y)].symbol() == c
        })
    })
}

/// `(x, y)` of each repo's name inside `area`, first match from the top.
fn name_cells(
    app: &App,
    buffer: &ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
) -> Vec<(u16, u16)> {
    app.repos
        .iter()
        .map(|r| {
            (area.top()..area.bottom())
                .find_map(|y| {
                    find_in_row(buffer, area, y, &format!(" {} ", r.name)).map(|x| (x + 1, y))
                })
                .unwrap_or_else(|| panic!("{} not drawn", r.name))
        })
        .collect()
}

/// The list, every detail pane and the header, in both watch states,
/// draw nothing from the private-use area under `icons = "plain"`.
/// Fails if any of the six glyphs, or a call site drawing one, still
/// reaches for a Nerd Font codepoint in `plain`.
#[test]
fn plain_icons_draw_no_private_use_glyph_in_the_detail_view() {
    let mut app = icon_fleet(0);
    app.view = View::Detail;
    set_active_icons(Icons::Plain);
    for selected in 0..app.repos.len() {
        app.selected = selected;
        for watch in [true, false] {
            app.watch_mode = watch;
            let buffer = draw_screen(&app, SCREEN_W);
            let found = private_use_cells(&app, &buffer);
            assert!(
                found.is_empty(),
                "repo {selected}, watch {watch}: private-use glyphs at {found:?}"
            );
        }
    }
}

/// The compact grid, the same way.
#[test]
fn plain_icons_draw_no_private_use_glyph_in_the_compact_grid() {
    let mut app = icon_fleet(4);
    app.view = View::Compact;
    set_active_icons(Icons::Plain);
    let buffer = draw_screen(&app, SCREEN_W);
    let found = private_use_cells(&app, &buffer);
    assert!(found.is_empty(), "private-use glyphs at {found:?}");
}

/// The control for the two scans above: the same screens under the Nerd
/// set do draw Nerd Font glyphs, so an empty scan in `plain` is the
/// setting at work and not a scan that cannot see them. Fails if the Nerd
/// set stops drawing them or the fixture stops carrying them.
#[test]
fn the_nerd_set_draws_the_nerd_font_glyphs() {
    let mut app = icon_fleet(0);
    set_active_icons(Icons::Nerd);
    app.watch_mode = true;
    app.view = View::Detail;
    let detail = draw_screen(&app, SCREEN_W);
    let drawn: Vec<String> = private_use_cells(&app, &detail)
        .into_iter()
        .map(|(_, _, s)| s)
        .collect();
    for kind in [
        crate::model::RepoKind::Org,
        crate::model::RepoKind::Private,
        crate::model::RepoKind::Public,
    ] {
        let glyph = crate::ui::badges::kind_glyph(kind).unwrap();
        assert!(drawn.iter().any(|s| s == glyph), "{kind:?} glyph missing");
    }
    assert!(drawn.iter().any(|s| s == crate::ui::badges::STASH_GLYPH));
    let eye = crate::ui::watch_icon(true);
    assert!(eye.chars().all(is_private_use), "{eye:?}");
    assert!(drawn.iter().any(|s| s == eye));

    app.view = View::Compact;
    let compact = draw_screen(&app, SCREEN_W);
    assert!(!private_use_cells(&app, &compact).is_empty());
}

/// In `plain` the list rows keep one name column and one right edge for
/// the status token, a repo of unknown kind (blank glyph) included. Fails
/// if a plain glyph measures other than one column, which would break
/// `badges::GLYPH_W` or the badge width the dot fill is computed from.
#[test]
fn plain_icons_keep_the_repo_list_aligned() {
    let mut app = icon_fleet(0);
    app.view = View::Detail;
    set_active_icons(Icons::Plain);
    let buffer = draw_screen(&app, SCREEN_W);
    let list = crate::ui::panel_rects(buffer.area, &app).repo_list;
    let names = name_cells(&app, &buffer, list);

    let name_x = names[0].0;
    assert!(
        names.iter().all(|(x, _)| *x == name_x),
        "names at {names:?}"
    );

    // The status token is the last thing before the border on every row.
    let right_edges: Vec<u16> = names
        .iter()
        .map(|&(_, y)| {
            (list.left()..list.right() - 1)
                .rev()
                .find(|&x| buffer[(x, y)].symbol() != " ")
                .unwrap()
        })
        .collect();
    assert!(
        right_edges.iter().all(|x| *x == right_edges[0]),
        "status tokens end at {right_edges:?}"
    );

    // And the name column is where the Nerd set puts it. The control
    // selects its set itself and proves it drew it, so it cannot quietly
    // compare plain with plain.
    let mut nerd = icon_fleet(0);
    nerd.view = View::Detail;
    set_active_icons(Icons::Nerd);
    let nerd_buffer = draw_screen(&nerd, SCREEN_W);
    let nerd_names = name_cells(&nerd, &nerd_buffer, list);
    let org = crate::ui::badges::kind_glyph(crate::model::RepoKind::Org).unwrap();
    let (org_x, org_y) = nerd_names[0];
    assert_eq!(
        nerd_buffer[(org_x - 2, org_y)].symbol(),
        org,
        "the control did not draw the Nerd set"
    );
    assert_eq!(nerd_names[0].0, name_x);
}

/// In `plain` the compact grid's rows share their columns: each name in
/// the second row starts under the one above it. Fails if a plain glyph
/// or badge measured differently from what `compact.rs` reserves for it.
#[test]
fn plain_icons_keep_the_compact_grid_aligned() {
    let mut app = icon_fleet(4);
    app.view = View::Compact;
    set_active_icons(Icons::Plain);
    let buffer = draw_screen(&app, SCREEN_W);
    let names = name_cells(&app, &buffer, buffer.area);

    let mut rows: Vec<u16> = names.iter().map(|(_, y)| *y).collect();
    rows.sort();
    rows.dedup();
    assert!(rows.len() >= 2, "the fixture must wrap: {names:?}");
    let columns = |y: u16| -> Vec<u16> {
        let mut xs: Vec<u16> = names
            .iter()
            .filter(|(_, ny)| *ny == y)
            .map(|(x, _)| *x)
            .collect();
        xs.sort();
        xs
    };
    let first = columns(rows[0]);
    for &y in &rows[1..] {
        let row = columns(y);
        assert_eq!(row[..], first[..row.len()], "row {y} drifted: {names:?}");
    }
}

/// The org glyph on the first list row and the header's watch mark, as
/// drawn under whatever set is active.
fn row_and_header_marks(app: &App) -> (String, String) {
    let buffer = draw_screen(app, SCREEN_W);
    let list = crate::ui::panel_rects(buffer.area, app).repo_list;
    let (x, y) = name_cells(app, &buffer, list)[0];
    let header: String = (0..buffer.area.width)
        .map(|x| buffer[(x, 0)].symbol())
        .collect();
    let watch = header
        .split_whitespace()
        .take_while(|word| *word != "watch")
        .last()
        .unwrap_or_default()
        .to_string();
    (buffer[(x - 2, y)].symbol().to_string(), watch)
}

/// One source: selecting a set changes what the rows and the header draw
/// together, and a thread that never selected one draws the Nerd set.
/// Fails if the header and the rows read different settings, or if a
/// fresh thread starts on anything but Nerd.
#[test]
fn selecting_a_set_changes_the_rows_and_the_header_together() {
    use crate::model::RepoKind;
    let mut app = icon_fleet(0);
    app.view = View::Detail;
    app.watch_mode = true;
    let nerd_org = crate::ui::badges::kind_glyph(RepoKind::Org).unwrap();

    set_active_icons(Icons::Plain);
    let (org, watch) = row_and_header_marks(&app);
    assert_eq!(org, "@");
    assert_eq!(watch, "o");

    set_active_icons(Icons::Nerd);
    let (org, watch) = row_and_header_marks(&app);
    assert_eq!(org, nerd_org);
    assert!(watch.chars().all(is_private_use), "{watch:?}");

    set_active_icons(Icons::Plain);
    let fresh = std::thread::spawn(|| {
        let mut app = icon_fleet(0);
        app.view = View::Detail;
        app.watch_mode = true;
        (active_icons(), row_and_header_marks(&app))
    })
    .join()
    .unwrap();
    assert_eq!(fresh.0, Icons::Nerd);
    assert_eq!(fresh.1.0, nerd_org);
    assert!(fresh.1.1.chars().all(is_private_use), "{:?}", fresh.1.1);
}
fn timed_out() -> crate::git::FetchOutcome {
    crate::git::FetchOutcome::Failed(crate::git::FetchFailure {
        kind: crate::git::FetchFailureKind::TimedOut,
        detail: None,
    })
}

/// FZ-35b (#85), criterion 3: a fetch cut at the cap is not counted by
/// the timeout backoff, neither holding a path nor resetting a count.
#[test]
fn the_fetch_backoff_ignores_a_fetch_cut_at_the_cap() {
    let t0 = Local::now();
    let too_big =
        crate::git::FetchOutcome::Failed(crate::git::FetchFailure::too_big(300 * 1024 * 1024));
    let mut b = FetchBackoff::default();
    b.record("~/r", &too_big, t0);
    assert_eq!(b.timeouts("~/r"), 0);
    assert!(!b.holds("~/r", t0, 60));
    b.record("~/r", &timed_out(), t0);
    b.record("~/r", &too_big, t0 + chrono::Duration::seconds(1));
    assert_eq!(b.timeouts("~/r"), 1, "the count stands");
}

/// FZ-35b (#85), criterion 7: an `f` still downloading past
/// `SWEEP_STUCK_AFTER` from its start keeps its report and its refresh,
/// timed from its last progress; one silent that long is given up on.
#[test]
fn a_fetch_report_lives_while_its_fetch_reports_progress() {
    let stuck = std::time::Duration::from_secs(180);
    let long_ago =
        |secs| std::time::Instant::now().checked_sub(std::time::Duration::from_secs(secs));
    let mut app = make_app(2);
    let path = app.repos[0].path.clone().unwrap();
    let quiet = app.repos[1].path.clone().unwrap();
    app.manual_fetches.expect([path.clone()]);
    app.refresh_awaits_poll = true;
    app.settle_refresh_poll([path.clone()]);
    // Started 200 s ago, under the 180 s limit's nose.
    app.manual_fetches.started = long_ago(200);
    app.refresh_fetches_started = long_ago(200);
    // Another repo's progress is no sign of life for this batch.
    app.apply_fetch_progress(&quiet, receiving(1));
    assert!(
        app.manual_fetches.started.unwrap().elapsed() > stuck,
        "another repo's progress kept this batch alive"
    );
    app.apply_fetch_progress(&path, receiving(300 * 1024 * 1024));
    app.expire_refresh_fetches(stuck);
    assert!(!app.manual_fetches.awaiting.is_empty(), "the tally expired");
    assert!(
        app.is_refreshing(),
        "the refresh gave up on a fetch still moving"
    );
    let report = app.manual_fetches.land(
        &path,
        "repo-0",
        &crate::git::FetchOutcome::Failed(crate::git::FetchFailure {
            kind: crate::git::FetchFailureKind::Unreachable,
            detail: None,
        }),
    );
    assert_eq!(report.map(|r| r.total), Some(1), "the report was lost");

    let mut silent = make_app(1);
    let path = silent.repos[0].path.clone().unwrap();
    silent.manual_fetches.expect([path.clone()]);
    silent.manual_fetches.started = long_ago(200);
    silent.expire_refresh_fetches(stuck);
    assert_eq!(
        silent.manual_fetches.land(&path, "repo-0", &timed_out()),
        None,
        "a silent batch expires as before"
    );
}

/// FZ-34 (#83), R3: after the nth consecutive timeout a path is held for
/// `backoff_secs(n + 1)` from that timeout: two intervals, then four,
/// doubling up to the cap.
#[test]
fn consecutive_fetch_timeouts_hold_two_intervals_then_four_up_to_the_cap() {
    let t0 = Local::now();
    let s = chrono::Duration::seconds;
    let mut b = FetchBackoff::default();
    assert!(!b.holds("~/r", t0, 60), "nothing recorded holds nothing");
    b.record("~/r", &timed_out(), t0);
    assert_eq!(b.timeouts("~/r"), 1);
    assert!(b.holds("~/r", t0 + s(119), 60));
    assert!(!b.holds("~/r", t0 + s(120), 60));
    let t1 = t0 + s(120);
    b.record("~/r", &timed_out(), t1);
    assert_eq!(b.timeouts("~/r"), 2);
    assert!(b.holds("~/r", t1 + s(239), 60));
    assert!(!b.holds("~/r", t1 + s(240), 60));
    for i in 1..10 {
        b.record("~/r", &timed_out(), t1 + s(i));
    }
    let last = t1 + s(9);
    assert!(b.holds("~/r", last + s(BACKOFF_CAP_SECS - 1), 60));
    assert!(!b.holds("~/r", last + s(BACKOFF_CAP_SECS), 60));
    assert!(!b.holds("~/other", t0, 60), "a hold is per path");
}

/// A timeout older than the one already counted (a load's fetch landing
/// after a sweep's) is not counted again.
#[test]
fn an_older_timeout_is_not_counted_twice() {
    let t0 = Local::now();
    let mut b = FetchBackoff::default();
    b.record("~/r", &timed_out(), t0);
    b.record("~/r", &timed_out(), t0);
    b.record("~/r", &timed_out(), t0 - chrono::Duration::seconds(5));
    assert_eq!(b.timeouts("~/r"), 1);
}

/// Only a success or a manual refresh lifts the hold; another failure
/// kind neither holds a path nor resets the count.
#[test]
fn a_success_or_a_clear_lifts_the_hold_and_other_failures_do_not() {
    use crate::git::{FetchFailure, FetchFailureKind, FetchOutcome};
    let t0 = Local::now();
    let mut b = FetchBackoff::default();
    let unreachable = FetchOutcome::Failed(FetchFailure {
        kind: FetchFailureKind::Unreachable,
        detail: None,
    });
    b.record("~/u", &unreachable, t0);
    assert!(!b.holds("~/u", t0, 60));

    b.record("~/r", &timed_out(), t0);
    b.record("~/r", &unreachable, t0 + chrono::Duration::seconds(1));
    b.record("~/r", &FetchOutcome::NotAttempted, t0);
    assert_eq!(b.timeouts("~/r"), 1);
    assert!(b.holds("~/r", t0 + chrono::Duration::seconds(1), 60));

    b.record("~/r", &FetchOutcome::Succeeded, t0);
    assert!(!b.holds("~/r", t0, 60));
    assert_eq!(b.timeouts("~/r"), 0);

    b.record("~/r", &timed_out(), t0);
    b.clear();
    assert!(!b.holds("~/r", t0, 60));
}

/// FZ-34, R5: a running fetch is not started twice. Landing it ends the
/// claim; a sweep that only rebuilt the repo does not.
#[test]
fn a_running_fetch_is_claimed_once_until_it_lands() {
    let mut app = make_app(1);
    let t0 = std::time::Instant::now();
    assert!(app.claim_fetch("~/repos/repo-0", t0));
    assert!(
        !app.claim_fetch("~/repos/repo-0", t0),
        "a second fetch started"
    );
    assert!(app.claim_fetch("~/repos/other", t0), "a claim is per path");
    app.fetch_landed("~/repos/repo-0", &crate::git::FetchOutcome::NotAttempted);
    assert!(
        !app.claim_fetch("~/repos/repo-0", t0),
        "a sweep without a fetch ended the claim"
    );
    app.fetch_landed("~/repos/repo-0", &timed_out());
    assert!(app.claim_fetch("~/repos/repo-0", t0));
    app.fetch_landed("~/repos/repo-0", &crate::git::FetchOutcome::Succeeded);
    assert!(app.claim_fetch("~/repos/repo-0", t0));
}

/// FZ-34, R5: a claim whose result never landed expires after the limit.
#[test]
fn a_fetch_claim_expires_after_the_stuck_limit() {
    let mut app = make_app(1);
    let t0 = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(180);
    assert!(app.claim_fetch("~/repos/repo-0", t0));
    app.expire_fetches_in_flight(limit, t0 + limit);
    assert!(
        !app.claim_fetch("~/repos/repo-0", t0),
        "expired at the limit"
    );
    app.expire_fetches_in_flight(limit, t0 + limit + std::time::Duration::from_secs(1));
    assert!(app.claim_fetch("~/repos/repo-0", t0));
}

/// FZ-35a (#84), the #83 reviewer's follow-up: a late landing from a
/// claim that expired does not end the newer claim on the same path; the
/// newer fetch's own landing does.
#[test]
fn a_late_landing_does_not_end_a_newer_claim() {
    let mut app = make_app(1);
    let t0 = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(180);
    assert!(app.claim_fetch("~/repos/repo-0", t0));
    let old = app.fetches_in_flight.token("~/repos/repo-0").unwrap();
    app.expire_fetches_in_flight(limit, t0 + std::time::Duration::from_secs(181));
    assert!(app.claim_fetch("~/repos/repo-0", t0 + std::time::Duration::from_secs(182)));
    let newer = app.fetches_in_flight.token("~/repos/repo-0").unwrap();
    app.fetch_landed_under("~/repos/repo-0", &timed_out(), Some(old));
    assert!(
        app.fetches_in_flight.contains_key("~/repos/repo-0"),
        "the late landing ended the newer claim"
    );
    app.fetch_landed_under(
        "~/repos/repo-0",
        &crate::git::FetchOutcome::NotAttempted,
        Some(newer),
    );
    assert!(app.fetches_in_flight.contains_key("~/repos/repo-0"));
    app.fetch_landed_under(
        "~/repos/repo-0",
        &crate::git::FetchOutcome::Succeeded,
        Some(newer),
    );
    assert!(!app.fetches_in_flight.contains_key("~/repos/repo-0"));
}

fn receiving(bytes: u64) -> crate::git::FetchProgress {
    crate::git::FetchProgress {
        phase: crate::git::FetchPhase::Receiving,
        objects_done: 1,
        objects_total: 2,
        bytes,
    }
}

/// FZ-35a (#84), criterion 3: progress lands on the row's fetch facts
/// and goes when the fetch lands; a sweep that did not fetch leaves it.
#[test]
fn progress_is_on_the_row_until_the_fetch_lands() {
    let mut app = make_app(2);
    let path = app.repos[0].path.clone().unwrap();
    app.needs_redraw = false;
    app.apply_fetch_progress(&path, receiving(5));
    assert_eq!(app.repos[0].fetch.progress, Some(receiving(5)));
    assert_eq!(app.repos[1].fetch.progress, None);
    assert!(app.needs_redraw);
    app.apply_fetch_progress("~/no/such/row", receiving(9));
    let now = Local::now();
    app.repos[0]
        .fetch
        .record(&crate::git::FetchOutcome::NotAttempted, now);
    assert_eq!(app.repos[0].fetch.progress, Some(receiving(5)));
    app.repos[0].fetch.record(&timed_out(), now);
    assert_eq!(app.repos[0].fetch.progress, None);
}

/// FZ-35a (#84): a row whose fetch claim expired, with nothing left to
/// land, stops showing progress; a row still claimed keeps it.
#[test]
fn an_expired_claim_takes_its_progress_off_the_row() {
    let mut app = make_app(2);
    let t0 = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(180);
    let (a, b) = (
        app.repos[0].path.clone().unwrap(),
        app.repos[1].path.clone().unwrap(),
    );
    assert!(app.claim_fetch(&a, t0));
    assert!(app.claim_fetch(&b, t0 + std::time::Duration::from_secs(100)));
    app.apply_fetch_progress(&a, receiving(5));
    app.apply_fetch_progress(&b, receiving(7));
    app.expire_fetches_in_flight(limit, t0 + std::time::Duration::from_secs(181));
    assert_eq!(app.repos[0].fetch.progress, None);
    assert_eq!(app.repos[1].fetch.progress, Some(receiving(7)));
}

/// The app measures the hold in its watch interval.
#[test]
fn the_app_holds_a_timed_out_path_in_watch_intervals() {
    let mut app = make_app(1);
    app.refresh_interval = 30;
    let t0 = Local::now();
    app.fetch_backoff.record("~/repos/repo-0", &timed_out(), t0);
    assert!(app.fetch_held("~/repos/repo-0", t0 + chrono::Duration::seconds(59)));
    assert!(!app.fetch_held("~/repos/repo-0", t0 + chrono::Duration::seconds(60)));
}

// --- The `worktrees` view (FZ-77) ---

/// An app on one repo with one linked worktree, pane 2 in the detail
/// view.
fn worktrees_app() -> App {
    let mut app = make_app(1);
    app.view = View::Detail;
    app.repos[0].worktrees = vec![crate::model::Worktree {
        path: "~/repos/repo-0-wt".to_string(),
        branch: "side".to_string(),
        head: "b2c3d4e".to_string(),
        dirty: Some(0),
        ..Default::default()
    }];
    app
}

/// `[` and `]` reach the view: forward from `graph`, back from
/// `branches`.
#[test]
fn brackets_reach_the_worktrees_view() {
    let mut app = worktrees_app();
    app.history_view = HistoryView::Graph;
    app.press_bracket(false);
    assert_eq!(app.history_view, HistoryView::Worktrees);
    app.history_view = HistoryView::Branches;
    app.press_bracket(true);
    assert_eq!(app.history_view, HistoryView::Worktrees);
    assert!(app.worktrees_wanted());
}

/// The log is read only while the view is on screen with the inspector
/// open, once the cursor has rested on the repo for the dwell, one read
/// per worktree at a time, held once landed, and read again after a
/// sweep.
#[test]
fn a_worktree_log_is_read_on_demand_and_again_after_a_sweep() {
    let start = std::time::Instant::now();
    let after = |ms: u64| start + std::time::Duration::from_millis(ms);
    let rested = after(2000) + crate::ci::DWELL;
    let mut app = worktrees_app();
    app.inspector_folded = false;
    for view in HistoryView::ALL {
        if view == HistoryView::Worktrees {
            continue;
        }
        app.history_view = view;
        assert_eq!(app.claim_worktree_log(start), None, "{view:?}");
        assert_eq!(app.claim_worktree_log(after(1000)), None, "{view:?}");
    }
    app.history_view = HistoryView::Worktrees;
    app.view = View::Compact;
    assert_eq!(app.claim_worktree_log(after(1500)), None, "compact");
    app.view = View::Detail;

    app.detail_selections[HistoryView::Worktrees.index()] = 1;
    assert_eq!(
        app.claim_worktree_log(after(2000)),
        None,
        "the dwell starts"
    );
    let early = rested - std::time::Duration::from_millis(1);
    assert_eq!(app.claim_worktree_log(early), None, "still dwelling");
    app.inspector_folded = true;
    assert_eq!(app.claim_worktree_log(rested), None, "inspector folded");
    app.inspector_folded = false;
    let read = app
        .claim_worktree_log(rested)
        .expect("reads the highlighted one");
    assert_eq!(read.key, "~/repos/repo-0-wt");
    assert!(read.dir.ends_with("repos/repo-0-wt"), "{:?}", read.dir);
    assert_eq!(app.claim_worktree_log(rested), None, "one at a time");
    app.apply_worktree_log(read, Vec::new());
    assert_eq!(app.worktree_log().map(<[_]>::len), Some(0));
    assert_eq!(app.claim_worktree_log(rested), None, "held, not read again");

    app.sweep_landed(chrono::Local::now());
    assert!(
        app.claim_worktree_log(rested).is_some(),
        "read again after a sweep"
    );
}

// --- The `notes` view (FZ-53) ---

/// An app on one repo at `/notes-fixture`, pane 2 in the detail view.
fn notes_app() -> App {
    let mut app = make_app(1);
    app.repos[0].path = Some("/notes-fixture".to_string());
    app.view = View::Detail;
    app.inspector_folded = false;
    app
}

fn note(rel: &str) -> crate::notes::NoteFile {
    crate::notes::NoteFile {
        rel: std::path::PathBuf::from(rel),
        notez: rel.starts_with("notez/").then(|| "notez".into()),
    }
}

/// Land a walk of `/notes-fixture` holding `files`.
fn walked(app: &mut App, files: &[&str]) {
    let walk = NotesWalk {
        key: "/notes-fixture".to_string(),
        root: "/notes-fixture".into(),
        generation: app.notes_generation,
    };
    let notes = crate::notes::Notes {
        files: files.iter().map(|f| note(f)).collect(),
    };
    app.apply_notes_walk(walk, Ok(notes));
}

/// The walk starts only while `notes` is on screen in the detail view,
/// and only once the cursor has rested on the row for the dwell. Fails
/// if any other view, or the compact grid, walks the repo.
#[test]
fn the_notes_walk_waits_for_its_view_and_the_dwell() {
    let start = std::time::Instant::now();
    let after = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut app = notes_app();
    for view in HistoryView::ALL {
        if view == HistoryView::Notes {
            continue;
        }
        app.history_view = view;
        assert_eq!(app.claim_notes_walk(start), None, "{view:?}");
        assert_eq!(app.claim_notes_walk(after(1000)), None, "{view:?}");
    }
    app.history_view = HistoryView::Notes;
    app.view = View::Compact;
    assert_eq!(app.claim_notes_walk(after(2000)), None, "compact");

    app.view = View::Detail;
    assert_eq!(app.claim_notes_walk(after(3000)), None, "the dwell starts");
    assert_eq!(app.claim_notes_walk(after(3299)), None, "still dwelling");
    let walk = app.claim_notes_walk(after(3300)).expect("walks");
    assert_eq!(walk.root, std::path::PathBuf::from("/notes-fixture"));
    assert_eq!(app.claim_notes_walk(after(4000)), None, "one at a time");
    assert!(app.notes.values().all(|s| s.walking));
}

/// A not-cloned row has nothing to walk.
#[test]
fn a_not_cloned_row_is_never_walked() {
    let start = std::time::Instant::now();
    let mut app = notes_app();
    app.repos[0].path = None;
    app.history_view = HistoryView::Notes;
    app.claim_notes_walk(start);
    let later = start + std::time::Duration::from_secs(5);
    assert_eq!(app.claim_notes_walk(later), None);
    assert!(app.notes.is_empty());
}

/// The walk is held for the session: leaving the view and coming back
/// draws it again without a walk, until a sweep lands or a suspend
/// returns (`invalidate_notes`). The old tree stays drawn meanwhile.
#[test]
fn coming_back_does_not_rewalk_until_a_sweep_or_a_suspend() {
    let start = std::time::Instant::now();
    let after = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    app.claim_notes_walk(start);
    app.claim_notes_walk(after(300)).expect("first walk");
    walked(&mut app, &["notez/a.md"]);

    app.history_view = HistoryView::Info;
    app.claim_notes_walk(after(400));
    app.history_view = HistoryView::Notes;
    app.claim_notes_walk(after(500));
    assert_eq!(app.claim_notes_walk(after(5000)), None, "held, not walked");
    assert_eq!(app.notes_listed().len(), 1);

    app.invalidate_notes();
    assert!(app.claim_notes_walk(after(5100)).is_some(), "walked again");
    assert_eq!(app.notes_listed().len(), 1, "the old tree stays drawn");
}

/// A landed sweep has the held walk read again: the `SweepDone` arm
/// goes through `sweep_landed`, so dropping its invalidation fails here.
#[test]
fn a_sweep_landing_rewalks_the_notes() {
    let start = std::time::Instant::now();
    let after = |ms: u64| start + std::time::Duration::from_millis(ms);
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    app.claim_notes_walk(start);
    app.claim_notes_walk(after(300)).expect("first walk");
    walked(&mut app, &["notez/a.md"]);
    assert_eq!(app.claim_notes_walk(after(5000)), None, "held, not walked");

    app.sweep_landed(chrono::Local::now());
    assert!(app.claim_notes_walk(after(5100)).is_some(), "walked again");
}

/// The body read is the highlighted file's alone, however far the cursor
/// moved since the last tick, and nothing outside the view. Read again
/// after a suspend returns, and with the inspector folded: the reader beside
/// the tree still shows the body (FZ-62).
#[test]
fn only_the_highlighted_file_is_read() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md", "notez/b.md", "notez/c.md"]);
    app.history_view = HistoryView::Info;
    assert_eq!(app.claim_note_read(), None, "another view reads nothing");

    app.history_view = HistoryView::Notes;
    app.select_commit_down();
    app.select_commit_down();
    let read = app.claim_note_read().expect("reads");
    assert_eq!(
        read.path,
        std::path::PathBuf::from("/notes-fixture/notez/c.md")
    );
    assert_eq!(app.claim_note_read(), None, "read once");

    // An answer for a file no longer highlighted is dropped.
    app.select_commit_up();
    let b = app.claim_note_read().expect("b");
    app.apply_note_read(read, Ok("c".to_string()));
    assert_eq!(app.note_body.as_ref().and_then(|b| b.text.clone()), None);
    app.apply_note_read(b, Ok("b".to_string()));
    assert_eq!(
        app.note_body.as_ref().and_then(|b| b.text.clone()),
        Some(Ok("b".to_string()))
    );

    app.invalidate_notes();
    assert!(app.claim_note_read().is_some(), "re-read after a suspend");

    app.invalidate_notes();
    app.inspector_folded = true;
    assert!(app.claim_note_read().is_some(), "folded card, open reader");
}

/// `m` widens the tree to every markdown file and back, keeping the
/// cursor on its file where the other tree lists it. Does nothing
/// outside the view.
#[test]
fn the_markdown_toggle_widens_the_tree_and_keeps_the_file() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["README.md", "docs/guide.md", "notez/a.md"]);
    assert_eq!(app.selected_commit_count(), 1);

    app.toggle_notes_markdown();
    assert!(app.notes_all_markdown);
    assert_eq!(app.selected_commit_count(), 3);
    assert_eq!(
        app.notes_highlighted(),
        Some("/notes-fixture/notez/a.md".into()),
        "the cursor stayed on its file"
    );

    app.history_view = HistoryView::Info;
    app.toggle_notes_markdown();
    assert!(app.notes_all_markdown, "not the view");
}

/// In the `notes` layout Tab walks repo list, tree, reader, strip and
/// Shift-Tab walks it back, with no card to stop on since FZ-68 round 2:
/// `3` does not reach an inspector there either. The reader takes focus
/// nowhere else, and leaving the view moves focus off it (FZ-62).
#[test]
fn focus_walks_tree_and_reader_in_the_notes_layout() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    let order = [
        FocusedPanel::RepoList,
        FocusedPanel::Detail,
        FocusedPanel::Reader,
        FocusedPanel::Contributions,
    ];
    app.focused_panel = FocusedPanel::RepoList;
    for want in order.iter().cycle().skip(1).take(order.len()) {
        app.toggle_panel_focus();
        assert_eq!(app.focused_panel, *want, "Tab");
    }
    assert_eq!(app.focused_panel, FocusedPanel::RepoList, "a full turn");
    for want in order.iter().rev() {
        app.toggle_panel_focus_back();
        assert_eq!(app.focused_panel, *want, "Shift-Tab");
    }
    app.focus_panel(FocusedPanel::Inspector);
    assert_eq!(
        app.focused_panel,
        FocusedPanel::RepoList,
        "no card to focus"
    );

    app.focus_panel(FocusedPanel::Reader);
    assert_eq!(app.focused_panel, FocusedPanel::Reader);
    app.cycle_history_view(false);
    assert_eq!(app.focused_panel, FocusedPanel::Detail, "left the view");
    app.focus_panel(FocusedPanel::Reader);
    assert_eq!(app.focused_panel, FocusedPanel::Detail, "no reader here");
    app.toggle_panel_focus();
    assert_eq!(app.focused_panel, FocusedPanel::Inspector, "Tab skips it");
}

/// `J`/`K` in the `notes` layout scroll the reader a line from the tree,
/// with the inspector's fold open (the default) as well as folded
/// (FZ-62 review F1; no card is docked since FZ-68 round 2, so the fold
/// must not matter), and from the reader itself, focus unmoved
/// (finding 6).
#[test]
fn shift_scroll_walks_tree_and_reader_in_the_notes_layout() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md"]);
    assert!(!app.inspector_folded, "the fold is open");

    app.focused_panel = FocusedPanel::Detail;
    assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Reader));
    app.reader_max_scroll.set(20);
    app.scroll_next_panel(3);
    assert_eq!(app.reader_scroll, 3, "J from the tree scrolls the reader");
    assert_eq!(app.inspector_scroll, 0, "not the inspector");

    assert_eq!(app.focused_panel, FocusedPanel::Detail, "focus unmoved");

    app.focused_panel = FocusedPanel::Reader;
    assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Reader));
    app.scroll_next_panel(-1);
    assert_eq!(app.reader_scroll, 2, "J/K on the reader scroll it");
    app.inspector_folded = true;
    assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Reader));
    app.focused_panel = FocusedPanel::Detail;
    assert_eq!(app.shift_scroll_target(), Some(FocusedPanel::Reader));
}

/// FZ-68 round 2: on a section header in `notes` pane 2 is the digest and
/// the inspector is docked, so it can hold focus; stepping onto the repo
/// brings the `notes` layout back, which docks none, and focus lands on
/// pane 2 rather than staying on a pane that is not drawn.
#[test]
fn leaving_a_header_in_notes_moves_focus_off_the_inspector() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    app.repos[0].location = crate::model::RepoLocation::LocalOnly;
    let section = crate::digest::Section::of(&app.repos[0]);
    app.land_on(ListRow::Header(section));
    assert!(!app.notes_layout(), "on the digest");
    app.focus_panel(FocusedPanel::Inspector);
    assert_eq!(app.focused_panel, FocusedPanel::Inspector, "docked here");

    app.land_on(ListRow::Repo(0));
    assert!(app.notes_layout(), "the notes layout is back");
    assert_eq!(app.focused_panel, FocusedPanel::Detail);
}

/// `H`/`L` page the reader by the height its last frame drew, from the
/// tree or the reader, focus unmoved and the tree's cursor where it was;
/// anywhere else they do nothing (Andreas, 2026-10-29, finding 6).
#[test]
fn h_and_l_page_the_reader_from_the_tree() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md", "notez/b.md"]);
    app.reader_max_scroll.set(100);
    app.reader_view_height.set(18);

    app.focused_panel = FocusedPanel::Detail;
    assert!(app.reader_pages());
    app.page_reader(1);
    assert_eq!(app.reader_scroll, 18, "L: one page down");
    app.page_reader(1);
    assert_eq!(app.reader_scroll, 36);
    app.page_reader(-1);
    assert_eq!(app.reader_scroll, 18, "H: one page up");
    assert_eq!(app.focused_panel, FocusedPanel::Detail, "focus unmoved");
    assert_eq!(app.detail_selected(), 0, "the tree's cursor stayed");

    app.focused_panel = FocusedPanel::Reader;
    assert!(app.reader_pages(), "from the reader too");
    app.page_reader(-5);
    assert_eq!(app.reader_scroll, 0, "clamped at the top");

    for panel in [FocusedPanel::RepoList, FocusedPanel::Inspector] {
        app.focused_panel = panel;
        assert!(!app.reader_pages(), "{panel:?}");
    }
    app.focused_panel = FocusedPanel::Detail;
    app.history_view = HistoryView::Info;
    assert!(!app.reader_pages(), "no reader outside notes");
}

/// The reader keeps its own offset: moving the tree's cursor does not
/// scroll it, scrolling it does not move the cursor, and a new file
/// starts it at the top while a re-read of the same file keeps its
/// place.
#[test]
fn the_reader_scrolls_on_its_own_and_resets_on_a_new_file() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md", "notez/b.md"]);
    app.claim_note_read().expect("a");
    app.reader_max_scroll.set(30);
    app.scroll_reader(12);
    assert_eq!(app.reader_scroll, 12);
    assert_eq!(app.detail_selected(), 0, "the cursor stayed");
    assert_eq!(app.inspector_scroll, 0, "the card did not scroll");

    app.invalidate_notes();
    app.claim_note_read().expect("re-read of a");
    assert_eq!(app.reader_scroll, 12, "same file keeps its place");

    app.select_commit_down();
    assert_eq!(app.reader_scroll, 12, "the cursor does not scroll it");
    app.claim_note_read().expect("b");
    assert_eq!(app.reader_scroll, 0, "a new file starts at the top");

    app.scroll_reader(-5);
    assert_eq!(app.reader_scroll, 0, "clamped at the top");
    app.scroll_reader(1000);
    assert_eq!(app.reader_scroll, 30, "clamped to what was measured");
}

/// FZ-59: a markdown read carries the reader's width so the body is
/// rendered with it; a text file, or a reader that never drew, asks for
/// no render.
#[test]
fn a_markdown_read_asks_for_a_render_at_the_reader_width() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md", "notez/b.txt", "notez/TODO.md"]);
    assert_eq!(app.claim_note_read().unwrap().render_width, None, "undrawn");

    app.reader_width.set(48);
    app.invalidate_notes();
    let mut asked = Vec::new();
    for _ in 0..3 {
        let read = app.claim_note_read().unwrap();
        let name = read
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        asked.push((name, read.render_width));
        app.select_commit_down();
    }
    asked.sort();
    assert_eq!(
        asked,
        [
            ("TODO.md".to_string(), Some(48)),
            ("a.md".to_string(), Some(48)),
            ("b.txt".to_string(), None),
        ],
        "a TODO.md is markdown, a text file is not"
    );
}

/// FZ-59: a width the held render was not made for is claimed once, and
/// again only after the reader moves on to another width; a render for
/// a file the cursor has left is dropped.
#[test]
fn a_new_reader_width_claims_one_render_of_the_held_body() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md", "notez/b.md"]);
    let read = app.claim_note_read().expect("a");
    assert_eq!(app.claim_note_render(), None, "body not landed");
    app.apply_note_read(read.clone(), Ok("# A".to_string()));
    assert_eq!(app.claim_note_render(), None, "the reader never drew");

    app.reader_width.set(40);
    let render = app.claim_note_render().expect("a render at 40");
    assert_eq!((render.width, render.text.as_str()), (40, "# A"));
    assert_eq!(app.claim_note_render(), None, "in flight");
    let lines = crate::ui::markdown::render_markdown(&render.text, 40);
    let rendered = RenderedNote { width: 40, lines };
    app.apply_note_rendered(&render.path, render.generation, rendered.clone());
    assert_eq!(app.note_body.as_ref().unwrap().rendered, Some(rendered));
    assert_eq!(app.claim_note_render(), None, "held at this width");

    app.reader_width.set(60);
    assert_eq!(app.claim_note_render().map(|r| r.width), Some(60));

    app.select_commit_down();
    app.claim_note_read().expect("b");
    let stale = RenderedNote {
        width: 60,
        lines: Vec::new(),
    };
    app.apply_note_rendered(&read.path, read.generation, stale);
    assert_eq!(app.note_body.as_ref().unwrap().rendered, None, "dropped");
}

/// FZ-59 review: a render of the same file from an older generation,
/// landing after a sweep or a suspend asked for a re-read, is dropped:
/// the re-read body holds no render of the old text, and the render it
/// asked for at that width still lands.
#[test]
fn a_render_from_an_older_generation_of_the_same_file_is_dropped() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md"]);
    app.reader_width.set(40);
    let old = app.claim_note_read().expect("a");
    assert_eq!(old.render_width, Some(40));

    app.invalidate_notes();
    let new = app.claim_note_read().expect("re-read of a");
    assert_eq!(new.path, old.path, "the same file");
    assert_ne!(new.generation, old.generation);
    let stale = RenderedNote {
        width: 40,
        lines: crate::ui::markdown::render_markdown("# old", 40),
    };
    app.apply_note_rendered(&old.path, old.generation, stale);
    let body = app.note_body.as_ref().unwrap();
    assert_eq!(body.rendered, None, "the old text's render is dropped");
    assert_eq!(body.render_asked, Some(40), "the re-read's is still owed");

    let fresh = RenderedNote {
        width: 40,
        lines: crate::ui::markdown::render_markdown("# new", 40),
    };
    app.apply_note_rendered(&new.path, new.generation, fresh.clone());
    assert_eq!(app.note_body.as_ref().unwrap().rendered, Some(fresh));
}

/// FZ-59: `p` flips the reader between rendered and raw for the
/// session, in the `notes` layout only.
#[test]
fn the_raw_toggle_flips_for_the_session_in_the_notes_layout_only() {
    let mut app = notes_app();
    app.history_view = HistoryView::Info;
    app.toggle_reader_raw();
    assert!(!app.reader_raw, "not the notes view");
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md", "notez/b.md"]);
    app.toggle_reader_raw();
    assert!(app.reader_raw);
    app.select_commit_down();
    assert!(app.reader_raw, "one flag, not per file");
    app.toggle_reader_raw();
    assert!(!app.reader_raw);
}

/// Facts land beside the body they were read with, and an answer for a
/// file no longer highlighted is dropped, as a body is.
#[test]
fn note_facts_land_only_on_their_file() {
    let mut app = notes_app();
    app.history_view = HistoryView::Notes;
    walked(&mut app, &["notez/a.md", "notez/b.md"]);
    let a = app.claim_note_read().expect("a");
    assert_eq!(a.root, std::path::PathBuf::from("/notes-fixture"));
    let facts = crate::notes::NoteFacts {
        size: Some(3),
        ..Default::default()
    };
    app.select_commit_down();
    app.claim_note_read().expect("b");
    app.apply_note_facts(&a, facts.clone());
    assert_eq!(app.note_body.as_ref().and_then(|b| b.facts.clone()), None);
}

// FZ-81: the copy flash, as pure functions of an injected instant.

fn flash_at(from_right: bool) -> (YankFlash, std::time::Instant) {
    let start = std::time::Instant::now();
    let source = FlashSource {
        pane: FocusedPanel::RepoList,
        view: HistoryView::Info,
        row: 0,
    };
    let mut flash = YankFlash::default();
    flash.show(source, from_right, start);
    (flash, start)
}

fn ms(n: u64) -> std::time::Duration {
    std::time::Duration::from_millis(n)
}

/// The backgrounds of a 10-cell row at `start + at`.
fn row_at(flash: &YankFlash, start: std::time::Instant, at: u64) -> Vec<ratatui::style::Color> {
    (0..10)
        .map(|c| flash.cell_bg(start + ms(at), c, 10))
        .collect()
}

#[test]
fn yy_sweeps_the_row_from_the_left_then_fades_to_the_band() {
    use crate::ui::colors::{SELECTED_BG, YANKED_BG, mix};
    let (flash, start) = flash_at(false);
    let (y, b) = (YANKED_BG, SELECTED_BG);
    assert_eq!(row_at(&flash, start, 0), vec![b; 10], "nothing swept yet");
    assert_eq!(
        row_at(&flash, start, 50),
        [vec![y; 5], vec![b; 5]].concat(),
        "the left half at 50 ms"
    );
    assert_eq!(row_at(&flash, start, 100), vec![y; 10], "fully yanked");
    let half = mix(YANKED_BG, SELECTED_BG, 0.5);
    assert_ne!(half, y);
    assert_ne!(half, b);
    assert_eq!(row_at(&flash, start, 200), vec![half; 10], "halfway faded");
    assert_eq!(row_at(&flash, start, 300), vec![b; 10], "the plain band");
    assert_eq!(row_at(&flash, start, 900), vec![b; 10]);
}

#[test]
fn yp_sweeps_the_row_from_the_right() {
    use crate::ui::colors::{SELECTED_BG, YANKED_BG};
    let (flash, start) = flash_at(true);
    assert_eq!(
        row_at(&flash, start, 50),
        [vec![SELECTED_BG; 5], vec![YANKED_BG; 5]].concat()
    );
    assert_eq!(row_at(&flash, start, 100), vec![YANKED_BG; 10]);
}

#[test]
fn no_flash_paints_the_plain_band() {
    let flash = YankFlash::default();
    let now = std::time::Instant::now();
    assert_eq!(flash.cell_bg(now, 0, 10), crate::ui::colors::SELECTED_BG);
    assert_eq!(flash.source(), None);
}

/// The loop waits at most `YANK_FRAME` while the flash lasts, not at
/// all once it is due to end, and the normal tick after `tick` ends it.
#[test]
fn the_loop_waits_a_frame_while_the_flash_is_live() {
    let tick = ms(100);
    let (mut flash, start) = flash_at(false);
    for at in [0, 50, 150, 280] {
        assert!(flash.wait(tick, start + ms(at)) <= YANK_FRAME, "at {at} ms");
    }
    assert_eq!(flash.wait(tick, start + ms(295)), ms(5), "the last frame");
    assert_eq!(flash.wait(tick, start + ms(300)), ms(0), "due to end");
    assert!(flash.tick(start + ms(150)), "live: redraw");
    assert!(flash.is_active());
    assert!(flash.tick(start + ms(300)), "ended: one last redraw");
    assert!(!flash.is_active());
    assert_eq!(flash.wait(tick, start + ms(310)), tick, "the normal tick");
    assert!(!flash.tick(start + ms(320)), "nothing left to redraw");
}
