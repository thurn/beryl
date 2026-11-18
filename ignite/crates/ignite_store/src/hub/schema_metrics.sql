-- Each machine's efficiency rows (token and dollar costs read from provider transcripts), as the
-- daemon posts them to `POST /metrics/import`.
CREATE TABLE metrics (
  machine TEXT NOT NULL,
  source TEXT NOT NULL,
  data TEXT NOT NULL,                      -- the row, as JSON; an identical row is stored once
  at TEXT NOT NULL,                        -- when the hub received it
  PRIMARY KEY (machine, source, data)
) WITHOUT ROWID;
CREATE INDEX metrics_at ON metrics(at);
