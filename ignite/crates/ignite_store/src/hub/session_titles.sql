-- The latest title each Claude desktop session reported, so the dashboard can name a session by
-- its title instead of by its id. A session reports its title through `ignite`: the CLI with
-- each ticket write, and the daemon whenever a session in a build tree runs a command and its
-- title has changed since the daemon last reported it.
CREATE TABLE session_titles (
    session TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    updated_at TEXT NOT NULL
) WITHOUT ROWID;
