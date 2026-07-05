//! 2x2 app-menu overlay.

use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::widgets::{Block, Clear, Padding, Paragraph};
use ratatui::Frame;

use crate::app::{App, Page};

pub const ENTRIES: [(Page, &str); 4] = [
    (Page::Clock, "1  CLOCK"),
    (Page::Kanban, "2  KANBAN"),
    (Page::Calendar, "3  CALENDAR"),
    (Page::History, "4  NOTIFICATIONS"),
];

/// Popup rect + the four button cells (same order as ENTRIES).
/// Shared by draw and mouse hit-testing.
pub fn rects(area: Rect) -> (Rect, [Rect; 4]) {
    let w = area.width.min(48);
    let h = area.height.min(14);
    let popup = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let inner = popup.inner(Margin::new(1, 1));
    let [top, bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(inner);
    let [a, b] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .spacing(1)
        .areas(top);
    let [c, d] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .spacing(1)
        .areas(bottom);
    (popup, [a, b, c, d])
}

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let (popup, cells) = rects(area);
    f.render_widget(Clear, popup);
    f.render_widget(
        Block::bordered()
            .title(" MENU ".bold())
            .border_style(Style::default().fg(Color::Cyan)),
        popup,
    );

    for ((page, label), cell) in ENTRIES.iter().zip(cells) {
        let active = app.page == *page;
        let border = if active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let text = if active {
            label.cyan().bold()
        } else {
            label.white()
        };
        let pad_top = cell.height.saturating_sub(3) / 2;
        let block = Block::bordered()
            .border_style(border)
            .padding(Padding::new(0, 0, pad_top, 0));
        f.render_widget(Paragraph::new(text).centered().block(block), cell);
    }
}
