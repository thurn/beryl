-- The two budgets (docs/ignite.md, "Where regression checks run").
--
-- `class_rates` holds, for each UTC day it was asked for, each class's rate: billions of
-- instructions per second of wall time that the class's check commands retired, the median of
-- the daily rates over the week before that day. NULL when the week holds no such record.
CREATE TABLE class_rates (
    day TEXT NOT NULL,
    class TEXT NOT NULL,
    rate REAL,
    PRIMARY KEY (day, class)
) WITHOUT ROWID;

-- How a round split its checks by the tip budget: a JSON `RoundBudget`, NULL from a daemon
-- built before the budget.
ALTER TABLE tip_rounds ADD COLUMN budget TEXT;
