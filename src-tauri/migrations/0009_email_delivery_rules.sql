PRAGMA foreign_keys = ON;

ALTER TABLE app_settings ADD COLUMN smtp_verified_at TEXT;

CREATE TABLE IF NOT EXISTS email_delivery_rules (
  id TEXT PRIMARY KEY,
  match_dimension TEXT NOT NULL
    CHECK (match_dimension IN ('EVENT_KIND', 'CATEGORY', 'TAG')),
  match_value TEXT NOT NULL,
  strategy TEXT NOT NULL
    CHECK (strategy IN ('FIRST_DUE', 'DAILY_FIRST', 'EACH_PLAN', 'DAILY_DIGEST')),
  recipient TEXT NOT NULL,
  digest_local_time TEXT NOT NULL DEFAULT '18:00',
  enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_email_rules_match
ON email_delivery_rules(enabled, match_dimension, match_value);

ALTER TABLE channel_deliveries ADD COLUMN recipient TEXT;
ALTER TABLE channel_deliveries ADD COLUMN rule_id TEXT;
ALTER TABLE channel_deliveries ADD COLUMN strategy TEXT;
ALTER TABLE channel_deliveries ADD COLUMN local_date TEXT;

CREATE TABLE IF NOT EXISTS email_digest_queue (
  id TEXT PRIMARY KEY,
  rule_id TEXT NOT NULL REFERENCES email_delivery_rules(id) ON DELETE CASCADE,
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  reminder_event_id TEXT NOT NULL REFERENCES reminder_events(id) ON DELETE CASCADE,
  recipient TEXT NOT NULL,
  local_date TEXT NOT NULL,
  due_at TEXT NOT NULL,
  queued_at TEXT NOT NULL,
  UNIQUE(rule_id, item_id, local_date)
);

CREATE TABLE IF NOT EXISTS email_digest_deliveries (
  id TEXT PRIMARY KEY,
  recipient TEXT NOT NULL,
  local_date TEXT NOT NULL,
  idempotency_key TEXT NOT NULL UNIQUE,
  item_count INTEGER NOT NULL,
  attempted_at TEXT NOT NULL,
  submitted_at TEXT,
  result TEXT NOT NULL CHECK (result IN ('CLAIMED', 'SUBMITTED', 'FAILED', 'DISABLED')),
  error_code TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  next_attempt_at TEXT
);

INSERT OR IGNORE INTO email_delivery_rules (
  id, match_dimension, match_value, strategy, recipient,
  digest_local_time, enabled, created_at, updated_at
)
SELECT
  'legacy-tag-' || r.tag_id,
  'TAG',
  r.tag_id,
  CASE WHEN s.smtp_repeat_must_complete = 1 THEN 'EACH_PLAN' ELSE 'FIRST_DUE' END,
  s.smtp_to,
  '18:00',
  CASE WHEN s.smtp_enabled = 1 AND length(trim(s.smtp_to)) > 0 THEN 1 ELSE 0 END,
  strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
  strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM tag_notification_routes r
CROSS JOIN app_settings s
WHERE r.channel = 'email' AND r.enabled = 1 AND s.id = 1;

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (9, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
