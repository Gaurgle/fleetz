//! Markdown rendering for the `notes` reader (FZ-59).
//!
//! [`render_markdown`] turns a note's markdown into ratatui lines that are
//! already wrapped to the reader's width. The reader draws them without
//! wrapping them again, so `lines.len()` is the exact row count its scroll
//! clamp and `H`/`L` paging count against.
//!
//! Ported from notez (`crates/notez-cli/src/tui/markdown.rs`), its
//! tree-sitter highlighting included (FZ-71): a fence whose tag names a
//! language in `highlight.rs` is highlighted, any other fence is its lines
//! in the code style. Parsing is `pulldown-cmark` with tables, footnotes, strikethrough and
//! task lists on. Nothing is rendered as HTML: raw HTML is shown as its
//! source text. Every colour is a `colors.rs` token; this file adds no
//! ladder of its own.

use std::borrow::Cow;
use std::collections::HashMap;

use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::colors;
use super::highlight::{Language, MAX_HIGHLIGHT_BYTES, highlight_lines, language_available};

/// What a tab becomes in rendered text. Terminal width of a raw tab is
/// undefined, so it is expanded before measuring.
const TAB: &str = "    ";
/// The indent in front of every code block line.
const CODE_INDENT: &str = "  ";
const QUOTE_BAR: &str = "▎ ";
const BULLET: &str = "• ";
/// Where a table line wider than the reader is cut.
const CUT_MARK: &str = "…";
/// Between two table cells; [`TABLE_JOINT`] is its header rule crossing.
const TABLE_SEPARATOR: &str = " │ ";
const TABLE_JOINT: &str = "─┼─";
/// A table column is shrunk no narrower than this before the table clips.
const MIN_COLUMN_WIDTH: usize = 3;
/// The rule above the footnote definitions, at most this wide.
const FOOTNOTE_RULE_WIDTH: usize = 10;

/// Body text, the colour the raw reader draws in.
fn text_style() -> Style {
    Style::default().fg(colors::TEXT)
}

/// A heading, as notez draws it: level 1 bold mauve, level 2 bold sapphire,
/// deeper levels bold in the text colour.
pub fn heading_style(level: u8) -> Style {
    let style = Style::default().add_modifier(Modifier::BOLD);
    match level {
        1 => style.fg(colors::MAUVE),
        2 => style.fg(colors::SAPPHIRE),
        _ => style.fg(colors::TEXT),
    }
}

/// Inline code and fenced code: peach, as notez's `theme::code`, monospace
/// being the terminal's own.
pub fn code_style() -> Style {
    Style::default().fg(colors::PEACH)
}

/// The base style of uncaptured text in a highlighted code block: plain
/// text, not [`code_style`], whose colour [`syntax_string`] shares.
fn highlighted_code_base() -> Style {
    text_style()
}

// Syntax highlighting, one style per capture in `highlight.rs`, in notez's
// colours.

/// Keywords (`fn`, `def`, `if`, `return`).
pub fn syntax_keyword() -> Style {
    Style::default().fg(colors::MAUVE)
}

/// Function and method names, macros.
pub fn syntax_function() -> Style {
    Style::default()
        .fg(colors::SAPPHIRE)
        .add_modifier(Modifier::BOLD)
}

/// Type names, built-in types.
pub fn syntax_type() -> Style {
    Style::default().fg(colors::YELLOW)
}

/// String literals, escapes, JSON keys.
pub fn syntax_string() -> Style {
    Style::default().fg(colors::PEACH)
}

/// Numeric literals.
pub fn syntax_number() -> Style {
    Style::default().fg(colors::RED)
}

/// Comments, including doc comments.
pub fn syntax_comment() -> Style {
    Style::default()
        .fg(colors::OVERLAY)
        .add_modifier(Modifier::ITALIC)
}

/// Constants and built-in constants (`true`, `None`, `NULL`).
pub fn syntax_constant() -> Style {
    Style::default().fg(colors::RED)
}

/// Variables and parameters.
pub fn syntax_variable() -> Style {
    Style::default().fg(colors::TEXT)
}

/// Operators.
pub fn syntax_operator() -> Style {
    Style::default().fg(colors::SUBTEXT)
}

/// Brackets, delimiters and other punctuation.
pub fn syntax_punctuation() -> Style {
    Style::default().fg(colors::OVERLAY)
}

/// Attributes and annotations (`#[derive]`, `@Override`).
pub fn syntax_attribute() -> Style {
    Style::default()
        .fg(colors::YELLOW)
        .add_modifier(Modifier::ITALIC)
}

/// Fields, properties and TOML keys.
pub fn syntax_property() -> Style {
    Style::default().fg(colors::SAPPHIRE)
}

/// A link's text, in the colour repo names are drawn in.
pub fn link_style() -> Style {
    Style::default()
        .fg(colors::SAPPHIRE)
        .add_modifier(Modifier::UNDERLINED)
}

/// A link's url, a footnote marker, an image placeholder and a fence's
/// language: shown because they must be.
fn aside_style() -> Style {
    Style::default().fg(colors::OVERLAY)
}

fn quote_style() -> Style {
    Style::default()
        .fg(colors::SUBTEXT)
        .add_modifier(Modifier::ITALIC)
}

/// Rules and table separators are structure.
fn rule_style() -> Style {
    Style::default().fg(colors::SURFACE)
}

/// Render `text` as markdown, wrapped to `width` columns.
///
/// Every returned line is at most `width` columns wide, except where a
/// single character is wider than the space left. A `width` of 0 is
/// treated as 1. Words wrap on whitespace and are only split when one word
/// is wider than the available width. Code block lines wrap by character
/// instead, keeping every space, inside the code indent. A table is laid
/// out in columns: too wide, its
/// widest columns shrink and wrap, and if even [`MIN_COLUMN_WIDTH`] per
/// column does not fit, its lines are clipped with `…`. A footnote
/// reference shows as `[n]`, numbered in order of first reference, and the
/// definitions follow the document. An empty document gives no lines.
pub fn render_markdown(text: &str, width: u16) -> Vec<Line<'static>> {
    let normalized;
    let source = if text.contains('\r') {
        normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        normalized.as_str()
    } else {
        text
    };
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let is_highlighting = source.len() <= MAX_HIGHLIGHT_BYTES;
    let mut renderer = Renderer::new(usize::from(width.max(1)), is_highlighting);
    for event in Parser::new_ext(source, options) {
        renderer.event(event);
    }
    renderer.finish()
}

/// A table's styled inline text in one cell.
type Cell<'s> = Vec<(Cow<'s, str>, Style)>;

/// A table being collected, laid out once it ends.
struct Table<'s> {
    alignments: Vec<Alignment>,
    /// The header row first, then the body rows.
    rows: Vec<Vec<Cell<'s>>>,
    row: Vec<Cell<'s>>,
}

/// Footnote state. Labels are keyed lowercase, as pulldown-cmark matches
/// them case-insensitively.
#[derive(Default)]
struct Footnotes<'s> {
    /// The definition being read: its key and its events, replayed after
    /// the document.
    open: Option<(String, Vec<Event<'s>>)>,
    /// Definitions in source order; the first one for a label wins.
    definitions: Vec<(String, Vec<Event<'s>>)>,
    definition_index: HashMap<String, usize>,
    /// Keys in numbering order: `order[n - 1]` is footnote `n`.
    order: Vec<String>,
    numbers: HashMap<String, usize>,
}

impl Footnotes<'_> {
    /// The number of footnote `key`, assigning the next one on first use.
    fn number(&mut self, key: String) -> usize {
        if let Some(&number) = self.numbers.get(&key) {
            return number;
        }
        self.order.push(key.clone());
        let number = self.order.len();
        self.numbers.insert(key, number);
        number
    }
}

/// A block that prefixes every line inside it: a block quote's bar or a
/// list item's marker. `first` goes on the item's first line, `rest` on
/// every later one; both have the same width.
struct Container {
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
    is_first_used: bool,
    width: usize,
}

struct Renderer<'s> {
    width: usize,
    lines: Vec<Line<'static>>,
    containers: Vec<Container>,
    /// The next number of each open list, `None` for a bullet list.
    lists: Vec<Option<u64>>,
    item_depth: usize,
    /// Styled inline text of the current paragraph, heading or list item.
    inline: Vec<(Cow<'s, str>, Style)>,
    styles: Vec<Style>,
    /// Open links: destination and the `inline` index where the text starts.
    links: Vec<(String, usize)>,
    image_depth: usize,
    image_alt: String,
    code: Option<String>,
    /// The open fence's language, when its tag names one.
    code_language: Option<Language>,
    /// False for a document over [`MAX_HIGHLIGHT_BYTES`].
    is_highlighting: bool,
    html: Option<String>,
    table: Option<Table<'s>>,
    footnotes: Footnotes<'s>,
    needs_blank: bool,
}

impl<'s> Renderer<'s> {
    fn new(width: usize, is_highlighting: bool) -> Self {
        Self {
            width,
            lines: Vec::new(),
            containers: Vec::new(),
            lists: Vec::new(),
            item_depth: 0,
            inline: Vec::new(),
            styles: Vec::new(),
            links: Vec::new(),
            image_depth: 0,
            image_alt: String::new(),
            code: None,
            code_language: None,
            is_highlighting,
            html: None,
            table: None,
            footnotes: Footnotes::default(),
            needs_blank: false,
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_inline();
        if let Some(code) = self.code.take() {
            self.emit_code(&code);
        }
        if let Some(html) = self.html.take() {
            self.emit_source(&html);
        }
        self.emit_footnotes();
        self.lines
    }

    fn event(&mut self, event: Event<'s>) {
        if let Some((_, events)) = self.footnotes.open.as_mut() {
            if !matches!(event, Event::End(TagEnd::FootnoteDefinition)) {
                events.push(event);
                return;
            }
            if let Some((key, events)) = self.footnotes.open.take()
                && !self.footnotes.definition_index.contains_key(&key)
            {
                let index = self.footnotes.definitions.len();
                self.footnotes.definition_index.insert(key.clone(), index);
                self.footnotes.definitions.push((key, events));
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => self.text(text),
            Event::FootnoteReference(label) => {
                let number = self.footnotes.number(label.to_lowercase());
                let reference = format!("[{number}]");
                if self.image_depth > 0 {
                    self.image_alt.push_str(&reference);
                } else {
                    self.inline.push((Cow::Owned(reference), aside_style()));
                }
            }
            Event::Html(text) => match self.html.as_mut() {
                Some(html) => html.push_str(&text),
                None => self.text(text),
            },
            Event::Code(text) => {
                if self.image_depth > 0 {
                    self.image_alt.push_str(&text);
                } else {
                    let style = self.style().patch(code_style());
                    self.inline.push((into_cow(text), style));
                }
            }
            Event::SoftBreak => self.text(CowStr::Borrowed(" ")),
            Event::HardBreak => {
                if self.image_depth > 0 {
                    self.image_alt.push(' ');
                } else {
                    self.inline.push((Cow::Borrowed("\n"), self.style()));
                }
            }
            Event::Rule => {
                self.begin_block();
                let rule = "─".repeat(self.avail());
                self.emit_line(vec![Span::styled(rule, rule_style())]);
                self.needs_blank = true;
            }
            Event::TaskListMarker(is_done) => {
                let marker = if is_done { "[x] " } else { "[ ] " };
                self.inline.push((Cow::Borrowed(marker), self.style()));
            }
        }
    }

    fn start(&mut self, tag: Tag<'s>) {
        match tag {
            Tag::Paragraph => self.begin_block(),
            Tag::Heading { level, .. } => {
                self.begin_block();
                self.push_style(heading_style(level as u8));
            }
            Tag::BlockQuote(_) => {
                self.begin_block();
                let bar = vec![Span::styled(QUOTE_BAR, quote_style())];
                self.containers.push(Container {
                    first: bar.clone(),
                    rest: bar,
                    is_first_used: false,
                    width: str_width(QUOTE_BAR),
                });
                self.push_style(quote_style());
            }
            Tag::CodeBlock(kind) => {
                self.begin_block();
                if let CodeBlockKind::Fenced(info) = kind {
                    let info = info.trim();
                    self.code_language = Language::from_fence_tag(info);
                    if !info.is_empty() {
                        self.emit_wrapped(&[(Cow::Borrowed(info), aside_style())], CODE_INDENT);
                    }
                }
                self.code = Some(String::new());
            }
            Tag::HtmlBlock => {
                self.begin_block();
                self.html = Some(String::new());
            }
            Tag::List(start) => {
                self.begin_block();
                self.lists.push(start);
            }
            Tag::Item => {
                self.begin_block();
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}. ");
                        *number += 1;
                        marker
                    }
                    _ => BULLET.to_string(),
                };
                let width = str_width(&marker);
                self.containers.push(Container {
                    first: vec![Span::styled(marker, text_style())],
                    rest: vec![Span::raw(" ".repeat(width))],
                    is_first_used: false,
                    width,
                });
                self.item_depth += 1;
            }
            Tag::Table(alignments) => {
                self.begin_block();
                self.table = Some(Table {
                    alignments,
                    rows: Vec::new(),
                    row: Vec::new(),
                });
            }
            Tag::TableHead => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::FootnoteDefinition(label) => {
                self.footnotes.open = Some((label.to_lowercase(), Vec::new()));
            }
            Tag::Emphasis => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT))
            }
            Tag::Link { dest_url, .. } => {
                if self.image_depth == 0 {
                    self.links.push((dest_url.to_string(), self.inline.len()));
                    self.push_style(link_style());
                }
            }
            Tag::Image { .. } => self.image_depth += 1,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_inline();
                self.needs_blank = true;
            }
            TagEnd::Heading(_) => {
                self.flush_inline();
                self.styles.pop();
                self.needs_blank = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.containers.pop();
                self.styles.pop();
                self.needs_blank = true;
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.emit_code(&code);
                }
                self.needs_blank = true;
            }
            TagEnd::HtmlBlock => {
                if let Some(html) = self.html.take() {
                    self.emit_source(&html);
                }
                self.needs_blank = true;
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
                if self.item_depth == 0 {
                    self.needs_blank = true;
                }
            }
            TagEnd::Item => {
                self.flush_inline();
                self.containers.pop();
                self.item_depth = self.item_depth.saturating_sub(1);
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some(table) = self.table.as_mut() {
                    table.row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if tag == TagEnd::TableHead {
                    self.styles.pop();
                }
                if let Some(table) = self.table.as_mut() {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.emit_table(table);
                }
                self.needs_blank = true;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.styles.pop();
            }
            TagEnd::Link => {
                if self.image_depth == 0 {
                    self.styles.pop();
                    self.end_link();
                }
            }
            TagEnd::Image => {
                self.image_depth = self.image_depth.saturating_sub(1);
                if self.image_depth == 0 {
                    let alt = std::mem::take(&mut self.image_alt);
                    self.inline
                        .push((Cow::Owned(format!("[image: {alt}]")), aside_style()));
                }
            }
            _ => {}
        }
    }

    /// Append ` (url)` after the link text, or the url alone when the text
    /// is empty. An autolink, whose text is its url, is not repeated.
    fn end_link(&mut self) {
        let Some((url, start)) = self.links.pop() else {
            return;
        };
        if url.is_empty() {
            return;
        }
        let label: String = self.inline[start.min(self.inline.len())..]
            .iter()
            .map(|(text, _)| text.as_ref())
            .collect();
        if label.is_empty() {
            self.inline.push((Cow::Owned(url), link_style()));
        } else if label != url {
            self.inline
                .push((Cow::Owned(format!(" ({url})")), aside_style()));
        }
    }

    fn text(&mut self, text: CowStr<'s>) {
        if let Some(code) = self.code.as_mut() {
            code.push_str(&text);
        } else if let Some(html) = self.html.as_mut() {
            html.push_str(&text);
        } else if self.image_depth > 0 {
            self.image_alt.push_str(&text);
        } else {
            self.inline.push((into_cow(text), self.style()));
        }
    }

    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_else(text_style)
    }

    fn push_style(&mut self, style: Style) {
        let combined = self.style().patch(style);
        self.styles.push(combined);
    }

    /// Close the pending inline text and put the separating blank line in
    /// front of the block that starts now.
    fn begin_block(&mut self) {
        self.flush_inline();
        if self.needs_blank {
            self.needs_blank = false;
            self.emit_blank();
        }
    }

    fn prefix_width(&self) -> usize {
        self.containers.iter().map(|c| c.width).sum()
    }

    /// Columns left for content after the container prefixes, at least 1.
    fn avail(&self) -> usize {
        self.width.saturating_sub(self.prefix_width()).max(1)
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let segments = std::mem::take(&mut self.inline);
        self.emit_wrapped(&segments, "");
    }

    /// A code block's lines, highlighted when its fence tag names a
    /// language whose grammar loaded and the document is within
    /// [`MAX_HIGHLIGHT_BYTES`], each wrapped by character inside the code
    /// indent (`wrap_chars`). In a highlighted block uncaptured text is
    /// [`highlighted_code_base`], so string literals stand out; any other
    /// block stays in [`code_style`].
    fn emit_code(&mut self, code: &str) {
        let language = self.code_language.take();
        if code.is_empty() {
            return;
        }
        let body = code.strip_suffix('\n').unwrap_or(code);
        let plain: Vec<&str> = body
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect();
        let highlighted = language
            .filter(|&language| self.is_highlighting && language_available(language))
            .map(|language| highlight_lines(code, language))
            .filter(|lines| lines.len() == plain.len());
        match highlighted {
            Some(lines) => {
                for line in lines {
                    let spans: Vec<Span<'static>> = line
                        .into_iter()
                        .map(|span| {
                            let style = highlighted_code_base().patch(span.style);
                            span.style(style)
                        })
                        .collect();
                    self.emit_code_line(&spans);
                }
            }
            None => {
                for line in plain {
                    self.emit_code_line(&[Span::styled(line.to_string(), code_style())]);
                }
            }
        }
    }

    fn emit_code_line(&mut self, spans: &[Span<'static>]) {
        let avail = self.avail().saturating_sub(str_width(CODE_INDENT)).max(1);
        for content in wrap_chars(spans, avail) {
            let mut line = Vec::with_capacity(content.len() + 1);
            line.push(Span::raw(CODE_INDENT));
            line.extend(content);
            self.emit_line(line);
        }
    }

    /// Source text shown as is (raw HTML), one wrapped line group per
    /// source line.
    fn emit_source(&mut self, text: &str) {
        let body = text.trim_end_matches('\n');
        if body.is_empty() {
            return;
        }
        for line in body.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            self.emit_wrapped(&[(Cow::Borrowed(line), text_style())], "");
        }
    }

    /// Lay a table out in columns within the available width (see
    /// [`fit_columns`]): cells wrap inside their column, a row is as tall as
    /// its tallest cell, and the header row is followed by a rule.
    fn emit_table(&mut self, table: Table<'s>) {
        let row_width = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        let columns = row_width.max(table.alignments.len());
        if columns == 0 {
            return;
        }
        let mut natural = vec![0; columns];
        for row in &table.rows {
            for (column, cell) in row.iter().enumerate() {
                natural[column] = natural[column].max(cell_width(cell));
            }
        }
        let avail = self.avail();
        let separators = str_width(TABLE_SEPARATOR) * (columns - 1);
        let widths = fit_columns(&natural, avail.saturating_sub(separators));
        for (index, row) in table.rows.iter().enumerate() {
            let mut cells: Vec<Vec<Vec<Span<'static>>>> = (0..columns)
                .map(|column| match row.get(column) {
                    Some(cell) => wrap(cell, widths[column]),
                    None => vec![Vec::new()],
                })
                .collect();
            let height = cells.iter().map(Vec::len).max().unwrap_or(1);
            for line in 0..height {
                let mut spans = Vec::new();
                for (column, cell) in cells.iter_mut().enumerate() {
                    if column > 0 {
                        spans.push(Span::styled(TABLE_SEPARATOR, rule_style()));
                    }
                    let content = cell.get_mut(line).map(std::mem::take).unwrap_or_default();
                    let alignment = table.alignments.get(column).copied();
                    pad_cell(&mut spans, content, widths[column], alignment);
                }
                self.emit_table_line(spans, avail);
            }
            if index == 0 {
                let rule: Vec<String> = widths.iter().map(|&width| "─".repeat(width)).collect();
                let rule = Span::styled(rule.join(TABLE_JOINT), rule_style());
                self.emit_table_line(vec![rule], avail);
            }
        }
    }

    /// One table line without its trailing padding, clipped with `…` when
    /// it is still wider than `avail`.
    fn emit_table_line(&mut self, mut spans: Vec<Span<'static>>, avail: usize) {
        trim_end_spans(&mut spans);
        let width: usize = spans.iter().map(|span| str_width(&span.content)).sum();
        if width > avail {
            spans = clip_spans(spans, avail);
        }
        self.emit_line(spans);
    }

    /// The footnote definitions under a short rule, in number order, each
    /// prefixed `[n] `. Definitions never referenced follow in source order.
    fn emit_footnotes(&mut self) {
        if self.footnotes.definitions.is_empty() {
            return;
        }
        self.begin_block();
        let rule = "─".repeat(FOOTNOTE_RULE_WIDTH.min(self.avail()));
        self.emit_line(vec![Span::styled(rule, rule_style())]);
        self.needs_blank = true;
        let mut next = 0;
        let mut unreferenced = 0;
        loop {
            if next == self.footnotes.order.len() {
                let definitions = &self.footnotes.definitions;
                while unreferenced < definitions.len()
                    && self
                        .footnotes
                        .numbers
                        .contains_key(&definitions[unreferenced].0)
                {
                    unreferenced += 1;
                }
                let Some((key, _)) = definitions.get(unreferenced) else {
                    break;
                };
                let key = key.clone();
                self.footnotes.number(key);
            }
            let key = &self.footnotes.order[next];
            next += 1;
            let Some(&index) = self.footnotes.definition_index.get(key) else {
                continue;
            };
            let events = std::mem::take(&mut self.footnotes.definitions[index].1);
            self.emit_footnote(next, events);
        }
    }

    /// Replay one definition's events inside a `[n] ` marker container.
    fn emit_footnote(&mut self, number: usize, events: Vec<Event<'s>>) {
        self.begin_block();
        let marker = format!("[{number}] ");
        let width = str_width(&marker);
        self.containers.push(Container {
            first: vec![Span::styled(marker, aside_style())],
            rest: vec![Span::raw(" ".repeat(width))],
            is_first_used: false,
            width,
        });
        for event in events {
            self.event(event);
        }
        self.flush_inline();
        if self.containers.last().is_some_and(|c| !c.is_first_used) {
            self.emit_line(Vec::new());
        }
        self.containers.pop();
        self.needs_blank = true;
    }

    fn emit_wrapped(&mut self, segments: &[(Cow<'_, str>, Style)], indent: &'static str) {
        let indent_width = str_width(indent);
        let avail = self.avail().saturating_sub(indent_width).max(1);
        for content in wrap(segments, avail) {
            let mut spans = Vec::with_capacity(content.len() + 1);
            if !indent.is_empty() {
                spans.push(Span::raw(indent));
            }
            spans.extend(content);
            self.emit_line(spans);
        }
    }

    fn emit_line(&mut self, content: Vec<Span<'static>>) {
        let mut spans = Vec::with_capacity(self.containers.len() + content.len());
        for container in &mut self.containers {
            if container.is_first_used {
                spans.extend(container.rest.iter().cloned());
            } else {
                spans.extend(container.first.iter().cloned());
                container.is_first_used = true;
            }
        }
        spans.extend(content);
        self.lines.push(Line::from(spans));
    }

    /// A blank separator line, keeping quote bars but no trailing spaces.
    /// Never the first line of the output.
    fn emit_blank(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        let mut spans: Vec<Span<'static>> = self
            .containers
            .iter()
            .flat_map(|c| c.rest.iter().cloned())
            .collect();
        while let Some(last) = spans.last_mut() {
            let trimmed = last.content.trim_end();
            if trimmed.is_empty() {
                spans.pop();
            } else {
                let trimmed = trimmed.to_string();
                last.content = trimmed.into();
                break;
            }
        }
        self.lines.push(Line::from(spans));
    }
}

fn into_cow(text: CowStr<'_>) -> Cow<'_, str> {
    match text {
        CowStr::Borrowed(text) => Cow::Borrowed(text),
        other => Cow::Owned(other.to_string()),
    }
}

/// Display width of `text`, as ratatui measures it. Printable ASCII, the
/// common case, skips the Unicode tables: it is one column per byte.
fn str_width(text: &str) -> usize {
    if text.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return text.len();
    }
    Span::raw(text).width()
}

fn char_width(ch: char) -> usize {
    let mut buf = [0u8; 4];
    str_width(ch.encode_utf8(&mut buf))
}

/// Wrap one code line to `avail` columns by character, as notez does. Every
/// character is kept, spaces included, so leading indentation and inner
/// runs survive the break; a span's style continues on the next line. Tabs
/// expand to [`TAB`]. A character wider than `avail` gets a line of its
/// own. Always returns at least one line.
fn wrap_chars(spans: &[Span<'static>], avail: usize) -> Vec<Vec<Span<'static>>> {
    let avail = avail.max(1);
    let mut lines = Vec::new();
    let mut line: Vec<Span<'static>> = Vec::new();
    let mut width = 0;
    for span in spans {
        let style = span.style;
        let text: &str = &span.content;
        if text.bytes().all(|b| (0x20..0x7f).contains(&b)) && width + text.len() <= avail {
            width += text.len();
            push_styled(&mut line, text, style);
            continue;
        }
        let mut buf = [0u8; 4];
        for ch in text.chars() {
            let (piece, repeat): (&str, usize) = if ch == '\t' {
                (" ", TAB.len())
            } else {
                (ch.encode_utf8(&mut buf), 1)
            };
            let piece_width = str_width(piece);
            for _ in 0..repeat {
                if width + piece_width > avail && width > 0 {
                    lines.push(std::mem::take(&mut line));
                    width = 0;
                }
                width += piece_width;
                push_styled(&mut line, piece, style);
            }
        }
    }
    lines.push(line);
    lines
}

/// Append `text` to the line, merging into the last span when the style
/// matches.
fn push_styled(line: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if let Some(last) = line.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(text);
        return;
    }
    line.push(Span::styled(text.to_string(), style));
}

/// The width of a cell's widest line when it is not wrapped.
fn cell_width(cell: &[(Cow<'_, str>, Style)]) -> usize {
    wrap(cell, usize::MAX)
        .iter()
        .map(|line| line.iter().map(|span| str_width(&span.content)).sum())
        .max()
        .unwrap_or(0)
}

/// Column widths for a table whose columns want `natural` widths and may
/// use `budget` columns in total. Natural widths are kept when they fit.
/// Otherwise the widest columns shrink first: every column is capped at
/// the largest common width that fits, never below [`MIN_COLUMN_WIDTH`],
/// and the columns left over go to the capped columns from the left. When
/// even that minimum does not fit, the result is wider than `budget` and
/// the caller clips.
fn fit_columns(natural: &[usize], budget: usize) -> Vec<usize> {
    let capped = |cap: usize| -> usize { natural.iter().map(|&width| width.min(cap)).sum() };
    if natural.iter().sum::<usize>() <= budget {
        return natural.to_vec();
    }
    if capped(MIN_COLUMN_WIDTH) >= budget {
        return natural
            .iter()
            .map(|&width| width.min(MIN_COLUMN_WIDTH))
            .collect();
    }
    // `low` fits and `high` does not; the natural total is over budget.
    let mut low = MIN_COLUMN_WIDTH;
    let mut high = natural.iter().copied().max().unwrap_or(MIN_COLUMN_WIDTH);
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if capped(middle) <= budget {
            low = middle;
        } else {
            high = middle;
        }
    }
    let mut spare = budget - capped(low);
    let mut widths: Vec<usize> = natural.iter().map(|&width| width.min(low)).collect();
    for (width, &wanted) in widths.iter_mut().zip(natural) {
        if spare == 0 {
            break;
        }
        if wanted > low {
            *width += 1;
            spare -= 1;
        }
    }
    widths
}

/// Append one line of a cell, padded to `width` by its alignment.
fn pad_cell(
    spans: &mut Vec<Span<'static>>,
    content: Vec<Span<'static>>,
    width: usize,
    alignment: Option<Alignment>,
) {
    let used: usize = content.iter().map(|span| str_width(&span.content)).sum();
    let gap = width.saturating_sub(used);
    let (left, right) = match alignment {
        Some(Alignment::Right) => (gap, 0),
        Some(Alignment::Center) => (gap / 2, gap - gap / 2),
        _ => (0, gap),
    };
    if left > 0 {
        spans.push(Span::raw(" ".repeat(left)));
    }
    spans.extend(content);
    if right > 0 {
        spans.push(Span::raw(" ".repeat(right)));
    }
}

/// Drop trailing whitespace from the end of a line.
fn trim_end_spans(spans: &mut Vec<Span<'static>>) {
    while let Some(last) = spans.last_mut() {
        let trimmed = last.content.trim_end();
        if trimmed.is_empty() {
            spans.pop();
        } else {
            if trimmed.len() < last.content.len() {
                let trimmed = trimmed.to_string();
                last.content = trimmed.into();
            }
            return;
        }
    }
}

/// Cut a line to `avail - 1` columns and end it with `…` in the style of
/// the span it cuts.
pub(crate) fn clip_spans(spans: Vec<Span<'static>>, avail: usize) -> Vec<Span<'static>> {
    let limit = avail.saturating_sub(1);
    let mut clipped = Vec::new();
    let mut width = 0;
    for span in spans {
        let span_width = str_width(&span.content);
        if width + span_width <= limit {
            width += span_width;
            clipped.push(span);
            continue;
        }
        let mut kept = String::new();
        for ch in span.content.chars() {
            let ch_width = char_width(ch);
            if width + ch_width > limit {
                break;
            }
            width += ch_width;
            kept.push(ch);
        }
        if !kept.is_empty() {
            clipped.push(Span::styled(kept, span.style));
        }
        clipped.push(Span::styled(CUT_MARK, span.style));
        return clipped;
    }
    clipped
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    Word,
    Space,
    Break,
}

/// The run kind of the character starting at byte `index`, its length, and
/// whether it is printable ASCII (one column). ASCII is classified by byte,
/// the hot path on large notes.
fn classify(bytes: &[u8], text: &str, index: usize) -> (Run, usize, bool) {
    let byte = bytes[index];
    if byte < 0x80 {
        return match byte {
            b'\n' => (Run::Break, 1, false),
            b' ' => (Run::Space, 1, true),
            b'\t' | b'\r' | 0x0b | 0x0c => (Run::Space, 1, false),
            0x21..=0x7e => (Run::Word, 1, true),
            _ => (Run::Word, 1, false),
        };
    }
    let ch = text[index..].chars().next().unwrap_or(' ');
    let kind = if ch.is_whitespace() {
        Run::Space
    } else {
        Run::Word
    };
    (kind, ch.len_utf8(), false)
}

/// Word-wrap styled segments to `avail` columns. Always returns at least
/// one line. Whitespace at a soft wrap point is dropped; leading
/// whitespace at the start of the text or after a hard break is kept.
fn wrap(segments: &[(Cow<'_, str>, Style)], avail: usize) -> Vec<Vec<Span<'static>>> {
    let mut wrapper = Wrapper::new(avail);
    for (text, style) in segments {
        let text: &str = text;
        let bytes = text.as_bytes();
        let mut start = 0;
        let mut index = 0;
        let mut current: Option<Run> = None;
        let mut is_plain = true;
        while index < bytes.len() {
            let (kind, len, is_plain_char) = classify(bytes, text, index);
            if current != Some(kind) || kind == Run::Break {
                if let Some(previous) = current {
                    wrapper.run(previous, &text[start..index], *style, is_plain);
                }
                start = index;
                current = Some(kind);
                is_plain = true;
            }
            is_plain &= is_plain_char;
            index += len;
        }
        if let Some(kind) = current {
            wrapper.run(kind, &text[start..], *style, is_plain);
        }
    }
    wrapper.finish()
}

/// One piece of a word or of pending whitespace: text, style, width.
type Piece<'a> = (Cow<'a, str>, Style, usize);

struct Wrapper<'a> {
    avail: usize,
    lines: Vec<Vec<Span<'static>>>,
    line: Vec<Span<'static>>,
    line_width: usize,
    pending: Vec<Piece<'a>>,
    pending_width: usize,
    word: Vec<Piece<'a>>,
    word_width: usize,
    is_soft_start: bool,
}

impl<'a> Wrapper<'a> {
    fn new(avail: usize) -> Self {
        Self {
            avail: avail.max(1),
            lines: Vec::new(),
            line: Vec::new(),
            line_width: 0,
            pending: Vec::new(),
            pending_width: 0,
            word: Vec::new(),
            word_width: 0,
            is_soft_start: false,
        }
    }

    /// Feed one run. `is_plain` means printable ASCII, so its width is its
    /// length and the Unicode width tables can be skipped.
    fn run(&mut self, kind: Run, text: &'a str, style: Style, is_plain: bool) {
        match kind {
            Run::Break => {
                self.flush_word();
                self.hard_break();
            }
            Run::Space => {
                self.flush_word();
                self.space(text, style, is_plain);
            }
            Run::Word => {
                let width = if is_plain {
                    text.len()
                } else {
                    str_width(text)
                };
                self.word_width += width;
                self.word.push((Cow::Borrowed(text), style, width));
            }
        }
    }

    fn space(&mut self, run: &'a str, style: Style, is_plain: bool) {
        if self.is_soft_start && self.line_width == 0 {
            return;
        }
        let (run, width) = if is_plain {
            (Cow::Borrowed(run), run.len())
        } else {
            let run: Cow<'a, str> = Cow::Owned(run.replace('\t', TAB));
            let width = str_width(&run);
            (run, width)
        };
        self.pending_width += width;
        self.pending.push((run, style, width));
    }

    fn flush_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let mut word = std::mem::take(&mut self.word);
        let word_width = std::mem::take(&mut self.word_width);
        if self.line_width + self.pending_width + word_width <= self.avail {
            self.commit_pending();
        } else if self.line_width > 0 {
            self.soft_break();
        } else {
            self.clear_pending();
        }
        if self.line_width + word_width <= self.avail {
            for (text, style, width) in &word {
                self.push_piece(text, *style, *width);
            }
        } else {
            let mut buf = [0u8; 4];
            for (text, style, _) in &word {
                for ch in text.chars() {
                    let width = char_width(ch);
                    if self.line_width + width > self.avail && self.line_width > 0 {
                        self.soft_break();
                    }
                    self.push_piece(ch.encode_utf8(&mut buf), *style, width);
                }
            }
        }
        word.clear();
        self.word = word;
    }

    fn commit_pending(&mut self) {
        let mut pending = std::mem::take(&mut self.pending);
        self.pending_width = 0;
        for (text, style, width) in &pending {
            self.push_piece(text, *style, *width);
        }
        pending.clear();
        self.pending = pending;
    }

    fn clear_pending(&mut self) {
        self.pending.clear();
        self.pending_width = 0;
    }

    fn push_piece(&mut self, text: &str, style: Style, width: usize) {
        self.line_width += width;
        self.is_soft_start = false;
        if let Some(last) = self.line.last_mut()
            && last.style == style
        {
            last.content.to_mut().push_str(text);
            return;
        }
        self.line.push(Span::styled(text.to_string(), style));
    }

    fn soft_break(&mut self) {
        self.clear_pending();
        self.lines.push(std::mem::take(&mut self.line));
        self.line_width = 0;
        self.is_soft_start = true;
    }

    fn hard_break(&mut self) {
        self.clear_pending();
        self.lines.push(std::mem::take(&mut self.line));
        self.line_width = 0;
        self.is_soft_start = false;
    }

    fn finish(mut self) -> Vec<Vec<Span<'static>>> {
        self.flush_word();
        self.lines.push(self.line);
        self.lines
    }
}

#[cfg(test)]
mod tests;
