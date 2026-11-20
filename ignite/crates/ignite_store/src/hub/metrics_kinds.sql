-- Each posted efficiency row's kind: the export table it came from (`"table"` in the row), or
-- NULL for an export's header and for a findings report. The migration's code deletes the rows
-- the efficiency dashboard rebuilds from the hub's own records and fills in the kind of every
-- row left (`hub::metrics::prune_and_fill_kinds`), before the next migration indexes them.
ALTER TABLE metrics ADD COLUMN kind TEXT;
