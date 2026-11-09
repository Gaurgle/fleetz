use super::*;
use crate::digest::tests::repo;

const W: u16 = 120;
const H: u16 = 44;

/// Two github activity rows, one local changes row, two also active.
fn sectioned_app() -> App {
    let repos = vec![
        repo("alpha", RepoLocation::Primary),
        repo("bravo", RepoLocation::Primary),
        repo("charlie", RepoLocation::LocalOnly),
        repo("delta", RepoLocation::Elsewhere),
        repo("echo", RepoLocation::NotCloned),
    ];
    let mut app = App::new(repos, String::new(), W, H);
    app.view = app::View::Detail;
    app.focused_panel = FocusedPanel::RepoList;
    let pushed = chrono::Utc::now() - chrono::Duration::minutes(1);
    app.dataset = ["alpha", "bravo", "delta", "echo"]
        .iter()
        .map(|name| github::GithubRepo {
            name: name.to_string(),
            pushed_at: pushed,
            pr_count: 0,
            prs: vec![],
            ci_status: model::CiStatus::Unknown,
            ci_workflow: None,
            ci_duration: None,
            repo_kind: model::RepoKind::Unknown,
            owner: None,
            disk_usage: None,
        })
        .collect();
    app
}

fn screen(app: &App) -> Vec<String> {
    use ratatui::backend::TestBackend;
    let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
    terminal.draw(|f| ui::render(f, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..H)
        .map(|y| (0..W).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// The pane 1 row the renderer marks as the cursor's, trimmed.
fn marked_row(app: &App) -> (usize, String) {
    let rows = screen(app);
    let list = app.list_hits.get().area;
    let found: Vec<(usize, String)> = rows
        .iter()
        .enumerate()
        .filter_map(|(y, row)| {
            let cells: String = row
                .chars()
                .skip(list.x as usize)
                .take(list.width as usize)
                .collect();
            cells
                .trim_start()
                .starts_with('▸')
                .then(|| (y, cells.trim().to_string()))
        })
        .collect();
    assert_eq!(found.len(), 1, "one marked row: {found:?}");
    found.into_iter().next().unwrap()
}

async fn press_key(app: &mut App, code: KeyCode) {
    let args = Args::parse_from(["fleetz"]);
    let config = config::Config::default();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = CrosstermBackend::new(io::stdout());
    let viewport = ratatui::Viewport::Fixed(ratatui::layout::Rect::new(0, 0, W, H));
    let mut terminal =
        Terminal::with_options(backend, ratatui::TerminalOptions { viewport }).unwrap();
    handle_normal_input(
        app,
        &mut terminal,
        KeyEvent::new(code, KeyModifiers::NONE),
        &tx,
        &args,
        &config,
        Path::new("/nonexistent"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn j_and_k_stop_on_every_header_in_the_order_the_list_draws() {
    let mut app = sectioned_app();
    press_key(&mut app, KeyCode::Char('k')).await;
    assert_eq!(
        app.on_header(),
        Some(digest::Section::GithubActivity),
        "k from the first repo lands on its header"
    );
    let mut walked = vec![marked_row(&app)];
    for _ in 0..8 {
        press_key(&mut app, KeyCode::Char('j')).await;
        walked.push(marked_row(&app));
    }
    let ys: Vec<usize> = walked.iter().map(|(y, _)| *y).collect();
    let texts: Vec<&str> = walked.iter().map(|(_, t)| t.as_str()).collect();
    assert!(
        ys.windows(2).take(7).all(|w| w[0] < w[1]),
        "the cursor walks down the drawn rows: {walked:?}"
    );
    assert_eq!(ys[7], ys[8], "j stops at the last row");
    let expected = [
        "github activity",
        "alpha",
        "bravo",
        "local changes",
        "charlie",
        "also active",
        "delta",
        "echo",
    ];
    for (text, want) in texts.iter().zip(expected) {
        assert!(text.contains(want), "{text:?} is not {want:?}: {texts:?}");
    }
    // And back up through the same stops.
    for want in expected.iter().rev().skip(1) {
        press_key(&mut app, KeyCode::Char('k')).await;
        assert!(marked_row(&app).1.contains(want), "k back to {want}");
    }
}

#[tokio::test]
async fn the_compact_grid_has_no_header_stops() {
    let mut app = sectioned_app();
    app.view = app::View::Compact;
    app.selected = 2;
    press_key(&mut app, KeyCode::Char('k')).await;
    assert_eq!(app.selected, 1, "k skips the local changes header");
    assert_eq!(app.on_header(), None);
}

#[tokio::test]
async fn on_a_header_pane_2_draws_the_digest_and_leaving_restores_its_view() {
    let mut app = sectioned_app();
    app.history_view = app::HistoryView::Commits;
    let before = screen(&app).join("\n");
    assert!(before.contains("COMMITS"), "{before}");

    press_key(&mut app, KeyCode::Char('k')).await;
    let on_header = screen(&app).join("\n");
    assert!(on_header.contains("2 repos · 0 commits"), "{on_header}");
    assert!(on_header.contains("pushed 1m ago"), "{on_header}");
    assert!(!on_header.contains("COMMITS"), "{on_header}");

    press_key(&mut app, KeyCode::Char('j')).await;
    assert_eq!(app.history_view, app::HistoryView::Commits);
    assert_eq!(app.selected, 0);
    let after = screen(&app).join("\n");
    assert!(after.contains("COMMITS"), "{after}");
    assert!(!after.contains("2 repos · 0 commits"), "{after}");
}

#[tokio::test]
async fn t_cycles_the_digest_window_and_leaves_the_overlay_shut() {
    let mut app = sectioned_app();
    press_key(&mut app, KeyCode::Char('k')).await;
    let mut seen = vec![app.digest_window];
    for _ in 0..4 {
        press_key(&mut app, KeyCode::Char('t')).await;
        assert!(matches!(app.overlay, Overlay::None), "t opened the overlay");
        seen.push(app.digest_window);
    }
    use digest::DigestWindow::*;
    assert_eq!(seen, vec![Slot, Today, Yesterday, Week, Slot]);
    let drawn = screen(&app).join("\n");
    assert!(drawn.contains("slot · today · yesterday · week"), "{drawn}");

    // Off the header `t` is the time window overlay again.
    press_key(&mut app, KeyCode::Char('j')).await;
    press_key(&mut app, KeyCode::Char('t')).await;
    assert!(matches!(app.overlay, Overlay::TimeWindow(_)));
}

#[tokio::test]
async fn enter_on_a_digest_line_lands_on_its_repo_row() {
    let mut app = sectioned_app();
    app.selected = 3;
    press_key(&mut app, KeyCode::Char('k')).await;
    assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
    app.focused_panel = FocusedPanel::Detail;
    press_key(&mut app, KeyCode::Char('j')).await;
    assert_eq!(app.digest_selected, 1, "j walks the digest in pane 2");
    assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
    press_key(&mut app, KeyCode::Enter).await;
    assert_eq!(app.on_header(), None);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("echo"));
    assert_eq!(app.focused_panel, FocusedPanel::RepoList);
}

#[test]
fn a_click_or_a_hover_on_a_header_lands_on_it() {
    let mut app = sectioned_app();
    let rows = screen(&app);
    let list = app.list_hits.get().area;
    let y_of = |label: &str| {
        rows.iter()
            .position(|r| r.contains(label))
            .unwrap_or_else(|| panic!("{label} not drawn")) as u16
    };
    let x = list.x + 2;
    app.click_list(x, y_of("local changes"));
    assert_eq!(app.on_header(), Some(digest::Section::LocalChanges));
    app.click_list(x, y_of("delta"));
    assert_eq!(app.on_header(), None);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("delta"));
    // A rested hover lands on a header and on nothing else.
    screen(&app);
    let t0 = std::time::Instant::now();
    app.hover_list(x, y_of("bravo"), t0);
    app.settle_hover(t0 + app::HOVER_DWELL);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("delta"));
    app.hover_list(x, y_of("also active"), t0);
    app.settle_hover(t0 + app::HOVER_DWELL);
    assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
    // The blank row between sections is not a stop.
    let gap = y_of("also active") - 1;
    app.click_list(x, gap);
    assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
}

/// Review F6: a pointer passing over a header lands nowhere; one resting
/// on it for `HOVER_DWELL` lands there.
#[test]
fn a_hover_lands_on_a_header_only_after_the_pointer_rests() {
    let mut app = sectioned_app();
    let rows = screen(&app);
    let x = app.list_hits.get().area.x + 2;
    let y_of = |label: &str| rows.iter().position(|r| r.contains(label)).unwrap() as u16;
    let ms = std::time::Duration::from_millis;
    let t0 = std::time::Instant::now();
    app.hover_list(x, y_of("also active"), t0);
    app.settle_hover(t0 + ms(100));
    assert_eq!(app.on_header(), None, "not rested yet");
    app.hover_list(x, y_of("delta"), t0 + ms(150));
    app.settle_hover(t0 + ms(1000));
    assert_eq!(app.on_header(), None, "passed over, not rested on");
    assert_eq!(app.selected, 0);

    let t1 = t0 + ms(2000);
    app.hover_list(x, y_of("also active"), t1);
    app.hover_list(x, y_of("also active"), t1 + ms(200));
    app.settle_hover(t1 + app::HOVER_DWELL - ms(1));
    assert_eq!(
        app.on_header(),
        None,
        "the dwell runs from the first arrival"
    );
    app.settle_hover(t1 + app::HOVER_DWELL);
    assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));

    // Leaving the list cancels a pending hover.
    app.hover_list(x, y_of("local changes"), t1);
    app.clear_hover();
    app.settle_hover(t1 + ms(5000));
    assert_eq!(app.on_header(), Some(digest::Section::AlsoActive));
}

/// Review B1: the compact grid never writes `list_hits`, so a click
/// there after a detail frame must not resolve against that frame.
#[test]
fn a_click_in_compact_after_a_detail_frame_leaves_the_selection() {
    let mut app = sectioned_app();
    let rows = screen(&app);
    let x = app.list_hits.get().area.x + 2;
    let y = rows.iter().position(|r| r.contains("echo")).unwrap() as u16;
    app.toggle_view();
    assert_eq!(app.view, app::View::Compact);
    app.click_list(x, y);
    let t0 = std::time::Instant::now();
    app.hover_list(x, y - 3, t0);
    app.settle_hover(t0 + app::HOVER_DWELL);
    assert_eq!(app.selected, 0);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("alpha"));
}

/// Review F5: on a header `J`/`K` drive the digest's pane 2 cursor and
/// the digest inspector, never the hidden repo's detail or `ci` bodies.
#[tokio::test]
async fn shift_j_on_a_header_drives_the_digest_panes() {
    let mut app = sectioned_app();
    app.history_view = app::HistoryView::Ci;
    press_key(&mut app, KeyCode::Char('k')).await;
    assert!(app.on_header().is_some());
    app.detail_max_scroll.set(10);
    app.inspector_max_scroll.set(10);
    press_key(&mut app, KeyCode::Char('J')).await;
    assert_eq!(app.digest_selected, 1, "J from the list walks the digest");
    assert_eq!(app.detail_scroll(), 0, "the hidden view did not scroll");
    assert_eq!(app.detail_selected(), 0);

    app.focused_panel = FocusedPanel::Detail;
    press_key(&mut app, KeyCode::Char('J')).await;
    assert_eq!(
        app.inspector_scroll, 1,
        "J from pane 2 scrolls the digest inspector"
    );
    assert_eq!(app.ci_body_scrolls, [0; 3], "not the hidden `ci` bodies");
    press_key(&mut app, KeyCode::Char('K')).await;
    assert_eq!(app.inspector_scroll, 0);
    assert_eq!(app.history_view, app::HistoryView::Ci);
}

/// Review F7: `g` puts `selected` back on 0, which is the github
/// activity section's first repo in the group too; the header the cursor
/// stood on must not come back with it.
#[tokio::test]
async fn a_group_change_does_not_bring_the_header_back() {
    let mut app = sectioned_app();
    app.group_names = vec!["work".to_string()];
    app.repos[0].groups = vec!["work".to_string()];
    app.repos[2].groups = vec!["work".to_string()];
    press_key(&mut app, KeyCode::Char('k')).await;
    assert_eq!(app.on_header(), Some(digest::Section::GithubActivity));
    press_key(&mut app, KeyCode::Char('g')).await;
    assert_eq!(app.active_group, Some(0));
    assert!(
        app.list_rows()
            .contains(&app::ListRow::Header(digest::Section::GithubActivity))
    );
    assert_eq!(app.selected, 0);
    assert_eq!(app.on_header(), None);
    assert_eq!(app.selected_repo().map(|r| r.name.as_str()), Some("alpha"));
}

/// FZ-60: `u` reaches the base toggle from the key handler, in
/// `changes` only.
#[tokio::test]
async fn u_toggles_the_changes_base_in_its_view_only() {
    let mut app = sectioned_app();
    app.view = app::View::Detail;
    app.history_view = app::HistoryView::Info;
    press_key(&mut app, KeyCode::Char('u')).await;
    assert_eq!(app.changes_base, git::ChangesBase::Upstream);
    app.history_view = app::HistoryView::Changes;
    press_key(&mut app, KeyCode::Char('u')).await;
    assert_eq!(app.changes_base, git::ChangesBase::DefaultBranch);
}
