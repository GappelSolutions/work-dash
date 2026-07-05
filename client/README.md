# work-dash-client

Ratatui UI client for the Pi dashboard (see ../ARCHITECTURE.md). Seed data
only — no laptop push channel or persistence yet.

```
cargo run -p work-dash-client
```

## Pages

- **Clock + Notifications** (start page)
- **Kanban** — 3 columns, keyboard card moves
- **Calendar** — today's events, current/next highlighted
- **Notification history** — newest on top, capped at 10
- **Idle / Leave** — clock over pipes.sh-style background animation; same
  view the laptop-disconnected state will use

## Input

Touch-first: bottom-right `[ MENU ]` opens the 2×2 app menu, `[ LEAVE ]`
enters idle, tap anywhere on idle returns. Keyboard for dev:

| Key | Action |
| --- | --- |
| `m` | toggle menu |
| `l` | leave (idle page) |
| `1`–`4` | Clock / Kanban / Calendar / Notifications |
| `←→↑↓` | select column/card |
| `,` `.` | move kanban card left/right |
| `q` / Ctrl-C | quit |
