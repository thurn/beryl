-- The counted releases of each ticket since its body or acceptance last changed or a person
-- last released it: one row per release by a worker giving up or by Ignite taking the ticket
-- back. Two rows from different sessions stop the hub offering the ticket (see `releases`).
CREATE TABLE ticket_releases (
    ticket TEXT NOT NULL,
    n INTEGER NOT NULL,
    session TEXT NOT NULL,
    cause TEXT NOT NULL,
    note TEXT NOT NULL,
    at TEXT NOT NULL,
    PRIMARY KEY (ticket, n)
) WITHOUT ROWID;
