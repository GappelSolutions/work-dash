//! Leave / offline view: aquarium background + centered clock, nothing else.

use chrono::Local;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::bigtext;

pub fn draw(f: &mut Frame, _app: &mut App, area: Rect) {
    // Aquarium already rendered by ui::draw.
    let now = Local::now();
    let time = now.format("%H:%M:%S").to_string();
    let date = now.format("%A, %d %B %Y").to_string();

    let big = bigtext::big_lines(&time);
    let box_w = bigtext::width(&time) + 6;
    let box_h = 5 + 4; // digits + blank + date + borders

    if box_w <= area.width && box_h <= area.height {
        let popup = Rect::new(
            area.x + (area.width - box_w) / 2,
            area.y + (area.height - box_h) / 2,
            box_w,
            box_h,
        );
        f.render_widget(Clear, popup);

        let mut lines: Vec<Line> = big
            .into_iter()
            .map(|l| Line::from(l).style(Style::default().fg(Color::Cyan).bold()))
            .collect();
        lines.push(Line::default());
        lines.push(Line::from(date).style(Style::default().fg(Color::Gray)));

        let block = Block::bordered()
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(Paragraph::new(lines).centered().block(block), popup);
    } else {
        // Tiny terminal fallback: plain one-line clock.
        f.render_widget(
            Paragraph::new(format!("{time}  {date}")).centered(),
            Rect::new(area.x, area.y + area.height / 2, area.width, 1),
        );
    }
}
