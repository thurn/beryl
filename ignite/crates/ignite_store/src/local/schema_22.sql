-- Schema version 22. A regression goes first to the worker whose ticket caused it, because
-- that worker still holds the change's context. A CLI worker's session ends when its ticket
-- lands, so the daemon keeps what is needed to resume its conversation:
--
-- - `cli_slots.fallback_prompt`: set while a slot's first turn resumes another session's
--   transcript. If that turn ends as anything but a clean turn, the slot starts the ticket
--   again on a new transcript with this prompt.
-- - `cli_landed`: one row per ticket whose CLI slot was freed after the ticket stopped being
--   held (it landed), with the transcript, app and model of its last turn and when. Rows older
--   than 24 hours are never used and are deleted when another row is written.
--
-- Every column is nullable or has a default, as in schema version 20.
--
-- Additive: a new column and a new table.

ALTER TABLE cli_slots ADD COLUMN fallback_prompt TEXT;

CREATE TABLE cli_landed (
    ticket TEXT NOT NULL DEFAULT '' PRIMARY KEY,
    app TEXT NOT NULL DEFAULT '',
    transcript TEXT,
    model TEXT,
    landed_at TEXT
);
