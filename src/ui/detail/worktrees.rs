use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use super::{clip_spans, commit_age, pad_to, section_label, span_width, wash, wrap_line};
use crate::app::{App, FocusedPanel};
use crate::model::RepoStatus;
use crate::ui::badges;
use crate::ui::colors;
use crate::ui::format_age;

/// The inspector while pane 2 shows `worktrees` (FZ-77): the highlighted
/// worktree's facts, then its newest commits as the read landed them.
pub(super) fn render_worktree_inspector(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focused_panel == FocusedPanel::Inspector;
    let border_color = if focused {
        colors::LAVENDER
    } else {
        colors::SURFACE
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::new(2, 2, 1, 0))
        .title_top(crate::ui::panel_title(
            crate::ui::INSPECTOR_NUM,
            "inspector",
            focused,
        ));
    let inner = block.inner(area);
    let lines: Vec<Line> = worktree_inspector_lines(app)
        .iter()
        .flat_map(|l| wrap_line(l, inner.width as usize))
        .collect();
    let max_scroll = (lines.len() as u16).saturating_sub(inner.height);
    app.inspector_max_scroll.set(max_scroll);
    let scroll = app.inspector_scroll.min(max_scroll);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

/// What the `worktrees` inspector draws for the highlighted worktree: its
/// path, branch, HEAD, dirty count, ahead and behind against its upstream,
/// the age of its last commit, then its newest commits in the branches
/// preview's row form. Ahead and behind come from the main checkout's
/// `branches`, as `worktree_table`'s column does.
fn worktree_inspector_lines(app: &App) -> Vec<Line<'static>> {
    let (Some(repo), Some(wt)) = (app.selected_repo(), app.worktree_highlighted()) else {
        return vec![Line::from(Span::styled(
            "no worktree selected",
            Style::default().fg(colors::OVERLAY),
        ))];
    };
    let label =
        |text: &str| Span::styled(format!("{text:<8}"), Style::default().fg(colors::OVERLAY));
    let plain = |text: String, color| Span::styled(text, Style::default().fg(color));
    let log = app.worktree_log();
    let mut lines = vec![Line::from(Span::styled(
        wt.path.clone(),
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD),
    ))];

    let branch = match wt.detached {
        true => plain(format!("detached @ {}", wt.head), colors::PEACH),
        false => plain(wt.branch.clone(), colors::TEXT),
    };
    lines.push(Line::from(vec![label("branch"), branch]));

    let subject = log
        .and_then(|commits| commits.first())
        .filter(|c| {
            !wt.head.is_empty() && (c.hash.starts_with(&wt.head) || wt.head.starts_with(&c.hash))
        })
        .map(|c| c.message.clone());
    let mut head = vec![label("HEAD"), plain(wt.head.clone(), colors::HASH_CLR)];
    if let Some(subject) = subject {
        head.push(plain(format!(" {subject}"), colors::TEXT));
    }
    lines.push(Line::from(head));

    let dirty = match wt.dirty {
        None => plain("unknown".to_string(), colors::OVERLAY),
        Some(0) => plain("clean".to_string(), colors::OVERLAY),
        Some(n) => Span::styled(
            format!("{} ({n} changed)", crate::model::counters_text(0, 0, n)),
            Style::default().fg(badges::counter_color(0, 0, n)),
        ),
    };
    lines.push(Line::from(vec![label("dirty"), dirty]));

    let tracked = (!wt.detached)
        .then(|| repo.branches.iter().find(|b| b.name == wt.branch))
        .flatten();
    let sync = match tracked {
        None => plain("·".to_string(), colors::OVERLAY),
        Some(b) => match &b.upstream {
            None => plain("no upstream".to_string(), colors::OVERLAY),
            Some(up) if b.behind == 0 && b.ahead == 0 => {
                plain(format!("level with {up}"), colors::OVERLAY)
            }
            Some(up) => Span::styled(
                format!(
                    "{} against {up}",
                    crate::model::counters_text(b.behind, b.ahead, 0)
                ),
                Style::default().fg(badges::counter_color(b.behind, b.ahead, 0)),
            ),
        },
    };
    lines.push(Line::from(vec![label("sync"), sync]));

    let age = wt.head_at.map_or_else(|| "·".to_string(), format_age);
    lines.push(Line::from(vec![label("last"), plain(age, colors::OVERLAY)]));

    lines.push(Line::from(""));
    lines.push(Line::from(section_label("recent commits")));
    match log {
        None => lines.push(Line::from(plain("  reading…".to_string(), colors::OVERLAY))),
        Some([]) => lines.push(Line::from(plain(
            "  (no commits)".to_string(),
            colors::OVERLAY,
        ))),
        Some(commits) => {
            for c in commits {
                lines.push(Line::from(vec![
                    plain(format!("  {}", c.hash), colors::HASH_CLR),
                    Span::styled(
                        format!(" {} ", crate::ui::author_code(&c.author)),
                        Style::default().fg(colors::author_color(&c.author)),
                    ),
                    plain(c.message.clone(), colors::TEXT),
                    plain(format!("  {}", commit_age(c)), colors::OVERLAY),
                ]));
            }
        }
    }
    lines
}

/// A worktree's path as the `worktrees` section draws it: relative to the
/// repo when it lives inside it (`.claude/worktrees/issue-45`), `.` when it
/// is the repo, otherwise unchanged. Both are the `~`-collapsed spellings the
/// pane already prints, so this is a comparison of strings, never of disks.
/// A sibling that merely shares the prefix (`fleetz-hotfix` beside `fleetz`)
/// is outside.
fn path_within_repo(path: &str, repo_path: &str) -> String {
    let repo = repo_path.trim_end_matches('/');
    if repo.is_empty() {
        return path.to_string();
    }
    if path == repo {
        return ".".to_string();
    }
    match path
        .strip_prefix(repo)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => path.to_string(),
    }
}

/// Widest a worktree's branch column grows before the name is cut.
const WORKTREE_NAME_MAX: usize = 32;

/// The Info view's `worktrees` section: the label, then one row per linked
/// worktree with its branch (or `detached @ <sha>`), dirty count, ahead and
/// behind, the age of its last commit, and its path. Empty when the repo has
/// no linked worktree.
///
/// Ahead and behind come from the main checkout's `branches`: a linked
/// worktree's branch is a local branch of the same repository. A detached
/// worktree has nothing to compare, so that column reads `·`.
///
/// Every counter is spelled by `model::counters_text` and coloured by
/// `badges::counter_color`, the helpers the repo list's status token is
/// built from, so a worktree's `3!` is the repo list's `3!` in the repo
/// list's colour and cannot drift from it.
///
/// Each row fits `width`, the pane's: the path is cut to what the columns
/// before it leave, and a row whose columns alone are wider is clipped
/// (`clip_spans`), so one row is always one rendered row in the wrapping
/// body.
pub(super) fn worktree_rows(repo: &RepoStatus, width: usize) -> Vec<Line<'static>> {
    if repo.worktrees.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(section_label("worktrees"))];
    lines.extend(worktree_table(repo, &repo.worktrees, width));
    lines
}

/// The `worktrees` view's list (FZ-77): one row per worktree, the main
/// checkout first and marked `*` as the checked-out branch is, each row
/// `worktree_table`'s behind the cursor's caret column. Returns the lines
/// and the selected row; a row with nothing on disk says `not cloned`, and
/// a repo with no linked worktree says so under its main checkout.
pub(super) fn worktrees_view_rows(
    app: &App,
    repo: &RepoStatus,
    width: usize,
) -> (Vec<Line<'static>>, Option<usize>) {
    let dim = |text: &str| {
        Line::from(Span::styled(
            format!("  {text}"),
            Style::default().fg(colors::OVERLAY),
        ))
    };
    let listed = app.worktrees_listed();
    if listed.is_empty() {
        let text = match repo.is_not_cloned() {
            true => "not cloned",
            false => "no worktrees",
        };
        return (vec![dim(text)], None);
    }
    let selected = app.detail_selected().min(listed.len() - 1);
    let head_color = colors::ref_color(crate::model::RefKind::Head);
    let mut lines = Vec::with_capacity(listed.len() + 1);
    for (i, row) in worktree_table(repo, &listed, width.saturating_sub(2))
        .into_iter()
        .enumerate()
    {
        let is_sel = i == selected;
        let mut spans = vec![Span::styled(
            if is_sel { "▸ " } else { "  " },
            Style::default().fg(colors::SAPPHIRE),
        )];
        let mut cells = row.spans;
        if i == 0 && !cells.is_empty() {
            cells[0] = Span::styled("* ", Style::default().fg(head_color));
        }
        spans.extend(cells);
        if is_sel {
            spans = wash(pad_to(spans, width));
        }
        lines.push(Line::from(spans));
    }
    if listed.len() == 1 {
        lines.push(dim("no worktrees besides the main one"));
    }
    (lines, Some(selected))
}

/// One row per entry of `worktrees`, the columns `worktree_rows` draws:
/// that section and the `worktrees` view (FZ-77) both lay their rows out
/// here, so the two cannot drift. Each row starts with two blank columns,
/// the indent the view's main checkout marks.
fn worktree_table(
    repo: &RepoStatus,
    worktrees: &[crate::model::Worktree],
    width: usize,
) -> Vec<Line<'static>> {
    let token = |behind: u32, ahead: u32, dirty: u32| -> Span<'static> {
        Span::styled(
            crate::model::counters_text(behind, ahead, dirty),
            Style::default().fg(badges::counter_color(behind, ahead, dirty)),
        )
    };
    let unknown = |text: &str| Span::styled(text.to_string(), Style::default().fg(colors::OVERLAY));
    let repo_path = repo.path.as_deref().unwrap_or_default();

    struct Row {
        name: String,
        dirty: Option<Span<'static>>,
        sync: Option<Span<'static>>,
        age: String,
        path: String,
    }
    let rows: Vec<Row> = worktrees
        .iter()
        .map(|wt| {
            let branch = (!wt.detached)
                .then(|| repo.branches.iter().find(|b| b.name == wt.branch))
                .flatten();
            let level =
                branch.is_some_and(|b| b.upstream.is_some() && b.behind == 0 && b.ahead == 0);
            let dirty = match wt.dirty {
                None => Some(unknown("?")),
                Some(n) if n > 0 => Some(token(0, 0, n)),
                Some(_) if wt.detached => Some(unknown("·")),
                Some(_) if level => Some(token(0, 0, 0)),
                Some(_) => None,
            };
            let sync = match branch {
                None => Some(unknown("·")),
                Some(b) if b.upstream.is_none() => Some(unknown("no upstream")),
                Some(b) if b.behind > 0 || b.ahead > 0 => Some(token(b.behind, b.ahead, 0)),
                Some(_) => None,
            };
            let name = if wt.detached {
                format!("detached @ {}", wt.head)
            } else {
                wt.branch.clone()
            };
            Row {
                name: crate::ui::truncate(&name, WORKTREE_NAME_MAX),
                dirty,
                sync,
                age: wt.head_at.map_or_else(|| "·".to_string(), format_age),
                path: path_within_repo(&wt.path, repo_path),
            }
        })
        .collect();

    let width_of =
        |span: &Option<Span<'static>>| span.as_ref().map_or(0, |s| s.content.chars().count());
    let name_w = rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let dirty_w = rows.iter().map(|r| width_of(&r.dirty)).max().unwrap_or(0);
    let sync_w = rows.iter().map(|r| width_of(&r.sync)).max().unwrap_or(0);
    let age_w = rows
        .iter()
        .map(|r| r.age.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines = Vec::with_capacity(rows.len());
    for row in rows {
        let mut spans = vec![Span::raw("  ")];
        spans.extend(pad_to(
            vec![Span::styled(row.name, Style::default().fg(colors::TEXT))],
            name_w,
        ));
        for (cell, w) in [(row.dirty, dirty_w), (row.sync, sync_w)] {
            if w == 0 {
                continue;
            }
            spans.push(Span::raw("  "));
            spans.extend(pad_to(cell.into_iter().collect(), w));
        }
        spans.push(Span::styled(
            format!("  {:>age_w$}", row.age),
            Style::default().fg(colors::OVERLAY),
        ));
        let path_budget = width.saturating_sub(span_width(&spans) + 2);
        if path_budget > 0 {
            spans.push(Span::styled(
                format!("  {}", crate::ui::truncate(&row.path, path_budget)),
                Style::default().fg(colors::OVERLAY),
            ));
        }
        lines.push(Line::from(clip_spans(spans, width)));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::HistoryView;
    use crate::ui::detail::tests::repo_with_a_worktree;

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
}
