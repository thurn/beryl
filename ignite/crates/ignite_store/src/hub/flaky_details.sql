-- What a flaky record needs for the hub to apply its rules: whether the check is in the always
-- set, the command run it failed in (to spot one machine fault that flaked many checks), the
-- failed run's output to quote, and the ticket filed for it once one is.
ALTER TABLE flaky ADD COLUMN always INTEGER NOT NULL DEFAULT 0;
ALTER TABLE flaky ADD COLUMN command TEXT NOT NULL DEFAULT '';
ALTER TABLE flaky ADD COLUMN output TEXT NOT NULL DEFAULT '';
ALTER TABLE flaky ADD COLUMN filed TEXT;
