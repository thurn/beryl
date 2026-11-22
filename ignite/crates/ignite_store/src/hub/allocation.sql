-- Keep pending tickets and their previously configured weights so legacy release still works.
-- The pending state remains a dispatch hold until an allocation edit or a legacy release.
UPDATE tickets SET allocation = CASE
    WHEN type IN ('epic', 'milestone', 'meta') THEN workers
    WHEN priority = 0 THEN 300
    WHEN priority = 1 THEN 200
    WHEN priority = 3 THEN 50
    WHEN priority = 4 THEN 25
END;
