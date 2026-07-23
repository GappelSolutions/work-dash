pub mod break_screen;
pub mod calendar;
pub mod call_screen;
pub mod clock;
pub mod idle;
pub mod kanban;
pub mod menu;

use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Page};

const BUTTON_HEIGHT: u16 = 3;
const BUTTON_WIDTH: u16 = 11;
const BUTTON_GAP: u16 = 1;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Aquarium covers the whole frame; pages float content windows on top.
    app.aquarium.ensure_size(area.width, area.height);
    f.render_widget(&app.aquarium, area);

    // Incoming call trumps everything, including an in-progress break.
    if app.call_active.is_some() {
        call_screen::draw(f, app, area);
        return;
    }

    // Break alarm trumps every page.
    if app.break_active {
        break_screen::draw(f, app, area);
        return;
    }

    if app.page == Page::Idle {
        idle::draw(f, app, area);
        return;
    }

    match app.page {
        Page::Clock => clock::draw(f, app, area),
        Page::Calendar => calendar::draw(f, app, area),
        Page::Kanban => kanban::draw(f, app, area),
        Page::Idle => unreachable!(),
    }

    draw_corner_buttons(f, area);

    if app.menu_open {
        menu::draw(f, app, area);
    }
}

/// Hit rect of the `[ MENU ]` button, floating bottom-left.
pub fn menu_button_rect(area: Rect) -> Rect {
    let w = BUTTON_WIDTH.min(area.width);
    let h = BUTTON_HEIGHT.min(area.height);
    let x = (area.x + BUTTON_WIDTH + BUTTON_GAP).min(area.right().saturating_sub(w));
    Rect::new(x, area.bottom().saturating_sub(h), w, h)
}

/// Hit rect of the `[ LEAVE ]` button, floating bottom-left.
pub fn leave_button_rect(area: Rect) -> Rect {
    let w = BUTTON_WIDTH.min(area.width);
    let h = BUTTON_HEIGHT.min(area.height);
    Rect::new(area.x, area.bottom().saturating_sub(h), w, h)
}

fn draw_corner_buttons(f: &mut Frame, area: Rect) {
    let leave = leave_button_rect(area);
    f.render_widget(Clear, leave);
    f.render_widget(
        Paragraph::new("LEAVE".yellow().bold())
            .centered()
            .block(Block::bordered()),
        leave,
    );

    let menu = menu_button_rect(area);
    f.render_widget(Clear, menu);
    f.render_widget(
        Paragraph::new("MENU".cyan().bold())
            .centered()
            .block(Block::bordered()),
        menu,
    );
}
