PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tags (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  color TEXT NOT NULL,
  position INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1))
);

CREATE TABLE IF NOT EXISTS item_tags (
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (item_id, tag_id)
);

ALTER TABLE items ADD COLUMN deleted_at TEXT;
ALTER TABLE items ADD COLUMN deleted_from_status TEXT
  CHECK (deleted_from_status IS NULL OR deleted_from_status IN ('OPEN', 'DONE', 'ARCHIVED'));

CREATE INDEX IF NOT EXISTS idx_item_tags_tag ON item_tags(tag_id, item_id);
CREATE INDEX IF NOT EXISTS idx_items_deleted_at ON items(status, deleted_at);

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
