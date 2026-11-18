-- Command lines the `ignite` executable refused to parse, so the hub can find the verbs and
-- flags agents look for and do not find.
CREATE TABLE rejected_commands (
    id INTEGER PRIMARY KEY,
    machine TEXT NOT NULL,
    session TEXT,
    ticket TEXT,
    -- The recognized subcommands, then the rejected word, such as `ticket dep list`.
    verb TEXT NOT NULL,
    -- JSON array of every argument.
    args TEXT NOT NULL,
    kind TEXT NOT NULL,
    suggestion TEXT,
    at TEXT NOT NULL
);
CREATE INDEX rejected_commands_at ON rejected_commands (at);
