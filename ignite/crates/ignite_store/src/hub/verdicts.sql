-- Each verdict: the record that one exact state of a change passed the landing gate. The key
-- is the git tree ID of the ticket tree's files, the merge base against the integration
-- branch, and the digest of the gate's settings. `verdict` is the daemon's own JSON, which
-- the hub does not read. Verdicts older than 30 days are deleted.
CREATE TABLE verdicts (
    tree TEXT NOT NULL,
    base TEXT NOT NULL,
    settings TEXT NOT NULL,
    verdict TEXT NOT NULL,
    passed_at TEXT NOT NULL,
    PRIMARY KEY (tree, base, settings)
);
CREATE INDEX verdicts_by_age ON verdicts (passed_at);
