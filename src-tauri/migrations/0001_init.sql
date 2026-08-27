PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  default_due_time TEXT NOT NULL,
  workdays TEXT NOT NULL,
  overtime_interval_minutes INTEGER NOT NULL CHECK (overtime_interval_minutes BETWEEN 5 AND 240),
  quiet_hours_enabled INTEGER NOT NULL CHECK (quiet_hours_enabled IN (0, 1)),
  quiet_start TEXT NOT NULL,
  quiet_end TEXT NOT NULL,
  global_shortcut TEXT NOT NULL,
  notifications_enabled INTEGER NOT NULL CHECK (notifications_enabled IN (0, 1))
);

CREATE TABLE IF NOT EXISTS categories (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  color TEXT NOT NULL,
  position INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1))
);

CREATE TABLE IF NOT EXISTS items (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 4000),
  notes TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL CHECK (status IN ('OPEN', 'DONE', 'ARCHIVED', 'DELETED')),
  category_id TEXT REFERENCES categories(id) ON DELETE SET NULL,
  due_at TEXT NOT NULL,
  due_local_date TEXT NOT NULL,
  due_local_time TEXT NOT NULL,
  due_source TEXT NOT NULL CHECK (due_source IN ('EXPLICIT', 'DEFAULT_EOD', 'ROLLED_OVER')),
  rollover_policy TEXT NOT NULL CHECK (rollover_policy IN ('NONE', 'NEXT_WORKDAY_EOD', 'DAILY')),
  rollover_count INTEGER NOT NULL DEFAULT 0,
  completion_policy TEXT NOT NULL CHECK (completion_policy IN ('NORMAL', 'MUST_COMPLETE_TODAY')),
  repeat_interval_minutes INTEGER CHECK (repeat_interval_minutes IS NULL OR repeat_interval_minutes BETWEEN 5 AND 240),
  next_reminder_at TEXT,
  reminder_paused INTEGER NOT NULL DEFAULT 0 CHECK (reminder_paused IN (0, 1)),
  bypass_app_quiet_hours INTEGER NOT NULL DEFAULT 0 CHECK (bypass_app_quiet_hours IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT,
  revision INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_items_status_due ON items(status, due_at);
CREATE INDEX IF NOT EXISTS idx_items_next_reminder ON items(status, reminder_paused, next_reminder_at);
CREATE INDEX IF NOT EXISTS idx_items_local_date ON items(due_local_date);

CREATE TABLE IF NOT EXISTS reminder_events (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  scheduled_for TEXT NOT NULL,
  sent_at TEXT NOT NULL,
  idempotency_key TEXT NOT NULL UNIQUE,
  result TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS item_events (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  event_type TEXT NOT NULL,
  event_data TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS drafts (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  content TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO app_settings (
  id, default_due_time, workdays, overtime_interval_minutes,
  quiet_hours_enabled, quiet_start, quiet_end, global_shortcut, notifications_enabled
) VALUES (
  1, '18:00', '[1,2,3,4,5]', 30,
  1, '22:30', '07:30', 'CommandOrControl+Shift+Space', 1
);

INSERT OR IGNORE INTO categories (id, name, color, position) VALUES
  ('work', '工作', '#627D98', 10),
  ('personal', '个人', '#6E8B74', 20),
  ('later', '稍后', '#9381A8', 30);

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

