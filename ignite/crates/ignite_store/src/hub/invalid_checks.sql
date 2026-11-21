-- Keep each check's failure evidence even if its regression ticket is reused.
ALTER TABLE regressions ADD COLUMN culprit_ticket TEXT;
ALTER TABLE regressions ADD COLUMN failure_output TEXT;
ALTER TABLE check_fix_outcomes ADD COLUMN failure_output TEXT;
-- Empty queues a review, NULL means no review is needed, and a ticket ID records the remedy.
ALTER TABLE check_fix_outcomes ADD COLUMN remedy_ticket TEXT;
CREATE INDEX check_fix_outcomes_pending_remedy
    ON check_fix_outcomes (close_seq, check_name) WHERE remedy_ticket = '';
