-- Schema version 3. For each leased tree, the newest comment and history entry of its ticket
-- that the tree's session has been shown, so that each change to the ticket is printed on one
-- command only, and a restarted daemon does not print it again. The row names its ticket: a row
-- left by an earlier lease of the tree counts for nothing.

CREATE TABLE shown (
  tree TEXT PRIMARY KEY,
  ticket TEXT NOT NULL,
  comment INTEGER NOT NULL,               -- the newest comment number shown
  history INTEGER NOT NULL                -- the newest history sequence number shown
) WITHOUT ROWID;
