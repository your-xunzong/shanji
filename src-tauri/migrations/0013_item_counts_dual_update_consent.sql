PRAGMA foreign_keys = ON;

ALTER TABLE app_settings ADD COLUMN show_item_counts INTEGER NOT NULL DEFAULT 1
  CHECK (show_item_counts IN (0, 1));

ALTER TABLE update_state ADD COLUMN permission_scope_version INTEGER NOT NULL DEFAULT 1
  CHECK (permission_scope_version >= 1);

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (13, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
