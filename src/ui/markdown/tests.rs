use super::*;
use std::time::Instant;

fn text_of(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn texts(lines: &[Line<'_>]) -> Vec<String> {
    lines.iter().map(text_of).collect()
}

/// The first span whose text is exactly `needle`.
fn span<'a>(lines: &'a [Line<'static>], needle: &str) -> &'a Span<'static> {
    lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content == needle)
        .unwrap_or_else(|| panic!("no span {needle:?} in {:?}", texts(lines)))
}

fn has(style: Style, modifier: Modifier) -> bool {
    style.add_modifier.contains(modifier)
}

/// FZ-59: no new ladder, code, links and rules are existing tokens.
/// FZ-70: code is peach.
#[test]
fn styles_are_colors_tokens() {
    assert_eq!(code_style().fg, Some(colors::PEACH));
    assert_eq!(link_style().fg, Some(colors::SAPPHIRE));
    assert_eq!(rule_style().fg, Some(colors::SURFACE));
}

/// FZ-70, ported from notez: levels 1 and 2 have their own colour and
/// deeper levels are bold in the text colour.
#[test]
fn headings_one_and_two_have_their_own_style_and_deeper_levels_are_bold() {
    let lines = render_markdown("# One\n\n## Two\n\n### Three\n\n###### Six", 40);
    assert_eq!(texts(&lines), ["One", "", "Two", "", "Three", "", "Six"]);
    assert_eq!(span(&lines, "One").style, heading_style(1));
    assert_eq!(span(&lines, "Two").style, heading_style(2));
    assert_ne!(heading_style(1), heading_style(2));
    assert_eq!(heading_style(1).fg, Some(colors::MAUVE));
    assert_eq!(heading_style(2).fg, Some(colors::SAPPHIRE));
    for deep in ["Three", "Six"] {
        let style = span(&lines, deep).style;
        assert!(has(style, Modifier::BOLD), "{deep} not bold");
        assert_eq!(style.fg, Some(colors::TEXT), "{deep} not text colour");
    }
    assert!(has(heading_style(1), Modifier::BOLD));
    assert!(has(heading_style(2), Modifier::BOLD));
}

#[test]
fn emphasis_is_italic_and_strong_is_bold() {
    let lines = render_markdown("a *it* b **bo** c ***both***", 80);
    assert_eq!(texts(&lines), ["a it b bo c both"]);
    assert!(has(span(&lines, "it").style, Modifier::ITALIC));
    assert!(!has(span(&lines, "it").style, Modifier::BOLD));
    assert!(has(span(&lines, "bo").style, Modifier::BOLD));
    let both = span(&lines, "both").style;
    assert!(has(both, Modifier::BOLD) && has(both, Modifier::ITALIC));
    assert_eq!(span(&lines, "a ").style, text_style());
}

#[test]
fn inline_code_uses_the_code_style() {
    let lines = render_markdown("run `cargo test` now", 80);
    assert_eq!(texts(&lines), ["run cargo test now"]);
    assert_eq!(span(&lines, "cargo test").style, code_style());
}

#[test]
fn fenced_code_keeps_the_language_as_a_dim_first_line_and_indents_the_code() {
    let lines = render_markdown("```rust\nfn main() {\n    go();\n}\n```\n", 80);
    assert_eq!(
        texts(&lines),
        ["  rust", "  fn main() {", "      go();", "  }"]
    );
    assert_eq!(span(&lines, "rust").style, aside_style());
    assert_eq!(span(&lines, "fn").style, syntax_keyword());
    assert_eq!(span(&lines, "go").style, syntax_function());
}

#[test]
fn fenced_code_without_a_language_has_no_language_line() {
    let lines = render_markdown("```\nplain\n\nafter blank\n```", 80);
    assert_eq!(texts(&lines), ["  plain", "  ", "  after blank"]);
    assert_eq!(span(&lines, "plain").style, code_style());
}

#[test]
fn indented_code_renders_like_a_fence_without_a_language() {
    let lines = render_markdown("text\n\n    let x = 1;\n    let y = 2;\n", 80);
    assert_eq!(texts(&lines), ["text", "", "  let x = 1;", "  let y = 2;"]);
    assert_eq!(span(&lines, "let x = 1;").style, code_style());
}

/// FZ-59, as notez: a code line wider than the reader wraps inside the
/// indent rather than being cut. Width 8 leaves 6 columns after it.
#[test]
fn code_lines_wider_than_the_pane_wrap_inside_the_indent() {
    let lines = render_markdown("```\nabcdefghij\n```", 8);
    assert_eq!(texts(&lines), ["  abcdef", "  ghij"]);
    for line in &lines {
        assert!(line.width() <= 8, "{:?}", text_of(line));
        assert_eq!(line.spans[1].style, code_style());
    }
    let lines = render_markdown("```\n\tx\n```", 80);
    assert_eq!(texts(&lines), ["      x"], "a tab is four columns");
}

/// Code wraps by character, so the leading spaces and inner runs
/// survive the break (notez NZ-25 follow-up 1).
#[test]
fn code_wraps_by_character_keeping_leading_and_inner_spaces() {
    let lines = render_markdown("```\n        abcdefghijkl\n```", 8);
    assert_eq!(texts(&lines), ["        ", "    abcd", "  efghij", "  kl"]);
    let lines = render_markdown("```\nab    cd  ef\n```", 8);
    assert_eq!(texts(&lines), ["  ab    ", "  cd  ef"]);
    for line in &lines {
        assert_eq!(line.spans[0].content, CODE_INDENT);
        assert_eq!(line.spans[0].style, Style::default());
    }
    for line in render_markdown("```\n漢漢漢漢\n```", 6) {
        assert!(line.width() <= 6, "{:?}", text_of(&line));
    }
}

#[test]
fn bullet_lists_use_bullets() {
    let lines = render_markdown("- one\n- two\n* three", 80);
    assert_eq!(texts(&lines), ["• one", "• two", "", "• three"]);
}

#[test]
fn numbered_lists_count_from_their_start() {
    let lines = render_markdown("3. three\n4. four\n9. five", 80);
    assert_eq!(texts(&lines), ["3. three", "4. four", "5. five"]);
}

#[test]
fn nested_lists_indent_two_columns_per_level() {
    let lines = render_markdown("- a\n  - b\n    - c\n- d", 80);
    assert_eq!(texts(&lines), ["• a", "  • b", "    • c", "• d"]);
}

#[test]
fn list_item_continuation_lines_align_with_the_text() {
    let lines = render_markdown("- alpha beta gamma", 10);
    assert_eq!(texts(&lines), ["• alpha", "  beta", "  gamma"]);
}

#[test]
fn block_quotes_have_a_bar_in_the_quote_style() {
    let lines = render_markdown("> quoted\n>\n> more", 80);
    assert_eq!(texts(&lines), ["▎ quoted", "▎", "▎ more"]);
    assert_eq!(span(&lines, "▎ ").style, quote_style());
    assert_eq!(span(&lines, "quoted").style, quote_style());
}

#[test]
fn nested_quote_gets_two_bars() {
    let lines = render_markdown("> > deep", 80);
    assert_eq!(texts(&lines), ["▎ ▎ deep"]);
}

/// A `---` rule is a box-drawing line, never a dash character.
#[test]
fn horizontal_rule_spans_the_width_in_box_drawing() {
    let lines = render_markdown("above\n\n---\n\nbelow", 12);
    let rule = "─".repeat(12);
    assert_eq!(texts(&lines), ["above", "", rule.as_str(), "", "below"]);
    assert_eq!(lines[2].spans[0].style, rule_style());
    assert!(!rule.contains(['\u{2013}', '\u{2014}']));
}

#[test]
fn links_show_their_text_in_the_link_color_then_the_dimmed_url() {
    let lines = render_markdown("see [the docs](https://x.io/d) here", 80);
    assert_eq!(texts(&lines), ["see the docs (https://x.io/d) here"]);
    assert_eq!(span(&lines, "the docs").style, link_style());
    assert_eq!(span(&lines, " (https://x.io/d)").style, aside_style());
}

#[test]
fn autolinks_show_the_url_once() {
    let lines = render_markdown("<https://x.io>", 80);
    assert_eq!(texts(&lines), ["https://x.io"]);
    assert_eq!(span(&lines, "https://x.io").style, link_style());
}

#[test]
fn images_show_an_alt_placeholder() {
    let lines = render_markdown("![a cat](cat.png)", 80);
    assert_eq!(texts(&lines), ["[image: a cat]"]);
    assert_eq!(span(&lines, "[image: a cat]").style, aside_style());
}

#[test]
fn a_table_renders_as_columns_with_a_rule_under_the_header() {
    let source = "| a | b |\n|---|---|\n| 1 | **2** |\n\nafter";
    let lines = render_markdown(source, 80);
    assert_eq!(texts(&lines), ["a │ b", "──┼──", "1 │ 2", "", "after"]);
    assert!(has(span(&lines, "a").style, Modifier::BOLD));
    assert!(!has(span(&lines, "1").style, Modifier::BOLD));
    assert!(has(span(&lines, "2").style, Modifier::BOLD));
    assert_eq!(span(&lines, " │ ").style, rule_style());
    assert_eq!(span(&lines, "──┼──").style, rule_style());
}

#[test]
fn table_columns_honour_left_centre_and_right_alignment() {
    let source = "| Left | Centre | Right |\n|:-----|:------:|------:|\n\
                  | a | b | c |\n| long cell | mid | 1234567 |\n";
    let lines = render_markdown(source, 80);
    assert_eq!(
        texts(&lines),
        [
            "Left      │ Centre │   Right",
            "──────────┼────────┼────────",
            "a         │   b    │       c",
            "long cell │  mid   │ 1234567",
        ]
    );
}

#[test]
fn a_table_wider_than_the_pane_shrinks_the_widest_column_and_wraps_it() {
    let source = "| k | description |\n|---|---|\n| x | one two three four five |\n";
    let lines = render_markdown(source, 20);
    let rule = format!("──┼{}", "─".repeat(17));
    assert_eq!(
        texts(&lines),
        [
            "k │ description",
            rule.as_str(),
            "x │ one two three",
            "  │ four five",
        ]
    );
    for line in &lines {
        assert!(line.width() <= 20, "{:?}", text_of(line));
    }
}

#[test]
fn a_table_that_cannot_fit_is_clipped_with_an_ellipsis() {
    let source = "| aaaaa | bbbbb | ccccc | ddddd |\n|---|---|---|---|\n| 1 | 2 | 3 | 4 |\n";
    let lines = render_markdown(source, 10);
    assert_eq!(text_of(&lines[0]), "aaa │ bbb…");
    assert_eq!(text_of(&lines[2]), "────┼────…");
    assert_eq!(lines.len(), 4);
    for line in &lines {
        assert!(line.width() <= 10, "{:?}", text_of(line));
        assert!(text_of(line).ends_with('…'), "{:?}", text_of(line));
    }
}

#[test]
fn a_table_inside_a_quote_has_one_bar_per_line() {
    let lines = render_markdown("> | a | b |\n> |---|---|\n> | 1 | 2 |", 80);
    assert_eq!(texts(&lines), ["▎ a │ b", "▎ ──┼──", "▎ 1 │ 2"]);
    for line in &lines {
        assert_eq!(text_of(line).matches('▎').count(), 1);
    }
}

#[test]
fn a_footnote_reference_is_numbered_and_its_definition_follows_the_document() {
    let source = "[^a]: Defined first.\n\nBody[^a] text.\n\nMore.";
    let lines = render_markdown(source, 40);
    let rule = "─".repeat(10);
    assert_eq!(
        texts(&lines),
        [
            "Body[1] text.",
            "",
            "More.",
            "",
            rule.as_str(),
            "",
            "[1] Defined first."
        ]
    );
    assert_eq!(span(&lines, "[1]").style, aside_style());
    assert_eq!(span(&lines, "[1] ").style, aside_style());
}

#[test]
fn footnotes_are_numbered_in_the_order_first_referenced() {
    let source = "x[^b] y[^a] z[^b]\n\n[^a]: Alpha.\n\n[^b]: Beta.";
    let lines = render_markdown(source, 40);
    let rule = "─".repeat(10);
    assert_eq!(
        texts(&lines),
        [
            "x[1] y[2] z[1]",
            "",
            rule.as_str(),
            "",
            "[1] Beta.",
            "",
            "[2] Alpha."
        ]
    );
}

#[test]
fn wide_characters_in_cells_keep_every_table_line_within_the_width() {
    let source = "| 名前 | 説明 |\n|---|---|\n| 漢字 | 漢字漢字漢字漢字漢字漢字 |\n";
    for width in [0, 1, 5, 9, 12, 20, 40] {
        let lines = render_markdown(source, width);
        let limit = usize::from(width.max(1));
        for line in &lines {
            assert!(line.width() <= limit, "{width}: {:?}", text_of(line));
        }
    }
}

#[test]
fn raw_html_blocks_pass_through_as_text() {
    let lines = render_markdown("<div>\n<b>hi</b>\n</div>\n\nafter", 80);
    assert_eq!(texts(&lines), ["<div>", "<b>hi</b>", "</div>", "", "after"]);
}

#[test]
fn inline_html_passes_through_as_text() {
    let lines = render_markdown("a <kbd>K</kbd> b", 80);
    assert_eq!(texts(&lines), ["a <kbd>K</kbd> b"]);
}

#[test]
fn hard_break_starts_a_new_line_and_soft_break_is_a_space() {
    let lines = render_markdown("one\ntwo  \nthree\\\nfour", 80);
    assert_eq!(texts(&lines), ["one two", "three", "four"]);
}

#[test]
fn strikethrough_is_crossed_out() {
    let lines = render_markdown("keep ~~gone~~", 80);
    assert!(has(span(&lines, "gone").style, Modifier::CROSSED_OUT));
    assert!(!has(span(&lines, "keep ").style, Modifier::CROSSED_OUT));
}

#[test]
fn task_list_items_show_checkboxes() {
    let lines = render_markdown("- [ ] todo\n- [x] done", 80);
    assert_eq!(texts(&lines), ["• [ ] todo", "• [x] done"]);
}

#[test]
fn blocks_are_separated_by_one_blank_line() {
    let lines = render_markdown("# H\npara\n- item\n\n> q", 80);
    assert_eq!(texts(&lines), ["H", "", "para", "", "• item", "", "▎ q"]);
}

#[test]
fn paragraphs_wrap_at_width_20_without_breaking_words() {
    let source = "The quick brown fox jumps over the lazy dog and keeps running \
                  until the preview pane runs out of columns entirely.";
    let lines = render_markdown(source, 20);
    let got = texts(&lines);
    assert_eq!(
        got,
        [
            "The quick brown fox",
            "jumps over the lazy",
            "dog and keeps",
            "running until the",
            "preview pane runs",
            "out of columns",
            "entirely.",
        ]
    );
    for line in &lines {
        assert!(line.width() <= 20, "{:?} is wider than 20", text_of(line));
    }
}

#[test]
fn width_zero_and_one_do_not_panic() {
    let source =
        "# Head\n\nsome words\n\n- a\n  - b\n\n> q\n\n```\ncode\n```\n\n---\n\n| t |\n|---|";
    for width in [0, 1] {
        let lines = render_markdown(source, width);
        assert!(!lines.is_empty());
        assert_eq!(texts(&lines)[0], "H");
    }
    assert_eq!(texts(&render_markdown("ab", 0)), ["a", "b"]);
}

#[test]
fn crlf_input_renders_like_lf() {
    let lf = "# T\n\none\ntwo\n\n```\nx\ny\n```\n\n| a |\n|---|\n";
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(render_markdown(&crlf, 40), render_markdown(lf, 40));
    assert!(
        texts(&render_markdown(&crlf, 40))
            .iter()
            .all(|l| !l.contains('\r'))
    );
}

#[test]
fn an_unterminated_fence_renders_the_rest_as_code() {
    let lines = render_markdown("before\n\n```sh\necho hi\n# not a heading\n", 80);
    assert_eq!(
        texts(&lines),
        ["before", "", "  sh", "  echo hi", "  # not a heading"]
    );
    assert_eq!(span(&lines, "# not a heading").style, syntax_comment());
}

/// FZ-71, notez's `fenced_rust_block_is_highlighted_and_keeps_the_line_count`.
#[test]
fn fenced_rust_block_is_highlighted_and_keeps_the_line_count() {
    let source = "# T\n\n```rust\nfn main() {\n    let x = 1; // c\n}\n```\n\nafter\n";
    let lines = render_markdown(source, 80);
    assert_eq!(
        texts(&lines),
        [
            "T",
            "",
            "  rust",
            "  fn main() {",
            "      let x = 1; // c",
            "  }",
            "",
            "after"
        ]
    );
    assert_eq!(span(&lines, "fn").style, syntax_keyword());
    assert_eq!(span(&lines, "let").style, syntax_keyword());
    assert_eq!(span(&lines, "// c").style, syntax_comment());
    assert_eq!(span(&lines, "rust").style, aside_style());
    assert_eq!(lines[3].spans[0].content, CODE_INDENT);
    let plain = lines[3]
        .spans
        .iter()
        .find(|s| s.content == " ")
        .expect("uncaptured space");
    assert_eq!(plain.style, highlighted_code_base());
}

/// FZ-71, notez's version of the unterminated fence: an `sh` fence keeps
/// `# not a heading` as a comment.
#[test]
fn an_sh_fence_highlights_a_hash_line_as_a_comment() {
    let lines = render_markdown("before\n\n```sh\necho hi\n# not a heading\n", 80);
    assert_eq!(
        texts(&lines),
        ["before", "", "  sh", "  echo hi", "  # not a heading"]
    );
    assert_eq!(span(&lines, "# not a heading").style, syntax_comment());
}

#[test]
fn an_untagged_fence_stays_in_the_code_style() {
    let lines = render_markdown("```\nfn main() {}\n```", 80);
    assert_eq!(texts(&lines), ["  fn main() {}"]);
    assert_eq!(lines[0].spans.len(), 2);
    assert_eq!(span(&lines, "fn main() {}").style, code_style());
}

#[test]
fn an_unknown_fence_tag_renders_plain_code() {
    let lines = render_markdown("```yaml\nkey: \"value\" # c\n```", 80);
    assert_eq!(texts(&lines), ["  yaml", "  key: \"value\" # c"]);
    assert_eq!(lines[1].spans.len(), 2);
    assert_eq!(span(&lines, "key: \"value\" # c").style, code_style());
}

#[test]
fn wrapped_highlighted_code_carries_styles_across_the_break() {
    let lines = render_markdown("```rust\nlet s = 1; // abcdefgh\n```", 12);
    assert_eq!(
        texts(&lines),
        ["  rust", "  let s = 1;", "   // abcdef", "  gh"]
    );
    assert_eq!(span(&lines, "let").style, syntax_keyword());
    let head = lines[2].spans.last().unwrap();
    assert_eq!(head.content, "// abcdef");
    assert_eq!(head.style, syntax_comment());
    let tail = &lines[3].spans[1];
    assert_eq!(tail.content, "gh");
    assert_eq!(tail.style, syntax_comment());
    for line in &lines {
        assert!(line.width() <= 12, "{:?}", text_of(line));
    }
}

#[test]
fn a_document_over_the_limit_renders_fences_plain() {
    let mut doc = String::from("```rust\nfn main() {}\n```\n\n");
    while doc.len() <= MAX_HIGHLIGHT_BYTES {
        doc.push_str("filler text for the size limit.\n\n");
    }
    let lines = render_markdown(&doc, 80);
    assert_eq!(texts(&lines[..2]), ["  rust", "  fn main() {}"]);
    assert_eq!(lines[1].spans[1].content, "fn main() {}");
    assert_eq!(lines[1].spans[1].style, code_style());
}

/// FZ-71: the 12 capture styles are notez's, from `colors.rs`.
#[test]
fn capture_styles_are_notez_colours() {
    let bold = Modifier::BOLD;
    let italic = Modifier::ITALIC;
    let table = [
        (syntax_keyword(), colors::MAUVE, None),
        (syntax_function(), colors::SAPPHIRE, Some(bold)),
        (syntax_type(), colors::YELLOW, None),
        (syntax_string(), colors::PEACH, None),
        (syntax_number(), colors::RED, None),
        (syntax_comment(), colors::OVERLAY, Some(italic)),
        (syntax_constant(), colors::RED, None),
        (syntax_variable(), colors::TEXT, None),
        (syntax_operator(), colors::SUBTEXT, None),
        (syntax_punctuation(), colors::OVERLAY, None),
        (syntax_attribute(), colors::YELLOW, Some(italic)),
        (syntax_property(), colors::SAPPHIRE, None),
    ];
    for (style, fg, modifier) in table {
        assert_eq!(style.fg, Some(fg));
        assert_eq!(style.add_modifier, modifier.unwrap_or(Modifier::empty()));
    }
}

#[test]
fn an_empty_document_renders_no_lines() {
    assert!(render_markdown("", 40).is_empty());
    assert!(render_markdown("\n\n  \n", 40).is_empty());
}

#[test]
fn a_long_single_token_is_split_at_the_width() {
    let token = "x".repeat(25);
    let lines = render_markdown(&format!("a {token} b"), 10);
    assert_eq!(texts(&lines), ["a", "xxxxxxxxxx", "xxxxxxxxxx", "xxxxx b"]);
}

/// The fixture a capped read can deliver: 256 KiB (`notes::BODY_CAP`)
/// of the mix a long note has.
pub fn capped_fixture() -> String {
    let chunk = "## Section\n\nSome *emphasised* text with `code` and a \
                 [link](https://example.com) that goes on for a while so the \
                 wrapper has work to do.\n\n- item one\n- item two\n  - nested\n\n\
                 > a quote\n\n```rust\nfn f() {}\n```\n\n| a | b |\n|---|---|\n\
                 | 1 | 2 |\n\n---\n\n";
    let cap = crate::notes::BODY_CAP;
    let mut doc = String::with_capacity(cap + chunk.len());
    while doc.len() + chunk.len() <= cap {
        doc.push_str(chunk);
    }
    doc
}

/// FZ-59: a capped file renders on the UI thread, once per file and
/// width (`App::reader_render`). A guard against quadratic behaviour,
/// not the measurement, which runs in release and is reported against
/// `docs/performance-baseline.md`; the bound is loose so a debug build
/// on a shared runner never flakes.
#[test]
fn a_capped_note_renders_in_bounded_time() {
    let doc = capped_fixture();
    let started = Instant::now();
    let lines = render_markdown(&doc, 80);
    let elapsed = started.elapsed();
    eprintln!(
        "capped note: {} bytes, {} lines, {elapsed:?}",
        doc.len(),
        lines.len()
    );
    assert!(lines.len() > 10_000);
    assert!(elapsed.as_secs_f64() < 2.0, "rendering took {elapsed:?}");
}
