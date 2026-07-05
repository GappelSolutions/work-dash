CREATE TABLE tasks (
  id            INTEGER PRIMARY KEY,
  text          TEXT    NOT NULL,
  category      TEXT    NOT NULL CHECK (category IN ('urgent','deadline','admin','creative')),
  phase         TEXT    NOT NULL DEFAULT 'untouched'
                        CHECK (phase IN ('untouched','wip','done')),
  assigned_date TEXT,
  position      INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT    NOT NULL,
  updated_at    TEXT    NOT NULL,
  completed_at  TEXT,
  deleted_at    TEXT
);
CREATE INDEX idx_tasks_date     ON tasks (assigned_date) WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_category ON tasks (category, assigned_date) WHERE deleted_at IS NULL;

CREATE TABLE task_history (
  id         INTEGER PRIMARY KEY,
  task_id    INTEGER NOT NULL REFERENCES tasks(id),
  action     TEXT    NOT NULL,
  field      TEXT,
  old_value  TEXT,
  new_value  TEXT,
  changed_at TEXT    NOT NULL
);
CREATE INDEX idx_history_task ON task_history (task_id, changed_at);

CREATE TABLE calendar_events (
  id           INTEGER PRIMARY KEY,
  external_id  TEXT    NOT NULL UNIQUE,
  title        TEXT    NOT NULL,
  start_at     TEXT    NOT NULL,
  end_at       TEXT    NOT NULL,
  place        TEXT,
  is_cancelled INTEGER NOT NULL DEFAULT 0,
  received_at  TEXT    NOT NULL
);
CREATE INDEX idx_calendar_start ON calendar_events (start_at) WHERE is_cancelled = 0;

CREATE TABLE teams_events (
  id         INTEGER PRIMARY KEY,
  kind       TEXT    NOT NULL,
  text       TEXT    NOT NULL,
  payload    TEXT,
  created_at TEXT    NOT NULL
);
CREATE INDEX idx_teams_created ON teams_events (created_at DESC);
