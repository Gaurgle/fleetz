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
        app.github_freshness.data_time =
            crate::app::DataTime::At(now - chrono::Duration::minutes(1));
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
        app.github_freshness.data_time =
            crate::app::DataTime::At(now - chrono::Duration::minutes(1));
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
}
