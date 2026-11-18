-- A daemon's local database, schema version 1. The migration runner creates `meta`, which
-- also holds the machine's ID and the last feed position the daemon has read.

CREATE TABLE trees (
  id TEXT PRIMARY KEY,                     -- tree-07
  path TEXT NOT NULL,
  kind TEXT NOT NULL,                      -- ticket|landing
  ticket TEXT, session TEXT, branch TEXT,
  built_commit TEXT,                       -- the commit its last build was made at
  leased_at TEXT,
  last_used TEXT
) WITHOUT ROWID;

CREATE TABLE grants (                      -- cores and resources held by running commands
  id INTEGER PRIMARY KEY,
  handle TEXT,
  class TEXT NOT NULL,                     -- land|agent|tip|batch|idle
  cores INTEGER NOT NULL,
  resources TEXT NOT NULL DEFAULT '[]',    -- JSON array of names
  tree TEXT, ticket TEXT,
  pid INTEGER,                             -- the supervisor's process
  granted_at TEXT NOT NULL
);

CREATE TABLE handles (
  id TEXT PRIMARY KEY,                     -- L-5521
  verb TEXT NOT NULL, session TEXT, tree TEXT,
  state TEXT NOT NULL,                     -- running|done
  exit INTEGER,
  reply TEXT,                              -- JSON: the reply, once done
  started_at TEXT NOT NULL, finished_at TEXT
) WITHOUT ROWID;

CREATE TABLE outbox (                      -- not yet sent to the hub
  seq INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,                      -- record|result|flaky|metrics
  body TEXT NOT NULL,                      -- JSON
  at TEXT NOT NULL
);

CREATE TABLE results (                     -- this machine's copy of the result cache
  check_name TEXT NOT NULL, hash TEXT NOT NULL,
  platform TEXT NOT NULL DEFAULT '',
  outcome TEXT NOT NULL,
  record INTEGER,                          -- the hub's record ID, once known
  at TEXT NOT NULL,
  output TEXT,
  PRIMARY KEY (check_name, hash, platform)
) WITHOUT ROWID;

CREATE TABLE pool (
  session TEXT PRIMARY KEY,
  app TEXT NOT NULL,                       -- claude|codex
  role TEXT NOT NULL,                      -- worker|dispatcher
  state TEXT NOT NULL,                     -- idle|working|stalled|down
  ticket TEXT,
  models TEXT NOT NULL DEFAULT '[]',       -- JSON: model tiers its app can run
  added_at TEXT NOT NULL
) WITHOUT ROWID;
