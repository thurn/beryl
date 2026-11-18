-- Each check family's listing as some machine made it, under the family's name and its
-- listing hash (the hash of everything the family's `list` command can read), so that a
-- listing one machine made serves the others. `listing` is the daemon's own JSON, which the
-- hub does not read. Only the newest listings of each family are kept.
CREATE TABLE listings (
    family TEXT NOT NULL,
    hash TEXT NOT NULL,
    listing TEXT NOT NULL,
    machine TEXT NOT NULL,
    at TEXT NOT NULL,
    PRIMARY KEY (family, hash)
);
CREATE INDEX listings_by_age ON listings (family, at);
