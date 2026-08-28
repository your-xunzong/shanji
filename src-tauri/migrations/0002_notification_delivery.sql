ALTER TABLE reminder_events ADD COLUMN submitted_at TEXT;
ALTER TABLE reminder_events ADD COLUMN error_code TEXT;
ALTER TABLE reminder_events ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN notification_failure_count INTEGER NOT NULL DEFAULT 0;

UPDATE reminder_events SET result = 'CLAIMED' WHERE result = 'QUEUED';

INSERT INTO schema_migrations (version, applied_at)
VALUES (2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
