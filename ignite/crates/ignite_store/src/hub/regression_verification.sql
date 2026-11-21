-- A verified tip can omit checks deferred to another platform. Keep the check results
-- that actually passed so the hub can recognize complete regression fixes.
ALTER TABLE tips ADD COLUMN regression_passes TEXT;
ALTER TABLE regressions ADD COLUMN platform TEXT;
