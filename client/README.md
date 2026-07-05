# work-dash-client

Ratatui UI client for the Pi dashboard (see `../README.md` for the overall
architecture). Read-mostly: today's board comes from `work-dash-server`;
adding/editing/scheduling tasks happens in the server's CMS, not here.

```
./run                                                    # offline, seed data
WORK_DASH_SERVER_URL=http://<server>:<port> \
WORK_DASH_API_KEY=<key> ./run                            # networked
```

Without both env vars set, the client runs entirely on the bundled seed
data (`src/seed.rs`) — no network thread spawned, nothing changes from
before `src/net.rs` existed.

## Pages

- **Clock + Notifications** (start page)
- **Kanban** — 4 categories (URGENT / DEADLINE / ADMIN / CREATIVE),
  filtered to today; tap a card to cycle its phase (untouched → wip →
  done), which also `PATCH`es the server when networked
- **Calendar** — today's events, current/next highlighted
- **Notification history** — newest on top, capped at 10
- **Idle / Leave** — clock over a randomly-picked ASCII background
  animation; also the automatic fallback when the server is unreachable
  (auto-reconnects and resyncs)

## Input

Touch-first: bottom-right `[ MENU ]` opens the 2×2 app menu, `[ LEAVE ]`
enters idle, tap anywhere on idle returns. Keyboard for dev:

| Key | Action |
| --- | --- |
| `m` | toggle menu |
| `l` | leave (idle page) |
| `1`–`4` | Clock / Kanban / Calendar / Notifications |
| `q` / Ctrl-C | quit |
