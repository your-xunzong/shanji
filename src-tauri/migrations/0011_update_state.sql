PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS update_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  auto_check_enabled INTEGER NOT NULL DEFAULT 0 CHECK (auto_check_enabled IN (0, 1)),
  permission_prompted INTEGER NOT NULL DEFAULT 0 CHECK (permission_prompted IN (0, 1)),
  last_checked_at TEXT,
  last_check_result TEXT CHECK (last_check_result IN ('UP_TO_DATE', 'UPDATE_AVAILABLE', 'FAILED')),
  snoozed_version TEXT,
  snoozed_until TEXT,
  last_notified_version TEXT,
  release_notes_seen_version TEXT,
  updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO update_state (id, updated_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (11, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
