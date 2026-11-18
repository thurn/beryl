-- The one-time code a reservation's hand-off must send to claim the ticket.
ALTER TABLE reservations ADD COLUMN code TEXT NOT NULL DEFAULT '';

-- Pauses of hand-offs, set by `ignite pool pause`: one row per paused machine, and the row
-- '*' while every machine is paused.
CREATE TABLE pauses (
  machine TEXT PRIMARY KEY,
  at TEXT NOT NULL
) WITHOUT ROWID;
