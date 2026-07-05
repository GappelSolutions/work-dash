use chrono::{DateTime, Duration, Local};

use crate::aquarium::AquariumField;
use crate::seed;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Clock,
    Calendar,
    Kanban,
    /// Notification history: newest on top, capped at 10.
    History,
    /// Leave / laptop-disconnected view: clock + aquarium only.
    Idle,
}

/// Most-recent-first cap for the notification history.
pub const MAX_NOTIFICATIONS: usize = 10;

pub enum NotifKind {
    Call,
    Reminder,
    Break,
    Info,
}

pub struct Notification {
    pub time: DateTime<Local>,
    pub kind: NotifKind,
    pub text: String,
}

pub struct CalendarEvent {
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub title: String,
    pub place: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Untouched,
    Wip,
    Done,
}

impl Phase {
    pub fn next(self) -> Phase {
        match self {
            Phase::Untouched => Phase::Wip,
            Phase::Wip => Phase::Done,
            Phase::Done => Phase::Untouched,
        }
    }
}

pub struct Card {
    pub text: String,
    pub phase: Phase,
}

pub struct Column {
    pub title: String,
    pub cards: Vec<Card>,
}

pub struct App {
    pub page: Page,
    pub menu_open: bool,
    pub should_quit: bool,
    pub aquarium: AquariumField,
    pub next_break: DateTime<Local>,
    /// Set when a break alarm fires; drives the fullscreen break overlay.
    pub break_active: bool,
    /// Free-running tick counter, used to strobe the break overlay.
    pub flash: u32,
    pub events: Vec<CalendarEvent>,
    pub notifications: Vec<Notification>,
    pub columns: Vec<Column>,
}

impl App {
    pub fn new() -> App {
        App {
            page: Page::Clock,
            menu_open: false,
            should_quit: false,
            aquarium: AquariumField::new(0, 0),
            next_break: seed::next_break(),
            break_active: false,
            flash: 0,
            events: seed::calendar_events(),
            notifications: seed::notifications(),
            columns: seed::kanban(),
        }
    }

    pub fn goto(&mut self, page: Page) {
        // Returning from leave (Idle) cancels the leave: restart break interval.
        if self.page == Page::Idle && page != Page::Idle {
            self.next_break = seed::next_break();
        }
        self.page = page;
        self.menu_open = false;
    }

    /// Record a notification: newest on top, history capped at `MAX_NOTIFICATIONS`.
    /// Wired to the laptop push channel later; seeded for now.
    #[allow(dead_code)]
    pub fn notify(&mut self, kind: NotifKind, text: impl Into<String>) {
        self.notifications.insert(
            0,
            Notification {
                time: Local::now(),
                kind,
                text: text.into(),
            },
        );
        self.notifications.truncate(MAX_NOTIFICATIONS);
    }

    /// Advance a card's phase (untouched -> wip -> done -> untouched).
    pub fn cycle_card(&mut self, col: usize, card: usize) {
        if let Some(c) = self.columns.get_mut(col).and_then(|c| c.cards.get_mut(card)) {
            c.phase = c.phase.next();
        }
    }

    pub fn on_tick(&mut self) {
        self.flash = self.flash.wrapping_add(1);

        // Break alarm is hourly (see ARCHITECTURE.md); roll forward once due
        // and raise the fullscreen break overlay. Suppressed while on leave.
        if self.page != Page::Idle {
            while self.next_break <= Local::now() {
                self.next_break += Duration::hours(1);
                self.break_active = true;
            }
        }

        // Aquarium runs as background noise on every page.
        self.aquarium.step();
        self.aquarium.step();
    }

    /// Time remaining until the next break alarm, floored at zero.
    pub fn time_until_break(&self) -> Duration {
        (self.next_break - Local::now()).max(Duration::zero())
    }
}
