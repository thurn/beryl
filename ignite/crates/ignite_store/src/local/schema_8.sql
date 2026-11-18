-- Schema version 8. A restarted daemon keeps two pieces of in-memory state that it would
-- otherwise forget.
--
-- `claim_activity` has one row per held ticket tree: when its session last ran a command and
-- where it is in the stall procedure, so that the 30-minute silence clock does not start over
-- at every restart. `stage` is `active`, `asked`, `messaged` or `retry`.
--
-- `dispatch_queue` holds the dispatcher's queue as JSON rows, one per entry. `kind` names the
-- list the entry belongs to (`pending`, `given`, `reserved`, `orphan`, `refused`, `stall`,
-- `meta`) and `key` places it within that list.
--
-- Additive: two new tables.

CREATE TABLE claim_activity (
    tree TEXT PRIMARY KEY,
    ticket TEXT NOT NULL,
    active_at TEXT NOT NULL,
    stage TEXT NOT NULL,
    checked_at TEXT,
    messaged_at TEXT,
    second INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE dispatch_queue (
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (kind, key)
);
