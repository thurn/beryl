-- Schema version 4. A pool member records the dispatcher session that added it (its parent)
-- and the files its last ticket touched, which the daemon reports to the hub so that a ticket
-- goes to the worker whose last ticket shared the most files with it. `pool_settings` holds
-- the machine's pause, which a restart must keep.

ALTER TABLE pool ADD COLUMN parent TEXT;
ALTER TABLE pool ADD COLUMN files TEXT NOT NULL DEFAULT '[]';  -- JSON array of paths

CREATE TABLE pool_settings (
  key TEXT PRIMARY KEY,                    -- paused
  value TEXT NOT NULL
) WITHOUT ROWID;
