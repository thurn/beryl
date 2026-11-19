-- Schema version 17. What the landing gate remembers from one gate to the next: the checks each
-- change's last passing gate chose by score, and the rate the gates' check commands achieved.
-- `ignite check` and `ignite land` each run in a verb process of their own, so memory kept in
-- one process never reached the next gate of the same change.
--
-- Additive: new tables.

CREATE TABLE gate_choices (
    -- The change's own files, sorted, one per line.
    change TEXT PRIMARY KEY,
    -- The checks the gate chose by score, one per line.
    checks TEXT NOT NULL,
    -- Milliseconds since the Unix epoch.
    at INTEGER NOT NULL
);

CREATE TABLE gate_rate (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    -- Milliseconds since the Unix epoch.
    at INTEGER NOT NULL,
    -- Billions of instructions per second of running checks.
    rate REAL NOT NULL
);
