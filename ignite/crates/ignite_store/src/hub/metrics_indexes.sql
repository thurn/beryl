-- The hub's two readers of `metrics` look up rows by source (the efficiency rule's `findings`)
-- or by kind (the backlog's `window_dollars`), each over a span of arrival times. Without these
-- indexes each reading scanned the whole table, 1.5 million rows on 2026-11-20.
CREATE INDEX metrics_source_at ON metrics(source, at);
CREATE INDEX metrics_kind_at ON metrics(kind, at);
