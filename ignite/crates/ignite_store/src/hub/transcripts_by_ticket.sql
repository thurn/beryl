-- The dashboard shows a ticket's transcripts at `/<ticket>/session`, and its ticket page lists
-- the sessions that have lines on the ticket, so lines are also found by ticket.
CREATE INDEX transcript_lines_by_ticket ON transcript_lines (ticket, session, seq);
