ALTER TABLE app_settings
ADD COLUMN autostart_enabled INTEGER NOT NULL DEFAULT 0 CHECK (autostart_enabled IN (0, 1));

ALTER TABLE app_settings
ADD COLUMN onboarding_version INTEGER NOT NULL DEFAULT 0 CHECK (onboarding_version >= 0);

INSERT INTO schema_migrations (version, applied_at)
VALUES (3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
