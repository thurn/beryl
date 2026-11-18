-- Each time someone switched a check on or off by hand (`ignite flaky on|off`): who did it,
-- from which machine, the state before and after, their note, and the flaky ticket the hub
-- commented on, if the check had one.
CREATE TABLE check_state_changes (
    id INTEGER PRIMARY KEY,
    check_name TEXT NOT NULL,
    state TEXT NOT NULL,                   -- on|off
    previous TEXT NOT NULL,                -- on|off|uncached
    actor TEXT NOT NULL,
    machine TEXT NOT NULL,
    note TEXT NOT NULL,
    ticket TEXT,
    at TEXT NOT NULL
);
CREATE INDEX check_state_changes_check ON check_state_changes (check_name, id);
