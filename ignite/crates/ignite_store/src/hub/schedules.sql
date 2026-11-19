-- Meta-tickets: a ticket of type `meta` is a template that the hub copies into a new child
-- ticket every `every_hours` hours (see `ignite_core::schedule`).
ALTER TABLE tickets ADD COLUMN every_hours INTEGER;    -- a meta-ticket's interval in hours
