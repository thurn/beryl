CREATE TABLE machine_snapshots (
    machine TEXT NOT NULL REFERENCES machines(id),
    at TEXT NOT NULL,
    free_disk_gib INTEGER,
    reclaimable_bytes INTEGER,
    reclaimable_measured_at TEXT,
    next_ticket TEXT,
    next_ticket_disk_gib INTEGER,
    start_block TEXT,
    start_reason TEXT,
    cpu_percent INTEGER,
    queued_cores INTEGER NOT NULL,
    swap_used_mb INTEGER,
    idle_workers INTEGER NOT NULL,
    working_workers INTEGER NOT NULL,
    stalled_workers INTEGER NOT NULL,
    down_workers INTEGER NOT NULL,
    ready_tickets INTEGER NOT NULL,
    PRIMARY KEY (machine, at)
) WITHOUT ROWID;
CREATE INDEX machine_snapshots_by_time ON machine_snapshots (at);
