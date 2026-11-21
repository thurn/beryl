-- The provider chosen before the worker exists. Keep it on the reservation until claim.
ALTER TABLE reservations ADD COLUMN provider TEXT;
