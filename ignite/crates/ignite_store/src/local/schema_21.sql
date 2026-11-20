-- Schema version 21. `check_sightings`: the latest outcome of each check for each session on this
-- machine, from `ignite test`, `check` and `land`, so that the dispatcher can see when several
-- sessions fail one check the same way (a shared failure) and when the check passes again. The
-- result cache cannot say this: it is keyed by inputs, not sessions, and a second session that
-- finds a cached failure writes nothing to it.
--
-- - `outcome` is `pass` or `fail`.
-- - `hash` is the inputs' hash; two failures on the same inputs are the same failure.
-- - `first_line` is the first line of the failure's message; `None` for a cached failure, whose
--   message only says it failed before.
-- - `tip_unknown` is 1 when a landing was refused because the check's result at the tip could
--   not be found or run.
--
-- Additive: a new table.

CREATE TABLE check_sightings (
    session TEXT NOT NULL DEFAULT '',
    check_name TEXT NOT NULL DEFAULT '',
    verb TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    hash TEXT,
    first_line TEXT,
    tip_unknown INTEGER NOT NULL DEFAULT 0,
    at TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (session, check_name)
);
CREATE INDEX check_sightings_by_age ON check_sightings (at);
