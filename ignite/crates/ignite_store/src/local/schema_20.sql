-- Schema version 20. CLI mode, where the daemon runs each worker as a headless CLI process, one
-- process per turn. Nothing about a CLI worker lives in the daemon's memory, so these tables
-- hold all of it:
--
-- - `cli_slots`: one row per worker slot of an app (`app`, `slot` from 1), with the Ignite
--   session and ticket it works, its state, the latest turn's number, the CLI's transcript ID,
--   the model and effort every turn asks for, and what the slot waits for.
-- - `cli_turns`: one row per turn of a session, recorded as `starting` in the transaction that
--   marks its messages delivered, before its supervisor starts. `run` is the supervised run's
--   directory once found; `lines_applied` counts the lines of `output.log` whose effects were
--   applied, so reading the log again changes nothing.
-- - `cli_messages`: `ignite tell`, stall reminders and wake-ups for a CLI session. A message
--   with no `delivered_turn` waits for the next turn's prompt.
-- - `cli_holds`: one row per app on a CLI hold: no turn starts for the app until a probe turn
--   succeeds.
--
-- Every column is nullable or has a default, so a later build that adds a column can still
-- read the rows an older build wrote, and an older build can read a newer build's rows.
--
-- Additive: new tables.

CREATE TABLE cli_slots (
    app TEXT NOT NULL DEFAULT '',
    slot INTEGER NOT NULL DEFAULT 0,
    session TEXT,
    ticket TEXT,
    state TEXT NOT NULL DEFAULT 'free',
    turn INTEGER NOT NULL DEFAULT 0,
    transcript TEXT,
    model TEXT,
    effort TEXT,
    since TEXT,
    reservation TEXT,
    next_prompt TEXT,
    wait_handle TEXT,
    resume_at TEXT,
    park_reason TEXT,
    interrupted_in_row INTEGER NOT NULL DEFAULT 0,
    block_sent INTEGER NOT NULL DEFAULT 0,
    backed_off INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (app, slot)
);
CREATE INDEX cli_slots_by_session ON cli_slots (session);

CREATE TABLE cli_turns (
    session TEXT NOT NULL DEFAULT '',
    turn INTEGER NOT NULL DEFAULT 0,
    app TEXT NOT NULL DEFAULT '',
    slot INTEGER NOT NULL DEFAULT 0,
    ticket TEXT,
    prompt TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'starting',
    run TEXT,
    started_at TEXT,
    ended_at TEXT,
    exit INTEGER,
    end_kind TEXT,
    transcript TEXT,
    model TEXT,
    cli_version TEXT,
    cost_usd REAL,
    lines_applied INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (session, turn)
);

CREATE TABLE cli_messages (
    id INTEGER PRIMARY KEY,
    session TEXT NOT NULL DEFAULT '',
    ticket TEXT,
    text TEXT NOT NULL DEFAULT '',
    urgent INTEGER NOT NULL DEFAULT 0,
    queued_at TEXT,
    delivered_turn INTEGER
);
CREATE INDEX cli_messages_by_session ON cli_messages (session, delivered_turn);

CREATE TABLE cli_holds (
    app TEXT NOT NULL DEFAULT '' PRIMARY KEY,
    reason TEXT NOT NULL DEFAULT '',
    since TEXT,
    cli_version TEXT,
    message TEXT,
    probed_at TEXT,
    reported INTEGER NOT NULL DEFAULT 0
);
