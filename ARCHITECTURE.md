# Work Dashboard — Architecture

Wall-mounted 7" Raspberry Pi dashboard. Cyberdeck feel, not a web page.

## Constraint driving the design

Microsoft Graph / Teams only reachable from inside the company network.
Pi is **not** allowed to reach into the work network. Only the work laptop
can talk to Microsoft. Connection direction: laptop → Pi, one-way. Pi never
dials out for MS data.

No home server. Removed — laptop pushes straight to Pi over LAN.

## Components

```
Work laptop (has Graph + Teams access, on company network)
  ├── WinEventHook Teams-call watcher
  ├── Graph calendar poller (device-code auth, cached refresh token)
  └── push events ──▶ Raspberry Pi (static IP, LAN)
                        ├── Rust process: in-memory state, break-alarm timer,
                        │   appointment-reminder scheduler, WS/REST listener
                        └── Ratatui UI client (same box, full-screen pages)
```

### Work laptop (Windows)

- **Teams incoming-call detection**: `windows` crate, `SetWinEventHook(EVENT_OBJECT_CREATE)`
  watching for the Teams incoming-call toast window; read caller name via
  `uiautomation` crate. Event-driven, no polling.
  (Rejected: Notification Listener API — requires MSIX package identity, too
  much friction for a personal exe.)
- **Calendar poll**: separate thread, `reqwest` + `oauth2` crate, device-code
  flow once at setup, refresh token cached to disk (`%APPDATA%`), poll Graph
  every 1–5 min.
- **Runs as**: Task Scheduler entry at logon. (`windows-service` crate only
  if backgrounding becomes necessary later.)
- Pushes both event types to Pi's static IP via WS/HTTP POST.

### Raspberry Pi

- Static IP on LAN.
- Single Rust process: passive receiver + in-memory state + local timers.
- No persistence yet — kanban is **seed data only**, storage design
  deferred.
- Break alarm (hourly, full-screen until dismissed) is a **local Pi timer**,
  does not depend on the laptop.
- Appointment reminder (1h before) depends on calendar data pushed from
  laptop; Pi schedules the actual reminder firing locally.
- UI: Ratatui + Crossterm, fullscreen Alacritty.

## UI structure

Full-screen pages, not one dense terminal view.

- Bottom-right button toggles a 2×2 app menu overlay:
  - Clock + Notifications (combined page)
  - Kanban
  - Calendar
  - Notification history (newest on top, capped at 10)
- **Leave button**: manual toggle to the offline/fallback view — same code
  path as laptop-disconnected state. Shows clock + ASCII background
  animation only, no widgets.

## Background animation

Dropped: video → ASCII shader pipeline (real-time or prerendered) —
luminance/Sobel-derived density from a source video wasn't dense enough at
usable terminal grid sizes, and coupling animation resolution to font size
kept fighting the kanban layout's need for a denser grid.

Replaced with: a set of **predefined ASCII animations** (hand-authored or
sourced frame-by-frame text art — e.g. classic terminal demos/loops), picked
**at random** each time the idle/leave page is shown. No shader math, no
video ingestion — just a small library of animation assets played back.

- Each animation = a sequence of plain text frames (or a simple frame format
  with per-cell color), looped.
- Random pick from the library on page entry, not on every frame.
- No coupling to terminal font size/grid resolution beyond "fits the fixed
  page dimensions" — much less fragile than the shader approach.
- Asset format and playback loop: TBD, but trivial compared to shader
  reimplementation — just cycle stored frames on a timer.

## Communication protocol

Laptop → Pi push events (WS or HTTP POST), e.g.:

```json
{ "type": "incoming_call", "caller": "John Doe" }
```

```json
{ "type": "calendar_event", "title": "...", "start": "...", "end": "..." }
```

Pi-internal events (no laptop involvement): break alarm, appointment
reminder firing, kanban card moves (in-memory only for now).

## Open items

- Break-alarm interval: hardcoded 1h for now, configurable later?
- Kanban persistence: deferred, revisit after core dashboard works.
- Touch feel inside Alacritty/Ratatui: still unverified — build a small
  tap/drag/scroll prototype before investing further.
- Font size: single fixed size for whole app vs per-page switching via
  Alacritty IPC (`alacritty msg config`) — undecided, leaning single fixed
  size unless kanban density forces the issue.
- ASCII animation library: source/author actual animations, pick asset
  format, decide frame-timing/loop mechanism.
