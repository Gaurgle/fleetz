use super::*;
use crate::ui::detail::tests::{
    drawn_rows, frame_rows, land_rendered, notes_pane_buffer, notes_view_app, reader_buffer,
    summary_text,
};

/// The same in the `notes` layout, whose pane 1 carries the summary box
/// as every detail layout's does (FZ-95).
#[test]
fn in_the_notes_layout_the_frame_draws_the_message_once() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    app.set_status("copied 1abc123".to_string(), false);
    let (w, h) = (140u16, 44u16);
    let pane2 = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app)
        .detail
        .expect("pane 2");
    let notes = notes_pane_body(pane2, &app).expect("a tree");
    assert!(notes.facts.is_some(), "the header pins at {h} rows");
    let (rows, last) = frame_rows(&app, w, h);
    assert!(
        rows.iter().any(|r| r.contains("copied 1abc123")),
        "pane 1 drew no message:\n{}",
        rows.join("\n")
    );
    assert!(!last.contains("copied"), "the footer repeats it: {last:?}");
    let summary = summary_text(&app);
    assert!(
        summary.is_empty() || !last.contains(&summary),
        "the footer repeats it: {last:?}"
    );
}

/// The reader (FZ-62, formerly the inspector) reads the highlighted file:
/// its name in the facts at the tree's foot (FZ-93), then its body, or
/// the read's error in place.
#[test]
fn the_notes_inspector_shows_the_body_or_the_error() {
    let draw = |app: &crate::app::App| notes_pane_rows(120, 12, app).join("\n");
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let read = app.claim_note_read().expect("a read");
    assert!(draw(&app).contains("reading…"));

    app.apply_note_read(read.clone(), Ok("# Plan\nship it".to_string()));
    let text = draw(&app);
    for want in ["# Plan", "ship it"] {
        assert!(text.contains(want), "{want} missing:\n{text}");
    }
    let facts = foot_text(&app);
    assert!(facts.starts_with("─ a.md · markdown"), "{facts:?}");

    app.note_body.as_mut().unwrap().text = None;
    app.apply_note_read(read, Err("permission denied".to_string()));
    assert!(draw(&app).contains("could not read: permission denied"));
}

fn notes_pane_rows(w: u16, h: u16, app: &crate::app::App) -> Vec<String> {
    let buf = notes_pane_buffer(w, h, app);
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// The tree's foot (FZ-93) as `notes_foot_spans` builds it at a width
/// that holds every fact, so a test reads each one; what a narrow foot
/// keeps is `the_foot_holds_its_row_when_narrow`'s.
fn foot_text(app: &crate::app::App) -> String {
    notes_foot_spans(app, 400)
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>()
        .trim_end_matches('─')
        .to_string()
}

/// The facts beside the header name the file, its kind and size (FZ-68
/// round 2, the reader's border before), and its body scrolls by its
/// own offset: the tree's cursor stays where it was, and a scrolled body
/// starts further down.
#[test]
fn the_reader_titles_the_file_and_scrolls_its_body() {
    let mut app = notes_view_app(&[("notez/README.md", Some("notez"))]);
    app.focused_panel = FocusedPanel::Reader;
    let read = app.claim_note_read().expect("a read");
    let body: Vec<String> = (1..=40).map(|i| format!("line {i}")).collect();
    let facts = crate::notes::NoteFacts {
        size: Some(4300),
        ..Default::default()
    };
    app.apply_note_facts(&read, facts);
    app.apply_note_read(read, Ok(body.join("\n")));

    let facts = foot_text(&app);
    assert!(
        facts.starts_with("─ README.md · markdown · 4.2 KiB"),
        "{facts:?}"
    );
    // Too short to pin the header: the text from pane 2's first inner
    // row, under its top padding, to the row above its bottom border
    // (FZ-68).
    let draw = |app: &crate::app::App| notes_pane_rows(120, 12, app);
    let rows = draw(&app);
    assert!(rows[2].contains("line 1 "), "{}", rows[2]);

    assert_eq!(app.reader_view_height.get(), 9, "one H/L page measured");
    app.scroll_reader(10);
    let rows = draw(&app);
    assert!(rows[2].contains("line 11"), "{}", rows.join("\n"));
    assert!(!rows.join("\n").contains("line 1 "), "{}", rows.join("\n"));
    assert_eq!(app.detail_selected(), 0, "the tree's cursor did not move");

    // Past the end it parks on the last page, as the inspector does.
    app.scroll_reader(1000);
    let rows = draw(&app);
    assert!(rows[10].contains("line 40"), "{}", rows.join("\n"));
}

const MARKDOWN_FIXTURE: &str = "# Plan\n\n- ship *it* now\n- run `cargo test`\n\n\
                                See [the docs](https://x.io).";

fn buffer_rows(buf: &ratatui::buffer::Buffer) -> Vec<String> {
    let area = buf.area;
    (0..area.height)
        .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

/// The style of the cell where `needle` starts on screen.
fn style_at(buf: &ratatui::buffer::Buffer, needle: &str) -> Style {
    for (y, row) in buffer_rows(buf).iter().enumerate() {
        let cells: Vec<char> = row.chars().collect();
        let want: Vec<char> = needle.chars().collect();
        if let Some(x) = cells.windows(want.len()).position(|w| w == want) {
            return buf[(x as u16, y as u16)].style();
        }
    }
    panic!("{needle} not on screen:\n{}", buffer_rows(buf).join("\n"));
}

/// FZ-59: a markdown file draws rendered by default, through the
/// `colors.rs` tokens: the heading in the title colour, bold, without
/// its `#`; list bullets; emphasis italic; code in the code colour; a
/// link's text in the link colour beside its url. The facts say
/// `rendered`; `p` shows the source unchanged and the facts say `raw`.
#[test]
fn the_reader_renders_markdown_and_r_shows_the_source() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    land_rendered(&mut app, MARKDOWN_FIXTURE);
    let buf = reader_buffer(&app);
    let text = buffer_rows(&buf).join("\n");
    let facts = foot_text(&app);
    assert!(facts.contains("a.md · markdown · rendered"), "{facts:?}");
    for want in [
        "Plan",
        "• ship it now",
        "• run cargo test",
        "See the docs (https://x.io).",
    ] {
        assert!(text.contains(want), "{want} missing:\n{text}");
    }
    assert!(!text.contains("# Plan"), "the heading's # is gone:\n{text}");
    let heading = style_at(&buf, "Plan");
    assert_eq!(heading.fg, Some(colors::MAUVE), "the title colour");
    assert!(heading.add_modifier.contains(Modifier::BOLD));
    assert!(
        style_at(&buf, "it now")
            .add_modifier
            .contains(Modifier::ITALIC)
    );
    assert_eq!(style_at(&buf, "cargo test").fg, Some(colors::PEACH));
    assert_eq!(style_at(&buf, "the docs").fg, Some(colors::SAPPHIRE));
    assert_eq!(style_at(&buf, "(https").fg, Some(colors::OVERLAY));

    app.toggle_reader_raw();
    let text = buffer_rows(&reader_buffer(&app)).join("\n");
    let facts = foot_text(&app);
    assert!(facts.contains("a.md · markdown · raw"), "{facts:?}");
    for line in MARKDOWN_FIXTURE.lines().filter(|l| !l.is_empty()) {
        assert!(text.contains(line), "source line {line:?} missing:\n{text}");
    }
}

/// FZ-59: a text file is always raw: no render is asked for, its facts
/// name no mode, and `p` does not change what it shows.
#[test]
fn a_text_file_stays_raw_whatever_the_toggle() {
    let mut app = notes_view_app(&[("notez/notes.txt", Some("notez"))]);
    reader_buffer(&app);
    let read = app.claim_note_read().expect("a read");
    assert_eq!(read.render_width, None, "no render for text");
    app.apply_note_read(read, Ok("# not a heading".to_string()));
    let before = buffer_rows(&reader_buffer(&app)).join("\n");
    let facts = foot_text(&app);
    assert!(facts.starts_with("─ notes.txt · text · "), "{facts:?}");
    assert!(!facts.contains("rendered") && !facts.contains("raw"));
    assert!(before.contains("# not a heading"), "{before}");
    app.toggle_reader_raw();
    assert_eq!(buffer_rows(&reader_buffer(&app)).join("\n"), before);
    assert_eq!(foot_text(&app), facts);
}

/// The facts at the tree's foot (FZ-93; beside the header in FZ-68 round
/// 2, the `[3] file` card before) say every fact the accepted design
/// keeps for a file: name,
/// kind, size, where it lives, modified and the last commit, which is
/// "untracked" when git knew nothing of the file. Path, project and
/// created were the card's alone.
#[test]
fn the_facts_render_every_fact_and_untracked() {
    let mut app = notes_view_app(&[(".notez/scratch.md", Some(".notez"))]);
    let read = app.claim_note_read().expect("a read");
    let at = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
    app.apply_note_facts(
        &read,
        crate::notes::NoteFacts {
            size: Some(812),
            modified: Some(at),
            created: Some(at),
        },
    );
    let draw = |app: &crate::app::App| foot_text(app);
    // Before git answers the date says it is still being read.
    assert!(draw(&app).contains("last commit reading…"));
    app.apply_note_commit(&read, crate::notes::LastCommit::Untracked);
    let text = draw(&app);
    let stamp = card_time(at);
    for want in [
        "scratch.md",
        "markdown · 812 B",
        &format!(".notez/ · modified {stamp}"),
        "last commit untracked",
    ] {
        assert!(text.contains(want), "{want} missing:\n{text}");
    }

    app.apply_note_commit(
        &read,
        crate::notes::LastCommit::At("2026-10-29T10:50:00+01:00".to_string()),
    );
    let text = draw(&app);
    let committed = card_commit_time("2026-10-29T10:50:00+01:00");
    assert!(text.contains(&format!("last commit {committed}")), "{text}");
}

/// FZ-68 (was FZ-67's box): pane 2 keeps one outer border in the
/// `notes` layout, its top border whole, and the preview is its right
/// column, divided from the tree by a line from under the top border to
/// `┴` on the bottom, crossed by the rule (round 2), titled
/// `notes preview` right of it above the rule; the repo header is drawn
/// left of the divider only. Fails while the preview is a box of its own
/// inside pane 2.
#[test]
fn the_notes_preview_is_a_divided_column_of_pane_2() {
    let (w, h) = (160u16, 44u16);
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    app.contributions_folded = true;
    let read = app.claim_note_read().expect("a read");
    app.apply_note_read(read, Ok("hello".to_string()));
    let rows = drawn_rows(w, h, |f| crate::ui::render(f, &app));
    let rects = crate::ui::panel_rects(Rect::new(0, 0, w, h), &app);
    let pane2 = rects.detail.expect("pane 2");
    let reader = rects.reader.expect("preview");
    let notes = notes_pane_body(pane2, &app).expect("a tree");
    let divider = notes.divider;
    let span = |y: u16, from: u16, to: u16| -> String {
        rows[y as usize]
            .chars()
            .skip(from as usize)
            .take((to - from) as usize)
            .collect()
    };
    let cell = |x: u16, y: u16| rows[y as usize].chars().nth(x as usize).unwrap();

    // One border on pane 2's top row: its two corners and nothing
    // between them that opens a second box.
    let top = span(pane2.y, pane2.x, pane2.right());
    assert!(top.starts_with('╭') && top.ends_with('╮'), "{top}");
    assert_eq!(top.matches('╭').count(), 1, "one outer border: {top}");
    assert!(top.contains("[2]"), "the pane's title: {top}");
    assert!(top.contains("NOTES"), "the view tabs: {top}");
    let bottom = span(pane2.bottom() - 1, pane2.x, pane2.right());
    assert!(bottom.starts_with('╰') && bottom.ends_with('╯'), "{bottom}");
    assert_eq!(bottom.matches('╰').count(), 1, "no second box: {bottom}");

    // The divider: under the top border, which it does not cut, joined
    // to the bottom one, a line on every inner row but the rule's.
    let rule = notes.rule.expect("pinned at 44 rows");
    assert!(!top.contains('┬'), "the top border is whole: {top}");
    assert_eq!(cell(divider, pane2.bottom() - 1), '┴', "{bottom}");
    for y in (pane2.y + 1..pane2.bottom() - 1).filter(|y| *y != rule) {
        assert_eq!(cell(divider, y), '│', "row {y}:\n{}", rows.join("\n"));
    }

    // The preview's title right of the divider level with `NOTES`, and
    // its text right of the divider, inside pane 2.
    let preview_title = span(rule - 1, divider, pane2.right());
    assert!(
        preview_title.starts_with("│ notes preview "),
        "{preview_title}"
    );
    assert!(span(rule - 1, pane2.x, divider).contains("NOTES"));
    assert_eq!(reader.x, divider + 2, "one column in from the divider");
    assert!(
        reader.right() < pane2.right() && reader.bottom() < pane2.bottom(),
        "{reader:?} in {pane2:?}"
    );
    assert!(span(reader.y, reader.x, reader.right()).contains("hello"));

    // The header left of the divider only: its rows hold the repo's
    // state and path in the tree's column, and nothing of them crosses.
    let header = notes.header.expect("pinned at 44 rows");
    let repo = app.selected_repo().expect("a repo");
    let state = repo.long_status();
    let path = repo.path.clone().expect("cloned");
    let left = |y: u16| span(y, pane2.x, divider);
    assert!(left(header.y).contains(&state), "{}", left(header.y));
    assert!(
        (header.y..header.bottom()).any(|y| left(y).contains(&path)),
        "{}",
        rows.join("\n")
    );
    // Right of the divider on those rows nothing, row for row: the
    // window sits on pane 1's border (FZ-95, 2026-11-09). Then `notes
    // preview` on the row above the rule.
    let facts = notes.facts.expect("pinned at 44 rows");
    assert_eq!((facts.y, facts.bottom()), (header.y, rule));
    let mut want: Vec<String> = vec![String::new(); usize::from(facts.height - 1)];
    want.push(READER_LABEL.to_string());
    for (i, y) in (header.y..rule).enumerate() {
        let right = span(y, divider + 1, pane2.right() - 1);
        // The path only: `synced`, the state of this fixture, is also
        // a word of the fleet's summary that sits here (FZ-93), and the
        // row-for-row match below pins everything right of the divider.
        assert!(
            !right.contains(&path),
            "header row {y} crosses the divider: {right}"
        );
        assert_eq!(right.trim(), want[i], "row {y} right of the divider");
    }

    // The file's facts on the tree's last row, left of the divider.
    let foot = notes.foot.expect("a foot at 44 rows");
    assert_eq!(foot.bottom(), notes.body.bottom() + 1, "under the tree");
    assert!(foot.right() <= divider, "left of the divider: {foot:?}");
    assert!(left(foot.y).contains("─ a.md · "), "{}", left(foot.y));

    // The tree on the left of the same body: its mode line on the
    // tree's first row, before the divider.
    let mode = left(notes.body.y);
    assert!(mode.contains("notes: [notez]"), "the tree: {mode}");
}

/// FZ-67, moved to pane 2's border by FZ-68, beside the header by round
/// 2 and to the tree's foot by FZ-93: the file's facts hold their row on
/// a narrow pane. The facts give way before the name, and a name wider
/// than the row is cut at it, never crossing the divider or wrapping
/// onto the bottom border. Fails while a fact can overprint another.
#[test]
fn the_foot_holds_its_row_when_narrow() {
    let long = "notez/a-file-name-far-too-long-for-the-facts-block-at-eighty.md";
    let mut app = notes_view_app(&[(long, Some("notez"))]);
    let read = app.claim_note_read().expect("a read");
    let facts = crate::notes::NoteFacts {
        size: Some(4300),
        ..Default::default()
    };
    app.apply_note_facts(&read, facts);
    app.apply_note_read(read, Ok("body".to_string()));
    let (w, h) = (80u16, 24u16);
    let notes = notes_pane_body(Rect::new(0, 0, w, h), &app).expect("a tree");
    let foot = notes.foot.expect("a foot at 24 rows");
    let name = long.trim_start_matches("notez/");
    assert!(name.len() > usize::from(foot.width), "the name overflows");
    let rows = notes_pane_rows(w, h, &app);
    let row: String = rows[foot.y as usize]
        .chars()
        .skip(foot.x as usize)
        .take(foot.width as usize)
        .collect();
    assert!(row.starts_with("─ a-file-name"), "{row}");
    assert!(row.ends_with('…'), "the name is cut at the row: {row}");
    assert!(!row.contains("4.2 KiB"), "the facts gave way first: {row}");
    let divider: String = rows[foot.y as usize]
        .chars()
        .nth(notes.divider as usize)
        .into_iter()
        .collect();
    assert_eq!(divider, "│", "the divider is not overprinted");

    // Wider, the facts come back in their order of rank: the size before
    // the kind, the kind before where it lives.
    let short = notes_view_app(&[("notez/plan.md", Some("notez"))]);
    let at = |width: usize| -> String {
        notes_foot_spans(&short, width)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    };
    let full = at(400);
    assert!(
        full.contains("plan.md · markdown · rendered · notez/"),
        "{full}"
    );
    let narrow = at(30);
    assert!(narrow.starts_with("─ plan.md"), "{narrow}");
    assert!(
        !narrow.contains("notez/"),
        "where it lives goes first: {narrow}"
    );
    assert_eq!(narrow.chars().count(), 30, "the rule fills the row");
}

/// FZ-68: the divider and the outline take focus by side. The top
/// border is one segment, lit with either side (round 2); below it the
/// tree focused lights the left outline and the divider, the preview's
/// outline dim, and the reader focused lights its outline and the
/// divider, the left dim.
#[test]
fn the_divider_lights_with_either_side_and_each_segment_with_its_own() {
    let mut app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let (w, h) = (140u16, 24u16);
    let notes = notes_pane_body(Rect::new(0, 0, w, h), &app).expect("a tree");
    let divider = notes.divider;
    let (mid, bottom) = (h / 2, h - 1);
    // The top border, the left border and the bottom left of the
    // divider, the divider and the rule's crossing, the right border
    // and the bottom right of the divider.
    let colours = |app: &crate::app::App| {
        let buf = notes_pane_buffer(w, h, app);
        let rule = notes.rule.expect("pinned at 24 rows");
        // The top border's plain cells nearest each corner, past the
        // titles.
        let plain = |x: &u16| buf[(*x, 0)].symbol() == "─";
        let first = (1..w - 1).find(plain).expect("a border cell");
        let last = (1..w - 1).rev().find(plain).expect("a border cell");
        assert!(first < divider && last > divider, "{first} {last}");
        [
            (first, 0),
            (last, 0),
            (0, mid),
            (1, bottom),
            (divider, mid),
            (divider, rule),
            (w - 1, mid),
            (w - 2, bottom),
        ]
        .map(|(x, y)| buf[(x, y)].fg)
    };
    let (on, off) = (colors::LAVENDER, colors::SURFACE);

    app.focused_panel = FocusedPanel::Detail;
    let want = [on, on, on, on, on, on, off, off];
    assert_eq!(colours(&app), want, "the tree focused");
    app.focused_panel = FocusedPanel::Reader;
    let want = [on, on, off, off, on, on, on, on];
    assert_eq!(colours(&app), want, "the reader focused");
    app.focused_panel = FocusedPanel::RepoList;
    assert_eq!(colours(&app), [off; 8], "neither");
}

/// FZ-68: `panel_rects` puts the reader right of the divider, one
/// column in, on the tree's rows under the rule (round 2), down to the
/// bottom border, up to the right padding. `at` answers `Reader` there,
/// and `Detail` on the divider, on a tree row and on the facts block.
#[test]
fn panel_rects_put_the_reader_right_of_the_divider_under_the_rule() {
    let area = Rect::new(0, 0, 140, 44);
    let app = notes_view_app(&[("notez/a.md", Some("notez"))]);
    let rects = crate::ui::panel_rects(area, &app);
    let pane2 = rects.detail.expect("pane 2");
    let reader = rects.reader.expect("preview");
    let notes = notes_pane_body(pane2, &app).expect("a tree");
    assert_eq!(reader, notes.reader, "one geometry");
    assert_eq!(reader.x, notes.divider + 2);
    assert_eq!(reader.y, notes.rule.expect("pinned") + 1);
    assert_eq!(reader.y, notes.body.y, "level with the tree");
    assert_eq!(reader.bottom(), pane2.bottom() - 1);
    assert_eq!(reader.right(), pane2.right() - 2);
    let facts = notes.facts.expect("pinned");
    for (x, y) in [
        (facts.x, facts.y),
        (facts.x + facts.width / 2, facts.bottom() - 1),
        (facts.x, notes.rule.expect("pinned")),
    ] {
        assert_eq!(
            rects.at(x, y),
            Some(FocusedPanel::Detail),
            "facts ({x}, {y})"
        );
    }
    for y in [reader.y, reader.y + reader.height / 2, reader.bottom() - 1] {
        assert_eq!(rects.at(reader.x, y), Some(FocusedPanel::Reader));
        assert_eq!(rects.at(notes.divider, y), Some(FocusedPanel::Detail));
    }
    let tree_row = notes.body.y;
    assert_eq!(
        rects.at(notes.body.x + 1, tree_row),
        Some(FocusedPanel::Detail),
        "a tree row"
    );
    assert!(notes.body.right() < notes.divider, "the tree is left of it");
}

/// FZ-68 round 2, the facts moved to the tree's foot by FZ-93: the file's
/// name in the tree's colour for a file name, its kind word in the
/// colour of the tree's kind glyph, the size dim, and the mode in the
/// mode line's mark; `notes preview` level with `NOTES` right of the
/// divider, in the notes view's colour and bold like the headliner word.
#[test]
fn the_foot_shows_name_kind_size_and_mode_in_their_colours() {
    let mut app = notes_view_app(&[("notez/plan.md", Some("notez"))]);
    let read = app.claim_note_read().expect("a read");
    let facts = crate::notes::NoteFacts {
        size: Some(4300),
        ..Default::default()
    };
    app.apply_note_facts(&read, facts);
    app.apply_note_read(read, Ok("body".to_string()));
    let (w, h) = (120u16, 24u16);
    let notes = notes_pane_body(Rect::new(0, 0, w, h), &app).expect("a tree");
    let block = notes.facts.expect("pinned at 24 rows");
    let buf = notes_pane_buffer(w, h, &app);
    let rows = buffer_rows(&buf);
    // The style of the cell where `needle` starts on row `y`, right of
    // the divider or left of it.
    let at = |y: u16, needle: &str, right: bool| -> Style {
        let cells: Vec<char> = rows[y as usize].chars().collect();
        let want: Vec<char> = needle.chars().collect();
        let x = cells
            .windows(want.len())
            .enumerate()
            .position(|(x, w)| w == want && (x as u16 > notes.divider) == right)
            .unwrap_or_else(|| panic!("{needle} not on row {y}:\n{}", rows.join("\n")));
        buf[(x as u16, y)].style()
    };
    let label = block.bottom() - 1;
    assert_eq!(label, notes.rule.expect("pinned") - 1, "level with NOTES");

    // The tree's own row for the file, left of the divider.
    let tree_row = (notes.body.y..notes.body.bottom())
        .find(|y| rows[*y as usize].contains("plan.md"))
        .expect("the tree draws plan.md");
    let glyph = badges::note_kind_icon(crate::notes::NoteKind::Markdown);
    let tree_name = at(tree_row, "plan.md", false);
    let tree_glyph = at(tree_row, glyph, false);

    // The foot's facts (FZ-93), built where every one fits: the style of
    // the span holding `needle`.
    let foot = notes_foot_spans(&app, 400);
    let style_of = |needle: &str| -> Style {
        foot.iter()
            .find(|s| s.content.contains(needle))
            .unwrap_or_else(|| panic!("{needle} not in the foot: {foot:?}"))
            .style
    };
    assert_eq!(style_of("plan.md").fg, tree_name.fg, "the name");
    assert_eq!(style_of("markdown").fg, tree_glyph.fg, "the kind");
    assert_eq!(style_of("4.2 KiB").fg, Some(colors::OVERLAY), "the size");
    let mark = at(notes.body.y, "[notez]", false);
    let mode = style_of("rendered");
    assert_eq!((mode.fg, mode.add_modifier), (mark.fg, mark.add_modifier));
    assert_eq!(style_of("notez/").fg, Some(colors::TEXT), "where");
    let title = at(label, "notes preview", true);
    let notes_colour = colors::history_view_color(HistoryView::Notes);
    assert_eq!(title.fg, Some(notes_colour), "the notes view's colour");
    assert!(title.add_modifier.contains(Modifier::BOLD));
    let headliner = at(label, "NOTES", false);
    assert_eq!(
        (title.fg, title.add_modifier),
        (headliner.fg, headliner.add_modifier)
    );
}
