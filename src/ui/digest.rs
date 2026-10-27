//! Drawing the slot digest (FZ-45): pane 2 on a section header, and the
//! inspector's detail of the digest line under pane 2's cursor. The model
//! is `crate::digest`; nothing here decides what a line says.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::{App, FocusedPanel};
use crate::digest::{Digest, DigestLine, DigestWindow, LineBody, Section};
use crate::model::{CiStatus, RepoLocation};
use crate::ui::{badges, colors, format_age, truncate};

/// Widest the name column grows before names are cut.
const NAME_MAX: usize = 24;

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The totals line: the window as repoz heads it, then the sums over the
/// touched repos.
pub fn totals_text(digest: &Digest) -> String {
    let t = digest.totals();
    let sums = match digest.section {
        Section::LocalChanges => [
            plural(t.repos, "repo", "repos"),
            format!("{} dirty", t.dirty),
            plural(t.stashes as usize, "stash", "stashes"),
            format!("{} unpushed", t.unpushed),
        ],
        Section::GithubActivity | Section::AlsoActive => [
            plural(t.repos, "repo", "repos"),
            plural(t.commits, "commit", "commits"),
            plural(t.prs as usize, "PR", "PRs"),
            format!("{} red CI", t.red_ci),
        ],
    };
    format!("{}{TOTALS_GAP}{}", digest.label, sums.join(" · "))
}

const TOTALS_GAP: &str = "   ";

/// The totals line as drawn `width` wide: one line when it fits, else the
/// window on one and the sums under it, so a narrow pane keeps the sums.
pub fn totals_rows(digest: &Digest, width: usize) -> Vec<String> {
    let whole = totals_text(digest);
    if whole.chars().count() <= width {
        return vec![whole];
    }
    let sums = whole[digest.label.len() + TOTALS_GAP.len()..].to_string();
    vec![truncate(&digest.label, width), truncate(&sums, width)]
}

/// What a line says after its name, before the CI and PR tail.
fn body_text(line: &DigestLine) -> String {
    match &line.body {
        LineBody::Counted { commits } => {
            let mut text = plural(commits.len(), "commit", "commits");
            let authors = line.authors();
            if authors.len() > 1 {
                let codes: Vec<String> =
                    authors.iter().map(|a| crate::ui::author_code(a)).collect();
                text.push_str(&format!(" by {}", codes.join(", ")));
            }
            text
        }
        LineBody::PushedOnly => match line.pushed_at {
            Some(at) => format!("pushed {}", format_age(at)),
            None => "pushed".to_string(),
        },
        LineBody::Reading => "reading the log…".to_string(),
        LineBody::Worklist {
            dirty,
            stashes,
            unpushed,
        } => {
            let mut parts = Vec::new();
            if *dirty > 0 {
                parts.push(format!("{dirty} dirty"));
            }
            if *stashes > 0 {
                parts.push(plural(*stashes as usize, "stash", "stashes"));
            }
            if *unpushed > 0 {
                parts.push(format!("{unpushed} unpushed"));
            }
            if parts.is_empty() {
                "clean".to_string()
            } else {
                parts.join(" · ")
            }
        }
    }
}

/// The CI and PR tail of a counted line: the poll's fields in hand. A
/// pushed-only line says nothing more, and the worklist is local.
fn tail_spans(line: &DigestLine) -> Vec<Span<'static>> {
    if !matches!(line.body, LineBody::Counted { .. } | LineBody::Reading) {
        return Vec::new();
    }
    let mut spans = Vec::new();
    let (glyph, color, word) = badges::ci_display(&line.ci, false);
    if line.ci != CiStatus::Unknown {
        spans.push(Span::styled(
            format!("{glyph} {word}"),
            Style::default().fg(color),
        ));
    }
    if line.prs > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            plural(line.prs as usize, "PR", "PRs"),
            Style::default().fg(colors::PR_CLR),
        ));
    }
    spans
}

fn width_of(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// One digest line, `width` columns wide: marker, name, what moved, the
/// newest subject in the slack, and the CI and PR tail flush right.
pub fn line_spans(
    line: &DigestLine,
    selected: bool,
    name_w: usize,
    width: usize,
) -> Vec<Span<'static>> {
    let marker = if selected { "▸ " } else { "  " };
    let name_style = if selected {
        Style::default()
            .fg(colors::SAPPHIRE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::SAPPHIRE)
    };
    let name = truncate(&line.name, name_w);
    let body_color = match line.body {
        LineBody::PushedOnly | LineBody::Reading => colors::OVERLAY,
        _ => colors::TEXT,
    };
    let mut spans = vec![
        Span::raw(marker),
        Span::styled(format!("{name:<name_w$}  "), name_style),
        Span::styled(body_text(line), Style::default().fg(body_color)),
    ];
    let tail = tail_spans(line);
    let tail_w = width_of(&tail);
    if let LineBody::Counted { commits } = &line.body
        && let Some(newest) = commits.first()
    {
        let room = width.saturating_sub(width_of(&spans) + 2 + tail_w + 2);
        if room >= 4 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                truncate(&newest.message, room),
                Style::default().fg(colors::OVERLAY),
            ));
        }
    }
    if !tail.is_empty() {
        let gap = width.saturating_sub(width_of(&spans) + tail_w).max(2);
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(tail);
    }
    spans
}

/// The window switcher in pane 2's border: the four windows `t` cycles,
/// the one in force lit.
fn window_switcher(active: DigestWindow, focused: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(" t ", Style::default().fg(colors::OVERLAY))];
    for (i, w) in DigestWindow::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        let style = if *w == active {
            Style::default()
                .fg(if focused {
                    colors::LAVENDER
                } else {
                    colors::TEXT
                })
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors::OVERLAY)
        };
        spans.push(Span::styled(w.name(), style));
    }
    spans.push(Span::raw(" "));
    Line::from(spans).right_aligned()
}

/// Pane 2 on a section header: the totals line, then one line per touched
/// repo, scrolled to keep pane 2's cursor in view.
pub fn render_pane(
    frame: &mut Frame,
    area: Rect,
    panel: Block<'_>,
    app: &App,
    digest: &Digest,
    focused: bool,
) {
    let number = crate::ui::panel_title(2, "", focused).spans.swap_remove(0);
    let title = Line::from(vec![
        number,
        Span::styled(
            format!("{} ", digest.section.label()),
            Style::default()
                .fg(colors::MAUVE)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let panel = panel
        .title_top(title.left_aligned())
        .title_top(window_switcher(digest.window, focused));
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    let width = inner.width as usize;

    let mut lines: Vec<Line> = totals_rows(digest, width)
        .into_iter()
        .map(|row| {
            Line::from(Span::styled(
                row,
                Style::default()
                    .fg(colors::TEXT)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    lines.push(Line::from(""));
    if digest.lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "nothing moved in this window",
            Style::default().fg(colors::OVERLAY),
        )));
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }
    let selected = app.digest_selected.min(digest.lines.len() - 1);
    let name_w = digest
        .lines
        .iter()
        .map(|l| l.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(NAME_MAX);
    let body_rows = (inner.height as usize).saturating_sub(lines.len()).max(1);
    let first = (selected + 1).saturating_sub(body_rows);
    for (i, line) in digest.lines.iter().enumerate().skip(first).take(body_rows) {
        lines.push(Line::from(line_spans(line, i == selected, name_w, width)));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn label(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default()
            .fg(colors::MAUVE)
            .add_modifier(Modifier::BOLD),
    ))
}

fn dim(text: String) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(colors::OVERLAY)))
}

/// What the inspector says about one digest line: that repo's commits in
/// the window with author and time, its PR titles, and its CI run; for the
/// worklist, its counts and stashes.
pub fn inspector_lines(app: &App, digest: &Digest, line: &DigestLine) -> Vec<Line<'static>> {
    let repo = &app.repos[line.repo];
    let mut out = vec![
        Line::from(vec![
            Span::styled(
                line.name.clone(),
                Style::default()
                    .fg(colors::SAPPHIRE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", body_text(line)),
                Style::default().fg(colors::OVERLAY),
            ),
        ]),
        Line::from(""),
    ];
    match &line.body {
        LineBody::Counted { commits } => {
            out.push(label(&format!("commits {}", digest.label)));
            for c in commits {
                let age = c.date.map(format_age).unwrap_or_default();
                out.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", c.hash),
                        Style::default().fg(colors::HASH_CLR),
                    ),
                    Span::styled(
                        format!("{} ", c.author),
                        Style::default().fg(colors::SUBTEXT),
                    ),
                    Span::styled(format!("{age:<8}"), Style::default().fg(colors::OVERLAY)),
                    Span::styled(c.message.clone(), Style::default().fg(colors::TEXT)),
                ]));
            }
        }
        LineBody::PushedOnly if repo.location == RepoLocation::NotCloned => {
            out.push(dim("not cloned: what moved is on GitHub only".to_string()));
        }
        LineBody::PushedOnly => {
            out.push(dim(
                "not fetched since the window began, so its commits are not counted".to_string(),
            ));
        }
        LineBody::Reading => out.push(dim("reading the log…".to_string())),
        LineBody::Worklist {
            dirty,
            stashes,
            unpushed,
        } => {
            out.push(Line::from(format!("{dirty} dirty files")));
            out.push(Line::from(format!("{unpushed} unpushed commits")));
            out.push(Line::from(plural(*stashes as usize, "stash", "stashes")));
            for entry in &repo.stash_entries {
                out.push(dim(format!("  {entry}")));
            }
            return out;
        }
    }
    if line.prs > 0 {
        out.push(Line::from(""));
        out.push(label("pull requests"));
        for pr in &repo.prs {
            out.push(Line::from(vec![
                Span::styled(
                    format!("#{} ", pr.number),
                    Style::default().fg(colors::PR_CLR),
                ),
                Span::styled(pr.title.clone(), Style::default().fg(colors::TEXT)),
                Span::styled(
                    format!("  {}", pr.author),
                    Style::default().fg(colors::OVERLAY),
                ),
            ]));
        }
        if repo.prs.is_empty() {
            out.push(dim(format!(
                "{} open; titles with -p",
                plural(line.prs as usize, "PR", "PRs")
            )));
        }
    }
    if line.ci != CiStatus::Unknown {
        let (glyph, color, word) = badges::ci_display(&line.ci, false);
        let mut spans = vec![Span::styled(
            format!("{glyph} CI {word}"),
            Style::default().fg(color),
        )];
        for part in [&repo.ci_workflow, &repo.ci_duration].into_iter().flatten() {
            spans.push(Span::styled(
                format!("  {part}"),
                Style::default().fg(colors::OVERLAY),
            ));
        }
        out.push(Line::from(""));
        out.push(Line::from(spans));
    }
    out
}

/// The inspector on a section header: the detail of pane 2's digest line.
pub fn render_inspector(frame: &mut Frame, area: Rect, app: &App, digest: &Digest) {
    let focused = app.focused_panel == FocusedPanel::Inspector;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused {
            colors::LAVENDER
        } else {
            colors::SURFACE
        }))
        .padding(Padding::new(2, 2, 1, 0))
        .title_top(crate::ui::panel_title(
            crate::ui::INSPECTOR_NUM,
            "inspector",
            focused,
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = match digest.lines.last() {
        None => vec![dim(format!("nothing moved {}", digest.label))],
        Some(_) => {
            let line = &digest.lines[app.digest_selected.min(digest.lines.len() - 1)];
            inspector_lines(app, digest, line)
        }
    };
    let width = inner.width as usize;
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|l| {
            let text: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
            if text.chars().count() <= width {
                l
            } else {
                let style = l.spans.first().map(|s| s.style).unwrap_or_default();
                Line::from(Span::styled(truncate(&text, width), style))
            }
        })
        .collect();
    let max_scroll = (lines.len() as u16).saturating_sub(inner.height);
    app.inspector_max_scroll.set(max_scroll);
    let scroll = app.inspector_scroll.min(max_scroll);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn a_counted_line_names_the_count_the_newest_subject_ci_and_prs() {
        let mut l = line(
            "fleetz",
            LineBody::Counted {
                commits: vec![
                    commit(
                        "a1",
                        "Andreas Roos",
                        "feat(ci): the ci view heads its columns",
                    ),
                    commit("b2", "Andreas Roos", "older"),
                    commit("c3", "Andreas Roos", "oldest"),
                ],
            },
        );
        l.ci = CiStatus::Passed;
        l.prs = 1;
        let row = text(&line_spans(&l, false, 16, 100));
        assert!(
            row.starts_with("  fleetz            3 commits  feat(ci): the ci view"),
            "{row}"
        );
        assert!(row.ends_with("◉ passed  1 PR"), "{row}");
        assert_eq!(row.chars().count(), 100, "{row}");
        assert!(!row.contains(" by "), "one author is not named: {row}");
    }

    #[test]
    fn a_line_with_two_authors_names_both() {
        let mut l = line(
            "bonsai-education",
            LineBody::Counted {
                commits: vec![commit("a1", "Ann Bee", "x"), commit("b2", "Cid Dee", "y")],
            },
        );
        l.ci = CiStatus::Failed;
        let row = text(&line_spans(&l, true, 16, 80));
        assert!(
            row.starts_with("▸ bonsai-education  2 commits by AB, CD"),
            "{row}"
        );
        assert!(row.ends_with("◉ failed"), "{row}");
    }

    #[test]
    fn a_pushed_only_line_says_when_and_nothing_more() {
        let mut l = line("repoz", LineBody::PushedOnly);
        l.pushed_at = Some(chrono::Utc::now() - chrono::Duration::minutes(125));
        l.ci = CiStatus::Failed;
        l.prs = 3;
        let row = text(&line_spans(&l, false, 6, 80));
        assert_eq!(row.trim_end(), "  repoz   pushed 2h ago");
    }

    #[test]
    fn a_local_line_is_the_worklist() {
        let l = line(
            "scratch",
            LineBody::Worklist {
                dirty: 4,
                stashes: 1,
                unpushed: 2,
            },
        );
        let row = text(&line_spans(&l, false, 7, 80));
        assert_eq!(row.trim_end(), "  scratch  4 dirty · 1 stash · 2 unpushed");
        let clean = line(
            "idle",
            LineBody::Worklist {
                dirty: 0,
                stashes: 0,
                unpushed: 0,
            },
        );
        assert_eq!(
            text(&line_spans(&clean, false, 4, 80)).trim_end(),
            "  idle  clean"
        );
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
        assert_eq!(
            totals_text(&digest(Section::GithubActivity, vec![a, b])),
            "since 2026-10-26  Evening (18-00)   2 repos · 2 commits · 2 PRs · 1 red CI"
        );
        let local = line(
            "c",
            LineBody::Worklist {
                dirty: 3,
                stashes: 2,
                unpushed: 1,
            },
        );
        assert_eq!(
            totals_text(&digest(Section::LocalChanges, vec![local])),
            "since 2026-10-26  Evening (18-00)   1 repo · 3 dirty · 2 stashes · 1 unpushed"
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
            totals_rows(&d, 50),
            vec![
                "since 2026-10-26  Evening (18-00)".to_string(),
                "1 repo · 0 commits · 0 PRs · 0 red CI".to_string()
            ]
        );
    }

    #[test]
    fn no_em_or_en_dash_in_the_digest_words() {
        let words = [
            totals_text(&digest(Section::AlsoActive, vec![])),
            body_text(&line("x", LineBody::Reading)),
        ];
        for w in words {
            assert!(!w.contains('\u{2014}') && !w.contains('\u{2013}'), "{w}");
        }
    }
}
