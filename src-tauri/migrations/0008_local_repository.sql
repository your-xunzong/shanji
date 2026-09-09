PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS app_instance (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  instance_id TEXT NOT NULL,
  created_at TEXT NOT NULL
);

INSERT OR IGNORE INTO app_instance (id, instance_id, created_at)
VALUES (1, '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE IF NOT EXISTS data_repository_config (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  path TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  last_synced_at TEXT,
  last_package_at TEXT,
  last_error_code TEXT
);

CREATE TABLE IF NOT EXISTS repository_seen_revisions (
  source_instance_id TEXT NOT NULL,
  item_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  content_hash TEXT NOT NULL,
  destination_item_id TEXT,
  seen_at TEXT NOT NULL,
  PRIMARY KEY (source_instance_id, item_id)
);

CREATE TABLE IF NOT EXISTS item_tombstones (
  item_id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL,
  deleted_at TEXT NOT NULL,
  source_instance_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS merge_conflicts (
  id TEXT PRIMARY KEY,
  original_item_id TEXT NOT NULL,
  conflict_item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  source_instance_id TEXT NOT NULL,
  detected_at TEXT NOT NULL,
  resolved_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_merge_conflicts_open
ON merge_conflicts(resolved_at, detected_at);

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
