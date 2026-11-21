CREATE TABLE fleet_changes (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    actor TEXT NOT NULL,
    summary TEXT NOT NULL,
    settings TEXT NOT NULL
);

CREATE TABLE provider_usage (
    provider TEXT NOT NULL,
    at TEXT NOT NULL,
    machine TEXT NOT NULL,
    status TEXT NOT NULL,
    label TEXT,
    percent_used REAL,
    resets_at TEXT,
    note TEXT
);

-- A missing label describes a sample with no windows. It has its own slot per provider.
CREATE UNIQUE INDEX provider_usage_window ON provider_usage (provider, (label IS NULL), coalesce(label, ''));

CREATE TABLE placement_deferrals (
    ticket TEXT PRIMARY KEY REFERENCES tickets(id) ON DELETE CASCADE ON UPDATE CASCADE,
    first_at TEXT NOT NULL
);

ALTER TABLE tickets ADD COLUMN provider TEXT;
