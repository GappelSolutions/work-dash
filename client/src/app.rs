use chrono::{DateTime, Duration, Local};

use crate::aquarium::AquariumField;
use crate::net::{self, NetConfig, NetEvent, ServerTask};
use crate::seed;

/// Column order/titles are fixed and match the server's 4 categories
/// (index i <-> CATEGORY_ORDER[i]) — see server `Category::ALL`.
const CATEGORY_ORDER: [&str; 4] = ["urgent", "deadline", "admin", "creative"];

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

    fn as_server_str(self) -> &'static str {
        match self {
            Phase::Untouched => "untouched",
            Phase::Wip => "wip",
            Phase::Done => "done",
        }
    }

    fn from_server_str(s: &str) -> Phase {
        match s {
            "wip" => Phase::Wip,
            "done" => Phase::Done,
            _ => Phase::Untouched,
        }
    }
}

pub struct Card {
    pub text: String,
    pub phase: Phase,
    /// `Some(id)` for a server-backed task; `None` for offline seed data,
    /// which has nothing to PATCH back to.
    pub id: Option<i64>,
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
    /// `Some` when `WORK_DASH_SERVER_URL`/`WORK_DASH_API_KEY` were set at
    /// startup — used to PATCH phase changes back. `None` means fully
    /// offline (seed data, no net thread running).
    net_config: Option<NetConfig>,
    pub connected: bool,
    /// True when `Page::Idle` was entered automatically by a disconnect
    /// (not the user's manual "leave" button) — lets a reconnect return the
    /// user to `Clock` without also yanking them out of a deliberate leave.
    auto_idle: bool,
}

fn empty_columns() -> Vec<Column> {
    CATEGORY_ORDER
        .iter()
        .map(|c| Column {
            title: c.to_uppercase(),
            cards: Vec::new(),
        })
        .collect()
}

impl App {
    pub fn new(net_config: Option<NetConfig>) -> App {
        let networked = net_config.is_some();
        App {
            page: Page::Clock,
            menu_open: false,
            should_quit: false,
            aquarium: AquariumField::new(0, 0),
            next_break: seed::next_break(),
            break_active: false,
            flash: 0,
            events: if networked {
                Vec::new()
            } else {
                seed::calendar_events()
            },
            notifications: if networked {
                Vec::new()
            } else {
                seed::notifications()
            },
            columns: if networked { empty_columns() } else { seed::kanban() },
            net_config,
            connected: false,
            auto_idle: false,
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

    /// Fold one message from the background network thread into app state.
    pub fn apply_net_event(&mut self, ev: NetEvent) {
        match ev {
            NetEvent::Connected => {
                self.connected = true;
                if self.page == Page::Idle && self.auto_idle {
                    self.auto_idle = false;
                    self.goto(Page::Clock);
                }
            }
            NetEvent::Disconnected => {
                self.connected = false;
                if self.page != Page::Idle {
                    self.auto_idle = true;
                    self.goto(Page::Idle);
                }
            }
            NetEvent::Snapshot(snap) => {
                self.columns = columns_from_tasks(&snap.tasks);
                self.events = snap
                    .calendar
                    .into_iter()
                    .filter_map(calendar_event_from_server)
                    .collect();
                self.notifications = snap
                    .teams
                    .into_iter()
                    .filter_map(notification_from_server)
                    .collect();
                self.notifications.truncate(MAX_NOTIFICATIONS);
            }
            NetEvent::TaskUpserted(task) => self.upsert_task(task),
            NetEvent::TaskDeleted { id } => self.remove_task(id),
            NetEvent::CalendarUpdated { events } => {
                self.events = events
                    .into_iter()
                    .filter_map(calendar_event_from_server)
                    .collect();
            }
            NetEvent::TeamsEvent(ev) => {
                if let Some(n) = notification_from_server(ev) {
                    self.notifications.insert(0, n);
                    self.notifications.truncate(MAX_NOTIFICATIONS);
                }
            }
        }
    }

    fn remove_task(&mut self, id: i64) {
        for col in &mut self.columns {
            col.cards.retain(|c| c.id != Some(id));
        }
    }

    /// A task_upserted event covers create/patch/restore for any day, not
    /// just today — remove-then-reinsert-if-still-today handles a phase
    /// change, a category move, and a date reassignment away from today
    /// (which should make the card disappear) with one code path.
    fn upsert_task(&mut self, task: ServerTask) {
        self.remove_task(task.id);
        let today = Local::now().date_naive().to_string();
        if task.assigned_date.as_deref() != Some(today.as_str()) {
            return;
        }
        if let Some(idx) = CATEGORY_ORDER.iter().position(|c| *c == task.category) {
            self.columns[idx].cards.push(Card {
                text: task.text,
                phase: Phase::from_server_str(&task.phase),
                id: Some(task.id),
            });
        }
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
    /// Optimistic local update; when networked, also fires a PATCH so the
    /// server (and therefore the CMS) picks up the change.
    pub fn cycle_card(&mut self, col: usize, card: usize) {
        let Some(c) = self.columns.get_mut(col).and_then(|c| c.cards.get_mut(card)) else {
            return;
        };
        c.phase = c.phase.next();
        if let (Some(id), Some(cfg)) = (c.id, &self.net_config) {
            net::patch_task_phase(cfg, id, c.phase.as_server_str());
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

fn columns_from_tasks(tasks: &[ServerTask]) -> Vec<Column> {
    let mut columns = empty_columns();
    for t in tasks {
        if let Some(idx) = CATEGORY_ORDER.iter().position(|c| *c == t.category) {
            columns[idx].cards.push(Card {
                text: t.text.clone(),
                phase: Phase::from_server_str(&t.phase),
                id: Some(t.id),
            });
        }
    }
    columns
}

fn calendar_event_from_server(e: net::ServerCalendarEvent) -> Option<CalendarEvent> {
    let start = DateTime::parse_from_rfc3339(&e.start_at)
        .ok()?
        .with_timezone(&Local);
    let end = DateTime::parse_from_rfc3339(&e.end_at)
        .ok()?
        .with_timezone(&Local);
    Some(CalendarEvent {
        start,
        end,
        title: e.title,
        place: e.place,
    })
}

fn notification_from_server(e: net::ServerTeamsEvent) -> Option<Notification> {
    let kind = match e.kind.as_str() {
        "call" => NotifKind::Call,
        "reminder" => NotifKind::Reminder,
        "info" => NotifKind::Info,
        _ => return None,
    };
    let time = DateTime::parse_from_rfc3339(&e.created_at)
        .map(|t| t.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now());
    Some(Notification {
        time,
        kind,
        text: e.text,
    })
}
