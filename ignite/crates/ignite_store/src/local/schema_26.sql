-- The most recent failed probe can have a different cause from the original hold.
ALTER TABLE cli_holds ADD COLUMN probe_reason TEXT;
ALTER TABLE cli_holds ADD COLUMN probe_message TEXT;
