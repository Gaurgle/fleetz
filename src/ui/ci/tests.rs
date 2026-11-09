use super::*;
use crate::app::{FocusedPanel, HistoryView, View};
use chrono::{Datelike, TimeZone, Timelike};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

const RUNS: &str = include_str!("../../ci/fixtures/runs.json");

fn runs() -> Vec<WorkflowRun> {
    crate::ci::parse_runs(RUNS).unwrap()
}

// Wall-clock components, not the offset: CI runs in UTC, and the tests assert local-time strings.
fn local(text: &str) -> DateTime<Local> {
    let wall = DateTime::parse_from_rfc3339(text).unwrap().naive_local();
    Local
        .with_ymd_and_hms(
            wall.year(),
            wall.month(),
            wall.day(),
            wall.hour(),
            wall.minute(),
            wall.second(),
        )
        .single()
        .expect("unambiguous local time")
}

// The instant the text names, whatever the machine's zone.
fn instant(text: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Local)
}

fn key() -> crate::app::RemoteKey {
    (Some("gaurgle".to_string()), "fleetz".to_string())
}

fn repo() -> RepoStatus {
    use crate::model;
    RepoStatus {
        daily: None,
        daily_key: None,
        last_local: None,
        name: "fleetz".to_string(),
        path: None,
        location: model::RepoLocation::Primary,
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
        repo_kind: model::RepoKind::Unknown,
        owner: Some("Gaurgle".to_string()),
        remote_url: None,
        remote_only_branches: vec![],
        remote_branches: vec![],
        work_key: None,
        fetch: model::FetchFacts::default(),
        authors: Default::default(),
    }
}

/// No jitter: the ladder and the resume land on whole numbers.
struct NoJitter;

impl crate::app::Jitter for NoJitter {
    fn backoff_factor(&mut self) -> f64 {
        1.0
    }
    fn resume_secs(&mut self) -> i64 {
        0
    }
}

/// An app on one GitHub repo, pane 2 showing `ci`, nothing polled.
fn app() -> App {
    let mut app = App::new(Vec::new(), String::new(), 120, 44);
    app.repos = vec![repo()];
    app.view = View::Detail;
    app.history_view = HistoryView::Ci;
    app.jitter = Box::new(NoJitter);
    app
}

/// Land a successful list of `runs` at `now`.
fn landed(app: &mut App, runs: Vec<WorkflowRun>, now: DateTime<Local>) {
    let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    app.apply_ci(
        crate::ci::CiAnswer {
            key: key(),
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
        now,
    );
}

fn failed(app: &mut App, failure: crate::github::Failure, now: DateTime<Local>) {
    let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    app.apply_ci(
        crate::ci::CiAnswer {
            key: key(),
            attempt: request.attempt,
            runs: None,
            jobs: None,
            log: None,
            full_log: None,
            failure: Some(failure),
            budget: None,
            tip: None,
        },
        now,
    );
}

fn draw(app: &App, width: u16, height: u16) -> (crate::ui::PanelRects, Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| crate::ui::render(f, app)).unwrap();
    let rects = crate::ui::panel_rects(Rect::new(0, 0, width, height), app);
    (rects, terminal.backend().buffer().clone())
}

fn row_text(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

/// The detail pane's rows as drawn, one string each.
fn detail_rows(app: &App) -> Vec<String> {
    let (rects, buffer) = draw(app, 160, 50);
    let detail = rects.detail.expect("detail pane laid out");
    (detail.y..detail.y + detail.height)
        .map(|y| {
            (detail.x..detail.x + detail.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

/// The start of a run's title, which a narrow title column truncates.
fn head(run: &WorkflowRun) -> String {
    run.title.chars().take(20).collect()
}

fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn status_text(app: &App) -> Option<String> {
    status_line(app, Local::now()).map(|spans| spans.iter().map(|s| s.content.as_ref()).collect())
}

// --- Title and status line ---

#[test]
fn the_title_carries_the_freshness_mark() {
    let now = local("2026-10-21T12:05:00+02:00");
    let mut app = app();
    assert_eq!(freshness_mark(None, now), None, "nothing before an answer");
    landed(&mut app, runs(), local("2026-10-21T12:02:00+02:00"));
    let mark = freshness_mark(app.ci.get(&key()), now).unwrap();
    assert_eq!(mark.content, "as of 12:02");
    let offline = crate::github::Failure::new(crate::github::FailureKind::Offline, "x");
    failed(&mut app, offline, now);
    let mark = freshness_mark(app.ci.get(&key()), now).unwrap();
    assert_eq!(mark.content, "⚠ 12:05", "dated by the failed attempt");
    assert_eq!(mark.style.fg, Some(colors::PEACH));
}

#[test]
fn the_status_line_names_each_empty_state() {
    let mut app = app();
    let loading = status_text(&app).unwrap();
    assert!(loading.contains("loading"), "{loading}");

    landed(&mut app, Vec::new(), Local::now());
    assert_eq!(status_text(&app).as_deref(), Some(" no workflow runs "));

    let mut signed_out = self::app();
    let auth = crate::github::Failure::new(crate::github::FailureKind::Auth, "no token");
    failed(&mut signed_out, auth, Local::now());
    assert_eq!(status_text(&signed_out).as_deref(), Some(" not signed in "));

    let mut offline = self::app();
    let failure = crate::github::Failure::new(crate::github::FailureKind::Offline, "x");
    failed(&mut offline, failure, Local::now());
    assert_eq!(status_text(&offline).as_deref(), Some(" offline "));

    let mut local_only = self::app();
    local_only.repos[0].owner = None;
    assert_eq!(
        status_text(&local_only).as_deref(),
        Some(" no GitHub remote ")
    );

    landed(&mut app, runs(), Local::now());
    assert_eq!(status_text(&app), None, "the runs speak for themselves");
}

#[test]
fn a_rate_limit_says_until_when() {
    let now = local("2026-10-21T12:00:10+02:00");
    let mut app = app();
    let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    let reset = local("2026-10-21T12:40:00+02:00");
    let reading = crate::github::BudgetReading {
        limit: 5000,
        remaining: 0,
        used: 5000,
        reset: reset.timestamp(),
        date: Some(now.with_timezone(&Utc)),
        token_id: "tok".to_string(),
    };
    app.apply_ci(
        crate::ci::CiAnswer {
            key: key(),
            attempt: request.attempt,
            runs: None,
            jobs: None,
            log: None,
            full_log: None,
            failure: Some(crate::github::Failure::rate_limit(
                "API rate limit exceeded",
            )),
            budget: Some(reading),
            tip: None,
        },
        now,
    );
    let text: String = status_line(&app, now)
        .unwrap()
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(text, " rate limited until 12:40 ");
}

/// FZ-30d, ruling 5: the view's first line is the freshness mark and
/// the status folded into one, unpadded. Fails while the two live in a
/// band's title and bottom border.
#[test]
fn the_first_line_folds_the_mark_and_the_status_together() {
    let now = local("2026-10-21T12:05:00+02:00");
    let first = |app: &App| line_text(&first_line(app, now));

    let mut app = app();
    assert!(first(&app).ends_with("loading"), "{}", first(&app));

    landed(&mut app, runs(), local("2026-10-21T12:02:00+02:00"));
    assert_eq!(first(&app), "as of 12:02");

    let offline = crate::github::Failure::new(crate::github::FailureKind::Offline, "x");
    failed(&mut app, offline, now);
    assert_eq!(first(&app), "⚠ 12:05  offline");

    let mut empty = self::app();
    landed(&mut empty, Vec::new(), local("2026-10-21T12:02:00+02:00"));
    assert_eq!(first(&empty), "as of 12:02  no workflow runs");

    let mut local_only = self::app();
    local_only.repos[0].owner = None;
    assert_eq!(first(&local_only), "no GitHub remote");

    let mut dirty = self::app();
    dirty.dirty_mode = true;
    assert_eq!(first(&dirty), "GitHub off under -d");

    let mut signed_out = self::app();
    let auth = crate::github::Failure::new(crate::github::FailureKind::Auth, "no token");
    failed(&mut signed_out, auth, now);
    assert_eq!(first(&signed_out), "⚠ 12:05  not signed in");
}

// --- Run rows ---

#[test]
fn a_run_row_has_every_column_and_the_title_takes_the_slack() {
    let runs = runs();
    let now = local("2026-10-21T13:16:30+02:00");
    let widths = Widths::of(&runs, now);
    let text = |width| -> String {
        let line = run_line(&runs[1], &widths, width, now, false).marked(false, false);
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    };
    let wide = text(160);
    let run = &runs[1];
    let branch = crate::ui::truncate(run.branch.as_deref().unwrap(), BRANCH_MAX);
    assert!(
        branch.ends_with('…'),
        "the fixture's branch is past the cap"
    );
    for cell in [
        run.workflow.as_deref().unwrap(),
        branch.as_str(),
        run.event.as_str(),
        run.title.as_str(),
        "ago",
    ] {
        assert!(wide.contains(cell), "{cell} missing from {wide}");
    }
    assert!(wide.contains(&run_duration(run, now.with_timezone(&Utc))));
    assert_eq!(wide.chars().count(), 160, "the title fills the row");
    let narrow = text(90);
    assert_eq!(narrow.chars().count(), 90);
    assert!(narrow.contains('…'), "the title gives way: {narrow}");
    assert!(narrow.ends_with("ago"), "duration and age kept: {narrow}");
}

#[test]
fn the_lamp_takes_the_ci_colours_and_pulses_while_running() {
    let runs = runs();
    assert_eq!(run_status(&runs[0]), CiStatus::Pending, "in progress");
    assert_eq!(run_status(&runs[1]), CiStatus::Passed);
    assert_eq!(run_status(&runs[2]), CiStatus::Failed);
    let now = local("2026-10-21T13:16:30+02:00");
    let widths = Widths::of(&runs, now);
    let lamp = |run, dim| run_line(run, &widths, 120, now, dim).0[0].style.fg;
    assert_eq!(lamp(&runs[1], false), Some(colors::GREEN));
    assert_eq!(lamp(&runs[2], false), Some(colors::RED));
    assert_eq!(lamp(&runs[0], false), Some(colors::PEACH));
    assert_eq!(lamp(&runs[0], true), Some(colors::SURFACE), "the off phase");
    assert_eq!(
        lamp(&runs[1], true),
        Some(colors::GREEN),
        "only pending pulses"
    );
    let mut cancelled = runs[1].clone();
    cancelled.conclusion = Some("cancelled".to_string());
    assert_eq!(run_status(&cancelled), CiStatus::Unknown);
}

#[test]
fn the_duration_ticks_while_the_run_is_live_and_holds_once_done() {
    let runs = runs();
    let live = &runs[0];
    let at = |t: &str| DateTime::parse_from_rfc3339(t).unwrap().with_timezone(&Utc);
    // Started 11:16:07Z.
    assert_eq!(run_duration(live, at("2026-10-21T11:16:15Z")), "8s");
    assert_eq!(run_duration(live, at("2026-10-21T11:18:10Z")), "2m 03s");
    let done = &runs[1];
    let held = run_duration(done, at("2026-10-21T11:30:00Z"));
    assert_eq!(held, run_duration(done, at("2026-10-22T11:30:00Z")));
    let mut long = done.clone();
    long.started = Some(long.updated - chrono::Duration::seconds(3840));
    assert_eq!(run_duration(&long, at("2026-10-22T11:30:00Z")), "1h 04m");
}

#[test]
fn the_age_always_says_ago() {
    let run = &runs()[0];
    let at = |t: &str| DateTime::parse_from_rfc3339(t).unwrap().with_timezone(&Utc);
    assert_eq!(run_age(run, at("2026-10-21T11:16:30Z")), "1m ago");
    assert_eq!(run_age(run, at("2026-10-21T11:19:07Z")), "3m ago");
    assert_eq!(run_age(run, at("2026-10-21T14:16:07Z")), "3h ago");
    assert_eq!(run_age(run, at("2026-10-22T12:00:00Z")), "1d ago");
}

// --- The `ci` view in pane 2 ---

/// `ci` is pane 2's sixth view: its tab names it, its first line says
/// how current the runs are, and the recorded runs follow one row each,
/// the cursor on the first. Fails while the runs live in a band.
#[test]
fn the_ci_view_draws_the_held_runs_under_its_first_line() {
    let mut app = app();
    app.focused_panel = FocusedPanel::Detail;
    landed(&mut app, runs(), Local::now());
    let rows = detail_rows(&app);
    assert!(rows[0].contains("CI"), "the tab: {}", rows[0]);
    let first = rows
        .iter()
        .position(|r| r.contains("as of "))
        .expect("the first line");
    let held = runs();
    for (i, run) in held.iter().enumerate() {
        let row = &rows[first + 2 + i];
        assert!(row.contains(&head(run)), "row {i}: {row}");
        assert_eq!(row.contains('▸'), i == 0, "cursor on the first: {row}");
    }
    assert!(
        rows.iter().all(|r| !r.contains("[5]")),
        "no band, no fifth pane"
    );
}

/// With nothing held the view is its first line alone: the empty state
/// and no rows.
#[test]
fn the_ci_view_shows_its_empty_states_on_the_first_line() {
    let mut app = app();
    landed(&mut app, Vec::new(), Local::now());
    let rows = detail_rows(&app);
    assert!(
        rows.iter().any(|r| r.contains("no workflow runs")),
        "{rows:#?}"
    );
    assert!(rows.iter().all(|r| !r.contains('▸')), "no cursor");

    let mut local_only = self::app();
    local_only.repos[0].owner = None;
    let rows = detail_rows(&local_only);
    assert!(rows.iter().any(|r| r.contains("no GitHub remote")));
}

/// `j`/`k` walk the runs through the view's own selection slot, and the
/// marked row follows. Fails while the cursor lives on `CiState`.
#[test]
fn the_cursor_is_the_views_selection_and_the_marked_row_follows() {
    let mut app = app();
    app.focused_panel = FocusedPanel::Detail;
    landed(&mut app, runs(), Local::now());
    assert_eq!(app.selected_commit_count(), 3);
    app.detail_nav_down();
    app.detail_nav_down();
    app.detail_nav_down();
    assert_eq!(app.detail_selected(), 2, "clamped to the last run");
    let rows = detail_rows(&app);
    let marked: Vec<&String> = rows.iter().filter(|r| r.contains('▸')).collect();
    assert_eq!(marked.len(), 1);
    assert!(marked[0].contains(&head(&runs()[2])), "{}", marked[0]);
    app.detail_nav_up();
    assert_eq!(app.detail_selected(), 1);
    assert_eq!(
        app.detail_selections[HistoryView::Commits.index()],
        0,
        "another view's slot untouched"
    );
}

/// The #91 review's follow-up: a shorter list landing for the repo on
/// screen pulls the cursor onto its last run. Fails while the cursor can
/// sit past the end of the list.
#[test]
fn a_shorter_list_landing_clamps_the_cursor() {
    let mut app = app();
    landed(&mut app, runs(), Local::now());
    app.set_detail_selected(2);
    landed(&mut app, runs()[..1].to_vec(), Local::now());
    assert_eq!(app.detail_selected(), 0);
    landed(&mut app, Vec::new(), Local::now());
    assert_eq!(app.detail_selected(), 0, "an empty list parks it at 0");
}

/// A click on a run row moves the view's cursor there, through the
/// pane's own hit map.
#[test]
fn a_click_on_a_run_row_selects_it() {
    let mut app = app();
    landed(&mut app, runs(), Local::now());
    let (_, buffer) = draw(&app, 160, 50);
    let title = head(&runs()[2]);
    let y = (0..50)
        .find(|&y| row_text(&buffer, y).contains(title.as_str()))
        .expect("third run drawn");
    let hits = app.detail_hits.borrow().clone();
    let click = hits.at(hits.body.x + 4, y).expect("a run row");
    app.click_detail(&click);
    assert_eq!(app.detail_selected(), 2);
}

// --- The `ci` view's width (FZ-40) ---

/// His screenshot's workflow, on a manual dispatch from `main`.
const RELEASE: &str = "Release Traffic preview OTA";

/// The fixture's runs with a fourth run named as in his screenshot.
fn runs_with_release() -> Vec<WorkflowRun> {
    let mut runs = runs();
    let mut release = runs[1].clone();
    release.workflow = Some(RELEASE.to_string());
    release.branch = Some("main".to_string());
    release.event = "workflow_dispatch".to_string();
    runs.push(release);
    runs
}

/// His screenshot's rows (FZ-82): `Frontend CI` on `feat/font-previews`
/// from a `pull_request`, `9m 14s`, `17h ago`, with a long title.
fn screenshot_runs() -> Vec<WorkflowRun> {
    let created = Utc::now() - chrono::Duration::hours(17);
    (0..3)
        .map(|i| {
            let mut run = runs()[1].clone();
            run.id += i;
            run.workflow = Some("Frontend CI".to_string());
            run.branch = Some("feat/font-previews".to_string());
            run.event = "pull_request".to_string();
            run.title = "feat(fonts): preview every font in the picker".to_string();
            run.created = created;
            run.started = Some(created);
            run.updated = created + chrono::Duration::seconds(554);
            run
        })
        .collect()
}

/// His report of 2026-11-05: on a narrow pane 2 every run row kept the
/// trigger and broke `37s   6m ago` onto a second row. Pane 2 drawn
/// alone at each width from 40 to 80 columns: one rendered row per run,
/// each ending in its age, the headline too. Fails while a row is laid
/// out wider than the body it is drawn in.
#[test]
fn a_run_row_never_wraps_on_a_narrow_pane() {
    let mut app = app();
    app.focused_panel = FocusedPanel::Detail;
    landed(&mut app, screenshot_runs(), Local::now());
    for pane in 40..=80u16 {
        let mut terminal = Terminal::new(TestBackend::new(pane, 30)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                crate::ui::detail::render(f, Rect::default(), area, None, None, &app);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..30).map(|y| row_text(&buffer, y)).collect();
        let first = rows
            .iter()
            .position(|r| r.contains("as of "))
            .unwrap_or_else(|| panic!("the first line at {pane}: {rows:#?}"));
        let body = |row: &str| row.trim_matches('│').trim_end().to_string();
        let headline = body(&rows[first + 1]);
        assert!(headline.ends_with("age"), "headline at {pane}: {rows:#?}");
        for i in 0..3 {
            let row = body(&rows[first + 2 + i]);
            assert!(row.ends_with("17h ago"), "run {i} at {pane}: {rows:#?}");
            assert!(row.contains("9m 14s"), "run {i} at {pane}: {rows:#?}");
        }
        let marked: usize = rows[first + 2..first + 5]
            .iter()
            .map(|r| r.matches('▸').count())
            .sum();
        assert_eq!(marked, 1, "the cursor on one row at {pane}");
        assert!(
            body(&rows[first + 5]).is_empty(),
            "nothing under the runs at {pane}: {rows:#?}"
        );
    }
}

/// FZ-82: below the row's floor (lamp, duration and age, 23 columns)
/// the run rows and the headline are cut at the inner width, never
/// wrapped. Panes 20 to 26 hold 16 to 22 columns inside. Fails while a
/// row past the floor wraps its age onto a second row.
#[test]
fn below_its_floor_a_run_row_is_clipped_not_wrapped() {
    let mut app = app();
    app.focused_panel = FocusedPanel::Detail;
    landed(&mut app, screenshot_runs(), Local::now());
    for pane in 20..=26u16 {
        let mut terminal = Terminal::new(TestBackend::new(pane, 30)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                crate::ui::detail::render(f, Rect::default(), area, None, None, &app);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..30).map(|y| row_text(&buffer, y)).collect();
        let first = rows
            .iter()
            .position(|r| r.contains("as of "))
            .unwrap_or_else(|| panic!("the first line at {pane}: {rows:#?}"));
        let body = |row: &str| row.trim_matches('│').trim().to_string();
        assert!(
            !body(&rows[first + 1]).contains('◉'),
            "the headline on one row at {pane}: {rows:#?}"
        );
        for i in 0..3 {
            let row = body(&rows[first + 2 + i]);
            assert!(row.contains('◉'), "run {i} on one row at {pane}: {rows:#?}");
        }
        assert!(
            body(&rows[first + 5]).is_empty(),
            "nothing wrapped under the runs at {pane}: {rows:#?}"
        );
        // The pinned header draws unwrapped, so also hold the lines
        // themselves to the inner width: a short pane scrolls the
        // headline with the body, which wraps.
        let inner = usize::from(pane) - 4;
        let now = Local::now();
        let head = headline(&app, inner, now).expect("runs are listed");
        assert!(head.width() <= inner, "headline at {pane}: {head:?}");
        for row in run_rows(&app, inner, now, 0, true) {
            assert!(row.width() <= inner, "run at {pane}: {row:?}");
        }
    }
}

/// His look on #95: at 170 columns the workflow was cut at 18
/// characters beside 60 empty ones. Fails while the column is capped
/// short of the name.
#[test]
fn at_170_columns_the_workflow_name_is_drawn_in_full() {
    let mut app = app();
    landed(&mut app, runs_with_release(), Local::now());
    let (rects, buffer) = draw(&app, 170, 50);
    let detail = rects.detail.expect("detail pane laid out");
    let rows: Vec<String> = (detail.y..detail.y + detail.height)
        .map(|y| row_text(&buffer, y))
        .collect();
    assert!(
        rows.iter().any(|r| r.contains(RELEASE)),
        "{RELEASE} cut: {rows:#?}"
    );
}

/// The headline and the run rows of `runs_with_release` at `width`, as
/// text. Headed widths: workflow 27, branch 30, trigger 17, duration 8,
/// age 7; so every column from 123, no trigger from 104, no title from
/// 82, and below 82 the workflow name gives way.
fn laid_out(width: usize) -> (String, Vec<String>) {
    let now = local("2026-10-21T13:16:30+02:00");
    let mut app = app();
    landed(&mut app, runs_with_release(), now);
    let headline = headline(&app, width, now).expect("runs are listed");
    let rows = run_rows(&app, width, now, 0, false);
    (line_text(&headline), rows.iter().map(line_text).collect())
}

/// The fixture's long branch, as its capped column draws it.
fn capped_branch() -> String {
    crate::ui::truncate("feat/79-date-the-failed-attempt", BRANCH_MAX)
}

#[test]
fn a_wide_row_shows_every_column_under_its_word() {
    let (head_row, rows) = laid_out(170);
    assert_eq!(
        head_row.split_whitespace().collect::<Vec<_>>(),
        HEADINGS,
        "{head_row}"
    );
    let release = &rows[3];
    for cell in [
        RELEASE,
        "main",
        "workflow_dispatch",
        runs()[1].title.as_str(),
    ] {
        assert!(release.contains(cell), "{cell} missing: {release}");
    }
    assert!(rows[0].contains(&capped_branch()), "{}", rows[0]);
    for row in &rows {
        assert_eq!(row.chars().count(), 170, "the title fills: {row}");
        assert!(row.ends_with("ago"), "{row}");
    }
    // In columns, not bytes: the lamp is wider in UTF-8 than the blank
    // the headline holds for it.
    let at = |row: &str, text: &str| row[..row.find(text).unwrap()].chars().count();
    assert_eq!(at(&head_row, "workflow"), at(release, RELEASE));
    assert_eq!(at(&head_row, "branch"), at(release, "main"));
    assert_eq!(at(&head_row, "trigger"), at(release, "workflow_dispatch"));
    assert_eq!(at(&head_row, "title"), at(release, &runs()[1].title));
}

#[test]
fn the_trigger_goes_first() {
    let (head_row, rows) = laid_out(110);
    assert!(!head_row.contains("trigger"), "{head_row}");
    assert!(head_row.contains("title"), "{head_row}");
    let release = &rows[3];
    assert!(!release.contains("workflow_dispatch"), "{release}");
    assert!(release.contains(RELEASE), "{release}");
    assert!(release.contains(&head(&runs()[1])), "{release}");
    assert_eq!(release.chars().count(), 110);
}

#[test]
fn the_title_goes_second() {
    let (head_row, rows) = laid_out(90);
    assert_eq!(
        head_row.split_whitespace().collect::<Vec<_>>(),
        ["workflow", "branch", "duration", "age"],
        "{head_row}"
    );
    assert!(!rows[3].contains(&head(&runs()[1])), "{}", rows[3]);
    assert!(
        rows[3].contains(RELEASE),
        "nothing truncated yet: {}",
        rows[3]
    );
    assert!(rows[0].contains(&capped_branch()), "{}", rows[0]);
    for row in &rows {
        assert_eq!(row.chars().count(), 90, "{row}");
        assert!(row.ends_with("ago"), "{row}");
    }
}

#[test]
fn then_the_workflow_name_truncates_and_the_branch_holds() {
    let (head_row, rows) = laid_out(70);
    assert!(head_row.starts_with("    workflow  "), "{head_row}");
    assert!(rows[3].contains("Release Traffi…  main"), "{}", rows[3]);
    assert!(rows[0].contains(&capped_branch()), "{}", rows[0]);
    // Branch last: only past the workflow's floor.
    let (_, rows) = laid_out(50);
    assert!(rows[3].contains("Release…  main"), "{}", rows[3]);
    assert!(rows[0].contains("feat/79-date-the…"), "{}", rows[0]);
}

#[test]
fn duration_and_age_stay_at_every_width() {
    let now = local("2026-10-21T13:16:30+02:00").with_timezone(&Utc);
    let held = runs_with_release();
    for width in 23..=200 {
        let (head_row, rows) = laid_out(width);
        let words: Vec<&str> = head_row.split_whitespace().collect();
        assert!(words.ends_with(&["duration", "age"]), "{width}: {head_row}");
        assert!(head_row.ends_with("age"), "{width}: {head_row}");
        for (row, run) in rows.iter().zip(&held) {
            let age = run_age(run, now);
            let before_age = row.strip_suffix(&age).expect("age last");
            assert!(
                before_age.trim_end().ends_with(&run_duration(run, now)),
                "{width}: {row}"
            );
            assert!(row.chars().count() <= width, "{width}: {row}");
        }
    }
}

/// The headline sits between the first line and the runs, and the
/// cursor starts on the first run, not on it. No runs, no headline.
#[test]
fn the_headline_heads_the_runs_and_is_not_a_cursor_stop() {
    let mut app = app();
    app.focused_panel = FocusedPanel::Detail;
    landed(&mut app, runs(), Local::now());
    let rows = detail_rows(&app);
    let at = rows
        .iter()
        .position(|r| r.contains("workflow") && r.contains("duration"))
        .expect("the headline");
    assert!(rows[at - 1].contains("as of "), "{}", rows[at - 1]);
    assert!(!rows[at].contains('▸'), "{}", rows[at]);
    assert!(rows[at + 1].contains('▸'), "{}", rows[at + 1]);
    assert!(rows[at + 1].contains(&head(&runs()[0])), "{}", rows[at + 1]);
    assert_eq!(app.selected_commit_count(), 3, "runs only");

    landed(&mut app, Vec::new(), Local::now());
    assert!(headline(&app, 120, Local::now()).is_none());
    let rows = detail_rows(&app);
    assert!(rows.iter().all(|r| !r.contains("duration")), "{rows:#?}");
}

// --- The inspector's run bodies (FZ-30f) ---

const JOBS_FAILED: &str = include_str!("../../ci/fixtures/jobs-failed.json");
const LOG_FAILED: &str = include_str!("../../ci/fixtures/job-log-failed.txt");
const CI_YML: &str = include_str!("../../ci/fixtures/ci.yml");
/// The recorded failed run is the fixture's third.
const FAILED_RUN: usize = 2;
const FAILED_JOB: u64 = 113914377501;
const WORKFLOW: &str = ".github/workflows/ci.yml";

/// The runs fixture with the failed run naming its workflow file, which
/// the trimmed recording left out.
fn runs_with_path() -> Vec<WorkflowRun> {
    let mut runs = runs();
    runs[FAILED_RUN].path = Some(WORKFLOW.to_string());
    runs
}

fn jobs() -> Vec<Job> {
    crate::ci::parse_jobs(JOBS_FAILED).unwrap()
}

/// An app in `ci` on the recorded failed run, its jobs, tail and whole
/// log landed by one poll, as the poll for a highlighted failed run
/// lands them.
fn on_failed_run() -> App {
    let mut app = app();
    landed(&mut app, runs_with_path(), Local::now());
    app.set_detail_selected(FAILED_RUN);
    let run_id = runs()[FAILED_RUN].id;
    let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    let step = crate::ci::failing_step(&jobs()[0]).cloned().unwrap();
    app.apply_ci(
        crate::ci::CiAnswer {
            key: key(),
            attempt: request.attempt,
            runs: Some(crate::ci::Fetched::NotModified),
            jobs: Some((
                run_id,
                crate::ci::JobsAnswer::Fetched(crate::ci::Fetched::Fresh {
                    data: jobs(),
                    etag: None,
                }),
            )),
            log: Some((
                FAILED_JOB,
                LogTail::Lines(crate::ci::log_tail(LOG_FAILED, &step)),
            )),
            full_log: Some((FAILED_JOB, crate::ci::log_lines(LOG_FAILED))),
            failure: None,
            budget: None,
            tip: None,
        },
        Local::now(),
    );
    app
}

fn body_text(app: &App) -> Vec<String> {
    inspector_body(app, Local::now())
        .iter()
        .map(line_text)
        .collect()
}

fn state(app: &App) -> &CiState {
    &app.ci[&key()]
}

/// FZ-30f: the `jobs` body from recorded payloads: the run, each job
/// with its lamp and duration, its steps one per line, and under the
/// failed job's failing step the dimmed log tail. Fails while the
/// inspector says `no commit selected` on a run.
#[test]
fn the_jobs_body_draws_the_jobs_steps_and_the_failing_tail() {
    let app = on_failed_run();
    assert_eq!(app.ci_body, CiBody::Jobs, "jobs is the first body");
    let text = body_text(&app);
    assert!(text[0].contains("feat: pace GitHub polling"), "{text:#?}");
    let job = text.iter().position(|l| l.contains("check")).unwrap();
    assert!(text[job].starts_with("▸ ◉ check"), "marked: {}", text[job]);
    assert!(
        text[job].ends_with("54s"),
        "16:19:46 to 16:20:40: {}",
        text[job]
    );
    assert_eq!(text[job + 1], "    ✓ Set up job");
    let failing = text
        .iter()
        .position(|l| l == "    ✗ Run cargo clippy --all-targets -- -D warnings")
        .expect("the failing step on its own line");
    let tail = &text[failing + 1..failing + 1 + crate::ci::LOG_TAIL_LINES];
    assert_eq!(
        tail.last().unwrap(),
        "      ##[error]Process completed with exit code 101."
    );
    assert_eq!(
        text[failing + 1 + crate::ci::LOG_TAIL_LINES],
        "    · Run cargo test",
        "skipped, and the steps go on under the tail"
    );
    let lines = inspector_body(&app, Local::now());
    assert_eq!(
        lines[failing + 1].spans[0].style.fg,
        Some(colors::OVERLAY),
        "the tail is dimmed"
    );
}

/// The `log` body: the marked job's whole log, stamps stripped.
#[test]
fn the_log_body_draws_the_marked_jobs_whole_log() {
    let mut app = on_failed_run();
    app.inspector_pct = 30;
    app.ci_body = CiBody::Log;
    let text = body_text(&app);
    assert!(text.contains(&"check log".to_string()), "{text:#?}");
    let (job_id, log) = shown_log(&app).expect("the held log");
    assert_eq!(job_id, FAILED_JOB);
    assert_eq!(log, &crate::ci::log_lines(LOG_FAILED)[..]);
    assert!(log.iter().all(|l| !l.starts_with("2026-")));
    let (rects, buffer) = draw(&app, 160, 50);
    let inspector = rects.inspector.expect("inspector laid out");
    let drawn: Vec<String> = (inspector.y..inspector.y + inspector.height)
        .map(|y| row_text(&buffer, y))
        .collect();
    assert!(
        drawn
            .iter()
            .any(|r| r.contains("Current runner version: '2.337.0'")),
        "the log is drawn under its head: {drawn:#?}"
    );
}

/// The whole log is wrapped once per job and width and the copy kept:
/// a second frame draws the kept rows, a new width wraps again. Fails
/// while every frame re-wraps the log.
#[test]
fn the_whole_log_is_wrapped_once_per_job_and_width() {
    let mut app = on_failed_run();
    app.inspector_pct = 30;
    app.ci_body = CiBody::Log;
    draw(&app, 160, 50);
    let width = {
        let mut kept = app.ci_log_wrap.borrow_mut();
        let wrap = kept.as_mut().expect("wrapped on the first frame");
        assert_eq!(wrap.job_id, FAILED_JOB);
        wrap.lines[0] = Line::from("KEPT ROW");
        wrap.width
    };
    let (_, buffer) = draw(&app, 160, 50);
    let rows: Vec<String> = (0..50).map(|y| row_text(&buffer, y)).collect();
    assert!(
        rows.iter().any(|r| r.contains("KEPT ROW")),
        "not re-wrapped"
    );
    let (_, buffer) = draw(&app, 140, 50);
    let rows: Vec<String> = (0..50).map(|y| row_text(&buffer, y)).collect();
    assert!(
        rows.iter().all(|r| !r.contains("KEPT ROW")),
        "a new width wraps again"
    );
    assert_ne!(app.ci_log_wrap.borrow().as_ref().unwrap().width, width);
}

/// Another job's log at the same width is wrapped afresh, never drawn
/// from the first job's kept rows. Fails while the cache is keyed by
/// width alone.
#[test]
fn another_jobs_log_at_the_same_width_is_wrapped_afresh() {
    let mut app = on_failed_run();
    app.inspector_pct = 30;
    let mut second = jobs()[0].clone();
    second.id = 7;
    second.name = "docs".to_string();
    let run_id = runs()[FAILED_RUN].id;
    let held = app.ci.get_mut(&key()).unwrap();
    held.jobs.get_mut(&run_id).unwrap().jobs.push(second);
    held.full_logs.insert(
        7,
        Detail::Ready(vec!["the second job's own log".to_string()]),
    );
    app.ci_body = CiBody::Log;
    draw(&app, 160, 50);
    {
        let mut kept = app.ci_log_wrap.borrow_mut();
        let wrap = kept.as_mut().expect("wrapped on the first frame");
        assert_eq!(wrap.job_id, FAILED_JOB);
        wrap.lines[0] = Line::from("KEPT ROW");
    }
    app.ci_job_mark = Some(1);
    assert_eq!(app.ci_marked_job().map(|j| j.id), Some(7));
    let (_, buffer) = draw(&app, 160, 50);
    let rows: Vec<String> = (0..50).map(|y| row_text(&buffer, y)).collect();
    assert!(
        rows.iter().all(|r| !r.contains("KEPT ROW")),
        "the first job's rows are gone: {rows:#?}"
    );
    assert!(rows.iter().any(|r| r.contains("the second job's own log")));
    assert_eq!(app.ci_log_wrap.borrow().as_ref().unwrap().job_id, 7);
}

/// An app whose next `claim_ci_details` claims the second run's jobs
/// at `at`, so each gate below can be shown to hold that claim alone.
fn claimable() -> (App, std::time::Instant) {
    let mut app = on_failed_run();
    app.set_detail_selected(1);
    (app, std::time::Instant::now() + crate::ci::DWELL * 2)
}

#[test]
fn under_d_the_inspector_fetches_nothing() {
    let (mut app, at) = claimable();
    app.dirty_mode = true;
    assert!(app.claim_ci_details(Local::now(), at).is_empty());
    app.dirty_mode = false;
    assert_eq!(app.claim_ci_details(Local::now(), at).len(), 1);
}

#[test]
fn outside_the_ci_view_the_inspector_fetches_nothing() {
    let (mut app, at) = claimable();
    app.history_view = HistoryView::Branches;
    assert!(app.claim_ci_details(Local::now(), at).is_empty());
    app.history_view = HistoryView::Ci;
    assert_eq!(app.claim_ci_details(Local::now(), at).len(), 1);
}

#[test]
fn a_folded_inspector_fetches_nothing() {
    let (mut app, at) = claimable();
    app.inspector_folded = true;
    assert!(app.claim_ci_details(Local::now(), at).is_empty());
    app.inspector_folded = false;
    assert_eq!(app.claim_ci_details(Local::now(), at).len(), 1);
}

/// Take a `core` reading of `remaining` of 5000, resetting at 12:40.
fn read_budget(app: &mut App, remaining: u32, now: DateTime<Local>) {
    let reading = crate::github::BudgetReading {
        limit: 5000,
        remaining,
        used: 5000 - remaining,
        reset: local("2026-10-21T12:40:00+02:00").timestamp(),
        date: Some(now.with_timezone(&Utc)),
        token_id: "tok".to_string(),
    };
    app.ci_budget.observe(&reading, true, now, &mut NoJitter);
}

#[test]
fn a_spent_core_budget_holds_the_inspector_fetches_until_the_reset() {
    let (mut app, at) = claimable();
    let now = local("2026-10-21T12:00:10+02:00");
    read_budget(&mut app, 0, now);
    assert!(app.claim_ci_details(now, at).is_empty());
    let reset = local("2026-10-21T12:40:01+02:00");
    assert_eq!(app.claim_ci_details(reset, at).len(), 1);
}

/// F1 of the review: the inspector keeps the poll's reserve, so it
/// cannot spend the last tenth of the budget the poll paces around.
#[test]
fn the_inspector_fetches_honour_the_polls_reserve() {
    let (mut app, at) = claimable();
    let now = local("2026-10-21T12:00:10+02:00");
    read_budget(&mut app, 500, now);
    assert!(app.claim_ci_details(now, at).is_empty(), "at the reserve");
    let reset = local("2026-10-21T12:40:01+02:00");
    assert_eq!(app.claim_ci_details(reset, at).len(), 1);
    let (mut above, at) = claimable();
    read_budget(&mut above, 501, now);
    assert_eq!(above.claim_ci_details(now, at).len(), 1, "above it");
}

/// The `script` body: the workflow file at the run's commit, the failing
/// step's line marked when the run failed. Fails while the inspector
/// has no script.
#[test]
fn the_script_body_highlights_the_failing_steps_block() {
    let mut app = on_failed_run();
    app.ci_body = CiBody::Script;
    let sha = runs()[FAILED_RUN].head_sha.clone().unwrap();
    assert!(body_text(&app).contains(&"loading script".to_string()));
    app.ci.get_mut(&key()).unwrap().scripts.insert(
        (sha, WORKFLOW.to_string()),
        Detail::Ready(CI_YML.to_string()),
    );
    let text = body_text(&app);
    assert!(
        text.iter()
            .any(|l| l.contains(".github/workflows/ci.yml @ 93f4c8e"))
    );
    assert!(
        text.contains(&"✗ Run cargo clippy --all-targets -- -D warnings  (line 27)".to_string()),
        "{text:#?}"
    );
    let marked: Vec<&String> = text.iter().filter(|l| l.contains('▌')).collect();
    assert_eq!(marked.len(), 1, "{marked:#?}");
    assert!(marked[0].contains("- run: cargo clippy --all-targets -- -D warnings"));
    assert!(marked[0].starts_with("27 ▌"), "{}", marked[0]);

    // A passing run highlights nothing.
    app.ci.get_mut(&key()).unwrap().runs[FAILED_RUN].conclusion = Some("success".to_string());
    assert!(body_text(&app).iter().all(|l| !l.contains('▌')));
}

/// `[`/`]` on the focused inspector cycle its bodies, both ways, and the
/// title names the body; anywhere else they still cycle pane 2's views.
#[test]
fn brackets_cycle_the_bodies_on_the_focused_inspector_and_the_title_names_it() {
    let mut app = on_failed_run();
    app.focused_panel = FocusedPanel::Inspector;
    app.press_bracket(false);
    assert_eq!(app.ci_body, CiBody::Log);
    app.press_bracket(false);
    assert_eq!(app.ci_body, CiBody::Script);
    app.press_bracket(false);
    assert_eq!(app.ci_body, CiBody::Jobs, "wraps forward");
    app.press_bracket(true);
    assert_eq!(app.ci_body, CiBody::Script, "wraps backward");
    assert_eq!(app.history_view, HistoryView::Ci, "the view held");

    app.ci_body = CiBody::Log;
    let (rects, buffer) = draw(&app, 160, 50);
    let inspector = rects.inspector.expect("inspector laid out");
    let title = row_text(&buffer, inspector.y);
    assert!(title.contains("[/] JOBS · LOG · SCRIPT"), "{title}");

    app.focused_panel = FocusedPanel::Detail;
    app.press_bracket(false);
    assert_eq!(
        app.history_view,
        HistoryView::Notes,
        "pane 2 cycles its views"
    );
    assert_eq!(app.ci_body, CiBody::Log);
}

/// The cell where `needle` starts on row `y`, which must hold it.
fn cell_of<'a>(buffer: &'a Buffer, y: u16, needle: &str) -> &'a ratatui::buffer::Cell {
    let row = row_text(buffer, y);
    let byte = row
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not on row {y}: {row}"));
    let x = row[..byte].chars().count() as u16;
    &buffer[(x, y)]
}

/// FZ-64: with room, the inspector's top border names every body as a
/// tab, as pane 2's names its views: the shown one bright in the `ci`
/// tab's colour, the others dim, and the title no longer repeats the
/// body's word.
#[test]
fn the_inspector_border_names_every_body_with_the_shown_one_lit() {
    let mut app = on_failed_run();
    app.focused_panel = FocusedPanel::Inspector;
    app.ci_body = CiBody::Log;
    let (rects, buffer) = draw(&app, 160, 50);
    let inspector = rects.inspector.expect("inspector laid out");
    let title = row_text(&buffer, inspector.y);
    assert!(title.contains("[/] JOBS · LOG · SCRIPT "), "{title}");
    assert!(title.contains("[3] inspector "), "{title}");
    assert!(!title.contains("inspector  log"), "{title}");
    assert!(
        title.trim_end().ends_with("SCRIPT ╮"),
        "at the right: {title}"
    );

    let lit = cell_of(&buffer, inspector.y, "LOG");
    assert_eq!(lit.fg, colors::history_view_color(HistoryView::Ci));
    assert!(lit.modifier.contains(Modifier::BOLD));
    for dim in ["JOBS", "SCRIPT"] {
        assert_eq!(
            cell_of(&buffer, inspector.y, dim).fg,
            colors::SUBTEXT,
            "{dim}"
        );
    }
}

/// FZ-64: a border too narrow for all three keeps the shown body alone,
/// as pane 2 drops to `[/] CI`.
#[test]
fn a_narrow_inspector_border_keeps_the_shown_body_alone() {
    let mut app = on_failed_run();
    app.focused_panel = FocusedPanel::Inspector;
    app.ci_body = CiBody::Script;
    let (rects, buffer) = draw(&app, 40, 50);
    let inspector = rects.inspector.expect("inspector laid out");
    let title = row_text(&buffer, inspector.y);
    assert!(title.contains("[/] SCRIPT "), "{title}");
    assert!(!title.contains("JOBS"), "{title}");
    assert!(!title.contains("LOG"), "{title}");
    let lit = cell_of(&buffer, inspector.y, "SCRIPT");
    assert_eq!(lit.fg, colors::history_view_color(HistoryView::Ci));
    assert!(lit.modifier.contains(Modifier::BOLD));
}

/// FZ-64: the commit inspector keeps its plain title; the tabs belong to
/// a run only.
#[test]
fn the_commit_inspector_border_has_no_body_tabs() {
    let mut app = on_failed_run();
    app.focused_panel = FocusedPanel::Inspector;
    app.history_view = HistoryView::Commits;
    assert!(!app.inspects_ci_run());
    let (rects, buffer) = draw(&app, 160, 50);
    let inspector = rects.inspector.expect("inspector laid out");
    let title = row_text(&buffer, inspector.y);
    // The title, then nothing but the rule (and its resize grip) to the
    // corner.
    let rest = title
        .trim_end()
        .strip_prefix("╭ [3] inspector ")
        .unwrap_or_else(|| panic!("{title}"));
    assert!(
        rest.ends_with('╮') && rest.chars().all(|c| matches!(c, '─' | '⠶' | '╮')),
        "{title}"
    );
}

/// FZ-64: the resize grip sits mid-border and is painted after the
/// panes, so where the tabs would run under it they drop to the shown
/// body alone, clear of the mark. Before, widths 44 to 56 drew
/// `⠶⠶⠶⠶⠶⠶/] JOBS · LOG · SCRIPT`.
#[test]
fn the_inspector_tabs_never_run_under_the_resize_grip() {
    let mut app = on_failed_run();
    app.focused_panel = FocusedPanel::Inspector;
    app.ci_body = CiBody::Log;
    for width in 44..=56 {
        let (rects, buffer) = draw(&app, width, 50);
        let inspector = rects.inspector.expect("inspector laid out");
        let title = row_text(&buffer, inspector.y);
        let tabs = &title[title.find(" [/] ").unwrap_or_else(|| panic!("{title}"))..];
        assert!(!tabs.contains('⠶'), "w={width}: {title}");
        assert!(tabs.contains(" LOG "), "w={width}: {title}");
        let lit = cell_of(&buffer, inspector.y, "LOG");
        assert_eq!(
            lit.fg,
            colors::history_view_color(HistoryView::Ci),
            "w={width}"
        );
        assert!(title.contains('⠶'), "the grip is still drawn: {title}");
    }
}

/// FZ-64: a `ci` view with no run to inspect (loading here, and likewise
/// empty, offline or without a remote) has no bodies to step, and `[`/`]`
/// step pane 2 there, so its border shows no tabs.
#[test]
fn the_inspector_border_has_no_tabs_without_a_highlighted_run() {
    let mut app = app();
    app.focused_panel = FocusedPanel::Inspector;
    assert!(app.inspects_ci_run());
    assert!(app.ci_highlighted_run().is_none());
    let (rects, buffer) = draw(&app, 160, 50);
    let inspector = rects.inspector.expect("inspector laid out");
    let title = row_text(&buffer, inspector.y);
    assert!(title.contains("[3] inspector "), "{title}");
    assert!(!title.contains("[/]"), "{title}");
    assert!(!title.contains("JOBS"), "{title}");
}

/// Each body keeps its own scroll; a different highlighted run starts
/// every body at the top and the job mark on its default.
#[test]
fn a_run_change_resets_every_bodys_scroll_and_the_mark() {
    let mut app = on_failed_run();
    app.focused_panel = FocusedPanel::Inspector;
    app.inspector_max_scroll.set(100);
    app.ci_body = CiBody::Log;
    app.scroll_inspector(5);
    app.ci_body = CiBody::Script;
    app.scroll_inspector(2);
    assert_eq!(app.ci_body_scroll(), 2);
    app.ci_body = CiBody::Log;
    assert_eq!(app.ci_body_scroll(), 5, "kept per body");
    assert_eq!(app.inspector_scroll, 0, "the commit inspector's untouched");

    app.focused_panel = FocusedPanel::Detail;
    app.detail_nav_up();
    assert_eq!(app.ci_body_scroll(), 0, "reads as reset at once");
    app.detail_nav_down();
    assert_eq!(app.ci_body_scroll(), 0, "and stays reset");
    app.sync_ci_inspector(std::time::Instant::now());
    assert_eq!(app.ci_body_scrolls, [0, 0, 0]);
}

/// `j`/`k` in the focused `jobs` body move the mark the `log` body
/// follows; in the other bodies they scroll.
#[test]
fn j_and_k_move_the_job_mark_in_the_jobs_body() {
    let mut app = on_failed_run();
    let mut second = jobs()[0].clone();
    second.id = 7;
    second.name = "docs".to_string();
    second.conclusion = Some("success".to_string());
    let run_id = runs()[FAILED_RUN].id;
    let held = app.ci.get_mut(&key()).unwrap();
    held.jobs.get_mut(&run_id).unwrap().jobs.insert(0, second);
    assert_eq!(app.ci_marked_job_index(), Some(1), "the first failed job");
    app.focused_panel = FocusedPanel::Inspector;
    app.inspector_nav(-1);
    assert_eq!(app.ci_marked_job().unwrap().name, "docs");
    app.inspector_nav(-1);
    assert_eq!(app.ci_marked_job_index(), Some(0), "clamped");
    app.inspector_nav(1);
    app.inspector_nav(1);
    assert_eq!(app.ci_marked_job_index(), Some(1), "clamped at the last");
    app.ci_body = CiBody::Log;
    app.inspector_max_scroll.set(100);
    app.inspector_nav(1);
    assert_eq!(app.ci_marked_job_index(), Some(1), "log scrolls instead");
    assert_eq!(app.ci_body_scroll(), 1);
}

/// The inspector's poll reads the highlighted run's jobs while it is
/// open, and no run's once it is folded.
#[test]
fn the_highlighted_run_is_the_one_the_poll_expands() {
    let mut app = app();
    landed(&mut app, runs(), Local::now());
    app.set_detail_selected(1);
    app.sync_ci_inspector(std::time::Instant::now());
    assert_eq!(state(&app).expanded, Some(runs()[1].id));
    app.inspector_folded = true;
    app.sync_ci_inspector(std::time::Instant::now());
    assert_eq!(state(&app).expanded, None);
}

/// The fetch-once rules: nothing before the run has been highlighted
/// for the dwell; then the shown body's fetch once; held or loading, not
/// again; a failure again after a minute; a log the poll brought, never.
#[test]
fn each_body_fetches_once_per_job_or_commit_and_path() {
    use crate::ci::DetailWant;
    let mut app = on_failed_run();
    let now = Local::now();
    let t0 = std::time::Instant::now();
    let settled = t0 + crate::ci::DWELL;
    assert!(
        app.claim_ci_details(now, t0).is_empty(),
        "jobs and tail held"
    );

    // The poll brought the failed job's whole log: the body never asks.
    app.ci_body = CiBody::Log;
    assert!(app.claim_ci_details(now, settled).is_empty());

    // A run whose jobs no poll read: the jobs body asks once.
    app.ci_body = CiBody::Jobs;
    app.set_detail_selected(1);
    assert!(
        app.claim_ci_details(now, settled).is_empty(),
        "dwell restarts"
    );
    let later = settled + crate::ci::DWELL;
    let wants: Vec<DetailWant> = app
        .claim_ci_details(now, later)
        .into_iter()
        .map(|r| r.want)
        .collect();
    assert_eq!(
        wants,
        vec![DetailWant::Jobs {
            run_id: runs()[1].id
        }]
    );
    assert!(app.claim_ci_details(now, later).is_empty(), "loading");

    // The failed run's script: once per commit and path, the clone
    // named; a failure is asked again a minute on.
    app.set_detail_selected(FAILED_RUN);
    app.repos[0].path = Some("/tmp/fleetz".to_string());
    app.ci_body = CiBody::Script;
    let at = later + crate::ci::DWELL * 2;
    let requests = app.claim_ci_details(now, at);
    assert_eq!(requests.len(), 1, "{requests:#?}");
    let sha = runs()[FAILED_RUN].head_sha.clone().unwrap();
    assert_eq!(
        requests[0].want,
        DetailWant::Script {
            sha: sha.clone(),
            path: WORKFLOW.to_string(),
            clone: Some(std::path::PathBuf::from("/tmp/fleetz")),
        }
    );
    assert!(app.claim_ci_details(now, at).is_empty());
    app.apply_ci_detail(
        crate::ci::DetailAnswer {
            key: key(),
            got: crate::ci::DetailGot::Script {
                sha: sha.clone(),
                path: WORKFLOW.to_string(),
                result: Err(crate::github::Failure::new(
                    crate::github::FailureKind::Offline,
                    "no route",
                )),
            },
            budget: None,
        },
        now,
    );
    assert!(
        app.claim_ci_details(now + chrono::Duration::seconds(59), at)
            .is_empty()
    );
    assert_eq!(
        app.claim_ci_details(now + chrono::Duration::seconds(60), at)
            .len(),
        1,
        "again after a minute"
    );
}

/// A job's whole log the body is reading carries its tail, so the poll
/// does not download the same log for the tail.
#[test]
fn a_log_the_body_reads_is_not_read_again_by_the_poll() {
    let mut app = app();
    landed(&mut app, runs(), Local::now());
    app.set_detail_selected(FAILED_RUN);
    let run_id = runs()[FAILED_RUN].id;
    app.ci.get_mut(&key()).unwrap().jobs.insert(
        run_id,
        crate::app::CiJobs {
            etag: None,
            jobs: jobs(),
        },
    );
    app.ci_body = CiBody::Log;
    let t0 = std::time::Instant::now();
    app.claim_ci_details(Local::now(), t0);
    let requests = app.claim_ci_details(Local::now(), t0 + crate::ci::DWELL);
    let want = &requests[0].want;
    let crate::ci::DetailWant::Log { job_id, step } = want else {
        panic!("a log request: {want:?}")
    };
    assert_eq!(*job_id, FAILED_JOB);
    assert!(step.is_some(), "the tail is owed, so the step rides along");
    app.sync_ci_inspector(t0);
    let poll = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    assert_eq!(poll.expanded.unwrap().logs_held, vec![FAILED_JOB]);
}

/// The inspector's rows as drawn at 160 by 50.
fn inspector_rows(app: &App) -> Vec<String> {
    let (rects, buffer) = draw(app, 160, 50);
    let inspector = rects.inspector.expect("inspector laid out");
    (inspector.y..inspector.y + inspector.height)
        .map(|y| {
            (inspector.x..inspector.x + inspector.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

/// The first drawn row of the inspector's content that contains
/// `needle`, or `None` when it is not on screen.
fn drawn_row_of(app: &App, needle: &str) -> Option<usize> {
    inspector_rows(app).iter().position(|r| r.contains(needle))
}

/// `j`/`k` on the focused inspector and the wheel over it scroll the
/// shown `log` and `script` bodies as drawn, through the frame's own
/// measure. Pins the scroll path the fix cycle 3 look reported; it
/// already held on the base for these two bodies.
#[test]
fn j_k_and_the_wheel_scroll_the_log_and_script_bodies() {
    let mut app = on_failed_run();
    // Draws at the pre-FZ-88 inspector size (40%) it was written for.
    app.inspector_pct = 40;
    let sha = runs()[FAILED_RUN].head_sha.clone().unwrap();
    app.ci.get_mut(&key()).unwrap().scripts.insert(
        (sha, WORKFLOW.to_string()),
        Detail::Ready(CI_YML.to_string()),
    );
    app.focused_panel = FocusedPanel::Inspector;
    for (body, needle) in [
        (CiBody::Log, "Current runner version"),
        (CiBody::Script, "name: ci"),
    ] {
        app.ci_body = body;
        let at = drawn_row_of(&app, needle).expect("drawn at the top");
        app.inspector_nav(2);
        assert_eq!(drawn_row_of(&app, needle), Some(at - 2), "j: {body:?}");
        app.inspector_nav(-1);
        assert_eq!(drawn_row_of(&app, needle), Some(at - 1), "k: {body:?}");
        app.focused_panel = FocusedPanel::Detail;
        app.scroll_inspector(-1);
        assert_eq!(drawn_row_of(&app, needle), Some(at), "wheel: {body:?}");
        app.focused_panel = FocusedPanel::Inspector;
    }
}

/// A failed run whose jobs the inspector read from `/jobs` and no log
/// yet, highlighted in `ci`.
fn on_failed_run_without_log() -> App {
    let mut app = app();
    landed(&mut app, runs_with_path(), Local::now());
    app.set_detail_selected(FAILED_RUN);
    let run_id = runs()[FAILED_RUN].id;
    app.ci.get_mut(&key()).unwrap().jobs.insert(
        run_id,
        crate::app::CiJobs {
            etag: None,
            jobs: jobs(),
        },
    );
    app
}

/// FZ-30f fix cycle 3: the jobs body paints from `/jobs` at once, and
/// after the dwell reads the selected failed job's log itself, once per
/// job id. Fails while the body claims no log and says `loading log`
/// until the repo's next poll, up to a minute on.
#[test]
fn the_jobs_body_reads_the_selected_failed_jobs_log_after_the_dwell() {
    use crate::ci::DetailWant;
    let mut app = on_failed_run_without_log();
    let text = body_text(&app);
    assert!(
        text.iter().any(|l| l.contains("check")),
        "painted: {text:#?}"
    );
    assert!(text.iter().all(|l| !l.contains("loading log")), "{text:#?}");

    let now = Local::now();
    let t0 = std::time::Instant::now();
    assert!(app.claim_ci_details(now, t0).is_empty(), "the dwell first");
    let settled = t0 + crate::ci::DWELL;
    let wants: Vec<DetailWant> = app
        .claim_ci_details(now, settled)
        .into_iter()
        .map(|r| r.want)
        .collect();
    let step = crate::ci::failing_step(&jobs()[0]).cloned();
    assert_eq!(
        wants,
        vec![DetailWant::Log {
            job_id: FAILED_JOB,
            step: step.clone(),
        }]
    );
    assert!(app.claim_ci_details(now, settled).is_empty(), "loading");

    app.apply_ci_detail(
        crate::ci::DetailAnswer {
            key: key(),
            got: crate::ci::DetailGot::Log {
                job_id: FAILED_JOB,
                result: Ok(crate::ci::log_lines(LOG_FAILED)),
                tail: Some(LogTail::Lines(crate::ci::log_tail(
                    LOG_FAILED,
                    &step.unwrap(),
                ))),
            },
            budget: None,
        },
        now,
    );
    assert!(
        body_text(&app)
            .iter()
            .any(|l| l.ends_with("##[error]Process completed with exit code 101.")),
        "the tail landed"
    );
    assert!(app.claim_ci_details(now, settled).is_empty(), "held");
}

/// Only the selected job's log: a passing job the cursor moved to has
/// no tail to show, so nothing is read for it.
#[test]
fn the_jobs_body_reads_no_log_for_a_selected_passing_job() {
    let mut app = on_failed_run_without_log();
    let mut passing = jobs()[0].clone();
    passing.id = 7;
    passing.conclusion = Some("success".to_string());
    let run_id = runs()[FAILED_RUN].id;
    let held = app.ci.get_mut(&key()).unwrap();
    held.jobs.get_mut(&run_id).unwrap().jobs.insert(0, passing);
    app.focused_panel = FocusedPanel::Inspector;
    let t0 = std::time::Instant::now();
    app.claim_ci_details(Local::now(), t0);
    app.inspector_nav(-1);
    assert_eq!(app.ci_marked_job().map(|j| j.id), Some(7));
    assert!(
        app.claim_ci_details(Local::now(), t0 + crate::ci::DWELL)
            .is_empty()
    );
}

/// A poll in flight reads the failed job's log with the jobs, so the
/// body waits for it rather than download the same log twice; once the
/// poll landed without it, the body asks.
#[test]
fn the_jobs_body_leaves_the_log_to_a_poll_in_flight() {
    let mut app = on_failed_run_without_log();
    let t0 = std::time::Instant::now();
    app.claim_ci_details(Local::now(), t0);
    app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    let settled = t0 + crate::ci::DWELL;
    assert!(app.claim_ci_details(Local::now(), settled).is_empty());
    failed(
        &mut app,
        crate::github::Failure::new(crate::github::FailureKind::Offline, "no route"),
        Local::now(),
    );
    assert_eq!(app.claim_ci_details(Local::now(), settled).len(), 1);
}

/// Jobs as a list: one row per job, and only the selected job's steps
/// and tail beneath it. Fails while every job draws its steps.
#[test]
fn only_the_selected_job_shows_its_steps() {
    let mut app = on_failed_run();
    let mut passing = jobs()[0].clone();
    passing.id = 7;
    passing.name = "docs".to_string();
    passing.conclusion = Some("success".to_string());
    let run_id = runs()[FAILED_RUN].id;
    let held = app.ci.get_mut(&key()).unwrap();
    held.jobs.get_mut(&run_id).unwrap().jobs.push(passing);
    let text = body_text(&app);
    let docs = text.iter().position(|l| l.contains("docs")).unwrap();
    assert!(
        text[docs..].iter().all(|l| !l.contains("Set up job")),
        "the unselected job draws no steps: {text:#?}"
    );
    app.focused_panel = FocusedPanel::Inspector;
    app.inspector_nav(1);
    let text = body_text(&app);
    let docs = text.iter().position(|l| l.contains("docs")).unwrap();
    assert!(text[docs].starts_with("▸"), "{text:#?}");
    assert!(text[docs..].iter().any(|l| l.contains("Set up job")));
    assert!(
        text.iter().all(|l| !l.contains("##[error]")),
        "the failed job, unselected, drops its tail: {text:#?}"
    );
}

/// A run with `n` passing jobs and the recorded failed one last.
fn on_long_run(n: u64) -> App {
    let mut app = on_failed_run();
    let run_id = runs()[FAILED_RUN].id;
    let held = app.ci.get_mut(&key()).unwrap();
    let list = &mut held.jobs.get_mut(&run_id).unwrap().jobs;
    for i in 0..n {
        let mut passing = jobs()[0].clone();
        passing.id = 1000 + i;
        passing.name = format!("job {i:02}");
        passing.conclusion = Some("success".to_string());
        list.insert(i as usize, passing);
    }
    app
}

/// The jobs body follows its cursor: a failed job below the fold is
/// brought into view on arrival, `k` back to the first job scrolls up
/// to it, and the wheel goes on from where the body followed to. Fails
/// while the body never scrolls to the marked job.
#[test]
fn the_jobs_body_follows_the_job_cursor() {
    let mut app = on_long_run(30);
    app.focused_panel = FocusedPanel::Inspector;
    app.sync_ci_inspector(std::time::Instant::now());
    let row = drawn_row_of(&app, "▸").expect("the failed job is in view");
    assert!(inspector_rows(&app)[row].contains("check"));

    app.inspector_nav(-30);
    let row = drawn_row_of(&app, "▸").expect("the first job is in view");
    assert!(inspector_rows(&app)[row].contains("job 00"));
    let followed = app.ci_body_scroll();
    assert!(followed > 0, "it scrolled up to the job, not to the top");

    app.scroll_inspector(1);
    assert_eq!(app.ci_body_scroll(), followed + 1, "on from there");
    inspector_rows(&app);
    assert_eq!(app.ci_body_scroll(), followed + 1, "the wheel holds");
}

/// `]` on the focused inspector in `branches` enters `ci` and goes on
/// cycling pane 2's views, held runs or not: the streak is the views'.
/// Fails while the second `]` steps the bodies.
#[test]
fn brackets_from_the_inspector_in_branches_cycle_on_through_ci() {
    let mut app = on_failed_run();
    app.history_view = HistoryView::Branches;
    app.focused_panel = FocusedPanel::Inspector;
    app.press_bracket(false);
    assert_eq!(app.history_view, HistoryView::Ci);
    app.press_bracket(false);
    assert_eq!(app.history_view, HistoryView::Notes, "on through ci");
    assert_eq!(app.ci_body, CiBody::Jobs, "no body stepped");
}

/// Without a run in the inspector, `]` cycles pane 2's views: there is
/// no body to step. Fails while `ci` traps the focused inspector.
#[test]
fn brackets_cycle_views_while_the_inspector_holds_no_run() {
    let mut app = app();
    app.focused_panel = FocusedPanel::Inspector;
    app.press_bracket(false);
    assert_eq!(app.history_view, HistoryView::Notes);
    assert_eq!(app.ci_body, CiBody::Jobs);
}

// --- The `info` section ---

/// Four branches: `latest_per_branch` picks one run for each of the
/// three newest.
fn four_branch_runs() -> Vec<WorkflowRun> {
    let base = runs()[1].clone();
    ["main", "main", "feat/a", "feat/b", "feat/c"]
        .iter()
        .enumerate()
        .map(|(i, branch)| WorkflowRun {
            id: i as u64 + 1,
            title: format!("run {i} on {branch}"),
            branch: Some(branch.to_string()),
            ..base.clone()
        })
        .collect()
}

fn info_text(app: &App) -> Option<Vec<String>> {
    info_rows(app, &app.repos[0], 120, Local::now())
        .map(|lines| lines.iter().map(line_text).collect())
}

/// Held runs on four branches: the default branch's line, then the
/// latest run of each of the three newest branches. Fails while `info`
/// has no `ci` section.
#[test]
fn the_info_section_shows_the_default_branch_and_three_branches() {
    let mut app = app();
    app.repos[0].ci_status = CiStatus::Passed;
    landed(&mut app, four_branch_runs(), Local::now());
    let lines = info_text(&app).expect("a section");
    assert_eq!(lines.len(), 1 + INFO_RUNS, "{lines:#?}");
    assert_eq!(lines[0], "  ◉ default branch passed");
    for (line, title) in
        lines[1..]
            .iter()
            .zip(["run 0 on main", "run 2 on feat/a", "run 3 on feat/b"])
    {
        assert!(line.contains(title), "{title} missing: {line}");
    }
    assert!(lines.iter().all(|l| !l.contains('▸')), "nothing selectable");
}

/// No runs held (the repo was never visited in `ci`): the line and the
/// dim hint. No GitHub identity: no section at all.
#[test]
fn the_info_section_hints_at_runs_and_needs_a_github_identity() {
    let mut app = app();
    app.history_view = HistoryView::Info;
    app.repos[0].ci_status = CiStatus::Failed;
    let lines = info_text(&app).expect("a section");
    assert_eq!(lines, vec!["  ◉ default branch failed", "  ] for runs"]);
    app.repos[0].ci_status = CiStatus::Unknown;
    assert_eq!(info_text(&app).unwrap()[0], "  no checks");

    app.repos[0].owner = None;
    assert_eq!(info_text(&app), None);
}

/// The section is drawn in `info` between `branches` and `recent
/// commits`, ahead of `worktrees`, and drawing it polls nothing.
#[test]
fn info_draws_the_ci_section_and_polls_nothing() {
    let mut app = app();
    app.history_view = HistoryView::Info;
    app.repos[0].branches = vec![crate::model::BranchStatus {
        name: "main".to_string(),
        upstream: None,
        behind: 0,
        ahead: 0,
        is_head: true,
        tip: "abc1234".to_string(),
    }];
    app.repos[0].worktrees = vec![crate::model::Worktree {
        path: "~/repos/fleetz/.claude/worktrees/feat".to_string(),
        branch: "feature".to_string(),
        detached: false,
        head: "abc1234".to_string(),
        dirty: Some(0),
        head_at: None,
    }];
    app.repos[0].ci_status = CiStatus::Passed;
    assert!(!app.ci_wanted());
    let rows = detail_rows(&app);
    let at = |label: &str| {
        rows.iter()
            .position(|r| r.trim_matches(|c| c == '│' || c == ' ') == label)
    };
    let branches = at("branches").expect("branches label");
    let ci = at("ci").expect("ci label");
    assert!(branches < ci, "{rows:#?}");
    let worktrees = at("worktrees").expect("worktrees label");
    assert!(ci < worktrees, "ci precedes worktrees: {rows:#?}");
    assert!(rows[ci + 1].contains("default branch passed"));
    assert!(rows[ci + 2].contains(RUNS_HINT));
    assert!(app.ci.is_empty(), "nothing claimed");
}

// --- The lamp, the header and the `ci` view agree (FZ-83) ---

const TIP: &str = "cc524b4717d724cfe74fdde29c5f5ae3853e5cdf";

fn main_run(status: &str, conclusion: Option<&str>) -> WorkflowRun {
    WorkflowRun {
        branch: Some("main".to_string()),
        head_sha: Some(TIP.to_string()),
        status: Some(status.to_string()),
        conclusion: conclusion.map(str::to_string),
        ..runs()[1].clone()
    }
}

fn pr_run_in_progress() -> WorkflowRun {
    WorkflowRun {
        id: 99,
        branch: Some("feat/x".to_string()),
        head_sha: Some("feed000".to_string()),
        status: Some("in_progress".to_string()),
        conclusion: None,
        ..runs()[1].clone()
    }
}

fn build(state: CiStatus) -> crate::ci::Context {
    crate::ci::Context {
        name: "build".to_string(),
        state,
        description: None,
        url: Some("https://github.com/gaurgle/fleetz/actions/runs/1/job/2".to_string()),
        actions: true,
        source: "actions".to_string(),
        started: None,
        completed: None,
    }
}

fn vercel() -> crate::ci::Context {
    crate::ci::Context {
        name: "Vercel".to_string(),
        state: CiStatus::Failed,
        description: Some("Aliasing has failed".to_string()),
        url: Some("https://vercel.com/gaurgles-projects/portfolio/DJX3".to_string()),
        actions: false,
        source: "vercel".to_string(),
        started: None,
        completed: None,
    }
}

/// portfolio-site's `cc524b4` Vercel history, as recorded 2026-11-05.
fn vercel_history() -> Vec<crate::ci::StatusEvent> {
    [
        ("pending", "2026-11-05T11:23:52Z"),
        ("success", "2026-11-05T11:24:09Z"),
        ("pending", "2026-11-05T11:24:27Z"),
        ("pending", "2026-11-05T11:52:18Z"),
        ("success", "2026-11-05T11:52:43Z"),
        ("failure", "2026-11-05T12:25:38Z"),
    ]
    .into_iter()
    .map(|(state, at)| crate::ci::StatusEvent {
        context: "Vercel".to_string(),
        state: match state {
            "pending" => CiStatus::Pending,
            "success" => CiStatus::Passed,
            _ => CiStatus::Failed,
        },
        description: None,
        at: DateTime::parse_from_rfc3339(at)
            .unwrap()
            .with_timezone(&Utc),
    })
    .collect()
}

/// The fleet poll left `status`, with its data time at `at`.
fn fleet(app: &mut App, status: CiStatus, at: DateTime<Local>) {
    app.repos[0].ci_status = status;
    app.github_freshness.data_time = DataTime::At(at);
}

/// Land one `ci` poll at `now` that read `runs` and the tip at `TIP`,
/// whose statuses and check runs are all listed.
fn tip_landed(
    app: &mut App,
    runs: Vec<WorkflowRun>,
    statuses: Vec<crate::ci::Context>,
    checks: Vec<crate::ci::Context>,
    now: DateTime<Local>,
) {
    tip_landed_with(app, runs, statuses, checks, Vec::new(), now);
}

/// `tip_landed`, with the statuses' `history` read too.
fn tip_landed_with(
    app: &mut App,
    runs: Vec<WorkflowRun>,
    statuses: Vec<crate::ci::Context>,
    checks: Vec<crate::ci::Context>,
    history: Vec<crate::ci::StatusEvent>,
    now: DateTime<Local>,
) {
    use crate::ci::{Fetched, TipAnswer, TipChecks, TipStatus};
    let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    let tip = TipAnswer {
        status: Fetched::Fresh {
            data: TipStatus {
                sha: TIP.to_string(),
                statuses,
                complete: true,
            },
            etag: Some("W/\"st\"".to_string()),
        },
        history: Some((
            TIP.to_string(),
            Fetched::Fresh {
                data: history,
                etag: Some("W/\"hi\"".to_string()),
            },
        )),
        checks: Some((
            TIP.to_string(),
            Fetched::Fresh {
                data: TipChecks {
                    runs: checks,
                    complete: true,
                },
                etag: Some("W/\"cr\"".to_string()),
            },
        )),
    };
    app.apply_ci(
        crate::ci::CiAnswer {
            key: key(),
            attempt: request.attempt,
            runs: Some(Fetched::Fresh {
                data: runs,
                etag: None,
            }),
            jobs: None,
            log: None,
            full_log: None,
            failure: None,
            budget: None,
            tip: Some(tip),
        },
        now,
    );
}

fn t0() -> DateTime<Local> {
    instant("2026-11-05T12:30:00+01:00")
}

fn minutes(n: i64) -> chrono::Duration {
    chrono::Duration::minutes(n)
}

/// Pane 2's header CI word as drawn: from `CI` to the next section
/// divider, or to the pane's border where none follows.
fn header_ci(app: &App) -> Option<String> {
    detail_rows(app).iter().find_map(|row| {
        let at = row.find("CI ◉")?;
        let word = row[at..].split(" │ ").next().unwrap_or_default();
        Some(word.trim_end_matches(['│', ' ']).to_string())
    })
}

/// The repo list's rows as drawn, joined.
fn list_text(buffer: &Buffer, rect: Rect) -> String {
    (rect.y..rect.y + rect.height)
        .map(|y| {
            (rect.x..rect.x + rect.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The colour of the first `◉` inside `rect`.
fn lamp_color(buffer: &Buffer, rect: Rect) -> Option<ratatui::style::Color> {
    (rect.y..rect.y + rect.height).find_map(|y| {
        (rect.x..rect.x + rect.width)
            .find(|&x| buffer[(x, y)].symbol() == "◉")
            .map(|x| buffer[(x, y)].fg)
    })
}

/// The header lamp's colour: the `◉` after `CI ` in pane 2.
fn header_lamp_color(buffer: &Buffer, rect: Rect) -> Option<ratatui::style::Color> {
    (rect.y..rect.y + rect.height).find_map(|y| {
        (rect.x + 3..rect.x + rect.width).find_map(|x| {
            let ci = (0..3)
                .map(|d| buffer[(x - 3 + d, y)].symbol())
                .collect::<String>();
            (ci == "CI " && buffer[(x, y)].symbol() == "◉").then(|| buffer[(x, y)].fg)
        })
    })
}

/// Case 2 (11-05 12:30): the fleet poll still says pending, the tip's
/// check runs read after it say green. Lamp, header and `info` show
/// passed, and the header names the branch. Fails while all three draw
/// `ci_status`.
#[test]
fn a_fresher_tip_overrides_the_fleet_polls_stale_pending() {
    let mut app = app();
    fleet(&mut app, CiStatus::Pending, t0());
    tip_landed(
        &mut app,
        vec![main_run("completed", Some("success"))],
        vec![],
        vec![build(CiStatus::Passed)],
        t0() + minutes(1),
    );
    let shown = app.ci_shown(&app.repos[0]);
    assert_eq!(shown.status, CiStatus::Passed);
    assert_eq!(shown.branch.as_deref(), Some("main"));
    assert_eq!(header_ci(&app).as_deref(), Some("CI ◉ main passed"));
    let (rects, buffer) = draw(&app, 160, 50);
    assert_eq!(lamp_color(&buffer, rects.repo_list), Some(colors::GREEN));
    app.history_view = HistoryView::Info;
    assert_eq!(info_text(&app).unwrap()[0], "  ◉ default branch passed");
}

/// Contexts read before the fleet poll's data time are older than it:
/// the fleet's state wins, and no culprit is guessed from them.
#[test]
fn an_older_tip_leaves_the_fleet_polls_state() {
    let mut app = app();
    tip_landed(
        &mut app,
        vec![main_run("completed", Some("success"))],
        vec![],
        vec![build(CiStatus::Passed)],
        t0() - minutes(1),
    );
    fleet(&mut app, CiStatus::Failed, t0());
    let shown = app.ci_shown(&app.repos[0]);
    assert_eq!((shown.status, shown.culprit), (CiStatus::Failed, None));
    assert_eq!(header_ci(&app).as_deref(), Some("CI ◉ main failed"));
}

/// Nothing held for the repo: the fleet's state, and no branch name to
/// give, so the header keeps its old words.
#[test]
fn with_nothing_held_the_fleet_state_shows_unnamed() {
    let mut app = app();
    fleet(&mut app, CiStatus::Passed, t0());
    let shown = app.ci_shown(&app.repos[0]);
    assert_eq!(
        shown,
        crate::app::CiShown {
            status: CiStatus::Passed,
            branch: None,
            culprit: None,
        }
    );
    assert_eq!(header_ci(&app).as_deref(), Some("CI ◉ passed"));
}

/// Only the status answered for this tip (its check runs did not), or
/// the check runs ran past a page: not every context is known, so the
/// fleet's state stands.
#[test]
fn a_tip_without_its_check_runs_does_not_override() {
    use crate::ci::{Fetched, TipAnswer, TipStatus};
    let mut app = app();
    fleet(&mut app, CiStatus::Pending, t0());
    let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    let tip = TipAnswer {
        status: Fetched::Fresh {
            data: TipStatus {
                sha: TIP.to_string(),
                statuses: vec![],
                complete: true,
            },
            etag: None,
        },
        history: None,
        checks: None,
    };
    app.apply_ci(
        crate::ci::CiAnswer {
            key: key(),
            attempt: request.attempt,
            runs: Some(Fetched::Fresh {
                data: vec![main_run("completed", Some("success"))],
                etag: None,
            }),
            jobs: None,
            log: None,
            full_log: None,
            failure: Some(crate::github::Failure::new(
                crate::github::FailureKind::Offline,
                "offline",
            )),
            budget: None,
            tip: Some(tip),
        },
        t0() + minutes(1),
    );
    assert_eq!(app.ci_shown(&app.repos[0]).status, CiStatus::Pending);

    // Both answered, but the check runs ran past one page: still not
    // every context.
    let mut paged = self::app();
    fleet(&mut paged, CiStatus::Pending, t0());
    tip_landed(
        &mut paged,
        vec![main_run("completed", Some("success"))],
        vec![],
        vec![build(CiStatus::Passed)],
        t0() + minutes(1),
    );
    let tip = paged.ci.get_mut(&key()).unwrap().tip.as_mut().unwrap();
    tip.checks.as_mut().unwrap().complete = false;
    assert_eq!(paged.ci_shown(&paged.repos[0]).status, CiStatus::Pending);
}

/// A new tip forgets the old commit's check runs: until its own answer,
/// the fleet's state stands again.
#[test]
fn a_moved_tip_forgets_the_old_commits_check_runs() {
    use crate::ci::{Fetched, TipAnswer, TipStatus};
    let mut app = app();
    fleet(&mut app, CiStatus::Pending, t0());
    tip_landed(
        &mut app,
        vec![main_run("completed", Some("success"))],
        vec![],
        vec![build(CiStatus::Passed)],
        t0() + minutes(1),
    );
    assert_eq!(app.ci_shown(&app.repos[0]).status, CiStatus::Passed);
    let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    app.apply_ci(
        crate::ci::CiAnswer {
            key: key(),
            attempt: request.attempt,
            runs: Some(Fetched::NotModified),
            jobs: None,
            log: None,
            full_log: None,
            failure: Some(crate::github::Failure::new(
                crate::github::FailureKind::Offline,
                "offline",
            )),
            budget: None,
            tip: Some(TipAnswer {
                status: Fetched::Fresh {
                    data: TipStatus {
                        sha: "a1b2c3d".to_string(),
                        statuses: vec![],
                        complete: true,
                    },
                    etag: None,
                },
                history: None,
                checks: None,
            }),
        },
        t0() + minutes(2),
    );
    let tip = app.ci[&key()].tip.clone().unwrap();
    assert_eq!((tip.sha.as_str(), tip.checks), ("a1b2c3d", None));
    assert_eq!(app.ci_shown(&app.repos[0]).status, CiStatus::Pending);
}

/// Case 1 (11-04 18:33): a run in progress on a PR branch leaves the
/// default branch's state as it is, and adds the side-run mark beside
/// the lamp instead.
#[test]
fn a_pr_branch_run_in_progress_leaves_the_default_branchs_state() {
    let mut app = app();
    fleet(&mut app, CiStatus::Passed, t0());
    tip_landed(
        &mut app,
        vec![pr_run_in_progress(), main_run("completed", Some("success"))],
        vec![],
        vec![build(CiStatus::Passed)],
        t0() + minutes(1),
    );
    assert_eq!(app.ci_shown(&app.repos[0]).status, CiStatus::Passed);
    assert_eq!(header_ci(&app).as_deref(), Some("CI ◉ main passed"));
    assert!(app.ci_side_run(&app.repos[0]));
    let (rects, buffer) = draw(&app, 160, 50);
    assert_eq!(lamp_color(&buffer, rects.repo_list), Some(colors::GREEN));
    let list = list_text(&buffer, rects.repo_list);
    assert!(list.contains("◉ ◦ "), "{list}");
}

/// The side-run mark is for another branch only: not for a run on the
/// tip (the lamp covers that), not with no runs held, not while the
/// tip is unknown, and not once the fleet poll is newer than the runs.
#[test]
fn the_side_run_mark_needs_a_fresh_run_off_the_tip() {
    let mut on_tip = app();
    fleet(&mut on_tip, CiStatus::Passed, t0());
    tip_landed(
        &mut on_tip,
        vec![main_run("in_progress", None)],
        vec![],
        vec![build(CiStatus::Pending)],
        t0() + minutes(1),
    );
    assert!(!on_tip.ci_side_run(&on_tip.repos[0]), "the lamp says it");
    assert_eq!(on_tip.ci_shown(&on_tip.repos[0]).status, CiStatus::Pending);

    let nothing = app();
    assert!(!nothing.ci_side_run(&nothing.repos[0]), "no runs held");

    let mut no_tip = app();
    fleet(&mut no_tip, CiStatus::Passed, t0());
    landed(&mut no_tip, vec![pr_run_in_progress()], t0() + minutes(1));
    assert!(!no_tip.ci_side_run(&no_tip.repos[0]), "tip unknown");

    let mut stale = app();
    tip_landed(
        &mut stale,
        vec![pr_run_in_progress()],
        vec![],
        vec![build(CiStatus::Passed)],
        t0() - minutes(1),
    );
    fleet(&mut stale, CiStatus::Passed, t0());
    assert!(!stale.ci_side_run(&stale.repos[0]), "older than the fleet");
    let (rects, buffer) = draw(&stale, 160, 50);
    let list = list_text(&buffer, rects.repo_list);
    assert!(list.contains('◉'), "the row is drawn: {list}");
    assert!(!list.contains('◦'), "{list}");
}

/// The side-run mark is one cell and a space, counted in the row's
/// width, so the row still ends at its edge, under either icon set.
#[test]
fn the_side_run_mark_is_counted_in_the_rows_width() {
    use crate::app::Icons;
    let repo = repo();
    for icons in [Icons::Nerd, Icons::Plain] {
        badges::set_active_icons(icons);
        for side_run in [false, true] {
            let ci = badges::RowCi {
                status: &CiStatus::Passed,
                side_run,
            };
            let line = Line::from(crate::ui::detail::repo_row_spans_ci(
                &repo, ci, false, 60, false, false, None,
            ));
            assert_eq!(line.width(), 60, "{icons:?} side_run {side_run}");
            let text = line_text(&line);
            assert_eq!(text.contains("◉ ◦ "), side_run, "{text}");
        }
    }
    badges::set_active_icons(Icons::Nerd);
}

fn at_utc(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

/// portfolio-site on 11-05: a PR run started after Vercel's last state,
/// and `main`'s green run before it.
fn portfolio_runs() -> Vec<WorkflowRun> {
    let newer = WorkflowRun {
        created: at_utc("2026-11-05T13:00:00Z"),
        started: Some(at_utc("2026-11-05T13:00:00Z")),
        updated: at_utc("2026-11-05T13:01:00Z"),
        status: Some("completed".to_string()),
        conclusion: Some("success".to_string()),
        ..pr_run_in_progress()
    };
    let older = WorkflowRun {
        created: at_utc("2026-11-05T11:52:00Z"),
        started: Some(at_utc("2026-11-05T11:52:00Z")),
        updated: at_utc("2026-11-05T11:53:00Z"),
        ..main_run("completed", Some("success"))
    };
    vec![newer, older]
}

/// portfolio-site's `ci` view: its runs, the green Actions check and
/// the Vercel status with its recorded history.
fn portfolio_app() -> App {
    let mut app = app();
    fleet(&mut app, CiStatus::Failed, t0());
    tip_landed_with(
        &mut app,
        portfolio_runs(),
        vec![vercel()],
        vec![build(CiStatus::Passed)],
        vercel_history(),
        t0() + minutes(1),
    );
    app
}

/// The amendment's case 3 (portfolio-site, 11-05 13:36): the rollup is
/// red from a Vercel status while the Actions run is green. The header
/// names it, and the Vercel status is a row of the run list sorted by
/// time among the runs (its age is its final state's, 12:25:38, between
/// the 13:00 and the 11:52 runs): workflow `Vercel`, source `vercel`,
/// branch `main`, trigger `status`, its description as the title, the
/// latest job's duration (`ci::status_run`'s rule: 11:52:18 to
/// 11:52:43, 25s) and a red lamp. The Actions check is not listed twice.
#[test]
fn a_failing_status_context_is_named_in_the_header_and_listed() {
    let app = portfolio_app();
    let shown = app.ci_shown(&app.repos[0]);
    assert_eq!(shown.culprit.as_deref(), Some("Vercel"));
    assert_eq!(header_ci(&app).as_deref(), Some("CI ◉ main failed: Vercel"));
    let now = instant("2026-11-05T14:41:00+01:00");
    let rows: Vec<String> = run_rows(&app, 160, now, 1, false)
        .iter()
        .map(line_text)
        .collect();
    assert_eq!(rows.len(), 3, "{rows:#?}");
    assert!(rows[0].contains("feat/x"), "{}", rows[0]);
    assert!(
        rows[2].contains("main") && !rows[2].contains("Vercel"),
        "{}",
        rows[2]
    );
    let vercel_row = &rows[1];
    let words: Vec<&str> = vercel_row.split_whitespace().collect();
    assert_eq!(
        &words[..5],
        ["▸", "◉", "Vercel", "vercel", "main"],
        "{vercel_row}"
    );
    for cell in ["status", "Aliasing has failed", " 25s ", "1h ago"] {
        assert!(vercel_row.contains(cell), "{cell} missing: {vercel_row}");
    }
    let line = &run_rows(&app, 160, now, 1, false)[1];
    assert_eq!(line.spans[1].style.fg, Some(colors::RED), "the lamp");
    assert!(
        !rows.iter().any(|r| r.contains("build")),
        "the Actions check is the run list's own"
    );
}

/// Every context is an Actions check run: the run list says it all, so
/// no row is added.
#[test]
fn a_tip_of_actions_check_runs_adds_no_row() {
    let mut app = app();
    fleet(&mut app, CiStatus::Failed, t0());
    tip_landed(
        &mut app,
        vec![main_run("completed", Some("failure"))],
        vec![],
        vec![build(CiStatus::Failed)],
        t0() + minutes(1),
    );
    assert_eq!(app.ci_entries().len(), 1);
    assert_eq!(header_ci(&app).as_deref(), Some("CI ◉ main failed: build"));
    assert_eq!(app.selected_commit_count(), 1, "one run, no context stop");
}

/// The cursor walks the list as drawn: on the Vercel row, no run is
/// highlighted; on either side of it, the runs.
#[test]
fn the_cursor_walks_the_contexts_among_the_runs() {
    let mut app = portfolio_app();
    let held = portfolio_runs();
    assert_eq!(app.selected_commit_count(), 3);
    app.detail_selections[HistoryView::Ci.index()] = 1;
    assert_eq!(
        app.highlighted_ci_context().map(|c| c.name),
        Some("Vercel".to_string())
    );
    assert_eq!(app.highlighted_ci_run(), None);
    assert_eq!(app.ci_highlighted_run(), None);
    app.detail_selections[HistoryView::Ci.index()] = 0;
    assert_eq!(app.highlighted_ci_run(), Some(&held[0]));
    app.detail_selections[HistoryView::Ci.index()] = 2;
    assert_eq!(app.highlighted_ci_context(), None);
    assert_eq!(app.highlighted_ci_run(), Some(&held[1]));
    assert_eq!(app.ci_highlighted_run(), Some(&held[1]));
}

/// On a context's row the inspector has nothing to detail and says why.
#[test]
fn the_inspector_says_a_context_has_no_jobs_or_log() {
    let mut app = portfolio_app();
    app.detail_selections[HistoryView::Ci.index()] = 1;
    let body = inspector_body(&app, Local::now());
    assert_eq!(
        body.iter().map(line_text).collect::<Vec<_>>(),
        ["no jobs or log for vercel"]
    );
}

/// Every row names its source in that source's colour: `actions` in the
/// `ci` tab's maroon, `vercel` in its own; each known source has a
/// colour of its own and an unknown one is dim.
#[test]
fn the_source_column_names_and_colours_each_source() {
    let app = portfolio_app();
    let now = instant("2026-11-05T14:41:00+01:00");
    let rows = run_rows(&app, 160, now, 9, false);
    let source_span = |line: &Line, word: &str| {
        line.spans
            .iter()
            .find(|s| s.content.trim() == word)
            .and_then(|s| s.style.fg)
    };
    assert_eq!(source_span(&rows[0], "actions"), Some(colors::MAROON));
    assert_eq!(source_span(&rows[1], "vercel"), Some(colors::LAVENDER));
    let known: Vec<_> = std::iter::once("actions")
        .chain(crate::ci::KNOWN_SOURCES)
        .map(source_color)
        .collect();
    for (i, a) in known.iter().enumerate() {
        assert_ne!(*a, colors::OVERLAY, "a known source is not dim");
        for b in &known[i + 1..] {
            assert_ne!(a, b, "two sources share a colour");
        }
    }
    assert_eq!(source_color("jenkins"), colors::OVERLAY);
    let head = line_text(&headline(&app, 160, now).expect("a headline"));
    assert_eq!(
        head.split_whitespace().collect::<Vec<_>>(),
        [
            "workflow", "source", "branch", "trigger", "title", "duration", "age"
        ],
        "{head}"
    );
    // A list of Actions runs alone names no source: every row would say
    // `actions`.
    let mut actions_only = self::app();
    landed(&mut actions_only, runs(), now);
    let head = line_text(&headline(&actions_only, 160, now).unwrap());
    assert!(!head.contains("source"), "{head}");
    let row = line_text(&run_rows(&actions_only, 160, now, 9, false)[0]);
    assert!(!row.contains("actions"), "{row}");
}

/// On a narrowing pane the trigger goes first, then the source, then
/// the title, by `Widths::fit`'s order.
#[test]
fn the_source_sheds_after_the_trigger_and_before_the_title() {
    let app = portfolio_app();
    let now = instant("2026-11-05T14:41:00+01:00");
    let gone_below = |word: &str| {
        (23..=200)
            .rev()
            .find(|&w| {
                let head = line_text(&headline(&app, w, now).unwrap());
                !head.split_whitespace().any(|h| h == word)
            })
            .unwrap_or_else(|| panic!("{word} never sheds"))
    };
    let (trigger, source, title) = (
        gone_below("trigger"),
        gone_below("source"),
        gone_below("title"),
    );
    assert!(trigger > source, "trigger {trigger}, source {source}");
    assert!(source > title, "source {source}, title {title}");
    for w in [trigger, source, title] {
        for row in run_rows(&app, w, now, 9, false) {
            assert!(line_text(&row).chars().count() <= w, "{w}");
        }
    }
}

/// The next poll asks the tip again with what is held, so every read
/// can answer 304, and pays for them in its cost: 3 calls without
/// commit statuses, 4 with their history, one more with a run expanded
/// (1 and 2 before FZ-83).
#[test]
fn the_next_poll_asks_the_tip_conditionally_and_counts_it() {
    let mut with_statuses = portfolio_app();
    with_statuses.ci.get_mut(&key()).unwrap().expanded = None;
    assert_eq!(with_statuses.ci[&key()].poll_cost(), 4);
    with_statuses.ci.get_mut(&key()).unwrap().expanded = Some(1);
    assert_eq!(with_statuses.ci[&key()].poll_cost(), 5);
    let request = with_statuses.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    let tip = request.tip.expect("the tip is asked");
    assert!(tip.has_statuses);
    assert_eq!(tip.history_etag.as_deref(), Some("W/\"hi\""));

    let mut app = app();
    tip_landed(
        &mut app,
        vec![main_run("completed", Some("success"))],
        vec![],
        vec![build(CiStatus::Passed)],
        t0(),
    );
    app.ci.get_mut(&key()).unwrap().expanded = None;
    assert_eq!(app.ci[&key()].poll_cost(), 3);
    app.ci.get_mut(&key()).unwrap().expanded = Some(1);
    assert_eq!(app.ci[&key()].poll_cost(), 4);
    let request = app.claim_ci_poll(key(), "gaurgle".into(), "fleetz".into());
    assert_eq!(
        request.tip,
        Some(crate::ci::TipRequest {
            sha: Some(TIP.to_string()),
            status_etag: Some("W/\"st\"".to_string()),
            checks_etag: Some("W/\"cr\"".to_string()),
            has_statuses: false,
            history_etag: Some("W/\"hi\"".to_string()),
        })
    );
}

/// One function: for every fleet state, tip age and tip verdict, the
/// row lamp and the header lamp wear the same colour, the header the
/// same word, and both what `ci_shown` says.
#[test]
fn the_row_lamp_and_the_header_agree_in_every_case() {
    for fleet_status in [CiStatus::Passed, CiStatus::Failed, CiStatus::Pending] {
        for age in [None, Some(-1), Some(1)] {
            for tip_green in [true, false] {
                let mut app = app();
                if let Some(offset) = age {
                    let (statuses, check) = match tip_green {
                        true => (vec![], build(CiStatus::Passed)),
                        false => (vec![vercel()], build(CiStatus::Passed)),
                    };
                    tip_landed(
                        &mut app,
                        vec![main_run("completed", Some("success"))],
                        statuses,
                        vec![check],
                        t0() + minutes(offset),
                    );
                }
                fleet(&mut app, fleet_status.clone(), t0());
                let shown = app.ci_shown(&app.repos[0]);
                let expected = match (age, tip_green) {
                    (Some(1), true) => CiStatus::Passed,
                    (Some(1), false) => CiStatus::Failed,
                    _ => fleet_status.clone(),
                };
                let case = format!("{fleet_status:?} {age:?} {tip_green}");
                assert_eq!(shown.status, expected, "{case}");
                let (rects, buffer) = draw(&app, 160, 50);
                let detail = rects.detail.expect("detail");
                let (_, color, word) = badges::ci_display(&shown.status, false);
                assert_eq!(lamp_color(&buffer, rects.repo_list), Some(color), "{case}");
                assert_eq!(header_lamp_color(&buffer, detail), Some(color), "{case}");
                let header = header_ci(&app).unwrap();
                assert!(header.contains(word), "{case}: {header}");
            }
        }
    }
}
