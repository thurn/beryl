-- The digest of each CLI worker's transcript, one row per line (docs/ignite.md, "CLI mode"). A
-- line is keyed by its session, turn and place in the turn, so a daemon that sends a line twice
-- stores it once. `seq` orders lines as the hub received them, which is the position a reader
-- passes to ask only for later ones. Upkeep deletes lines a week after they arrived.
CREATE TABLE transcript_lines (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    machine TEXT NOT NULL,
    session TEXT NOT NULL,
    ticket TEXT,
    turn INTEGER NOT NULL,
    line_index INTEGER NOT NULL,
    kind TEXT NOT NULL,
    text TEXT NOT NULL,
    at TEXT,
    stored_at TEXT NOT NULL,
    UNIQUE (session, turn, line_index)
);
CREATE INDEX transcript_lines_by_session ON transcript_lines (session, seq);
CREATE INDEX transcript_lines_by_age ON transcript_lines (stored_at);
