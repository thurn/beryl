-- Schema version 11. Whether someone asked a verb running in a process of its own to stop, and
-- whether the verb refuses to be stopped now (as during a landing's push), so that a restarted
-- daemon answers the verb process's `host-hello`, `host-poll` and `host-refuse-stop` as the
-- previous daemon would have, and `ignite stop` is still refused during a push.
--
-- One row per handle that has either. `asked` is 1 once a stop was asked; `refused` holds the
-- reason while stopping is refused.
--
-- Additive: one new table.

CREATE TABLE handle_stops (
    handle TEXT PRIMARY KEY,
    asked INTEGER NOT NULL DEFAULT 0,
    refused TEXT,
    at TEXT NOT NULL
);
