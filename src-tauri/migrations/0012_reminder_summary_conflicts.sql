PRAGMA foreign_keys = ON;

ALTER TABLE app_settings ADD COLUMN important_default_reminder_plan TEXT NOT NULL DEFAULT 'EMPHASIS'
  CHECK (important_default_reminder_plan IN ('REPEAT', 'EMPHASIS', 'ONCE', 'FORCE', 'CUSTOM'));
ALTER TABLE app_settings ADD COLUMN repeat_detailed_notifications_enabled INTEGER NOT NULL DEFAULT 0
  CHECK (repeat_detailed_notifications_enabled IN (0, 1));

ALTER TABLE items ADD COLUMN reminder_plan_source TEXT NOT NULL DEFAULT 'MIGRATED'
  CHECK (reminder_plan_source IN ('EVENT_KIND_DEFAULT', 'IMPORTANT_DEFAULT', 'ITEM_OVERRIDE', 'MIGRATED'));

CREATE TABLE IF NOT EXISTS repository_conflict_resolutions (
  fingerprint TEXT PRIMARY KEY,
  conflict_id TEXT NOT NULL,
  original_item_id TEXT NOT NULL,
  conflict_item_id TEXT NOT NULL,
  resolution TEXT NOT NULL CHECK (resolution IN ('KEEP_ORIGINAL', 'KEEP_CONFLICT', 'KEEP_BOTH', 'AUTO_DUPLICATE')),
  kept_content_hash TEXT,
  resolved_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_repository_conflict_resolution_items
ON repository_conflict_resolutions(original_item_id, conflict_item_id);

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (12, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
