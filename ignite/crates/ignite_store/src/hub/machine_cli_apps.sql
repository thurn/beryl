-- The apps each daemon dispatches itself, as reported by its latest heartbeat.
ALTER TABLE machines ADD COLUMN cli_apps TEXT NOT NULL DEFAULT '[]';
