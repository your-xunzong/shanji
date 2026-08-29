PRAGMA foreign_keys = ON;

ALTER TABLE app_settings ADD COLUMN persistent_notifications_enabled INTEGER NOT NULL DEFAULT 1 CHECK (persistent_notifications_enabled IN (0, 1));
ALTER TABLE app_settings ADD COLUMN overlay_reminders_enabled INTEGER NOT NULL DEFAULT 0 CHECK (overlay_reminders_enabled IN (0, 1));
ALTER TABLE app_settings ADD COLUMN repeat_unacknowledged_enabled INTEGER NOT NULL DEFAULT 0 CHECK (repeat_unacknowledged_enabled IN (0, 1));
ALTER TABLE app_settings ADD COLUMN unacknowledged_repeat_minutes INTEGER NOT NULL DEFAULT 60 CHECK (unacknowledged_repeat_minutes BETWEEN 5 AND 240);
ALTER TABLE app_settings ADD COLUMN smtp_enabled INTEGER NOT NULL DEFAULT 0 CHECK (smtp_enabled IN (0, 1));
ALTER TABLE app_settings ADD COLUMN smtp_host TEXT NOT NULL DEFAULT '';
ALTER TABLE app_settings ADD COLUMN smtp_port INTEGER NOT NULL DEFAULT 465 CHECK (smtp_port BETWEEN 1 AND 65535);
ALTER TABLE app_settings ADD COLUMN smtp_security TEXT NOT NULL DEFAULT 'tls' CHECK (smtp_security IN ('tls', 'starttls'));
ALTER TABLE app_settings ADD COLUMN smtp_from TEXT NOT NULL DEFAULT '';
ALTER TABLE app_settings ADD COLUMN smtp_to TEXT NOT NULL DEFAULT '';
ALTER TABLE app_settings ADD COLUMN smtp_username TEXT NOT NULL DEFAULT '';
ALTER TABLE app_settings ADD COLUMN smtp_repeat_must_complete INTEGER NOT NULL DEFAULT 0 CHECK (smtp_repeat_must_complete IN (0, 1));

ALTER TABLE items ADD COLUMN reminder_acknowledged_at TEXT;

CREATE TABLE IF NOT EXISTS channel_deliveries (
  id TEXT PRIMARY KEY,
  reminder_event_id TEXT NOT NULL REFERENCES reminder_events(id) ON DELETE CASCADE,
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  channel TEXT NOT NULL CHECK (channel IN ('overlay', 'email')),
  idempotency_key TEXT NOT NULL UNIQUE,
  attempted_at TEXT NOT NULL,
  submitted_at TEXT,
  result TEXT NOT NULL CHECK (result IN ('CLAIMED', 'SUBMITTED', 'FAILED', 'DISABLED')),
  error_code TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  next_attempt_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_channel_deliveries_item_channel
ON channel_deliveries(item_id, channel, attempted_at);

CREATE TABLE IF NOT EXISTS tag_notification_routes (
  tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  channel TEXT NOT NULL CHECK (channel = 'email'),
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  PRIMARY KEY (tag_id, channel)
);

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
