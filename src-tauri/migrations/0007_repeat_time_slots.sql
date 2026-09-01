PRAGMA foreign_keys = ON;

ALTER TABLE app_settings ADD COLUMN repeat_default_time_first TEXT NOT NULL DEFAULT '10:00';
ALTER TABLE app_settings ADD COLUMN repeat_default_time_second TEXT NOT NULL DEFAULT '17:00';

ALTER TABLE items ADD COLUMN repeat_time_mode TEXT NOT NULL DEFAULT 'SPECIFIED'
  CHECK (repeat_time_mode IN ('DEFAULT', 'SPECIFIED'));
ALTER TABLE items ADD COLUMN repeat_time_first TEXT;
ALTER TABLE items ADD COLUMN repeat_time_second TEXT;

UPDATE items
SET repeat_time_mode = 'SPECIFIED',
    repeat_time_first = due_local_time,
    repeat_time_second = NULL
WHERE reminder_plan = 'REPEAT';

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
