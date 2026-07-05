//! Notification history page — newest on top, capped at MAX_NOTIFICATIONS.
//! Floating window: 50% width, height fit to content, centered over aquarium.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, NotifKind, MAX_NOTIFICATIONS};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<Line> = Vec::new();
    for n in app.notifications.iter().take(MAX_NOTIFICATIONS) {
        let (tag, style) = match n.kind {
            NotifKind::Call => ("[CALL]", Style::default().fg(Color::Red).bold()),
            NotifKind::Reminder => ("[REM ]", Style::default().fg(Color::Yellow)),
            NotifKind::Break => ("[BRK ]", Style::default().fg(Color::Magenta)),
            NotifKind::Info => ("[INFO]", Style::default().fg(Color::DarkGray)),
        };
        items.push(Line::from(vec![
            Span::styled(
                format!(" {} ", n.time.format("%H:%M")),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(tag, style),
            Span::raw("  "),
            Span::raw(n.text.clone()),
        ]));
    }
    if items.is_empty() {
        items.push(Line::from(" no notifications".dark_gray()));
    }

    // Match the clock page's box width; height sized to content (+2 border), centered.
    let w = super::clock::window_width(area);
    let h = (items.len() as u16 + 2).min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let win = Rect::new(x, y, w, h);

    f.render_widget(Clear, win);
    f.render_widget(
        Paragraph::new(items).block(Block::bordered().title(" NOTIFICATIONS ".bold())),
        win,
    );
}
