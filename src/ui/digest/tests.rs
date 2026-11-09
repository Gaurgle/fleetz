use super::*;
use crate::digest::tests::commit;

fn text(spans: &[Span<'_>]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

fn line(name: &str, body: LineBody) -> DigestLine {
    DigestLine {
        repo: 0,
        name: name.to_string(),
        body,
        ci: CiStatus::Unknown,
        prs: 0,
        pushed_at: None,
    }
}

/// A fixture repo carrying the line's GitHub fields, as the row would.
fn repo_for(l: &DigestLine) -> crate::model::RepoStatus {
    let mut r = crate::digest::tests::repo(&l.name, crate::model::RepoLocation::Primary);
    r.ci_status = l.ci.clone();
    r.pr_count = l.prs;
    r
}

fn columns(lines: &[DigestLine], width: usize) -> Columns {
    Columns::of(&digest(Section::GithubActivity, lines.to_vec()), width)
}

/// The pane 1 row the digest line opens with, at the digest's width.
fn pane1_row(r: &crate::model::RepoStatus, selected: bool, cols: Columns) -> String {
    text(&crate::ui::detail::repo_row_spans(
        r, selected, cols.row, false, false, None,
    ))
}

fn counted(name: &str, commits: Vec<crate::model::Commit>) -> DigestLine {
    line(name, LineBody::Counted { commits })
}

#[test]
fn a_counted_line_is_the_pane_1_row_then_the_count_and_newest_subject() {
    let mut l = counted(
        "fleetz",
        vec![
            commit(
                "a1",
                "Andreas Roos",
                "feat(ci): the ci view heads its columns",
            ),
            commit("b2", "Andreas Roos", "older"),
            commit("c3", "Andreas Roos", "oldest"),
        ],
    );
    l.ci = CiStatus::Passed;
    l.prs = 1;
    let r = repo_for(&l);
    let cols = columns(std::slice::from_ref(&l), 120);
    let row = text(&line_spans(&l, &r, false, cols, 120, false, false));
    let shared = pane1_row(&r, false, cols);
    assert!(row.starts_with(&shared), "{row}\n{shared}");
    assert!(
        shared.ends_with("◉ 1PR ✓"),
        "pane 1's badges in their places: {shared}"
    );
    assert_eq!(
        &row[shared.len()..],
        "  3 commits  feat(ci): the ci view heads its columns"
    );
}

#[test]
fn a_line_with_two_authors_names_both_in_their_own_slot() {
    let mut l = counted(
        "bonsai-education",
        vec![commit("a1", "Ann Bee", "x"), commit("b2", "Cid Dee", "y")],
    );
    l.ci = CiStatus::Failed;
    let r = repo_for(&l);
    let cols = columns(std::slice::from_ref(&l), 100);
    let row = text(&line_spans(&l, &r, true, cols, 100, false, false));
    let shared = pane1_row(&r, true, cols);
    assert!(row.starts_with(&shared), "{row}");
    assert!(shared.starts_with("▸ "), "the cursor's marker is pane 1's");
    assert_eq!(&row[shared.len()..], "  2 commits  AB, CD  x");
}

#[test]
fn a_pushed_only_line_keeps_the_rows_badges_and_says_when() {
    let mut l = line("repoz", LineBody::PushedOnly);
    l.pushed_at = Some(chrono::Utc::now() - chrono::Duration::minutes(125));
    l.ci = CiStatus::Failed;
    l.prs = 3;
    let r = repo_for(&l);
    let cols = columns(std::slice::from_ref(&l), 100);
    let row = text(&line_spans(&l, &r, false, cols, 100, false, false));
    let shared = pane1_row(&r, false, cols);
    assert!(shared.ends_with("◉ 3PR ✓"), "{shared}");
    assert_eq!(&row[shared.len()..], "  pushed 2h ago");
}

#[test]
fn a_local_line_is_its_pane_1_row_whose_counters_are_the_worklist() {
    let l = line(
        "scratch",
        LineBody::Worklist {
            dirty: 4,
            stashes: 1,
            unpushed: 2,
        },
    );
    let mut r = crate::digest::tests::repo("scratch", crate::model::RepoLocation::LocalOnly);
    r.dirty = 4;
    r.stash_count = 1;
    r.ahead = 2;
    let cols = columns(std::slice::from_ref(&l), 100);
    let row = text(&line_spans(&l, &r, false, cols, 100, false, false));
    assert_eq!(row, pane1_row(&r, false, cols));
    assert!(
        row.ends_with(&format!("1{} 2↑ 4!", badges::stash_icon())),
        "{row}"
    );
}

/// Every slot starts in one column down the pane, so the eye reads a
/// column of counts and a column of subjects, as it reads pane 1's badges.
#[test]
fn the_digest_slots_line_up_down_the_pane() {
    let lines = vec![
        counted("a", vec![commit("1", "A", "first subject")]),
        counted(
            "a-much-longer-name",
            vec![
                commit("2", "Ann Bee", "second subject"),
                commit("3", "Cid Dee", "older"),
                commit("4", "Cid Dee", "older"),
                commit("5", "Cid Dee", "older"),
                commit("6", "Cid Dee", "older"),
                commit("7", "Cid Dee", "older"),
                commit("8", "Cid Dee", "older"),
                commit("9", "Cid Dee", "older"),
                commit("10", "Cid Dee", "older"),
                commit("11", "Cid Dee", "older"),
            ],
        ),
    ];
    let cols = columns(&lines, 120);
    let rows: Vec<String> = lines
        .iter()
        .map(|l| text(&line_spans(l, &repo_for(l), false, cols, 120, false, false)))
        .collect();
    let col = |row: &str, needle: &str| {
        row.chars().count() - row[row.find(needle).unwrap()..].chars().count()
    };
    assert_eq!(col(&rows[0], "1 commit"), col(&rows[1], "10 commits"));
    assert_eq!(col(&rows[0], "first"), col(&rows[1], "second"), "{rows:?}");
}

#[test]
fn a_narrow_pane_shrinks_the_name_as_pane_1_does_and_cuts_the_subject() {
    let l = counted(
        "a-rather-long-repository-name",
        vec![commit("1", "A", "a subject that will not fit")],
    );
    let r = repo_for(&l);
    let cols = columns(std::slice::from_ref(&l), 40);
    assert_eq!(cols.row, 40 - DIGEST_SLOT_MIN);
    let row = text(&line_spans(&l, &r, false, cols, 40, false, false));
    assert!(row.starts_with(&pane1_row(&r, false, cols)));
    assert!(row.contains('…'), "{row}");
    assert!(row.chars().count() <= 40, "{row}");
}

fn digest(section: Section, lines: Vec<DigestLine>) -> Digest {
    Digest {
        section,
        window: DigestWindow::Slot,
        label: "since 2026-10-26  Evening (18-00)".to_string(),
        lines,
    }
}

#[test]
fn the_totals_line_heads_the_window_and_sums_the_touched_repos() {
    let mut a = line(
        "a",
        LineBody::Counted {
            commits: vec![commit("1", "A", "x"), commit("2", "A", "y")],
        },
    );
    a.prs = 2;
    a.ci = CiStatus::Failed;
    let b = line("b", LineBody::PushedOnly);
    let rows = totals_rows(&digest(Section::GithubActivity, vec![a, b]), 200);
    assert_eq!(
        rows.iter().map(line_text).collect::<Vec<_>>(),
        vec!["since 2026-10-26  Evening (18-00)   2 repos · 2 commits · 2PR · 1 ◉ failed"]
    );
    // The PR count and the red lamp wear the rows' colours.
    let spans = &rows[0].spans;
    let pr = spans.iter().find(|s| s.content == "2PR").unwrap();
    assert_eq!(pr.style.fg, Some(colors::PR_CLR));
    let ci = spans.iter().find(|s| s.content.contains('◉')).unwrap();
    assert_eq!(
        ci.style.fg,
        Some(badges::ci_display(&CiStatus::Failed, false).1)
    );

    let local = line(
        "c",
        LineBody::Worklist {
            dirty: 3,
            stashes: 2,
            unpushed: 1,
        },
    );
    let rows = totals_rows(&digest(Section::LocalChanges, vec![local]), 200);
    assert_eq!(
        line_text(&rows[0]),
        format!(
            "since 2026-10-26  Evening (18-00)   1 repo · 1↑ 3! · 2{}",
            badges::stash_icon()
        )
    );
}

fn line_text(l: &Line<'_>) -> String {
    text(&l.spans)
}

#[test]
fn a_sum_of_zero_is_left_out_as_a_row_leaves_out_a_badge() {
    let d = digest(
        Section::GithubActivity,
        vec![line("a", LineBody::PushedOnly)],
    );
    assert_eq!(
        line_text(&totals_rows(&d, 200)[0]),
        "since 2026-10-26  Evening (18-00)   1 repo · 0 commits"
    );
}

#[test]
fn a_narrow_pane_puts_the_sums_under_the_window_rather_than_cut_them() {
    let d = digest(
        Section::GithubActivity,
        vec![line("a", LineBody::PushedOnly)],
    );
    assert_eq!(totals_rows(&d, 200).len(), 1);
    assert_eq!(
        totals_rows(&d, 40)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>(),
        vec!["since 2026-10-26  Evening (18-00)", "1 repo · 0 commits"]
    );
}

#[test]
fn no_em_or_en_dash_in_the_digest_words() {
    let words = [
        line_text(&totals_rows(&digest(Section::AlsoActive, vec![]), 200)[0]),
        body_text(&line("x", LineBody::Reading)),
    ];
    for w in words {
        assert!(!w.contains('\u{2014}') && !w.contains('\u{2013}'), "{w}");
    }
}

// Inspector and overflow (FZ-45 fix cycle 2).

fn rich_app(commits: usize) -> (App, Digest) {
    rich_app_with(commits, 2)
}

fn rich_app_with(commits: usize, prs: u32) -> (App, Digest) {
    let mut r = crate::digest::tests::repo("fleetz", crate::model::RepoLocation::Primary);
    r.ci_status = CiStatus::Failed;
    r.ci_workflow = Some("ci.yml".to_string());
    r.ci_duration = Some("2m".to_string());
    r.pr_count = prs;
    r.prs = (1..=prs)
        .map(|n| crate::model::PullRequest {
            number: n,
            title: format!("pull request title {n}"),
            author: "someone".to_string(),
            created_at: chrono::Utc::now(),
            review_status: crate::model::ReviewStatus::Unknown,
            url: String::new(),
        })
        .collect();
    let app = App::new(vec![r], String::new(), 120, 40);
    let mut l = counted(
        "fleetz",
        (0..commits)
            .map(|i| {
                commit(
                    &format!("h{i:03}"),
                    "Andreas Roos",
                    &format!("subject number {i}"),
                )
            })
            .collect(),
    );
    l.ci = CiStatus::Failed;
    l.prs = prs;
    (app, digest(Section::GithubActivity, vec![l]))
}

/// Every overflow marker's count, top to bottom.
fn markers(screen: &[String]) -> Vec<usize> {
    screen
        .iter()
        .filter_map(|row| {
            let rest = row.split("… and ").nth(1)?;
            rest.split(" more").next()?.parse().ok()
        })
        .collect()
}

/// Review 3 B1: twelve open PRs on a short inspector. The PR titles are
/// a list of their own, marked with how many PRs they left out; the
/// heading and the CI run still draw, and the commit list keeps its
/// title and its marker.
#[test]
fn many_prs_give_way_to_the_ci_run_and_the_commit_list() {
    let (app, d) = rich_app_with(30, 12);
    for (w, h) in [(80, 14), (170, 14)] {
        let screen = inspector_screen(&app, &d, w, h);
        let all = screen.join("\n");
        for fact in [
            "fleetz  30 commits",
            "◉ CI failed  ci.yml  2m",
            "commits since 2026-10-26",
        ] {
            assert!(all.contains(fact), "{w}x{h}: {fact:?} not drawn:\n{all}");
        }
        let prs_drawn = (1..=12)
            .filter(|n| all.contains(&format!("#{n} pull request")))
            .count();
        let commits_drawn = (0..30)
            .filter(|i| all.contains(&format!("h{i:03} ")))
            .count();
        let found = markers(&screen);
        let (pr_more, commit_more) = if w >= 170 {
            // Side by side: the lanes share rows, so read each lane.
            let left: Vec<String> = screen
                .iter()
                .map(|r| r.chars().take(85).collect())
                .collect();
            let right: Vec<String> = screen
                .iter()
                .map(|r| r.chars().skip(85).collect())
                .collect();
            (markers(&left)[0], markers(&right)[0])
        } else {
            assert_eq!(found.len(), 2, "{all}");
            (found[0], found[1])
        };
        assert_eq!(
            prs_drawn + pr_more,
            12,
            "{w}x{h}: the marker counts PRs:\n{all}"
        );
        assert_eq!(commits_drawn + commit_more, 30, "{w}x{h}:\n{all}");
    }
}

/// Review 3 B2: the facts never scroll. With the list scrolled, the
/// heading and every fact still draw, and the list starts further down.
#[test]
fn scrolling_moves_the_list_and_never_the_facts() {
    let (mut app, d) = rich_app(30);
    app.inspector_scroll = 5;
    for (w, h) in [(170, 14), (80, 18)] {
        let screen = inspector_screen(&app, &d, w, h);
        assert_every_fact_drawn(&screen);
        let all = screen.join("\n");
        assert!(!all.contains("h000 "), "{all}");
        assert!(all.contains("h005 "), "{all}");
    }
}

/// FZ-83 case 2: the fleet poll still says pending, the default branch's
/// check runs read after it say green. The digest line's pane 1 row draws
/// the lamp `App::ci_shown` gives, as pane 1's list does. Fails while
/// `render_pane` reads `ci_status`.
#[test]
fn a_digest_line_shows_the_fresher_verdict() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut alpha = counted("alpha", vec![commit("a1", "Ann Bee", "first")]);
    alpha.repo = 0;
    alpha.ci = CiStatus::Pending;
    let mut repo = repo_for(&alpha);
    repo.owner = Some("Gaurgle".to_string());
    let mut app = App::new(vec![repo], String::new(), 120, 40);
    let now = chrono::Local::now();
    app.github_freshness.data_time = crate::app::DataTime::At(now - chrono::Duration::minutes(1));
    let key = crate::ci::target(&app.repos[0])
        .expect("a GitHub identity")
        .0;
    app.ci.insert(
        key,
        crate::app::CiState {
            tip: Some(crate::app::CiTip {
                sha: "cc524b4".to_string(),
                statuses_complete: true,
                checks: Some(crate::ci::TipChecks {
                    runs: vec![crate::ci::Context {
                        name: "build".to_string(),
                        state: CiStatus::Passed,
                        description: None,
                        url: None,
                        actions: true,
                        source: "actions".to_string(),
                        started: None,
                        completed: None,
                    }],
                    complete: true,
                }),
                confirmed: Some(now),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let d = digest(Section::GithubActivity, vec![alpha]);

    let (w, h) = (100u16, 10u16);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let panel = Block::default().borders(Borders::ALL);
            render_pane(f, Rect::new(0, 0, w, h), panel, &app, &d, true)
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let lamp = (0..h)
        .find_map(|y| {
            let row: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
            row.contains(" alpha ").then_some(())?;
            (0..w)
                .find(|&x| buf[(x, y)].symbol() == "◉")
                .map(|x| buf[(x, y)].fg)
        })
        .expect("the line's lamp drawn");
    assert_eq!(lamp, colors::GREEN);
}

/// FZ-83 review: the same scenario as the row above, the fleet poll says
/// pending and the tip's check runs read after it say green. The
/// inspector card's `CI` line agrees with the row's lamp. Fails while
/// `facts` reads `line.ci`.
#[test]
fn a_digest_card_shows_the_fresher_verdict() {
    let mut alpha = counted("alpha", vec![commit("a1", "Ann Bee", "first")]);
    alpha.repo = 0;
    alpha.ci = CiStatus::Pending;
    let mut repo = repo_for(&alpha);
    repo.owner = Some("Gaurgle".to_string());
    let mut app = App::new(vec![repo], String::new(), 120, 40);
    let now = chrono::Local::now();
    app.github_freshness.data_time = crate::app::DataTime::At(now - chrono::Duration::minutes(1));
    let key = crate::ci::target(&app.repos[0])
        .expect("a GitHub identity")
        .0;
    app.ci.insert(
        key,
        crate::app::CiState {
            tip: Some(crate::app::CiTip {
                sha: "cc524b4".to_string(),
                statuses_complete: true,
                checks: Some(crate::ci::TipChecks {
                    runs: vec![crate::ci::Context {
                        name: "build".to_string(),
                        state: CiStatus::Passed,
                        description: None,
                        url: None,
                        actions: true,
                        source: "actions".to_string(),
                        started: None,
                        completed: None,
                    }],
                    complete: true,
                }),
                confirmed: Some(now),
                ..Default::default()
            }),
            ..Default::default()
        },
    );

    let card = facts(&app, &alpha).ci.expect("a CI line");
    let text: String = card.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("CI passed"), "{text}");
}

/// FZ-66: the cursor's digest line wears pane 1's band across the whole
/// pane, slots and the blank past them included; the line below has none.
#[test]
fn the_cursor_line_wears_the_band_to_the_pane_edge() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color as TermColor;

    let mut alpha = counted("alpha", vec![commit("a1", "Ann Bee", "first")]);
    alpha.repo = 0;
    let mut beta = counted("beta", vec![commit("b1", "Cid Dee", "second")]);
    beta.repo = 1;
    let repos = vec![repo_for(&alpha), repo_for(&beta)];
    let mut app = App::new(repos, String::new(), 120, 40);
    app.digest_selected = 0;
    let d = digest(Section::GithubActivity, vec![alpha, beta]);

    let (w, h) = (100u16, 10u16);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let panel = Block::default().borders(Borders::ALL);
            render_pane(f, Rect::new(0, 0, w, h), panel, &app, &d, true)
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let row_of = |needle: &str| {
        (0..h)
            .find(|&y| {
                let row: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
                row.contains(needle)
            })
            .unwrap_or_else(|| panic!("{needle:?} not drawn"))
    };
    let (on, off) = (row_of(" alpha "), row_of(" beta "));
    assert_eq!(off, on + 1, "beta is the line below the cursor's");
    let slot: String = (0..w).map(|x| buf[(x, on)].symbol()).collect();
    assert!(
        slot.contains("1 commit") && slot.contains("first"),
        "{slot}"
    );
    for x in 1..w - 1 {
        assert_eq!(buf[(x, on)].bg, colors::SELECTED_BG, "band at x {x}");
        assert_eq!(buf[(x, off)].bg, TermColor::Reset, "no band at x {x}");
    }
}

fn inspector_screen(app: &App, d: &Digest, w: u16, h: u16) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| render_inspector(f, Rect::new(0, 0, w, h), app, d))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

fn assert_every_fact_drawn(screen: &[String]) {
    let all = screen.join("\n");
    for fact in [
        "fleetz  30 commits",
        "pull requests",
        "#1 pull request title 1",
        "#2 pull request title 2",
        "◉ CI failed  ci.yml  2m",
        "commits since 2026-10-26",
    ] {
        assert!(all.contains(fact), "{fact:?} not drawn:\n{all}");
    }
}

/// The commits drawn and the count the marker gives sum to the list.
fn assert_marker_counts(screen: &[String], total: usize) {
    let all = screen.join("\n");
    let drawn = (0..total)
        .filter(|i| all.contains(&format!("h{i:03} ")))
        .count();
    let more = all
        .split("… and ")
        .nth(1)
        .and_then(|rest| rest.split(" more").next())
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("no overflow marker:\n{all}"));
    assert_eq!(drawn + more, total, "{all}");
}

#[test]
fn a_wide_inspector_draws_every_fact_beside_the_list_without_scrolling() {
    let (app, d) = rich_app(30);
    let screen = inspector_screen(&app, &d, 170, 14);
    assert_every_fact_drawn(&screen);
    let row_of = |needle: &str| screen.iter().position(|r| r.contains(needle)).unwrap();
    assert_eq!(
        row_of("pull requests"),
        row_of("h001"),
        "side by side:\n{}",
        screen.join("\n")
    );
    assert_marker_counts(&screen, 30);
}

#[test]
fn a_narrow_inspector_stacks_the_list_under_every_fact() {
    let (app, d) = rich_app(30);
    let screen = inspector_screen(&app, &d, 80, 18);
    assert_every_fact_drawn(&screen);
    let row_of = |needle: &str| screen.iter().position(|r| r.contains(needle)).unwrap();
    assert!(
        row_of("CI failed") < row_of("commits since"),
        "{}",
        screen.join("\n")
    );
    assert_marker_counts(&screen, 30);
}

#[test]
fn a_list_that_fits_has_no_marker() {
    let (app, d) = rich_app(2);
    for (w, h) in [(170, 14), (80, 18)] {
        let all = inspector_screen(&app, &d, w, h).join("\n");
        assert!(all.contains("h000") && all.contains("h001"), "{all}");
        assert!(!all.contains(" more"), "{all}");
    }
}

#[test]
fn a_long_subject_wraps_rather_than_being_cut() {
    let entries = vec![Line::from("one two three four five six seven")];
    let rows = fit_list(&entries, 0, 12, 10);
    assert!(rows.len() > 1);
    let joined: String = rows.iter().map(line_text).collect::<Vec<_>>().join(" ");
    assert!(
        joined.contains("seven") && !joined.contains('…'),
        "{joined}"
    );
}

#[test]
fn pane_2_marks_the_digest_lines_it_leaves_out() {
    assert_eq!(visible_lines(5, 0, 10), (0, 5, 0), "fits: no marker");
    assert_eq!(
        visible_lines(20, 0, 6),
        (0, 5, 15),
        "five shown, the marker row says 15"
    );
    let (first, shown, hidden) = visible_lines(20, 12, 6);
    assert!(
        first <= 12 && 12 < first + shown,
        "the cursor stays in view"
    );
    assert_eq!(first + shown + hidden, 20);
    assert_eq!(
        visible_lines(20, 19, 6),
        (14, 6, 0),
        "at the end the lines fill the pane"
    );
    // Review 3 F1: one body row holds the marker alone, inside the pane.
    assert_eq!(visible_lines(20, 0, 1), (0, 0, 20));
    assert_eq!(visible_lines(20, 7, 1), (0, 0, 20));
}
