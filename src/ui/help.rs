use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::colors;

/// Render the help overlay centered on screen.
pub fn render(frame: &mut Frame, area: Rect) {
    let popup_area = centered_rect(60, 70, area);

    // Clear the area behind the popup
    frame.render_widget(Clear, popup_area);

    let keybinds = [
        (
            "1 / 2 / 3",
            "Focus repo list / detail / inspector (Tab cycles)",
        ),
        (
            "mouse",
            "Click a panel to focus · scroll wheel scrolls any panel",
        ),
        (
            "j/k ↑/↓",
            "Repo list: move · Detail: pick commit · Inspector: scroll",
        ),
        (
            "⇧ J/K ↑/↓",
            "Scroll the detail pane (works from the repo list too)",
        ),
        ("PgUp/PgDn", "Scroll focused pane by a page"),
        ("v", "Toggle detail ↔ compact view"),
        ("enter", "Open repo in lazygit (pull/merge live here)"),
        ("e", "Open repo in $EDITOR"),
        ("o", "Open shell in repo dir"),
        ("O", "Open PR in browser"),
        ("f", "Fetch selected repo (no merge)"),
        ("F", "Fetch all repos"),
        (
            "b / s",
            "Branches: preview + switch (enter), clean up merged (space+d)",
        ),
        ("t", "Time window: pick how far back to show activity"),
        ("g", "Cycle group filter"),
        ("G", "Show all (clear group)"),
        (
            "/",
            "Search repos, or type #dirty #behind #ahead #ci #prs #stash to filter",
        ),
        ("0", "Clear #tag filters (keeps free-text search)"),
        (
            "r",
            "Refresh: rebuild listed repos only (offline, no fetch)",
        ),
        (
            "R",
            "Reload: re-fetch GitHub + rediscover repos (finds new ones)",
        ),
        (
            "w",
            "Toggle watch: sweep listed repos on the interval, fetch included",
        ),
        ("Esc", "Close search / help"),
        ("?", "Toggle this help"),
        ("q", "Quit"),
    ];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for (key, desc) in keybinds {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:>10}"),
                Style::default()
                    .fg(colors::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("   {desc}"), Style::default().fg(colors::OVERLAY)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press ? or Esc to close",
        Style::default().fg(colors::SURFACE),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::LAVENDER))
        .title(Span::styled(
            " Keybindings ",
            Style::default()
                .fg(colors::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup_area);
}

/// Return a centered Rect of `percent_x` x `percent_y` within `area`.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
