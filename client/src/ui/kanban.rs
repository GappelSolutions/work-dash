use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Phase};

/// Card content, always at least this many text lines tall (inside the border).
/// Frame total = this + 2 border lines, so 1 here means a 3-line-tall frame.
const MIN_CARD_LINES: u16 = 1;

pub struct CardHit {
    pub col: usize,
    pub card: usize,
    pub rect: Rect,
}

fn phase_color(phase: Phase) -> Color {
    match phase {
        Phase::Untouched => Color::Gray,
        Phase::Wip => Color::Yellow,
        Phase::Done => Color::Green,
    }
}

/// Greedy word-wrap line count for `text` at `width` columns — mirrors Ratatui's `Wrap`.
fn wrap_count(text: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    let mut lines: u16 = 1;
    let mut col = 0usize;
    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        let add = if col == 0 { word_len } else { word_len + 1 };
        if col != 0 && col + add - word_len > width {
            lines += 1;
            col = word_len.min(width);
        } else {
            col += add;
            if col > width {
                lines += 1;
                col = word_len.min(width);
            }
        }
    }
    lines.max(1)
}

/// Every card's screen rect, keyed by (column index, card index).
pub fn card_rects(area: Rect, app: &App) -> Vec<CardHit> {
    let area = area.inner(Margin::new(3, 1));
    let n = app.columns.len();
    let cols = Layout::horizontal(vec![Constraint::Ratio(1, n as u32); n]).split(area);

    let mut hits = Vec::new();
    for (i, (col, rect)) in app.columns.iter().zip(cols.iter()).enumerate() {
        // Inside the column border, below the title line.
        let inner = rect.inner(Margin::new(1, 1));
        let mut y = inner.y;
        for (j, card) in col.cards.iter().enumerate() {
            if y >= inner.bottom() {
                break;
            }
            let text_lines = wrap_count(&card.text, inner.width.saturating_sub(2)).max(MIN_CARD_LINES);
            let h = text_lines + 2; // + top/bottom border
            let card_rect = Rect::new(inner.x, y, inner.width, h);
            hits.push(CardHit {
                col: i,
                card: j,
                rect: card_rect,
            });
            y += h;
        }
    }
    hits
}

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    // Kanban needs density — keep most of the frame, thin aquarium border.
    let area = area.inner(Margin::new(3, 1));
    f.render_widget(Clear, area);
    let n = app.columns.len();
    let cols = Layout::horizontal(vec![Constraint::Ratio(1, n as u32); n]).split(area);

    for (col, rect) in app.columns.iter().zip(cols.iter()) {
        let title = format!(" {} ({}) ", col.title, col.cards.len());
        f.render_widget(
            Block::bordered()
                .title(title.bold())
                .border_style(Style::default().fg(Color::DarkGray)),
            *rect,
        );

        let inner = rect.inner(Margin::new(1, 1));
        if col.cards.is_empty() {
            f.render_widget(Paragraph::new("  (empty)".dark_gray()), inner);
            continue;
        }

        let mut y = inner.y;
        for card in col.cards.iter() {
            if y >= inner.bottom() {
                break;
            }
            let text_lines = wrap_count(&card.text, inner.width.saturating_sub(2)).max(MIN_CARD_LINES);
            let h = (text_lines + 2).min(inner.bottom().saturating_sub(y));
            let card_rect = Rect::new(inner.x, y, inner.width, h);
            let color = phase_color(card.phase);

            f.render_widget(
                Paragraph::new(Line::from(format!(" {} ", card.text)))
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(color))
                    .block(Block::bordered().border_style(Style::default().fg(color))),
                card_rect,
            );
            y += h;
        }
    }
}
