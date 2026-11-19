-- Checks that a landing found failing at the `staging` tip it was rebased onto, which tip
-- verification had missed (docs/ignite.md, "Tip verification"): one row per check and tip.
-- `kind` is `stale-pass` (tip verification passed the check at that tip or a later one),
-- `skipped` (a round at that tip or a later one left the check's family's checks unrun) or
-- `not-run` (a round finished there without running the check or leaving any of its family's
-- checks unrun). `reason` is the sentence the regression ticket's comment gives.
CREATE TABLE tip_misses (
    check_name TEXT NOT NULL,
    family TEXT NOT NULL,
    tip TEXT NOT NULL,
    kind TEXT NOT NULL,
    reason TEXT NOT NULL,
    ticket TEXT NOT NULL,
    at TEXT NOT NULL,
    PRIMARY KEY (check_name, tip)
) WITHOUT ROWID;
CREATE INDEX tip_misses_by_family ON tip_misses (family, at);
