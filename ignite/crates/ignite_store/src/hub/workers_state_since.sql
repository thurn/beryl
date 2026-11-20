-- When each worker last changed state, as the hub saw it across heartbeats, so upkeep can tell a
-- session that has been idle for a while from one that has only just finished a turn.
ALTER TABLE workers ADD COLUMN state_since TEXT;
