//! Fullscreen break alarm — strobing overlay that covers everything until
//! the user presses a key or clicks. Deliberately loud.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::bigtext;

/// Strobe palette, cycled by the app tick counter.
const COLORS: [Color; 4] = [
    Color::LightRed,
    Color::LightYellow,
    Color::LightMagenta,
    Color::LightCyan,
];

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    // ~200ms per colour at a 50ms tick.
    let color = COLORS[((app.flash / 4) as usize) % COLORS.len()];

    let word = "BREAK";
    let big = bigtext::big_lines(word);
    let big_w = bigtext::width(word);

    let mut lines: Vec<Line> = big
        .into_iter()
        .map(|l| Line::from(l).style(Style::default().fg(color).bold()))
        .collect();
    lines.push(Line::default());
    lines.push(Line::from("TIME TO STEP AWAY").style(Style::default().fg(color).bold()));
    lines.push(Line::default());
    lines.push(
        Line::from("press any key to dismiss").style(Style::default().fg(Color::Gray)),
    );

    let box_w = (big_w + 8).min(area.width);
    let box_h = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(box_w) / 2,
        area.y + area.height.saturating_sub(box_h) / 2,
        box_w,
        box_h,
    );

    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).centered().block(block), popup);
}
