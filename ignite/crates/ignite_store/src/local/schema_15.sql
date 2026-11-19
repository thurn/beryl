-- Schema version 15. Express runs that passed, by the git tree they checked and the `[express]`
-- section they ran, so that a landing of the same tree skips the commands that `ignite check`
-- just ran. A row is kept for an hour.
--
-- Additive: a new table.

CREATE TABLE express_passes (
    tree_id TEXT NOT NULL,
    config TEXT NOT NULL,
    at TEXT NOT NULL,
    PRIMARY KEY (tree_id, config)
);
