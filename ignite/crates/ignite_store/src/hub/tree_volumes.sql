CREATE TABLE machine_tree_volumes (
    machine TEXT PRIMARY KEY REFERENCES machines(id) ON DELETE CASCADE,
    observed_at TEXT NOT NULL,
    volumes TEXT NOT NULL
);
