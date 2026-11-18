-- Schema version 2. A tree has a state, so the daemon can refuse to lease a tree that is being
-- built or reset. The outbox's sequence numbers become `AUTOINCREMENT`, so one is never reused
-- after the outbox empties and a late acknowledgement cannot drop a newer entry. SQLite cannot
-- add `AUTOINCREMENT` to a table, so the outbox is rebuilt with its entries.

ALTER TABLE trees ADD COLUMN state TEXT NOT NULL DEFAULT 'free';  -- free|prebuilding|leased|landing|resetting

CREATE TABLE outbox_new (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,                      -- record|result|flaky|metrics
  body TEXT NOT NULL,                      -- JSON
  at TEXT NOT NULL
);
INSERT INTO outbox_new (seq, kind, body, at) SELECT seq, kind, body, at FROM outbox ORDER BY seq;
DROP TABLE outbox;
ALTER TABLE outbox_new RENAME TO outbox;
