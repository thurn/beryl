-- Every version of every ticket this machine has received from the hub, oldest first. A row is
-- never changed or removed: the triggers below refuse it, so that a bad edit reaching the
-- mirror adds a version and leaves the earlier ones to roll back to.
CREATE TABLE versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ticket TEXT NOT NULL,
    -- When this machine received the version, as `YYYY-MM-DDTHH:MM:SS.fffZ`, which sorts as
    -- text in time order.
    received_at TEXT NOT NULL,
    -- The hub's feed position of the change that brought it.
    seq INTEGER NOT NULL,
    -- The ticket as `GET /export` writes it: its columns, edges, comments, history and aliases.
    whole_json TEXT NOT NULL
);
CREATE INDEX versions_by_ticket ON versions (ticket, id);
CREATE INDEX versions_by_time ON versions (received_at);
CREATE TRIGGER versions_are_never_changed BEFORE UPDATE ON versions
BEGIN
    SELECT RAISE(ABORT, 'the ticket mirror is append-only');
END;
CREATE TRIGGER versions_are_never_removed BEFORE DELETE ON versions
BEGIN
    SELECT RAISE(ABORT, 'the ticket mirror is append-only');
END;

-- Where the mirror is in following the hub: its feed position, a copy in progress and when it
-- last caught up.
CREATE TABLE state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
