-- The model and reasoning effort of each CLI session with a transcript, as the machine's daemon
-- reports them with its lines, which the session page shows. A row is deleted with its
-- session's last lines.
CREATE TABLE transcript_sessions (
  session TEXT PRIMARY KEY,
  model TEXT,
  effort TEXT,
  updated_at TEXT NOT NULL
);
