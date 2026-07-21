//! Combined clock + notifications page — floating windows over the
//! aquarium background, sized to content so the aquarium stays visible
//! around them.

use chrono::Local;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, CalendarEvent, NotifKind, Phase};
use crate::bigtext;

/// One appointment row: title left (green + marker if in progress, else
/// yellow), room dim gray and time white, right-aligned.
fn appt_line(ev: &CalendarEvent, box_w: u16, active: bool) -> Line<'static> {
    let title_style = if active {
        Style::default().fg(Color::Green).bold()
    } else {
        Style::default().fg(Color::Yellow)
    };
    let left = if active {
        format!("\u{25b8} {}", ev.title)
    } else {
        ev.title.clone()
    };
    let time = ev.start.format("%H:%M").to_string();
    let right_len = ev.place.as_ref().map_or(0, |p| p.chars().count() + 2) + time.chars().count();
    let inner_w = box_w.saturating_sub(2) as usize;
    let gap_w = inner_w
        .saturating_sub(left.chars().count())
        .saturating_sub(right_len)
        .max(1);

    let mut spans = vec![Span::styled(left, title_style), Span::raw(" ".repeat(gap_w))];
    if let Some(place) = &ev.place {
        spans.push(Span::styled(format!("{place}  "), Style::default().fg(Color::DarkGray)));
    }
    spans.push(Span::styled(time, Style::default().fg(Color::White)));
    Line::from(spans)
}

/// Width of the clock column's floating windows. Shared so other pages
/// (e.g. notification history) can match the clock page's box width.
pub fn window_width(area: Rect) -> u16 {
    let now = Local::now();
    let time = now.format("%H:%M:%S").to_string();
    let date = now.format("%A, %d %B %Y").to_string();
    let big_w = bigtext::width(&time);
    let clock_w = if big_w + 4 <= area.width && area.height >= 16 {
        big_w.max(date.chars().count() as u16) + 6
    } else {
        (time.len() + date.len() + 8) as u16
    };
    clock_w.min(area.width)
}

/// Height reserved at the top of the page for the offline banner, whether or
/// not it's currently shown — keeps the centered stack below from jumping
/// when the connection drops/recovers.
const OFFLINE_BANNER_H: u16 = 3;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let now = Local::now();

    let show_offline_banner =
        app.networked() && !app.connected && area.height > OFFLINE_BANNER_H;
    let area = if show_offline_banner {
        let banner = Rect::new(area.x, area.y, area.width, OFFLINE_BANNER_H);
        f.render_widget(Clear, banner);
        f.render_widget(
            Paragraph::new(Line::from(
                "  SERVER UNREACHABLE — showing last known data  "
                    .red()
                    .bold(),
            ))
            .centered()
            .block(Block::bordered().border_style(Style::default().fg(Color::Red))),
            banner,
        );
        Rect::new(
            area.x,
            area.y + OFFLINE_BANNER_H,
            area.width,
            area.height - OFFLINE_BANNER_H,
        )
    } else {
        area
    };
    let time = now.format("%H:%M:%S").to_string();
    let date = now.format("%A, %d %B %Y").to_string();

    // Clock window: sized to the big digits.
    let big_w = bigtext::width(&time);
    let use_big = big_w + 4 <= area.width && area.height >= 16;
    let clock_h = if use_big { 9 } else { 3 };
    let clock_w = window_width(area);
    let clock_h = clock_h.min(area.height);

    let lines: Vec<Line> = if use_big {
        let mut l: Vec<Line> = bigtext::big_lines(&time)
            .into_iter()
            .map(|s| Line::from(s).style(Style::default().fg(Color::Cyan).bold()))
            .collect();
        l.push(Line::default());
        l.push(Line::from(date.clone().gray()));
        l
    } else {
        vec![Line::from(vec![
            Span::styled(time.clone(), Style::default().fg(Color::Cyan).bold()),
            Span::raw("  "),
            Span::styled(date.clone(), Style::default().fg(Color::Gray)),
        ])]
    };

    // Next two appointments: row 1 is the active meeting if there is one,
    // else the next upcoming; row 2 is whichever of next/second-next follows.
    let current_idx = app.events.iter().position(|e| e.start <= now && now < e.end);
    let upcoming: Vec<usize> = app
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.start > now)
        .map(|(i, _)| i)
        .collect();
    let (appt_row1, appt_row2) = if let Some(ci) = current_idx {
        (Some((ci, true)), upcoming.first().map(|&i| (i, false)))
    } else {
        (
            upcoming.first().map(|&i| (i, false)),
            upcoming.get(1).map(|&i| (i, false)),
        )
    };
    let appt_h_raw = 4;

    // Notifications list — latest 3, newest on top, sized to content.
    let mut items: Vec<Line> = Vec::new();
    for n in app.notifications.iter().take(3) {
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
    let notif_h_raw = if items.is_empty() { 0 } else { items.len() as u16 + 2 };

    // Active (WIP) tasks across all kanban columns — hidden entirely when none.
    let task_lines: Vec<Line> = app
        .columns
        .iter()
        .flat_map(|c| c.cards.iter())
        .filter(|c| c.phase == Phase::Wip)
        .map(|c| Line::from(c.text.clone().yellow()))
        .collect();
    let tasks_h_raw = if task_lines.is_empty() { 0 } else { task_lines.len() as u16 + 2 };

    // Next-break countdown, sized to content.
    let remaining = app.time_until_break();
    let break_text = format!(
        "{}m {:02}s until next break",
        remaining.num_minutes(),
        remaining.num_seconds() % 60
    );

    let break_h_raw = 3;

    // Every window shares the clock's width — it reads best as one column.
    let box_w = clock_w;

    // Stack break countdown, clock, next appointment and notifications as
    // one block, centered vertically (and horizontally) in the page instead
    // of pinned to the top.
    let gap = 1;
    let tasks_block_h = if tasks_h_raw > 0 { gap + tasks_h_raw } else { 0 };
    let notif_block_h = if notif_h_raw > 0 { gap + notif_h_raw } else { 0 };
    let total_h =
        break_h_raw + gap + clock_h + tasks_block_h + gap + appt_h_raw + notif_block_h;
    let start_y = if total_h <= area.height {
        area.y + (area.height - total_h) / 2
    } else {
        area.y
    };
    let box_x = area.x + (area.width - box_w) / 2;

    let break_box = Rect::new(box_x, start_y, box_w, break_h_raw.min(area.height));
    f.render_widget(Clear, break_box);
    f.render_widget(
        Paragraph::new(Line::from(break_text.magenta()))
            .centered()
            .block(Block::bordered().title(" NEXT BREAK ".bold())),
        break_box,
    );

    let clock_y = break_box.bottom() + gap;
    if clock_y >= area.bottom() {
        return;
    }
    let clock_h = clock_h.min(area.bottom() - clock_y);
    let clock = Rect::new(box_x, clock_y, box_w, clock_h);
    f.render_widget(Clear, clock);
    f.render_widget(
        Paragraph::new(lines)
            .centered()
            .block(Block::bordered().title(" CLOCK ".bold())),
        clock,
    );

    let mut next_y = clock.bottom() + gap;
    if tasks_h_raw > 0 {
        if next_y < area.bottom() {
            let tasks_h = tasks_h_raw.min(area.bottom() - next_y);
            let tasks_box = Rect::new(box_x, next_y, box_w, tasks_h);
            f.render_widget(Clear, tasks_box);
            f.render_widget(
                Paragraph::new(task_lines).block(Block::bordered().title(" ACTIVE TASKS ".bold())),
                tasks_box,
            );
            next_y = tasks_box.bottom() + gap;
        }
    }

    let appt_y = next_y;
    if appt_y >= area.bottom() {
        return;
    }
    let appt_h = appt_h_raw.min(area.bottom() - appt_y);
    let appt_box = Rect::new(box_x, appt_y, box_w, appt_h);
    f.render_widget(Clear, appt_box);

    let appt_lines: Vec<Line> = vec![
        match appt_row1 {
            Some((i, active)) => appt_line(&app.events[i], box_w, active),
            None => Line::from("no more appointments today".dark_gray()),
        },
        match appt_row2 {
            Some((i, active)) => appt_line(&app.events[i], box_w, active),
            None => Line::default(),
        },
    ];
    f.render_widget(
        Paragraph::new(appt_lines).block(Block::bordered().title(" APPOINTMENTS ".bold())),
        appt_box,
    );

    if notif_h_raw == 0 {
        return;
    }
    let notif_y = appt_box.bottom() + gap;
    if notif_y >= area.bottom() {
        return;
    }
    let notif_h = notif_h_raw.min(area.bottom() - notif_y);
    let notif = Rect::new(box_x, notif_y, box_w, notif_h);
    f.render_widget(Clear, notif);
    f.render_widget(
        Paragraph::new(items).block(Block::bordered().title(" NOTIFICATIONS ".bold())),
        notif,
    );
}
