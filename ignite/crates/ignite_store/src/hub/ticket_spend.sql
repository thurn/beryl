-- Each ticket window's spend, as the efficiency dashboard drew it for the ticket pages: one row
-- per session window that worked a ticket, holding that window's time and dollar segments in
-- order. Machines post them to `POST /metrics/import` with the source `spend`; a window posted
-- again replaces its row, since a window keeps growing until its session moves on.
CREATE TABLE ticket_spend (
  machine TEXT NOT NULL,
  window TEXT NOT NULL,                    -- the producer's key for the window, unique per machine
  ticket TEXT NOT NULL,
  start TEXT NOT NULL,
  end TEXT NOT NULL,
  data TEXT NOT NULL,                      -- the window as JSON: provider, session, time, dollars
  at TEXT NOT NULL,                        -- when the hub last received it
  PRIMARY KEY (machine, window)
) WITHOUT ROWID;
CREATE INDEX ticket_spend_ticket ON ticket_spend(ticket, start);
