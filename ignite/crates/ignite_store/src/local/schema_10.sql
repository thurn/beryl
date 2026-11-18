-- Schema version 10. `tip_state` holds the tip verifier's memory between rounds, so that a
-- restarted daemon does not report a failure or a stuck tip again, forget which checks it holds
-- off, or start the platform wait over.
--
-- One row (`id` is always 1). `body` is JSON with every field defaulted when missing, so a row
-- written by an older build loads in a newer one and the other way round.
--
-- Additive: a new table.

CREATE TABLE tip_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    body TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
