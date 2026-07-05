use chrono::Local;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let now = Local::now();
    let next_idx = app.events.iter().position(|e| e.start > now);

    let mut lines: Vec<Line> = vec![Line::default()];
    for (i, ev) in app.events.iter().enumerate() {
        let current = ev.start <= now && now < ev.end;
        let (marker, style) = if current {
            ("►", Style::default().fg(Color::Green).bold())
        } else if ev.end <= now {
            (" ", Style::default().fg(Color::DarkGray))
        } else if Some(i) == next_idx {
            ("▷", Style::default().fg(Color::Yellow))
        } else {
            (" ", Style::default())
        };

        let mut spans = vec![
            Span::styled(format!(" {marker} "), style),
            Span::styled(
                format!("{}–{}", ev.start.format("%H:%M"), ev.end.format("%H:%M")),
                style,
            ),
            Span::styled(format!("  {}", ev.title), style),
        ];
        if let Some(place) = &ev.place {
            spans.push(Span::styled(
                format!("  ({place})"),
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines.push(Line::from(spans));
        lines.push(Line::default());
    }

    let next_line = if let Some(i) = next_idx {
        let ev = &app.events[i];
        let mins = (ev.start - now).num_minutes();
        let mut spans = vec![
            Span::raw(format!("{} in ", ev.title)),
            Span::styled(format!("{mins} min left"), Style::default().fg(Color::Yellow)),
        ];
        if let Some(place) = &ev.place {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(place.clone(), Style::default().fg(Color::Cyan)));
        }
        Line::from(spans)
    } else if app.events.iter().any(|e| e.start <= now && now < e.end) {
        Line::default()
    } else {
        Line::from("done for today".green())
    };

    let title = format!(" CALENDAR — {} ", now.format("%A, %d %B"));

    // Floating window sized to content; aquarium stays visible around it.
    let w = 72.min(area.width);
    let next_h = 3u16;
    let h = (lines.len() as u16 + 3).min(area.height.saturating_sub(next_h));
    let total_h = (h + next_h).min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - total_h) / 2;

    let next_win = Rect::new(x, y, w, next_h);
    let win = Rect::new(x, y + next_h, w, h);

    f.render_widget(Clear, next_win);
    f.render_widget(
        Paragraph::new(next_line)
            .alignment(Alignment::Center)
            .block(Block::bordered().title(" Next Appointment ".bold())),
        next_win,
    );

    f.render_widget(Clear, win);
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title.bold())),
        win,
    );
}
