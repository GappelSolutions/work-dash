# work-dash

A wall-mounted 7" Raspberry Pi dashboard — cyberdeck feel, not a web page —
backed by a small self-hosted server that owns the actual task data,
calendar, and Teams notifications.

Daily workflow: sit down, look through tasks, add/delete them, assign a
date, drop each into one of four categories (**URGENT / DEADLINE / ADMIN /
CREATIVE**). The Pi always shows **today only**. Everything else — adding,
editing, scheduling — happens in the server's browser-based CMS.

## Architecture

```
Work laptop (Graph/Teams access, company network)
  │  PUT /api/calendar, PUT /api/teams   (Bearer API key)
  ▼
work-dash-server  (Rust · axum · SQLite)
  │  owns: tasks + history, calendar cache, teams/call feed
  │  serves: JSON API, SSE event stream, server-rendered CMS (login-gated)
  │
  ├──▶ SSE /api/events ──▶ Raspberry Pi client (Ratatui TUI, today-only, read-mostly)
  └──▶ browser (you)    ──▶ CMS board — the daily driver, add/edit/schedule/categorize
```

The server is the single source of truth. The laptop only ever pushes
*into* it (calendar/Teams events); the Pi only ever reads from it (plus one
write path — tapping a card cycles its phase). The CMS is where the actual
day-to-day task management happens.

Three independently-versioned pieces in this repo:

| Path | What | Status |
|---|---|---|
| `server/` | axum + SQLite backend: task/calendar/teams API, SSE fan-out, session+API-key auth, server-rendered CMS | done |
| `client/` | Ratatui TUI for the Pi — today's board, clock, calendar, notification history | done, wired to the server |
| `nixos/` | Declarative NixOS flake for the Pi 5: DSI touchscreen + `cage`/`foot` kiosk | done, hardware-unverified TODOs flagged |
| `laptop/` | Windows Graph-calendar poller + Teams incoming-call watcher | not built yet — pushes to the server, not the Pi |

## Server

Rust, `axum`, SQLite (`sqlx`), server-rendered HTML (`maud`) for the CMS —
no JS build step, no SPA. One binary, ~18MB Alpine container.

**Data model** — `tasks` (category, phase, `assigned_date`, soft-delete),
an append-only `task_history` (every field change, timestamped — this is
the "history with dates" requirement), `calendar_events` (upserted by Graph
event id), `teams_events` (call/reminder/info feed).

**HTTP API** (all under `/api`, JSON):

| | |
|---|---|
| `GET/POST /api/tasks`, `PATCH/DELETE /api/tasks/:id`, `POST /api/tasks/:id/restore`, `GET /api/tasks/:id/history` | task CRUD + audit trail. `?scope=day\|all\|backlog&date=` |
| `PUT/GET /api/calendar` | laptop bulk-upserts by `external_id`; today's events |
| `PUT/GET /api/teams` | laptop pushes a call/reminder/info event; recent feed |
| `GET /api/events` | SSE — `task_upserted`, `task_deleted`, `calendar_updated`, `teams_event`, `ping` |
| `GET /health` | compose healthcheck |

**Auth, two mechanisms:**
- Browser → **login mask** (`/login`, single password) → signed, http-only
  session cookie.
- Machine clients (Pi + laptop) → `Authorization: Bearer <API_KEY>` on every
  `/api/*` call including the SSE subscription. `API_KEYS` is a
  comma-separated set, so each client gets its own revocable key.

**CMS** (`/`) — the daily driver: day-nav (◀ Today ▶ + date picker), a
quick-add bar, the four category columns, a backlog tray for undated tasks,
and a read-only calendar/Teams strip. Plain HTML forms — phase-cycle,
move-category, set-date, edit-text, delete all work with zero JS. Icons are
vendored [Lucide](https://lucide.dev) SVGs inlined into the markup (no
emoji, no CDN).

## Client (Pi)

The TUI itself — clock, kanban, calendar, notification history, idle/leave
— is unchanged; see `client/README.md` for its pages and keybindings.
`client/src/net.rs` is the only new piece: a background thread that, when
`WORK_DASH_SERVER_URL` and `WORK_DASH_API_KEY` are both set, fetches
today's board on startup and then holds an SSE subscription, folding
updates into the same `App` state the UI already renders. Tapping a card
still just cycles its phase locally — it also fires a `PATCH` to the
server in the background.

Unset those two env vars and the client behaves exactly as before this
change: seed data, fully offline. On a real disconnect it falls back to
the existing idle/leave view and auto-reconnects.

```sh
./run                 # cargo run -p work-dash-client (offline, seed data)
WORK_DASH_SERVER_URL=http://<server>:<port> WORK_DASH_API_KEY=<key> ./run
```

## Raspberry Pi: NixOS, touch, and the "headless with color" question

Hardware target: Pi 5 + a 7" DSI touchscreen (800×480, MIPI DSI panel +
I2C touch controller).

The bare kernel text console was the first thing ruled out: it only
remaps **16 fixed color slots**, and touch needs a real input stack
(`libinput`) that only a Wayland/X compositor drives — a raw VT can't do
either. X + a window manager works but is a lot of moving parts for an
always-on panel. The answer: **`cage`** (a Wayland compositor that does
nothing but run one app fullscreen, with working `libinput` touch) running
**`foot`** (a truecolor, themable Wayland terminal that reports touch taps
as mouse events) running `work-dash-client`. The client already handles
mouse clicks — a touch tap arrives as the same event, no client change
needed.

`nixos/` has the flake: Pi 5 DSI display module, I2C touch + `libinput`,
`greetd` autologin straight into the `cage`/`foot` kiosk, `work-dash-client`
packaged as a Nix derivation. A few things can't be verified without the
physical hardware in hand (exact touch-controller overlay name, panel
mount rotation) — those are flagged as explicit TODOs in `nixos/flake.nix`
/ `nixos/configuration.nix` and `nixos/README.md`, not guessed at.

## Design notes carried over from the original single-Pi build

- **Break alarm**: hourly, fullscreen until dismissed, a purely local Pi
  timer — never depends on the server being reachable.
- **Idle/leave view**: clock over a randomly-picked ASCII animation (a
  small library of hand-authored/sourced frame sequences, looped). This is
  also the automatic fallback shown on a server disconnect.
- **UI structure**: full-screen pages, not one dense terminal view — a
  bottom-right menu button toggles a 2×2 page picker (Clock+Notifications,
  Kanban, Calendar, Notification history), a leave button drops to idle.

## Local development

```sh
./run                                    # Pi client, offline/seed data
cargo run -p work-dash-server            # server on :8080 (needs env, see below)
cargo test -p work-dash-server           # integration tests (isolated tempfile SQLite)
```

Server env: `DATABASE_URL`, `PORT`, `SESSION_PASSWORD`, `SESSION_SECRET`,
`API_KEYS` (comma-separated), `TZ`.

**Docker Compose** (server + CMS, for local end-to-end testing):

```sh
SESSION_PASSWORD=... SESSION_SECRET=... API_KEYS=dev-key docker compose up --build
```

Serves the CMS at `http://localhost:8080`, SQLite persisted in a named
volume.

## Deployment

`work-dash-server`'s image is built and pushed to
`ghcr.io/gappelsolutions/work-dash-server` by
`.github/workflows/deploy.yml` on every push to `main` that touches
`server/`, `client/`, or the workspace manifests.

It's deployed on the shared `GappelSolutions/server` NixOS host the same
way as the other in-house apps (e.g. `gappel-montage`): a
`files/apps/work-dash/compose.yml`, a `systemd`-managed `app-work-dash`
unit declared in `modules/app-stack.nix`, an `agenix`-encrypted env
(`SESSION_PASSWORD`, `SESSION_SECRET`, `API_KEYS`, `DATABASE_URL`, `PORT`,
`TZ`), and a Caddy route at `workdash.gappel.com`. That repo is the
NixOS source of truth — see it for exact deploy mechanics.

New image pushes don't auto-restart the running container (no self-hosted
CI redeploy step, unlike some of the older in-house apps) — a new build
takes effect on the next `systemctl restart app-work-dash`, manual
`nixos-rebuild switch`, or the host's Wednesday-02:30 auto-upgrade window.

The Pi points at it via `WORK_DASH_SERVER_URL=https://workdash.gappel.com`
and its own `WORK_DASH_API_KEY`; the (future) laptop pusher uses a
separate key from the same `API_KEYS` set.
