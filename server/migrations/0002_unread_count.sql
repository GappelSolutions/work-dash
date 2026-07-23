DROP TABLE teams_events;

CREATE TABLE unread_count (
  id    INTEGER PRIMARY KEY CHECK (id = 1),
  count INTEGER NOT NULL DEFAULT 0
);
INSERT INTO unread_count (id, count) VALUES (1, 0);
