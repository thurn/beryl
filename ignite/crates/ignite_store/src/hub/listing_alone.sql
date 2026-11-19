-- How long a tip round's listing ran with nothing else of the round running beside it, in
-- seconds (docs/ignite.md, "How long a round takes"). Only that part counts towards a slow
-- round. NULL from a daemon built before it was measured.
ALTER TABLE tip_rounds ADD COLUMN listing_alone_s REAL;
