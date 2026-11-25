-- The commit a tree was checked out at when it built the product, so that tip verification can
-- lease the landing tree that last built the products a family of checks needs.
ALTER TABLE builds ADD COLUMN built_commit TEXT;
