-- Image attachments on tickets (docs/ignite.md, "Attachments"). The file itself lives on the
-- hub's disk under its SHA-256, so the same image attached twice is stored once; this table
-- holds what each ticket calls it. Removing a file sets `removed_at` and keeps the row, so the
-- ticket can still say the image was there and when it went.
CREATE TABLE attachments (
    ticket TEXT NOT NULL,
    name TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    media_type TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    caption TEXT,
    created_at TEXT NOT NULL,
    created_by TEXT NOT NULL,
    removed_at TEXT,
    PRIMARY KEY (ticket, name)
);
CREATE INDEX attachments_by_sha256 ON attachments (sha256);
