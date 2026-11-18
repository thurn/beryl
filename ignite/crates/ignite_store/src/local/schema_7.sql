-- Schema version 7. `owner` names the verb that leased a landing tree: the handle of the
-- landing, such as `L-6`, or nothing for a holder without a handle. A restarted daemon keeps a
-- landing tree whose owner it runs again instead of resetting it. Additive: a new nullable
-- column.

ALTER TABLE trees ADD COLUMN owner TEXT;
