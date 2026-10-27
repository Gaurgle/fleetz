use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::colors;

/// One key and what it does. A description longer than a column holds is
/// split by hand into continuation lines, drawn under the first one.
struct Row {
    key: &'static str,
    desc: &'static [&'static str],
}

/// A titled run of related keys. A group is never split across columns.
struct Group {
    title: &'static str,
    rows: &'static [Row],
}

/// Every key the normal-mode handler (`main.rs::handle_normal_input`) and the
/// mouse handler respond to, plus Esc, which every overlay and the search box
/// share, and the search box's Enter. Keys that only mean something inside an
/// overlay (settings, time window, branches) are listed by that overlay's own
/// hint lines.
///
/// The `,` row's words are tied to `app::SETTING_FIELDS` labels by a test, so
/// a relabelled or added setting fails it rather than leaving this stale.
const GROUPS: [Group; 6] = [
    Group {
        title: "Navigation",
        rows: &[
            Row {
                key: "j/k ↑/↓",
                desc: &["move in the focused pane"],
            },
            Row {
                key: "J/K ⇧↑/↓",
                desc: &["scroll the next pane"],
            },
            Row {
                key: "PgUp/PgDn",
                desc: &["page detail or inspector"],
            },
            Row {
                key: "Tab ⇧Tab",
                desc: &["focus next / previous pane"],
            },
            Row {
                key: "1 2 3 4",
                desc: &["focus; again folds 3 and 4"],
            },
            Row {
                key: "h/l ←/→",
                desc: &["strip: day cursor by week"],
            },
            Row {
                key: "Home/End",
                desc: &["strip: first / last week"],
            },
            Row {
                key: "mouse",
                desc: &["click focus, [3]/[4] folds", "wheel, drag grip or strip"],
            },
        ],
    },
    Group {
        title: "Panes and views",
        rows: &[
            Row {
                key: "v",
                desc: &["detail / compact view"],
            },
            Row {
                key: "[ ]",
                desc: &["previous / next repo view"],
            },
            // One row for the two in-view expanders: neither column has a
            // row to spare at 80x24.
            Row {
                key: "B m",
                desc: &["info remotes / notes .md"],
            },
            Row {
                key: "c",
                desc: &["strip source: GitHub/local"],
            },
            Row {
                key: "< > + -",
                desc: &["resize list / inspector"],
            },
            Row {
                key: "=",
                desc: &["reset pane sizes"],
            },
        ],
    },
    Group {
        title: "Refresh and fetch",
        rows: &[
            Row {
                key: "r",
                desc: &["re-stat repos, poll GitHub"],
            },
            Row {
                key: "R",
                desc: &["reload GitHub, rediscover"],
            },
            Row {
                key: "f/F",
                desc: &["fetch this repo / all"],
            },
            Row {
                key: "w",
                desc: &["watch: timed sweep + fetch"],
            },
        ],
    },
    Group {
        title: "Filter and window",
        rows: &[
            Row {
                key: "/",
                desc: &[
                    "search, or filter by tag:",
                    "#dirty #behind #ahead #ci",
                    "#prs #stash; Enter keeps",
                ],
            },
            Row {
                key: "0",
                desc: &["clear #tags, keep text"],
            },
            Row {
                key: "g/G",
                desc: &["next group / all groups"],
            },
            Row {
                key: "t",
                desc: &["pick the activity window"],
            },
        ],
    },
    Group {
        title: "Open and overlays",
        rows: &[
            Row {
                key: "Enter",
                desc: &["lazygit; notes: $EDITOR"],
            },
            Row {
                key: "e",
                desc: &["$EDITOR in this repo"],
            },
            Row {
                key: "o",
                desc: &["shell in this repo"],
            },
            Row {
                key: "O",
                desc: &["first PR in the browser"],
            },
            Row {
                key: "y",
                desc: &["copy hash (or click it)"],
            },
            Row {
                key: "b s",
                desc: &["branches: switch, clean up"],
            },
            Row {
                key: ",",
                desc: &[
                    "settings: sweep and poll",
                    "intervals, watch, view,",
                    "work hours, pane sizes,",
                    "contrib, loading, icons",
                ],
            },
            Row {
                key: "?",
                desc: &["this help"],
            },
        ],
    },
    Group {
        title: "Close and quit",
        rows: &[
            Row {
                key: "Esc",
                desc: &["close, or clear the search"],
            },
            Row {
                key: "q Ctrl-c",
                desc: &["quit"],
            },
        ],
    },
];

/// How many of `GROUPS` go in the left column when there are two. Chosen so
/// both columns fit the inner 22 rows of an 80x24 terminal.
const LEFT_GROUPS: usize = 3;

/// Blank cells between a key and its description.
const KEY_GAP: usize = 2;
/// Blank cells between the two columns.
const COLUMN_GAP: u16 = 2;
/// Blank cells between the border and the text, on each side.
const SIDE_PAD: u16 = 1;

/// Keys get a color of their own, so the eye finds the key column first and
/// reads the gray description beside it second. The footer hints keep `TEXT`
/// for their keys; the two differ on purpose until the owner says otherwise.
const KEY_COLOR: Color = colors::SAPPHIRE;
/// What a key does: subtle beside the key it explains.
const DESC_COLOR: Color = colors::OVERLAY;
/// Group titles, in the palette's section-label color.
const TITLE_COLOR: Color = colors::MAUVE;

/// Render the help overlay centered on screen.
///
/// Two columns when the area is wide enough for both, one otherwise. The box
/// is sized from its content, clamped to the area; a column too tall for the
/// box drops whole rows from the bottom rather than cutting one in half.
/// Narrower than one column with its border and padding, the text clips at
/// the right edge: nothing smaller than 40x12 is designed for.
pub fn render(frame: &mut Frame, area: Rect) {
    draw(frame, area, &GROUPS, LEFT_GROUPS);
}

fn draw(frame: &mut Frame, area: Rect, groups: &[Group], left_groups: usize) {
    let key_width = groups
        .iter()
        .flat_map(|g| g.rows)
        .map(|r| Span::raw(r.key).width())
        .max()
        .unwrap_or(0);
    let column_width = groups
        .iter()
        .flat_map(|g| {
            let title = Span::raw(g.title).width();
            g.rows
                .iter()
                .flat_map(|r| r.desc)
                .map(move |d| (key_width + KEY_GAP + Span::raw(*d).width()).max(title))
        })
        .max()
        .unwrap_or(0) as u16;

    let split = left_groups.min(groups.len());
    let two_wide = 2 + SIDE_PAD + column_width + COLUMN_GAP + column_width + SIDE_PAD;
    let columns: Vec<Vec<Vec<Line>>> = if area.width >= two_wide {
        vec![
            blocks(&groups[..split], key_width),
            blocks(&groups[split..], key_width),
        ]
    } else {
        vec![blocks(groups, key_width)]
    };
    let natural_width = if columns.len() == 2 {
        two_wide
    } else {
        2 + SIDE_PAD + column_width + SIDE_PAD
    };
    let tallest = columns
        .iter()
        .map(|c| c.iter().map(Vec::len).sum::<usize>())
        .max()
        .unwrap_or(0) as u16;

    let popup_area = centered_fixed(
        natural_width.min(area.width),
        (tallest + 2).min(area.height),
        area,
    );
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::LAVENDER))
        .title(Span::styled(
            " Keybindings ",
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ))
        // In the frame, not a row of its own: every inner row is a key row.
        .title_bottom(
            Line::from(Span::styled(
                " ? or Esc to close ",
                Style::default().fg(colors::SUBTEXT),
            ))
            .right_aligned(),
        );
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let mut x = inner.x + SIDE_PAD;
    for column in columns {
        let column_area = Rect::new(x, inner.y, column_width, inner.height).intersection(inner);
        let lines = whole_blocks(column, inner.height as usize);
        frame.render_widget(Paragraph::new(lines), column_area);
        x = x.saturating_add(column_width + COLUMN_GAP);
    }
}

/// The lines of `groups`, in blocks that must be shown whole: a group's title
/// travels with its first row, and a row with its continuation lines.
fn blocks(groups: &[Group], key_width: usize) -> Vec<Vec<Line<'static>>> {
    let key_style = Style::default().fg(KEY_COLOR).add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(DESC_COLOR);
    let title_style = Style::default()
        .fg(TITLE_COLOR)
        .add_modifier(Modifier::BOLD);

    let mut out = Vec::new();
    for group in groups {
        for (i, row) in group.rows.iter().enumerate() {
            let mut lines = Vec::new();
            if i == 0 {
                lines.push(Line::from(Span::styled(group.title, title_style)));
            }
            // Padded by drawn width rather than a format width, which counts
            // chars: a key name with a wide glyph would push its column out.
            let pad = key_width - Span::raw(row.key).width() + KEY_GAP;
            for (n, desc) in row.desc.iter().enumerate() {
                let key = if n == 0 { row.key } else { "" };
                let pad = if n == 0 { pad } else { key_width + KEY_GAP };
                lines.push(Line::from(vec![
                    Span::styled(key, key_style),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(*desc, desc_style),
                ]));
            }
            out.push(lines);
        }
    }
    out
}

/// As many leading blocks as fit in `height` rows, each one whole.
fn whole_blocks(blocks: Vec<Vec<Line<'static>>>, height: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for block in blocks {
        if lines.len() + block.len() > height {
            break;
        }
        lines.extend(block);
    }
    lines
}

/// Return a centered Rect of fixed `width` x `height` within `area`.
fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{FILTER_TAGS, SETTING_FIELDS, SettingField};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// The screen after drawing `groups` into a `width` x `height` terminal,
    /// one string per row.
    fn screen_with(groups: &[Group], left: usize, width: u16, height: u16) -> Vec<String> {
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|f| draw(f, f.area(), groups, left)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    fn screen(width: u16, height: u16) -> Vec<String> {
        screen_with(&GROUPS, LEFT_GROUPS, width, height)
    }

    /// Rows whose key, first description line and every continuation line
    /// are not all on screen, the key directly left of its description.
    fn rows_not_shown<'a>(groups: &'a [Group], screen: &[String]) -> Vec<&'a str> {
        groups
            .iter()
            .flat_map(|g| g.rows)
            .filter(|row| {
                let paired = screen.iter().any(|line| {
                    line.match_indices(row.desc[0])
                        .any(|(at, _)| line[..at].trim_end().ends_with(row.key))
                });
                let rest = row.desc[1..]
                    .iter()
                    .all(|d| screen.iter().any(|line| line.contains(d)));
                !(paired && rest)
            })
            .map(|row| row.key)
            .collect()
    }

    /// Top-left and bottom-right corners of the overlay's frame.
    fn frame_corners(screen: &[String]) -> ((usize, usize), (usize, usize)) {
        let find = |corner: char| {
            screen
                .iter()
                .enumerate()
                .find_map(|(y, line)| line.chars().position(|c| c == corner).map(|x| (x, y)))
                .unwrap_or_else(|| panic!("no {corner} on screen:\n{}", screen.join("\n")))
        };
        (find('┌'), find('┘'))
    }

    fn all_text() -> Vec<&'static str> {
        GROUPS
            .iter()
            .flat_map(|g| {
                std::iter::once(g.title).chain(
                    g.rows
                        .iter()
                        .flat_map(|r| std::iter::once(r.key).chain(r.desc.iter().copied())),
                )
            })
            .collect()
    }

    #[test]
    fn every_row_is_whole_on_an_80x24_screen() {
        let screen = screen(80, 24);
        let missing = rows_not_shown(&GROUPS, &screen);
        assert!(
            missing.is_empty(),
            "not shown at 80x24: {missing:?}\n{}",
            screen.join("\n")
        );
        for group in &GROUPS {
            assert!(
                screen.iter().any(|l| l.contains(group.title)),
                "{}",
                group.title
            );
        }
    }

    /// Guards the guard: rows added past what a column holds must be reported
    /// as not shown, so `every_row_is_whole_on_an_80x24_screen` fails on them.
    /// Adds 22 rows, a whole 80x24 column's worth, so it overflows however
    /// full the left column is today.
    #[test]
    fn a_row_that_does_not_fit_at_80x24_is_caught() {
        let mut rows: Vec<Row> = GROUPS[0]
            .rows
            .iter()
            .map(|r| Row {
                key: r.key,
                desc: r.desc,
            })
            .collect();
        for _ in 0..22 {
            rows.push(Row {
                key: "X",
                desc: &["one row too many"],
            });
        }
        let rows: &'static [Row] = Box::leak(rows.into_boxed_slice());
        let mut groups: Vec<Group> = GROUPS
            .iter()
            .map(|g| Group {
                title: g.title,
                rows: g.rows,
            })
            .collect();
        groups[0].rows = rows;

        let screen = screen_with(&groups, LEFT_GROUPS, 80, 24);
        assert!(
            !rows_not_shown(&groups, &screen).is_empty(),
            "an overfull column went unnoticed:\n{}",
            screen.join("\n")
        );
    }

    /// The keys the event loop handles that the old help left out.
    #[test]
    fn home_end_and_ctrl_c_are_listed_and_visible() {
        let screen = screen(80, 24).join("\n");
        for key in ["Home/End", "Ctrl-c", "⇧Tab", "g/G", "f/F"] {
            assert!(screen.contains(key), "{key} missing:\n{screen}");
        }
    }

    #[test]
    fn every_filter_tag_is_named() {
        let screen = screen(80, 24).join("\n");
        for (_, tag) in FILTER_TAGS {
            assert!(screen.contains(&format!("#{tag}")), "#{tag} missing");
        }
    }

    /// Each setting is named in the `,` row by a word that also appears in its
    /// label. Relabelling a setting so its word no longer matches fails here,
    /// and a new `SettingField` fails to compile until it is given a word.
    #[test]
    fn the_settings_row_names_every_setting() {
        let row = GROUPS
            .iter()
            .flat_map(|g| g.rows)
            .find(|r| r.key == ",")
            .expect("a , row");
        let text = row.desc.join(" ");
        for field in SETTING_FIELDS {
            let word = match field {
                SettingField::RefreshInterval => "sweep",
                SettingField::GithubPollInterval => "poll",
                SettingField::Watch => "watch",
                SettingField::View => "view",
                SettingField::WorkStart | SettingField::WorkEnd => "work",
                SettingField::PersistPanes => "pane",
                SettingField::Contributions | SettingField::ContribCellSize => "contrib",
                SettingField::Intro => "loading",
                SettingField::Icons => "icons",
            };
            assert!(
                field.label().contains(word),
                "{:?} is labelled {:?}, which no longer says {word:?}: reword the , row",
                field,
                field.label()
            );
            assert!(text.contains(word), ", row does not name {word:?}: {text}");
        }
    }

    #[test]
    fn small_screens_show_whole_rows_and_do_not_panic() {
        for (width, height) in [(60, 20), (40, 12)] {
            let screen = screen(width, height);
            let hidden = rows_not_shown(&GROUPS, &screen);
            let shown: Vec<&str> = GROUPS
                .iter()
                .flat_map(|g| g.rows)
                .map(|r| r.key)
                .filter(|k| !hidden.contains(k))
                .collect();
            assert!(!shown.is_empty(), "{width}x{height}: nothing shown");
            assert_whole_rows(&screen, &format!("{width}x{height}"));
            // As many as fit: the box fills the height it was given.
            let ((_, top), (_, bottom)) = frame_corners(&screen);
            assert_eq!((top, bottom), (0, height as usize - 1), "{width}x{height}");
        }
    }

    /// Whether `row` has started on screen: its key directly left of the
    /// first few cells of its description, which a cut at the bottom (the
    /// continuation lines missing) or at the right edge still leaves visible.
    fn row_started(row: &Row, screen: &[String]) -> bool {
        let start: String = row.desc[0].chars().take(8).collect();
        screen.iter().any(|line| {
            line.match_indices(&start)
                .any(|(at, _)| line[..at].trim_end().ends_with(row.key))
        })
    }

    /// Every row is complete or absent, and no group title is left on screen
    /// without its first row.
    fn assert_whole_rows(screen: &[String], size: &str) {
        let hidden = rows_not_shown(&GROUPS, screen);
        for row in GROUPS.iter().flat_map(|g| g.rows) {
            assert!(
                !(row_started(row, screen) && hidden.contains(&row.key)),
                "{size}: {} only partly shown:\n{}",
                row.key,
                screen.join("\n")
            );
        }
        for group in &GROUPS {
            if screen.iter().any(|l| l.contains(group.title)) {
                assert!(
                    row_started(&group.rows[0], screen),
                    "{size}: {} shown without its first row:\n{}",
                    group.title,
                    screen.join("\n")
                );
            }
        }
    }

    /// Every height from the smallest box up, in two columns (80) and one (60,
    /// 40), so a multi-line row or a group title meets the bottom edge at
    /// some height whatever the text is: at 80x5 the `/` row, at 80x11 the
    /// `mouse` row and the "Open and overlays" title, at 80x18 the `,` row.
    #[test]
    fn rows_are_never_cut_at_the_bottom() {
        for width in [80, 60, 40] {
            for height in 3..=30 {
                assert_whole_rows(&screen(width, height), &format!("{width}x{height}"));
            }
        }
    }

    #[test]
    fn a_narrow_screen_gets_one_column() {
        let screen = screen(60, 50);
        let missing = rows_not_shown(&GROUPS, &screen);
        assert!(missing.is_empty(), "{missing:?}\n{}", screen.join("\n"));
        // One column: the first group's last row sits above the next title.
        let at = |needle: &str| screen.iter().position(|l| l.contains(needle)).unwrap();
        assert!(at("mouse") < at("Panes and views"));
        assert!(at("Panes and views") < at("Refresh and fetch"));
        assert!(at("Refresh and fetch") < at("Filter and window"));
    }

    #[test]
    fn a_large_screen_keeps_the_box_sized_to_its_content() {
        let small = frame_corners(&screen(80, 24));
        let large = frame_corners(&screen(120, 40));
        let size = |((x0, y0), (x1, y1)): ((usize, usize), (usize, usize))| (x1 - x0, y1 - y0);
        assert_eq!(size(large), size(small), "the box grew with the screen");
        let ((x0, y0), (x1, y1)) = large;
        assert!(x0 > 0 && y0 > 0 && x1 < 119 && y1 < 39, "not centred");
    }

    /// Keys, descriptions and group titles are each drawn in their own color,
    /// read from the cells, so a key stands out from the text explaining it.
    #[test]
    fn keys_descriptions_and_titles_have_distinct_colors() {
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| render(f, f.area())).unwrap();
        let buf = term.backend().buffer().clone();
        // The cell where `needle` starts, matched one cell per char.
        let fg_at = |needle: &str| {
            let chars: Vec<String> = needle.chars().map(String::from).collect();
            (0..24u16)
                .find_map(|y| {
                    (0..80u16)
                        .find(|&x| {
                            chars.iter().enumerate().all(|(i, c)| {
                                x + (i as u16) < 80 && buf[(x + i as u16, y)].symbol() == c
                            })
                        })
                        .map(|x| buf[(x, y)].fg)
                })
                .unwrap_or_else(|| panic!("{needle} not on screen"))
        };
        assert_eq!(fg_at("PgUp/PgDn"), KEY_COLOR, "key");
        assert_eq!(fg_at("page detail"), DESC_COLOR, "description");
        assert_eq!(fg_at("Navigation"), TITLE_COLOR, "group title");
        assert_ne!(KEY_COLOR, DESC_COLOR);
        assert_ne!(KEY_COLOR, TITLE_COLOR);
        assert_ne!(DESC_COLOR, TITLE_COLOR);
    }

    #[test]
    fn no_em_or_en_dash_in_the_help() {
        let dashes = ['\u{2014}', '\u{2013}'];
        for text in all_text() {
            assert!(!text.contains(dashes), "dash in {text:?}");
        }
        let source = include_str!("help.rs");
        assert!(!source.contains(dashes), "dash in help.rs");
    }
}
