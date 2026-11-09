//! A todoz board in the `notes` reader (FZ-63): a `TODO.md` parsed into its
//! sections, tri-state tasks, subtasks, tag flags and due dates, and drawn
//! the way `todoz` draws its rows. Read-only: nothing here writes a board.
//!
//! Ported from `Gaurgle/notez`, not depended on (the FZ-53 ruling): the
//! parser from `notez-core/src/todo/mod.rs` (`split_checkbox_line`,
//! `strip_due`) and `notez-core/src/tags.rs` (the five flags), the drawing
//! from `notez-cli/src/tui/todo_rows.rs`. Colours are fleetz's own tokens
//! (`colors.rs`); todoz's two off-palette shades of the unchecked bracket
//! for depths 1 and 2 are not carried over, the tree drawing already shows
//! the depth.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::colors;

/// The five tag flags in todoz's slot order, each with its hashtag and the
/// colour of its dot: important, prio, longterm, idea, blocked.
pub const FLAGS: [(&str, Color); 5] = [
    ("important", colors::RED),
    ("prio", colors::PEACH),
    ("longterm", colors::YELLOW),
    ("idea", colors::SAPPHIRE),
    ("blocked", colors::MAUVE),
];

/// A task's checkbox: `- [ ]`, `- [/]` or `- [x]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Unchecked,
    Half,
    Checked,
}

/// One line of a board.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    /// A markdown heading, its text without the `#`s: a board section.
    Heading(String),
    /// A checkbox line. `depth` is 0, 1 (two spaces) or 2 (four spaces);
    /// `flags` has bit `i` set for `FLAGS[i]`; `due` is the `@` token's date.
    Task {
        depth: u8,
        state: CheckState,
        text: String,
        flags: u8,
        due: Option<String>,
    },
    /// Anything else, shown as it is.
    Plain(String),
}

/// A parsed `TODO.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    items: Vec<Item>,
    /// Per item, the row it hangs under (`parent_of`).
    parents: Vec<Option<usize>>,
}

/// todoz's tree drawing (`theme::TREE_GLYPHS`).
const SECTION_OPEN: &str = "▼ ";
const ANCESTOR_BAR: &str = "│ ";
const ANCESTOR_BLANK: &str = "  ";
const BRANCH: &str = "├─";
const LAST_BRANCH: &str = "└─";
const FOLDER_OPEN: &str = "▾ ";
const LEAF: &str = "  ";

/// The dot field's width: a gutter column, five dots, a space.
const DOT_FIELD_COLS: usize = 7;
/// `[x] `.
const CHECKBOX_COLS: usize = 4;

/// Split one checkbox line into `(depth, state, text after the checkbox)`,
/// as todoz does; `None` for any other line.
fn split_checkbox_line(line: &str) -> Option<(u8, CheckState, &str)> {
    let depth = if line.starts_with("    - ") || line.starts_with("    -\t") {
        2
    } else if line.starts_with("  - ") || line.starts_with("  -\t") {
        1
    } else {
        0
    };
    let trimmed = line.trim();
    let state = if trimmed.starts_with("- [ ] ") {
        CheckState::Unchecked
    } else if trimmed.starts_with("- [/] ") {
        CheckState::Half
    } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
        CheckState::Checked
    } else {
        return None;
    };
    Some((depth, state, &trimmed[6..]))
}

/// Whether `word` is a due token's date: `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`.
fn is_due(word: &str) -> bool {
    match word.split_once('T') {
        Some((day, time)) => {
            chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok()
                && chrono::NaiveTime::parse_from_str(time, "%H:%M").is_ok()
        }
        None => word.len() == 10 && chrono::NaiveDate::parse_from_str(word, "%Y-%m-%d").is_ok(),
    }
}

/// Strip the due token from `text`: the last whitespace-delimited word that
/// is `@` and a date. Without one the text is returned unchanged.
fn strip_due(text: &str) -> (String, Option<String>) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let found = words.iter().enumerate().rev().find_map(|(i, w)| {
        let date = w.strip_prefix('@')?;
        is_due(date).then(|| (i, date.to_string()))
    });
    let Some((index, date)) = found else {
        return (text.to_string(), None);
    };
    let kept: Vec<&str> = words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| (i != index).then_some(*w))
        .collect();
    (kept.join(" "), Some(date))
}

/// Strip the `#tag` flags from `text`, case-insensitive, into a bit set.
fn parse_flags(text: &str) -> (String, u8) {
    let mut flags = 0u8;
    let mut kept: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        let found = word.strip_prefix('#').and_then(|rest| {
            FLAGS
                .iter()
                .position(|(key, _)| key.eq_ignore_ascii_case(rest))
        });
        match found {
            Some(i) => flags |= 1 << i,
            None => kept.push(word),
        }
    }
    (kept.join(" "), flags)
}

/// A markdown heading's text, `None` for any other line.
fn heading(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#');
    let level = line.len() - rest.len();
    ((1..=6).contains(&level) && rest.starts_with(' ')).then(|| rest.trim())
}

impl Board {
    /// Parse a board from the body the reader already holds.
    pub fn parse(text: &str) -> Self {
        let items = text
            .lines()
            .map(|line| {
                if let Some((depth, state, raw)) = split_checkbox_line(line) {
                    let (without_due, due) = strip_due(raw);
                    let (text, flags) = parse_flags(&without_due);
                    Item::Task {
                        depth,
                        state,
                        text,
                        flags,
                        due,
                    }
                } else if let Some(text) = heading(line) {
                    Item::Heading(text.to_string())
                } else {
                    Item::Plain(line.to_string())
                }
            })
            .collect::<Vec<_>>();
        let parents = Self::parents(&items);
        Self { items, parents }
    }

    /// The tasks still to do: unchecked and half done, as the border counts.
    pub fn open(&self) -> usize {
        self.items
            .iter()
            .filter(|i| {
                matches!(
                    i,
                    Item::Task {
                        state: CheckState::Unchecked | CheckState::Half,
                        ..
                    }
                )
            })
            .count()
    }

    fn depth(&self, idx: usize) -> Option<u8> {
        match self.items[idx] {
            Item::Task { depth, .. } => Some(depth),
            _ => None,
        }
    }

    /// The row a task hangs under: the nearest earlier task one level up,
    /// or its section heading. Plain lines are not in the tree.
    fn parent_of(&self, idx: usize) -> Option<usize> {
        self.parents[idx]
    }

    /// `parent_of` for every item in one pass: the nearest earlier heading
    /// or shallower task is the latest of the last heading and the last
    /// task at each shallower depth. A backwards scan per task was
    /// quadratic: 53 ms for a capped board of 6929 tasks in one section.
    fn parents(items: &[Item]) -> Vec<Option<usize>> {
        let mut heading = None;
        let mut last_at: [Option<usize>; 3] = [None; 3];
        items
            .iter()
            .enumerate()
            .map(|(i, item)| match item {
                Item::Heading(_) => {
                    heading = Some(i);
                    None
                }
                Item::Task { depth, .. } => {
                    let d = usize::from(*depth).min(2);
                    let parent = last_at[..d]
                        .iter()
                        .copied()
                        .chain([heading])
                        .max()
                        .flatten();
                    last_at[d] = Some(i);
                    parent
                }
                Item::Plain(_) => None,
            })
            .collect()
    }

    /// Per item, whether a later task shares its parent: it draws `├─` and
    /// its descendants a bar at its level.
    fn later_siblings(&self) -> Vec<bool> {
        let mut later = vec![false; self.items.len()];
        let mut seen = std::collections::HashSet::new();
        for i in (0..self.items.len()).rev() {
            if self.depth(i).is_some() {
                later[i] = !seen.insert(self.parent_of(i));
            }
        }
        later
    }

    /// The ancestor columns of task `idx`, outermost first.
    fn ancestor_levels(&self, idx: usize, later: &[bool]) -> String {
        let mut levels = Vec::new();
        let mut up = self.parent_of(idx);
        while let Some(p) = up.filter(|&p| self.depth(p).is_some()) {
            levels.push(if later[p] {
                ANCESTOR_BAR
            } else {
                ANCESTOR_BLANK
            });
            up = self.parent_of(p);
        }
        levels.reverse();
        levels.concat()
    }

    /// Whether the next task after `idx` hangs under it.
    fn has_child(&self, idx: usize) -> bool {
        self.items[idx + 1..]
            .iter()
            .position(|i| !matches!(i, Item::Plain(_)))
            .is_some_and(|off| self.parent_of(idx + 1 + off) == Some(idx))
    }

    /// The board's lines for a reader `width` columns wide: each heading
    /// as a section, each task with its tag dots, tree drawing, checkbox,
    /// text and due date, a long one wrapped under its text; other lines
    /// as they are.
    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let later = self.later_siblings();
        let branch_style = Style::default().fg(colors::SURFACE);
        let mut out = Vec::with_capacity(self.items.len());
        for (idx, item) in self.items.iter().enumerate() {
            match item {
                Item::Plain(text) => out.push(Line::from(Span::styled(
                    text.clone(),
                    Style::default().fg(colors::TEXT),
                ))),
                Item::Heading(text) => out.push(Line::from(vec![
                    Span::raw(" ".repeat(DOT_FIELD_COLS)),
                    Span::styled(SECTION_OPEN, branch_style),
                    Span::styled(
                        text.clone(),
                        Style::default()
                            .fg(colors::MAUVE)
                            .add_modifier(Modifier::BOLD),
                    ),
                ])),
                Item::Task {
                    state,
                    text,
                    flags,
                    due,
                    ..
                } => {
                    let has_child = self.has_child(idx);
                    let ancestors = self.ancestor_levels(idx, &later);
                    let mut branch = ancestors.clone();
                    branch.push_str(if later[idx] { BRANCH } else { LAST_BRANCH });
                    branch.push_str(if has_child { FOLDER_OPEN } else { LEAF });
                    let mut cont = ancestors;
                    cont.push_str(if later[idx] {
                        ANCESTOR_BAR
                    } else {
                        ANCESTOR_BLANK
                    });
                    cont.push_str(if has_child {
                        ANCESTOR_BAR
                    } else {
                        ANCESTOR_BLANK
                    });
                    let (mark, mark_color, bracket_color, text_color) = state_colors(*state);
                    let mut first = flag_dots(*flags);
                    first.push(Span::styled(branch.clone(), branch_style));
                    first.push(Span::styled("[", Style::default().fg(bracket_color)));
                    first.push(Span::styled(mark, Style::default().fg(mark_color)));
                    first.push(Span::styled("] ", Style::default().fg(bracket_color)));
                    let lead = DOT_FIELD_COLS + branch.chars().count() + CHECKBOX_COLS;
                    let text_width = width.saturating_sub(lead).max(1);
                    let mut body = text.clone();
                    if let Some(date) = due {
                        body.push_str(&format!(" @{date}"));
                    }
                    let due_from = due.as_ref().map(|_| text.chars().count());
                    let chunks = wrap_words(&body, text_width);
                    let mut consumed = 0;
                    for (n, chunk) in chunks.into_iter().enumerate() {
                        let mut spans = match n {
                            0 => std::mem::take(&mut first),
                            _ => vec![
                                Span::raw(" ".repeat(DOT_FIELD_COLS)),
                                Span::styled(cont.clone(), branch_style),
                                Span::raw(" ".repeat(CHECKBOX_COLS)),
                            ],
                        };
                        spans.extend(text_spans(&chunk.0, consumed, due_from, text_color, *state));
                        consumed = chunk.1;
                        out.push(Line::from(spans));
                    }
                }
            }
        }
        out
    }
}

/// The mark, its colour, the bracket colour and the text colour of a state,
/// as todoz draws them.
fn state_colors(state: CheckState) -> (&'static str, Color, Color, Color) {
    match state {
        CheckState::Checked => ("x", colors::SAPPHIRE, colors::OVERLAY, colors::OVERLAY),
        CheckState::Half => ("/", colors::YELLOW, colors::OVERLAY, colors::SUBTEXT),
        CheckState::Unchecked => (" ", colors::SURFACE, colors::SAPPHIRE, colors::TEXT),
    }
}

/// The due date's colour: dimmed once the task is checked.
fn due_color(state: CheckState) -> Color {
    match state {
        CheckState::Checked => colors::OVERLAY,
        _ => colors::PEACH,
    }
}

/// The dot field: a gutter column, a dot per flag in its colour or a dim
/// `·` where unset, and a space.
fn flag_dots(flags: u8) -> Vec<Span<'static>> {
    let mut spans = vec![Span::raw(" ")];
    for (i, (_, color)) in FLAGS.iter().enumerate() {
        spans.push(match flags & (1 << i) != 0 {
            true => Span::styled("●", Style::default().fg(*color)),
            false => Span::styled("·", Style::default().fg(colors::SURFACE)),
        });
    }
    spans.push(Span::raw(" "));
    spans
}

/// One wrapped chunk of a task's text, split into the text and due date
/// parts by character offset: `start` is the chunk's offset in the whole,
/// `due_from` where the ` @date` starts.
fn text_spans(
    chunk: &str,
    start: usize,
    due_from: Option<usize>,
    text_color: Color,
    state: CheckState,
) -> Vec<Span<'static>> {
    let text_style = Style::default().fg(text_color);
    let due_style = Style::default().fg(due_color(state));
    let len = chunk.chars().count();
    match due_from {
        Some(at) if at < start + len => {
            let split = at.saturating_sub(start);
            let byte = chunk
                .char_indices()
                .nth(split)
                .map_or(chunk.len(), |(i, _)| i);
            let (text, date) = chunk.split_at(byte);
            [
                Span::styled(text.to_string(), text_style),
                Span::styled(date.to_string(), due_style),
            ]
            .into_iter()
            .filter(|s| !s.content.is_empty())
            .collect()
        }
        _ => vec![Span::styled(chunk.to_string(), text_style)],
    }
}

/// Wrap `text` at spaces into chunks of at most `width` characters, a word
/// longer than that cut where it must. Each chunk comes with the character
/// offset in `text` just past it, so styles can follow the cuts. The space
/// a line breaks at is kept at the end of the earlier chunk.
fn wrap_words(text: &str, width: usize) -> Vec<(String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + width).min(chars.len());
        if end < chars.len()
            && let Some(space) = chars[start..end].iter().rposition(|c| *c == ' ')
        {
            end = start + space + 1;
        }
        out.push((chars[start..end].iter().collect(), end));
        start = end;
    }
    if out.is_empty() {
        out.push((String::new(), 0));
    }
    out
}

/// A board with every part: sections, the three states, subtasks two levels
/// deep, every tag, a due date and prose. Shared with the reader's test.
#[cfg(test)]
pub(crate) const FIXTURE: &str = "\
# Board
Loose prose line.
## Now
- [ ] ship the reader #prio @2026-11-01
  - [/] parse it #important
    - [x] split lines #idea @2026-10-30
  - [ ] draw it #longterm #blocked
- [x] done thing
## Later
- [/] half way
";

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn a_checkbox_line_splits_into_depth_state_and_text() {
        assert_eq!(
            split_checkbox_line("- [ ] a"),
            Some((0, CheckState::Unchecked, "a"))
        );
        assert_eq!(
            split_checkbox_line("  - [/] b"),
            Some((1, CheckState::Half, "b"))
        );
        assert_eq!(
            split_checkbox_line("    - [X] c"),
            Some((2, CheckState::Checked, "c"))
        );
        assert_eq!(split_checkbox_line("- a list item"), None);
        assert_eq!(split_checkbox_line("## heading"), None);
    }

    #[test]
    fn the_due_token_and_tags_are_stripped_from_the_text() {
        assert_eq!(
            strip_due("pay @2026-11-01 rent"),
            ("pay rent".to_string(), Some("2026-11-01".to_string()))
        );
        assert_eq!(
            strip_due("call @2026-11-01T09:30"),
            ("call".to_string(), Some("2026-11-01T09:30".to_string()))
        );
        assert_eq!(strip_due("mail @bob"), ("mail @bob".to_string(), None));
        assert_eq!(
            strip_due("on @2026-13-01"),
            ("on @2026-13-01".to_string(), None)
        );
        assert_eq!(
            parse_flags("fix it #PRIO #blocked #other"),
            ("fix it #other".to_string(), 0b10010)
        );
    }

    #[test]
    fn the_open_count_is_unchecked_and_half_tasks() {
        assert_eq!(Board::parse(FIXTURE).open(), 4);
        assert_eq!(Board::parse("- [x] a\nprose").open(), 0);
    }

    /// The rows todoz draws: the tree under each section, a parent's fold
    /// mark, the dots in slot order, and the due date after the text.
    #[test]
    fn a_board_draws_todoz_rows() {
        let rows = plain(&Board::parse(FIXTURE).lines(80));
        assert_eq!(
            rows,
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

    /// A long task wraps under its text, the tree's bars carried down.
    #[test]
    fn a_long_task_wraps_under_its_text() {
        let board = Board::parse("## S\n- [ ] alpha beta gamma delta\n- [ ] next");
        let rows = plain(&board.lines(28));
        assert_eq!(
            rows,
            [
                "       ▼ S",
                " ····· ├─  [ ] alpha beta ",
                "       │       gamma delta",
                " ····· └─  [ ] next",
            ]
        );
    }

    /// No new ladder: every colour a board draws, checkbox and tag dots
    /// included, is a `colors.rs` token, and the flags wear the colours
    /// todoz gives them.
    #[test]
    fn every_board_colour_is_a_colors_token() {
        let tokens = [
            colors::TEXT,
            colors::SUBTEXT,
            colors::OVERLAY,
            colors::SURFACE,
            colors::SAPPHIRE,
            colors::YELLOW,
            colors::PEACH,
            colors::RED,
            colors::MAUVE,
        ];
        let lines = Board::parse(FIXTURE).lines(80);
        for span in lines.iter().flat_map(|l| &l.spans) {
            if let Some(fg) = span.style.fg {
                assert!(tokens.contains(&fg), "{fg:?} on {:?}", span.content);
            }
        }
        assert_eq!(
            FLAGS.map(|(_, c)| c),
            [
                colors::RED,
                colors::PEACH,
                colors::YELLOW,
                colors::SAPPHIRE,
                colors::MAUVE
            ]
        );
        assert_eq!(
            state_colors(CheckState::Unchecked),
            (" ", colors::SURFACE, colors::SAPPHIRE, colors::TEXT)
        );
        assert_eq!(
            state_colors(CheckState::Half),
            ("/", colors::YELLOW, colors::OVERLAY, colors::SUBTEXT)
        );
        assert_eq!(
            state_colors(CheckState::Checked),
            ("x", colors::SAPPHIRE, colors::OVERLAY, colors::OVERLAY)
        );
    }

    /// The due date is its own colour, dimmed once the task is checked.
    #[test]
    fn a_due_date_dims_when_checked() {
        let lines = Board::parse("- [ ] a @2026-11-01\n- [x] b @2026-11-01").lines(80);
        let due = |l: &Line| l.spans.last().and_then(|s| s.style.fg);
        assert_eq!(due(&lines[0]), Some(colors::PEACH));
        assert_eq!(due(&lines[1]), Some(colors::OVERLAY));
    }
}
