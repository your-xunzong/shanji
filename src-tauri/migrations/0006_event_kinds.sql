PRAGMA foreign_keys = ON;

ALTER TABLE items ADD COLUMN event_kind TEXT
  CHECK (event_kind IS NULL OR event_kind IN ('ORDINARY', 'ONE_TIME', 'TODAY_MUST', 'WARNING', 'CONTINUOUS', 'MONTHLY', 'YEARLY'));
ALTER TABLE items ADD COLUMN reminder_plan TEXT NOT NULL DEFAULT 'ONCE'
  CHECK (reminder_plan IN ('REPEAT', 'EMPHASIS', 'ONCE', 'FORCE', 'CUSTOM'));
ALTER TABLE items ADD COLUMN important INTEGER NOT NULL DEFAULT 0 CHECK (important IN (0, 1));
ALTER TABLE items ADD COLUMN time_mode TEXT NOT NULL DEFAULT 'DEFAULT'
  CHECK (time_mode IN ('DEFAULT', 'SPECIFIED'));
ALTER TABLE items ADD COLUMN start_at TEXT;
ALTER TABLE items ADD COLUMN end_at TEXT;
ALTER TABLE items ADD COLUMN target_at TEXT;
ALTER TABLE items ADD COLUMN lead_value INTEGER CHECK (lead_value IS NULL OR lead_value > 0);
ALTER TABLE items ADD COLUMN lead_unit TEXT
  CHECK (lead_unit IS NULL OR lead_unit IN ('DAY', 'WEEK', 'MONTH', 'YEAR'));
ALTER TABLE items ADD COLUMN cadence_value INTEGER CHECK (cadence_value IS NULL OR cadence_value > 0);
ALTER TABLE items ADD COLUMN cadence_unit TEXT
  CHECK (cadence_unit IS NULL OR cadence_unit IN ('DAY', 'WEEK', 'MONTH', 'YEAR'));
ALTER TABLE items ADD COLUMN emphasis_max_per_day INTEGER NOT NULL DEFAULT 8
  CHECK (emphasis_max_per_day BETWEEN 1 AND 96);
ALTER TABLE items ADD COLUMN emphasis_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN emphasis_count_local_date TEXT;
ALTER TABLE items ADD COLUMN classification_next_reminder_at TEXT;
ALTER TABLE items ADD COLUMN series_occurrence_pending INTEGER NOT NULL DEFAULT 0
  CHECK (series_occurrence_pending IN (0, 1));
ALTER TABLE items ADD COLUMN series_anchor_month INTEGER
  CHECK (series_anchor_month IS NULL OR series_anchor_month BETWEEN 1 AND 12);
ALTER TABLE items ADD COLUMN series_anchor_day INTEGER
  CHECK (series_anchor_day IS NULL OR series_anchor_day BETWEEN 1 AND 31);

UPDATE items
SET event_kind = CASE
      WHEN completion_policy = 'MUST_COMPLETE_TODAY' THEN 'TODAY_MUST'
      WHEN category_id IS NOT NULL THEN 'ORDINARY'
      ELSE NULL
    END,
    reminder_plan = CASE
      WHEN completion_policy = 'MUST_COMPLETE_TODAY' THEN 'EMPHASIS'
      WHEN (SELECT repeat_unacknowledged_enabled FROM app_settings WHERE id = 1) = 1 THEN 'EMPHASIS'
      ELSE 'ONCE'
    END,
    repeat_interval_minutes = CASE
      WHEN completion_policy = 'MUST_COMPLETE_TODAY' THEN repeat_interval_minutes
      WHEN (SELECT repeat_unacknowledged_enabled FROM app_settings WHERE id = 1) = 1
        THEN COALESCE(repeat_interval_minutes, (SELECT unacknowledged_repeat_minutes FROM app_settings WHERE id = 1))
      ELSE repeat_interval_minutes
    END,
    time_mode = CASE WHEN due_source = 'EXPLICIT' THEN 'SPECIFIED' ELSE 'DEFAULT' END,
    classification_next_reminder_at = CASE
      WHEN category_id IS NULL AND status = 'OPEN' THEN due_at
      ELSE NULL
    END;

CREATE TABLE IF NOT EXISTS event_kind_defaults (
  event_kind TEXT PRIMARY KEY
    CHECK (event_kind IN ('ORDINARY', 'ONE_TIME', 'TODAY_MUST', 'WARNING', 'CONTINUOUS', 'MONTHLY', 'YEARLY')),
  reminder_plan TEXT NOT NULL
    CHECK (reminder_plan IN ('REPEAT', 'EMPHASIS', 'ONCE', 'FORCE', 'CUSTOM')),
  updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO event_kind_defaults (event_kind, reminder_plan, updated_at) VALUES
  ('ORDINARY', 'REPEAT', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('ONE_TIME', 'ONCE', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('TODAY_MUST', 'EMPHASIS', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('WARNING', 'REPEAT', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('CONTINUOUS', 'CUSTOM', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('MONTHLY', 'ONCE', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('YEARLY', 'ONCE', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE IF NOT EXISTS classification_reminder_events (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  scheduled_for TEXT NOT NULL,
  attempted_at TEXT NOT NULL,
  submitted_at TEXT,
  idempotency_key TEXT NOT NULL UNIQUE,
  result TEXT NOT NULL CHECK (result IN ('CLAIMED', 'SUBMITTED', 'FAILED', 'DISABLED')),
  error_code TEXT
);

CREATE INDEX IF NOT EXISTS idx_items_classification_due
ON items(event_kind, status, classification_next_reminder_at);

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
