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
    /// Leave / laptop-disconnected view: clock + aquarium only.
    Idle,
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
    /// Incoming-call banner: `Some(caller)` while a call is ringing.
    /// Ephemeral — mirrors the server's in-memory singleton, never a log
    /// entry. Trumps `break_active` (see `ui::draw`).
    pub call_active: Option<String>,
    /// Free-running tick counter, used to strobe the break overlay.
    pub flash: u32,
    pub events: Vec<CalendarEvent>,
    /// Persisted server-side; a single unread-messages count, nothing more.
    pub unread_count: u32,
    pub columns: Vec<Column>,
    /// `Some` when `WORK_DASH_SERVER_URL`/`WORK_DASH_API_KEY` were set at
    /// startup — used to PATCH phase changes back. `None` means fully
    /// offline (seed data, no net thread running).
    net_config: Option<NetConfig>,
    pub connected: bool,
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
            call_active: None,
            flash: 0,
            events: if networked {
                Vec::new()
            } else {
                seed::calendar_events()
            },
            unread_count: if networked { 0 } else { seed::unread_count() },
            columns: if networked { empty_columns() } else { seed::kanban() },
            net_config,
            connected: false,
        }
    }

    /// `true` when `WORK_DASH_SERVER_URL`/`WORK_DASH_API_KEY` were set at
    /// startup — distinguishes "briefly disconnected" from "never wired to
    /// a server, running on seed data by design" so the UI only warns in
    /// the former case.
    pub fn networked(&self) -> bool {
        self.net_config.is_some()
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
            }
            NetEvent::Disconnected => {
                self.connected = false;
            }
            NetEvent::Snapshot(snap) => {
                self.columns = columns_from_tasks(&snap.tasks);
                self.events = snap
                    .calendar
                    .into_iter()
                    .filter_map(calendar_event_from_server)
                    .collect();
                self.unread_count = snap.unread_count.max(0) as u32;
                self.call_active = if snap.call.active {
                    Some(snap.call.caller.unwrap_or_else(|| "Unknown caller".to_string()))
                } else {
                    None
                };
            }
            NetEvent::TaskUpserted(task) => self.upsert_task(task),
            NetEvent::TaskDeleted { id } => self.remove_task(id),
            NetEvent::CalendarUpdated { events } => {
                self.events = events
                    .into_iter()
                    .filter_map(calendar_event_from_server)
                    .collect();
            }
            NetEvent::UnreadCount(count) => {
                self.unread_count = count.max(0) as u32;
            }
            NetEvent::CallState { active, caller } => {
                self.call_active = if active {
                    Some(caller.unwrap_or_else(|| "Unknown caller".to_string()))
                } else {
                    None
                };
            }
        }
    }

    /// Dismisses the incoming-call banner: clears it locally and, when
    /// networked, tells the server so the flag is actually deleted (not
    /// just hidden on this one board) — other SSE listeners need to see it
    /// go away too.
    pub fn dismiss_call(&mut self) {
        self.call_active = None;
        if let Some(cfg) = &self.net_config {
            net::dismiss_call(cfg);
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
