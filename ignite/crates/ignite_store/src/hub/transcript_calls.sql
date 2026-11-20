-- A tool result's line names the line of the tool call it answers, by that line's index in the
-- same turn (docs/ignite.md, "CLI mode"). Lines of other kinds, and lines from a daemon that
-- sends no tool results, leave it empty.
ALTER TABLE transcript_lines ADD COLUMN call_index INTEGER;
