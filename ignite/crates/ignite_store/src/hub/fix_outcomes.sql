CREATE TABLE check_fix_outcomes (
    close_seq INTEGER NOT NULL,
    check_name TEXT NOT NULL,
    ticket TEXT NOT NULL,
    at TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('product', 'test-only', 'none')),
    source TEXT NOT NULL CHECK (source IN ('regression', 'flaky')),
    first_bad TEXT,
    fixing_commit TEXT,
    PRIMARY KEY (close_seq, check_name, source)
) WITHOUT ROWID;
CREATE INDEX check_fix_outcomes_by_check ON check_fix_outcomes (check_name, at);
CREATE INDEX check_fix_outcomes_by_time ON check_fix_outcomes (at);
CREATE INDEX check_fix_outcomes_by_ticket ON check_fix_outcomes (ticket);
CREATE INDEX regressions_by_ticket ON regressions (ticket);
CREATE INDEX flaky_by_ticket ON flaky (filed) WHERE filed IS NOT NULL;
