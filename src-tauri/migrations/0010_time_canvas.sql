PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS timeline_view_preferences (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  scale TEXT NOT NULL DEFAULT 'MONTH' CHECK (scale IN ('DAY', 'WEEK', 'MONTH', 'YEAR')),
  include_done INTEGER NOT NULL DEFAULT 0 CHECK (include_done IN (0, 1)),
  updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO timeline_view_preferences (id, scale, include_done, updated_at)
VALUES (1, 'MONTH', 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

INSERT OR IGNORE INTO schema_migrations (version, applied_at)
VALUES (10, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
