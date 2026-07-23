//! Incoming-call banner — trumps everything, including an in-progress break
//! (see `ui::draw`). Styled like `clock.rs`'s NEXT BREAK panel (bordered
//! block, centered text) rather than the strobing full-screen break alarm:
//! a call needs to read clearly at a glance, not blare.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let caller = app.call_active.as_deref().unwrap_or("Unknown caller");

    let lines = vec![
        Line::from("INCOMING CALL".green().bold()),
        Line::default(),
        Line::from(caller.to_string().white().bold()),
        Line::default(),
        Line::from("press any key to dismiss".gray()),
    ];

    let box_w = (caller.chars().count() as u16 + 12).max(28).min(area.width);
    let box_h = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(box_w) / 2,
        area.y + area.height.saturating_sub(box_h) / 2,
        box_w,
        box_h,
    );

    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(Color::Green));
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).centered().block(block), popup);
}
