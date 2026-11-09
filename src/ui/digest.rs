//! Drawing the slot digest (FZ-45): pane 2 on a section header, and the
//! inspector's detail of the digest line under pane 2's cursor. The model
//! is `crate::digest`; nothing here decides what a line says.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::{App, FocusedPanel};
use crate::digest::{Digest, DigestLine, DigestWindow, LineBody, Section};
use crate::model::{CiStatus, RepoLocation};
use crate::ui::{badges, colors, format_age, truncate};

/// Columns the digest's own slots keep beside the shared repo row before
/// that row gives up width, as pane 1 gives up name width before badges.
const DIGEST_SLOT_MIN: usize = 16;

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The sums over the touched repos, in pane 1's badge words and colours:
/// the PR count as the row's `2PR`, a red CI as its lamp with the word
/// `badges::ci_display` gives it, the worklist as the row's counters. A sum
/// of zero is left out, as a row leaves out a badge it does not have.
fn sums_spans(digest: &Digest) -> Vec<Span<'static>> {
    let t = digest.totals();
    let plain = |text: String| Span::styled(text, Style::default().fg(colors::TEXT));
    let mut parts = vec![plain(plural(t.repos, "repo", "repos"))];
    match digest.section {
        Section::LocalChanges => {
            // The row's own status token: `model::counters_text` in the
            // sync ladder's colour, `badges::counter_color`.
            if t.dirty > 0 || t.unpushed > 0 {
                parts.push(Span::styled(
                    crate::model::counters_text(0, t.unpushed, t.dirty),
                    Style::default().fg(badges::counter_color(0, t.unpushed, t.dirty)),
                ));
            }
            if t.stashes > 0 {
                parts.push(Span::styled(
                    format!("{}{}", t.stashes, badges::stash_icon()),
                    Style::default().fg(colors::STASH_CLR),
                ));
            }
        }
        Section::GithubActivity | Section::AlsoActive => {
            parts.push(plain(plural(t.commits, "commit", "commits")));
            if t.prs > 0 {
                parts.push(Span::styled(
                    format!("{}PR", t.prs),
                    Style::default().fg(colors::PR_CLR),
                ));
            }
            if t.red_ci > 0 {
                let (glyph, color, word) = badges::ci_display(&CiStatus::Failed, false);
                parts.push(Span::styled(
                    format!("{} {glyph} {word}", t.red_ci),
                    Style::default().fg(color),
                ));
            }
        }
    }
    let mut spans = Vec::new();
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(colors::SURFACE)));
        }
        spans.push(part);
    }
    spans
}

const TOTALS_GAP: &str = "   ";

fn label_span(digest: &Digest, width: usize) -> Span<'static> {
    Span::styled(
        truncate(&digest.label, width),
        Style::default()
            .fg(colors::TEXT)
            .add_modifier(Modifier::BOLD),
    )
}

/// The totals line as drawn `width` wide: the window as repoz heads it,
/// then the sums. One line when it fits, else the window on one and the
/// sums under it, so a narrow pane keeps the sums.
pub fn totals_rows(digest: &Digest, width: usize) -> Vec<Line<'static>> {
    let sums = sums_spans(digest);
    if digest.label.chars().count() + TOTALS_GAP.len() + width_of(&sums) <= width {
        let mut spans = vec![label_span(digest, width), Span::raw(TOTALS_GAP)];
        spans.extend(sums);
        return vec![Line::from(spans)];
    }
    vec![Line::from(label_span(digest, width)), Line::from(sums)]
}

fn width_of(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// The column widths every line of one digest shares: the repo row, then
/// the digest's own slots, each padded to its widest so every slot starts
/// in one column down the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    /// The shared repo row (`detail::repo_row_spans`), as pane 1 draws it.
    pub row: usize,
    /// The commit count, or the `pushed` and `reading` words.
    pub count: usize,
    /// The authors' initials, on a line with more than one.
    pub authors: usize,
}

impl Columns {
    pub fn of(digest: &Digest, width: usize) -> Self {
        let widest = |f: &dyn Fn(&DigestLine) -> String| {
            digest
                .lines
                .iter()
                .map(|l| f(l).chars().count())
                .max()
                .unwrap_or(0)
        };
        Self {
            row: crate::ui::detail::REPO_ROW_MAX_W.min(width.saturating_sub(DIGEST_SLOT_MIN)),
            count: widest(&count_text),
            authors: widest(&authors_text),
        }
    }
}

/// The digest's first slot: what moved in the window.
fn count_text(line: &DigestLine) -> String {
    match &line.body {
        LineBody::Counted { commits } => plural(commits.len(), "commit", "commits"),
        LineBody::PushedOnly => match line.pushed_at {
            Some(at) => format!("pushed {}", format_age(at)),
            None => "pushed".to_string(),
        },
        LineBody::Reading => "reading the log…".to_string(),
        // The worklist is the row's own counters.
        LineBody::Worklist { .. } => String::new(),
    }
}

/// The digest's second slot: the authors' initials when more than one
/// touched the repo.
fn authors_text(line: &DigestLine) -> String {
    let authors = line.authors();
    if authors.len() < 2 {
        return String::new();
    }
    let codes: Vec<String> = authors.iter().map(|a| crate::ui::author_code(a)).collect();
    codes.join(", ")
}

/// What a line says after its name, in the inspector's heading.
fn body_text(line: &DigestLine) -> String {
    match &line.body {
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
        _ => {
            let authors = authors_text(line);
            if authors.is_empty() {
                count_text(line)
            } else {
                format!("{} by {authors}", count_text(line))
            }
        }
    }
}

/// One digest line, `width` columns wide, in pane 1's row grammar: the
/// repo's row as the repo list draws it (marker, kind glyph, name, dots, and
/// the badges in their places: CI lamp, PRs, stash, worktrees, sync status
/// flush right), then the digest's own slots in a fixed order, dimmed as
/// pane 1 dims secondary text: the count, the authors, the newest subject.
/// Drawn with the fleet poll's lamp; `render_pane` draws `line_spans_ci`
/// (FZ-83), so this one is kept for the tests.
#[cfg(test)]
pub fn line_spans(
    line: &DigestLine,
    repo: &crate::model::RepoStatus,
    selected: bool,
    columns: Columns,
    width: usize,
    dim_pending: bool,
    not_in_last_poll: bool,
) -> Vec<Span<'static>> {
    let ci = crate::ui::badges::RowCi {
        status: &repo.ci_status,
        side_run: false,
    };
    line_spans_ci(
        line,
        repo,
        ci,
        selected,
        columns,
        width,
        dim_pending,
        not_in_last_poll,
    )
}

/// `line_spans` with the row's CI part as `ci` says (FZ-83): `render_pane`
/// passes `App::ci_shown` and `App::ci_side_run`, as pane 1's list does.
#[allow(clippy::too_many_arguments)]
pub fn line_spans_ci(
    line: &DigestLine,
    repo: &crate::model::RepoStatus,
    ci: crate::ui::badges::RowCi,
    selected: bool,
    columns: Columns,
    width: usize,
    dim_pending: bool,
    not_in_last_poll: bool,
) -> Vec<Span<'static>> {
    let mut spans = crate::ui::detail::repo_row_spans_ci(
        repo,
        ci,
        selected,
        columns.row,
        dim_pending,
        not_in_last_poll,
        None,
    );
    let mut room = width.saturating_sub(width_of(&spans));
    let mut slot = |spans: &mut Vec<Span<'static>>, text: String, w: usize, color: Color| {
        if room <= 2 || w == 0 {
            return;
        }
        let cell = truncate(&format!("{text:<w$}"), room - 2);
        room = room.saturating_sub(2 + cell.chars().count());
        spans.push(Span::raw("  "));
        spans.push(Span::styled(cell, Style::default().fg(color)));
    };
    let count_color = match line.body {
        LineBody::Counted { .. } => colors::TEXT,
        _ => colors::OVERLAY,
    };
    slot(&mut spans, count_text(line), columns.count, count_color);
    slot(
        &mut spans,
        authors_text(line),
        columns.authors,
        colors::OVERLAY,
    );
    if let LineBody::Counted { commits } = &line.body
        && let Some(newest) = commits.first()
    {
        let w = newest.message.chars().count();
        slot(&mut spans, newest.message.clone(), w, colors::OVERLAY);
    }
    while spans
        .last()
        .is_some_and(|s| !s.content.is_empty() && s.content.trim().is_empty())
    {
        spans.pop();
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

    let mut lines = totals_rows(digest, width);
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
    let columns = Columns::of(digest, width);
    let dim_pending = app.watch_mode && !app.blink_on;
    let body_rows = (inner.height as usize).saturating_sub(lines.len()).max(1);
    let (first, shown, hidden) = visible_lines(digest.lines.len(), selected, body_rows);
    for (i, line) in digest.lines.iter().enumerate().skip(first).take(shown) {
        let repo = &app.repos[line.repo];
        let shown = app.ci_shown(repo);
        let ci = crate::ui::badges::RowCi {
            status: &shown.status,
            side_run: app.ci_side_run(repo),
        };
        let mut spans = line_spans_ci(
            line,
            repo,
            ci,
            i == selected,
            columns,
            width,
            dim_pending,
            app.is_not_in_last_poll(repo),
        );
        // The cursor's band runs to the pane's edge, past the slots (FZ-66).
        if i == selected {
            let pad = width.saturating_sub(width_of(&spans));
            spans.push(Span::raw(" ".repeat(pad)));
            spans = crate::ui::detail::wash(spans);
        }
        lines.push(Line::from(spans));
    }
    if hidden > 0 {
        lines.push(crate::ui::detail::more_line(hidden));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Which of `total` digest lines pane 2 draws in `rows` rows, keeping the
/// cursor's in view: the first, how many, and how many below are left out.
/// When some are, the last row is the panes' overflow marker instead.
pub fn visible_lines(total: usize, selected: usize, rows: usize) -> (usize, usize, usize) {
    if total <= rows {
        return (0, total, 0);
    }
    if rows <= 1 {
        // Room for the marker alone: it stays inside the pane.
        return (0, 0, total);
    }
    let room = rows - 1;
    let first = (selected + 1).saturating_sub(room);
    let below = total - first - room.min(total - first);
    if below == 0 {
        // The cursor is in the last lines: they fill the pane, no marker.
        let first = total - rows;
        return (first, rows, 0);
    }
    (first, room, below)
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

/// The inspector's facts about one digest line, everything but its list.
/// The heading and the CI run always draw; the PR titles are a list of
/// their own that gives way to them, marked with how many PRs it left out.
/// Never scrolled.
pub struct Facts {
    /// The heading, why a line has no count, or the worklist's counts.
    pub head: Vec<Line<'static>>,
    /// The "pull requests" label and one entry per PR.
    pub prs: Option<(Line<'static>, Vec<Line<'static>>)>,
    /// The CI run.
    pub ci: Option<Line<'static>>,
}

impl Facts {
    /// Rows the facts take, everything shown, `width` wide.
    fn rows_wanted(&self, width: usize) -> usize {
        let prs = self
            .prs
            .as_ref()
            .map_or(0, |(_, e)| 2 + wrapped_rows(e, width));
        let ci = self
            .ci
            .as_ref()
            .map_or(0, |c| 1 + wrapped_rows(std::slice::from_ref(c), width));
        wrapped_rows(&self.head, width) + prs + ci
    }

    /// The facts in at most `rows` rows `width` wide: the heading first and
    /// the CI run last, both whole; the PR titles in what is left, ending
    /// in the overflow marker counting PRs when they do not all fit.
    pub fn fit(&self, width: usize, rows: usize) -> Vec<Line<'static>> {
        let wrap = |l: &Line<'static>| crate::ui::detail::wrap_line(l, width.max(1));
        let head: Vec<Line> = self.head.iter().flat_map(wrap).collect();
        let ci: Vec<Line> = self
            .ci
            .iter()
            .flat_map(|c| std::iter::once(Line::from("")).chain(wrap(c)))
            .collect();
        let mut out = head;
        if let Some((title, entries)) = &self.prs {
            let left = rows.saturating_sub(out.len() + ci.len());
            // A blank row before the label only when the list still keeps a
            // row of its own beside the label.
            if left >= 3 {
                out.push(Line::from(""));
            }
            if left >= 2 {
                out.push(title.clone());
                let room = rows.saturating_sub(out.len() + ci.len());
                out.extend(fit_list(entries, 0, width, room));
            }
        }
        out.extend(ci);
        out
    }
}

/// The inspector's facts about one digest line (`Facts`).
pub fn facts(app: &App, line: &DigestLine) -> Facts {
    let repo = &app.repos[line.repo];
    let mut out = vec![Line::from(vec![
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
    ])];
    match &line.body {
        LineBody::Counted { .. } => {}
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
            return Facts {
                head: out,
                prs: None,
                ci: None,
            };
        }
    }
    let prs = (line.prs > 0).then(|| {
        let mut entries: Vec<Line<'static>> = repo
            .prs
            .iter()
            .map(|pr| {
                Line::from(vec![
                    Span::styled(
                        format!("#{} ", pr.number),
                        Style::default().fg(colors::PR_CLR),
                    ),
                    Span::styled(pr.title.clone(), Style::default().fg(colors::TEXT)),
                    Span::styled(
                        format!("  {}", pr.author),
                        Style::default().fg(colors::OVERLAY),
                    ),
                ])
            })
            .collect();
        if entries.is_empty() {
            entries.push(dim(format!(
                "{} open; titles with -p",
                plural(line.prs as usize, "PR", "PRs")
            )));
        }
        (label("pull requests"), entries)
    });
    // The card's verdict is the row's lamp, `App::ci_shown` (FZ-83).
    let shown = app.ci_shown(repo).status;
    let ci = (shown != CiStatus::Unknown).then(|| {
        let (glyph, color, word) = badges::ci_display(&shown, false);
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
        Line::from(spans)
    });
    Facts { head: out, prs, ci }
}

/// The inspector's list for one digest line, its label and one entry per
/// item: the commits in the window with author and time, or the worklist's
/// stashes. The only part of the inspector that may overflow.
pub fn list_entries(
    app: &App,
    digest: &Digest,
    line: &DigestLine,
) -> Option<(Line<'static>, Vec<Line<'static>>)> {
    match &line.body {
        LineBody::Counted { commits } => Some((
            label(&format!("commits {}", digest.label)),
            commits
                .iter()
                .map(|c| {
                    let age = c.date.map(format_age).unwrap_or_default();
                    Line::from(vec![
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
                    ])
                })
                .collect(),
        )),
        LineBody::Worklist { .. } => {
            let stashes = &app.repos[line.repo].stash_entries;
            (!stashes.is_empty()).then(|| {
                (
                    label("stashes"),
                    stashes.iter().map(|e| dim(e.clone())).collect(),
                )
            })
        }
        LineBody::PushedOnly | LineBody::Reading => None,
    }
}

/// `entries` from `offset`, each wrapped to `width`, in `rows` rows. When
/// they do not all fit, the last row says how many were left out, with the
/// panes' one overflow marker (`detail::more_line`).
pub fn fit_list(
    entries: &[Line<'static>],
    offset: usize,
    width: usize,
    rows: usize,
) -> Vec<Line<'static>> {
    let wrapped: Vec<Vec<Line<'static>>> = entries
        .iter()
        .skip(offset)
        .map(|e| crate::ui::detail::wrap_line(e, width.max(1)))
        .collect();
    if wrapped.iter().map(Vec::len).sum::<usize>() <= rows {
        return wrapped.into_iter().flatten().collect();
    }
    let room = rows.saturating_sub(1);
    let mut out = Vec::new();
    let mut shown = 0;
    for entry in &wrapped {
        if out.len() + entry.len() > room {
            break;
        }
        out.extend(entry.iter().cloned());
        shown += 1;
    }
    if rows > 0 {
        out.push(crate::ui::detail::more_line(wrapped.len() - shown));
    }
    out
}

/// The inspector on a section header: the detail of pane 2's digest line,
/// laid across the inspector's whole width. Wide, the facts take the left
/// lane and the list the right (the split the commit inspector uses at
/// `detail::INSPECTOR_TWO_LANE_WIDTH`); narrow, the list stacks under the
/// facts. The facts never scroll; the list starts at `inspector_scroll` and
/// says what it left out.
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
    let Some(line) = digest.lines.get(
        app.digest_selected
            .min(digest.lines.len().saturating_sub(1)),
    ) else {
        app.inspector_max_scroll.set(0);
        frame.render_widget(
            Paragraph::new(dim(format!("nothing moved {}", digest.label))),
            inner,
        );
        return;
    };
    let facts = facts(app, line);
    let list = list_entries(app, digest, line);
    let entries = list.as_ref().map_or(0, |(_, e)| e.len());
    app.inspector_max_scroll
        .set(u16::try_from(entries.saturating_sub(1)).unwrap_or(u16::MAX));
    let offset = usize::from(app.inspector_scroll).min(entries.saturating_sub(1));

    let (facts_area, list_area) = match list {
        Some(_) if inner.width >= crate::ui::detail::INSPECTOR_TWO_LANE_WIDTH => {
            let h = Layout::horizontal([Constraint::Percentage(50), Constraint::Min(0)])
                .spacing(2)
                .split(inner);
            (h[0], Some(h[1]))
        }
        Some(_) => {
            // The facts and a blank row, but the list below keeps at least
            // its title and the marker row.
            let wanted = facts.rows_wanted(inner.width as usize) + 1;
            let height = wanted.min((inner.height as usize).saturating_sub(LIST_MIN_ROWS));
            let v = Layout::vertical([Constraint::Length(height as u16), Constraint::Min(0)])
                .split(inner);
            (v[0], Some(v[1]))
        }
        None => (inner, None),
    };
    frame.render_widget(
        Paragraph::new(facts.fit(facts_area.width as usize, facts_area.height as usize)),
        facts_area,
    );
    if let (Some(area), Some((title, entries))) = (list_area, list) {
        let mut rows = vec![title];
        rows.extend(fit_list(
            &entries,
            offset,
            area.width as usize,
            (area.height as usize).saturating_sub(1),
        ));
        frame.render_widget(Paragraph::new(rows), area);
    }
}

/// Rows a stacked list never gives up to the facts above it: its title and
/// the overflow marker.
const LIST_MIN_ROWS: usize = 2;

fn wrapped_rows(lines: &[Line<'_>], width: usize) -> usize {
    lines
        .iter()
        .map(|l| crate::ui::detail::wrap_line(l, width.max(1)).len())
        .sum()
}

#[cfg(test)]
mod tests;
