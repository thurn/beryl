-- Tip verification rounds (docs/ignite.md, "How long a round takes").
--
-- `tip_costs` holds the check costs and claim length a round at one tip cuts one family's
-- batches by, frozen the first time any machine asks, so every machine cuts the same batches.
-- `costs` is a JSON object of check name to CPU seconds.
CREATE TABLE tip_costs (
    commit_sha TEXT NOT NULL,
    family TEXT NOT NULL,
    claim_seconds REAL NOT NULL,
    costs TEXT NOT NULL,
    at TEXT NOT NULL,
    PRIMARY KEY (commit_sha, family)
);
CREATE INDEX tip_costs_by_age ON tip_costs (at);

-- One row per finished round: where its time went, in seconds.
CREATE TABLE tip_rounds (
    id INTEGER PRIMARY KEY,
    commit_sha TEXT NOT NULL,
    machine TEXT NOT NULL,
    at TEXT NOT NULL,
    wall_s REAL NOT NULL,
    ran INTEGER NOT NULL,
    cached INTEGER NOT NULL,
    checks_s REAL NOT NULL,
    listing_s REAL NOT NULL,
    building_s REAL NOT NULL,
    waiting_s REAL NOT NULL
);
CREATE INDEX tip_rounds_by_time ON tip_rounds (at);
