-- Schema version 9. `builds` holds what the build manager knows about each build product in
-- each tree, so that a restarted daemon does not run Cargo again for a product a tree already
-- holds, or for one whose last build with the same inputs failed.
--
-- One row per tree, product and set of packages (`packages`, a JSON array of package names,
-- sorted; `[]` for the whole workspace). `hash` is the input hash the product was last built or
-- last failed at. `outcome` is `held` (the tree holds the product built at `hash`) or `failed`
-- (the build at `hash` exited with a failing code, and `output` names the file with its output).
--
-- Additive: a new table.

CREATE TABLE builds (
    tree TEXT NOT NULL,
    product TEXT NOT NULL,
    packages TEXT NOT NULL,
    hash TEXT NOT NULL,
    outcome TEXT NOT NULL,
    output TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tree, product, packages)
);
