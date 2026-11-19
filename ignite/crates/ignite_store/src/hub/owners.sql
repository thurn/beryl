-- Who works each ticket, a field of its own instead of the `provider:` and `needs:<machine>`
-- labels it replaces (see `ignite_core::owner`). `owners::retire_labels` moves those labels into
-- these columns after this script.
ALTER TABLE tickets ADD COLUMN owner TEXT;              -- '<machine>' or '<machine>/<app>'
ALTER TABLE tickets ADD COLUMN needs_os TEXT;           -- macos|windows
ALTER TABLE tickets ADD COLUMN fallback INTEGER NOT NULL DEFAULT 0; -- 1: holds master back
