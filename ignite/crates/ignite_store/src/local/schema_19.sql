-- Schema version 19. Samples of the Claude plan's usage: the dispatcher calls the desktop app's
-- `get_usage` tool when the daemon asks, and the daemon stores one row per window of the plan
-- (a 5-hour window, a weekly window for all models, a weekly window for one model). A sample
-- whose status is not `ok` is one row with no window and a note. `posted` is 1 once the row
-- reached the hub, so a row the hub missed is sent with the next sample. Rows older than 30
-- days are deleted.
--
-- Additive: a new table.

CREATE TABLE plan_usage (
    id INTEGER PRIMARY KEY,
    at TEXT NOT NULL,
    machine TEXT NOT NULL,
    status TEXT NOT NULL,
    label TEXT,
    percent_used REAL,
    resets_at TEXT,
    note TEXT,
    posted INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX plan_usage_by_age ON plan_usage (at);
