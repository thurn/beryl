-- The efficiency feed resumes by ingestion ID even when two imports share an arrival time.
ALTER TABLE metrics ADD COLUMN ingest_id INTEGER;
CREATE UNIQUE INDEX metrics_ingest_id ON metrics(ingest_id);
CREATE INDEX metrics_export_headers ON metrics(
    machine, source, CAST(json_extract(data, '$.schema') AS INTEGER) DESC, ingest_id DESC
) WHERE kind IS NULL AND CASE WHEN json_valid(data) THEN json_type(data, '$.export') IS NOT NULL END;
