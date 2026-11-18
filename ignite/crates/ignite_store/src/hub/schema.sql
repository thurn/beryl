-- The hub's database, schema version 1. The migration runner creates `meta` (key, value),
-- which also holds `seq`, the global change counter, and the hashes of machine tokens.
--
-- The ticket tables are carried over from the ticket service unchanged (its events table is
-- dropped); `tickets` gains the columns from `workers` on. Column names that are SQL keywords
-- in the design (check, commit, group) are spelt check_name, commit_sha and check_group.

CREATE TABLE tickets (
  id          TEXT PRIMARY KEY,          -- 'q44s.7.24', no project prefix
  parent      TEXT,                      -- id with the last dotted part removed
  next_child  INTEGER NOT NULL DEFAULT 1,-- next number handed to a child
  title       TEXT NOT NULL,
  body        TEXT NOT NULL DEFAULT '',
  acceptance  TEXT NOT NULL DEFAULT '',
  design      TEXT NOT NULL DEFAULT '',
  notes       TEXT NOT NULL DEFAULT '',
  type        TEXT NOT NULL DEFAULT 'task',   -- task|epic|bug|feature|chore|decision|spike|story|milestone
  status      TEXT NOT NULL DEFAULT 'open',   -- open|in_progress|blocked|deferred|closed
  priority    INTEGER NOT NULL DEFAULT 2,     -- 0..4
  assignee    TEXT,
  last        INTEGER NOT NULL DEFAULT 0,     -- 1: runs after its siblings
  labels      TEXT NOT NULL DEFAULT '[]',     -- JSON array of strings
  metadata    TEXT NOT NULL DEFAULT '{}',     -- JSON object
  defer_until TEXT,
  close_reason TEXT,
  created_at  TEXT NOT NULL, created_by TEXT NOT NULL,
  updated_at  TEXT NOT NULL, closed_at TEXT,
  version     INTEGER NOT NULL,               -- value of seq at the last change
  workers     INTEGER,                        -- an epic's target number of workers
  max_workers INTEGER,                        -- an epic's hard cap on workers
  cores       REAL,                           -- a planner's figure for expected cores
  machine     TEXT,                           -- the machine working it
  session     TEXT,                           -- the session working it
  tree        TEXT,                           -- the build tree working it
  claimed_at  TEXT,
  landed_at   TEXT,
  landed      TEXT,                           -- JSON: {commits, files, note}
  saving      TEXT                            -- JSON: {rule, subject, unit, amount}
) WITHOUT ROWID;
CREATE INDEX tickets_live ON tickets(status) WHERE status != 'closed';
CREATE INDEX tickets_parent ON tickets(parent) WHERE parent IS NOT NULL;

CREATE TABLE edges (                       -- src depends on dst
  src TEXT NOT NULL, dst TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'blocks',     -- blocks|conditional-blocks|waits-for|related|discovered-from
  PRIMARY KEY (src, dst)
) WITHOUT ROWID;
CREATE INDEX edges_dst ON edges(dst);

CREATE TABLE comments (
  ticket TEXT NOT NULL, n INTEGER NOT NULL,
  author TEXT NOT NULL, at TEXT NOT NULL, text TEXT NOT NULL,
  legacy_id TEXT,                          -- ID of an imported Beads comment
  PRIMARY KEY (ticket, n)
) WITHOUT ROWID;

CREATE TABLE history (                     -- the changelog
  ticket TEXT NOT NULL, seq INTEGER NOT NULL,
  actor TEXT NOT NULL, at TEXT NOT NULL,
  change TEXT NOT NULL,                    -- JSON: {field: [old, new], ...}
  PRIMARY KEY (ticket, seq)
) WITHOUT ROWID;

CREATE TABLE leases (
  name TEXT PRIMARY KEY, holder TEXT NOT NULL, machine TEXT NOT NULL,
  expires_at TEXT NOT NULL, payload TEXT NOT NULL DEFAULT '{}',
  waiters TEXT NOT NULL DEFAULT '[]'       -- JSON array of {holder, machine, since}, arrival order
) WITHOUT ROWID;

CREATE TABLE aliases (alias TEXT PRIMARY KEY, id TEXT NOT NULL) WITHOUT ROWID;
CREATE INDEX aliases_id ON aliases(id);

CREATE TABLE machines (
  id TEXT PRIMARY KEY,                     -- the ID the daemon created
  name TEXT NOT NULL,                      -- hostname
  os TEXT NOT NULL,                        -- macos|windows
  cores INTEGER NOT NULL,
  memory_mb INTEGER NOT NULL,
  families TEXT NOT NULL DEFAULT '[]',     -- JSON array; empty means every family
  version TEXT NOT NULL,                   -- Ignite commit
  last_seen TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE workers (
  machine TEXT NOT NULL, session TEXT NOT NULL,
  app TEXT NOT NULL,                       -- claude|codex
  role TEXT NOT NULL,                      -- worker|dispatcher
  state TEXT NOT NULL,                     -- idle|working|stalled|down
  ticket TEXT,
  files TEXT NOT NULL DEFAULT '[]',        -- JSON: files its last ticket touched
  PRIMARY KEY (machine, session)
) WITHOUT ROWID;

CREATE TABLE reservations (
  ticket TEXT PRIMARY KEY, machine TEXT NOT NULL, session TEXT NOT NULL,
  expires_at TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE handoffs (
  seq INTEGER PRIMARY KEY,
  machine TEXT NOT NULL, ticket TEXT NOT NULL,
  kind TEXT NOT NULL,                      -- regression|improvement|product
  at TEXT NOT NULL
);
CREATE INDEX handoffs_machine ON handoffs(machine, seq);

CREATE TABLE results (                     -- the result cache
  check_name TEXT NOT NULL, hash TEXT NOT NULL,
  platform TEXT NOT NULL DEFAULT '',       -- empty for a portable check
  outcome TEXT NOT NULL,                   -- pass|fail
  machine TEXT NOT NULL, record INTEGER NOT NULL,
  at TEXT NOT NULL, last_lookup TEXT NOT NULL,
  output TEXT,                             -- where a failure's output is saved
  PRIMARY KEY (check_name, hash, platform)
) WITHOUT ROWID;
CREATE INDEX results_lookup ON results(last_lookup);

CREATE TABLE checks (
  name TEXT PRIMARY KEY, family TEXT NOT NULL,
  kind TEXT NOT NULL,                      -- regression|exploration
  state TEXT NOT NULL DEFAULT 'on',        -- on|off|uncached
  check_group TEXT NOT NULL
) WITHOUT ROWID;
CREATE INDEX checks_family ON checks(family);

CREATE TABLE check_stats (                 -- runs and failures after changes to an area
  check_name TEXT NOT NULL, area TEXT NOT NULL,
  runs INTEGER NOT NULL DEFAULT 0, failures INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (check_name, area)
) WITHOUT ROWID;

CREATE TABLE check_costs (
  check_name TEXT PRIMARY KEY,
  costs TEXT NOT NULL DEFAULT '[]',        -- JSON: the last 20 costs
  median REAL,
  unit TEXT NOT NULL                       -- instructions (billions)|cpu_s
) WITHOUT ROWID;

CREATE TABLE flaky (
  check_name TEXT NOT NULL, hash TEXT NOT NULL, commit_sha TEXT NOT NULL,
  machine TEXT NOT NULL, at TEXT NOT NULL,
  PRIMARY KEY (check_name, hash, commit_sha)
) WITHOUT ROWID;

CREATE TABLE regressions (
  check_name TEXT PRIMARY KEY,
  ticket TEXT NOT NULL,
  first_bad TEXT NOT NULL,                 -- the first bad landing's commit
  last_good TEXT NOT NULL,
  state TEXT NOT NULL                      -- open|fixed
) WITHOUT ROWID;

CREATE TABLE tips (
  commit_sha TEXT PRIMARY KEY,
  seen TEXT NOT NULL,                      -- when the hub first saw it
  verified TEXT,                           -- when verification finished
  state TEXT NOT NULL                      -- seen|verifying|verified|failed
) WITHOUT ROWID;

CREATE TABLE tip_claims (
  commit_sha TEXT NOT NULL, family TEXT NOT NULL, batch INTEGER NOT NULL,
  machine TEXT NOT NULL, expires_at TEXT NOT NULL,
  PRIMARY KEY (commit_sha, family, batch)
) WITHOUT ROWID;

CREATE TABLE records (
  id INTEGER PRIMARY KEY,
  machine TEXT NOT NULL, ticket TEXT, session TEXT, tree TEXT,
  verb TEXT NOT NULL, class TEXT NOT NULL, family TEXT,
  queued_s REAL NOT NULL, wall_s REAL NOT NULL, cpu_s REAL NOT NULL,
  instructions REAL, peak_mb INTEGER NOT NULL,
  outcome TEXT, commit_sha TEXT,
  ignite TEXT NOT NULL,                    -- the Ignite commit that made the record
  at TEXT NOT NULL
);
CREATE INDEX records_ticket ON records(ticket) WHERE ticket IS NOT NULL;
CREATE INDEX records_at ON records(at);

CREATE TABLE compile_units (
  record INTEGER NOT NULL, crate_name TEXT NOT NULL, mode TEXT NOT NULL,
  instructions REAL, cpu_s REAL NOT NULL, rebuilt INTEGER NOT NULL,
  PRIMARY KEY (record, crate_name, mode)
) WITHOUT ROWID;

CREATE TABLE findings (
  rule TEXT NOT NULL, subject TEXT NOT NULL,
  unit TEXT NOT NULL,                      -- dollars|agent-hours|core-hours
  cost REAL NOT NULL, saving REAL NOT NULL, -- per week
  ticket TEXT,
  closed_at TEXT,
  PRIMARY KEY (rule, subject)
) WITHOUT ROWID;

CREATE TABLE explore (
  family TEXT NOT NULL, day TEXT NOT NULL,
  bugs INTEGER NOT NULL DEFAULT 0, instructions REAL NOT NULL DEFAULT 0,
  PRIMARY KEY (family, day)
) WITHOUT ROWID;

CREATE TABLE rebuilds (
  name TEXT NOT NULL, commit_sha TEXT NOT NULL,
  instructions REAL NOT NULL,
  crates TEXT NOT NULL DEFAULT '{}',       -- JSON: instructions by crate
  PRIMARY KEY (name, commit_sha)
) WITHOUT ROWID;
