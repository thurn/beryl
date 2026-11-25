-- 1 when the user approved a blocking edge whose prerequisite lies outside the dependent
-- ticket's outermost epic (`ignite ticket dep add --external`). The hub refuses such an edge
-- otherwise, and skips the epic rule for an approved one when a ticket moves or an epic is
-- released.
ALTER TABLE edges ADD COLUMN external INTEGER NOT NULL DEFAULT 0;
