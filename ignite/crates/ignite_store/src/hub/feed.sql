-- Schema version 3: the change feed's journal. Each write that advances `meta.seq` leaves one
-- row per thing it changed, at the write's final position, so that a client that long-polls
-- `GET /feed?since=<seq>` learns what changed. Only the most recent positions are kept.
CREATE TABLE feed (
  id     INTEGER PRIMARY KEY,
  seq    INTEGER NOT NULL,
  kind   TEXT NOT NULL,                    -- ticket|lease|machine|result|...
  ticket TEXT,
  name   TEXT
);
CREATE INDEX feed_seq ON feed(seq);
