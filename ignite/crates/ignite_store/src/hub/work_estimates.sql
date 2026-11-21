ALTER TABLE tickets ADD COLUMN estimate_minutes INTEGER CHECK (
    estimate_minutes IS NULL OR (typeof(estimate_minutes) = 'integer' AND estimate_minutes BETWEEN 1 AND 4294967295)
);
ALTER TABLE tickets ADD COLUMN unfiled_minutes INTEGER CHECK (
    unfiled_minutes IS NULL OR (typeof(unfiled_minutes) = 'integer' AND unfiled_minutes BETWEEN 1 AND 4294967295)
);
