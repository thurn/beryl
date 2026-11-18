-- breaking: ticket IDs are stored as they are shown, a `T` and then capitals, digits, dots and
-- dashes (`q44s.7` becomes `TQ44S.7`), so an older build would no longer find them.
--
-- Columns that may name something other than a ticket (a record from a tree with no ticket,
-- or a ticket since deleted) are rewritten only where they name a ticket. The `tickets` table is
-- rewritten last, since every other statement looks its IDs up there. Ticket names inside free
-- text and the changelog are rewritten by `capital_ids::rewrite_text` after this script.
UPDATE edges SET src = 'T' || upper(src), dst = 'T' || upper(dst);
UPDATE comments SET ticket = 'T' || upper(ticket);
UPDATE history SET ticket = 'T' || upper(ticket);
UPDATE aliases SET id = 'T' || upper(id) WHERE id IN (SELECT id FROM tickets);
UPDATE workers SET ticket = 'T' || upper(ticket) WHERE ticket IN (SELECT id FROM tickets);
UPDATE reservations SET ticket = 'T' || upper(ticket) WHERE ticket IN (SELECT id FROM tickets);
UPDATE handoffs SET ticket = 'T' || upper(ticket) WHERE ticket IN (SELECT id FROM tickets);
UPDATE regressions SET ticket = 'T' || upper(ticket) WHERE ticket IN (SELECT id FROM tickets);
UPDATE records SET ticket = 'T' || upper(ticket) WHERE ticket IN (SELECT id FROM tickets);
UPDATE findings SET ticket = 'T' || upper(ticket) WHERE ticket IN (SELECT id FROM tickets);
UPDATE feed SET ticket = 'T' || upper(ticket) WHERE ticket IN (SELECT id FROM tickets);
UPDATE check_state_changes SET ticket = 'T' || upper(ticket)
  WHERE ticket IN (SELECT id FROM tickets);
UPDATE rejected_commands SET ticket = 'T' || upper(ticket)
  WHERE ticket IN (SELECT id FROM tickets);
UPDATE flaky SET filed = 'T' || upper(filed) WHERE filed IN (SELECT id FROM tickets);
-- A create request's key, kept for a day so that a retried request returns the ticket it made.
UPDATE meta SET value = json_set(value, '$.id', 'T' || upper(value ->> '$.id'))
  WHERE key GLOB 'create:*' AND value ->> '$.id' IN (SELECT id FROM tickets);
UPDATE tickets SET id = 'T' || upper(id), parent = 'T' || upper(parent);
