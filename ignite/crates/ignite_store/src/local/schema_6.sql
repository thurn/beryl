-- Schema version 6. `listings` keeps each check family's listing under the family's name and
-- its listing hash (the hash of everything the family's `list` command can read), so that a
-- restarted daemon lists a family again only when what its listing was made from changed.
-- `listing` is the daemon's own JSON. Only the newest listings of each family are kept.

CREATE TABLE listings (
  family TEXT NOT NULL,
  hash TEXT NOT NULL,
  listing TEXT NOT NULL,
  at TEXT NOT NULL,
  PRIMARY KEY (family, hash)
);

CREATE INDEX listings_by_age ON listings (family, at);
