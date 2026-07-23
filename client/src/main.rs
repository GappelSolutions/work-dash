mod app;
mod aquarium;
mod bigtext;
mod net;
mod seed;
mod ui;

use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use ratatui::layout::{Position, Rect};
use ratatui::DefaultTerminal;

use app::{App, Page};
use net::{NetConfig, NetEvent};

const TICK: Duration = Duration::from_millis(50);

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture)?;
    let res = run(&mut terminal);
    execute!(io::stdout(), DisableMouseCapture).ok();
    ratatui::restore();
    res
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let net_config = NetConfig::from_env();
    let (tx, rx) = mpsc::channel::<NetEvent>();
    if let Some(cfg) = net_config.clone() {
        net::spawn(cfg, tx);
    }

    let mut app = App::new(net_config);
    let mut last_tick = Instant::now();

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        let timeout = TICK.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => on_key(&mut app, key),
                Event::Mouse(m) => {
                    let size = terminal.size()?;
                    on_mouse(&mut app, m, Rect::new(0, 0, size.width, size.height));
                }
                _ => {}
            }
        }
        while let Ok(ev) = rx.try_recv() {
            app.apply_net_event(ev);
        }
        if last_tick.elapsed() >= TICK {
            app.on_tick();
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn on_key(app: &mut App, key: KeyEvent) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return;
    }

    // Incoming call: any key dismisses it, and trumps the break overlay.
    if app.call_active.is_some() {
        app.dismiss_call();
        return;
    }

    // Break overlay: any key dismisses it.
    if app.break_active {
        app.break_active = false;
        return;
    }

    // Idle: any key wakes up (q still quits).
    if app.page == Page::Idle {
        if key.code == KeyCode::Char('q') {
            app.should_quit = true;
        } else {
            app.goto(Page::Clock);
        }
        return;
    }

    if app.menu_open {
        match key.code {
            KeyCode::Esc | KeyCode::Char('m') => app.menu_open = false,
            KeyCode::Char('1') => app.goto(Page::Clock),
            KeyCode::Char('2') => app.goto(Page::Kanban),
            KeyCode::Char('3') => app.goto(Page::Calendar),
            KeyCode::Char('q') => app.should_quit = true,
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('m') => app.menu_open = true,
        KeyCode::Char('l') => app.goto(Page::Idle),
        KeyCode::Char('1') => app.goto(Page::Clock),
        KeyCode::Char('2') => app.goto(Page::Kanban),
        KeyCode::Char('3') => app.goto(Page::Calendar),
        _ => on_page_key(app, key.code),
    }
}

fn on_page_key(_app: &mut App, _code: KeyCode) {}

fn on_mouse(app: &mut App, m: MouseEvent, area: Rect) {
    if !matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
        return;
    }
    let pos = Position::new(m.column, m.row);

    // Incoming call: click anywhere dismisses it, and trumps the break overlay.
    if app.call_active.is_some() {
        app.dismiss_call();
        return;
    }

    // Break overlay: click anywhere dismisses it.
    if app.break_active {
        app.break_active = false;
        return;
    }

    // Idle: tap anywhere wakes up.
    if app.page == Page::Idle {
        app.goto(Page::Clock);
        return;
    }

    if app.menu_open {
        let (popup, cells) = ui::menu::rects(area);
        for ((page, _), cell) in ui::menu::ENTRIES.iter().zip(cells) {
            if cell.contains(pos) {
                app.goto(*page);
                return;
            }
        }
        if !popup.contains(pos) {
            app.menu_open = false;
        }
        return;
    }

    if ui::menu_button_rect(area).contains(pos) {
        app.menu_open = true;
        return;
    } else if ui::leave_button_rect(area).contains(pos) {
        app.goto(Page::Idle);
        return;
    }

    if app.page == Page::Kanban {
        for hit in ui::kanban::card_rects(area, app) {
            if hit.rect.contains(pos) {
                app.cycle_card(hit.col, hit.card);
                return;
            }
        }
    }
}
