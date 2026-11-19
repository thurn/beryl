-- Schema version 14. A Codex dispatcher's worker slots. Codex keeps no standing worker
-- sessions: each ticket runs in a new chat. `slots` is how many tickets a dispatcher's chats may
-- run at once (0 for every other member), and `single_use` marks a worker that is such a chat,
-- which the daemon removes from the pool once its ticket ends instead of handing it another.
--
-- Additive: two new columns with defaults.

ALTER TABLE pool ADD COLUMN slots INTEGER NOT NULL DEFAULT 0;
ALTER TABLE pool ADD COLUMN single_use INTEGER NOT NULL DEFAULT 0;
