-- Schema version 5. A cached result records the machine that produced it, so that a failure
-- from another machine can be told apart and run again here before it blocks anything. Rows
-- from before this version have no machine. `passes` holds how long each check command took
-- when it passed, from which the command's hang limit is computed.

ALTER TABLE results ADD COLUMN machine TEXT;

CREATE TABLE passes (
  family TEXT NOT NULL,
  command TEXT NOT NULL,                   -- list|run
  wall_s REAL NOT NULL,                    -- seconds it ran, waiting for cores excluded
  at TEXT NOT NULL
);

CREATE INDEX passes_by_command ON passes (family, command, at);
