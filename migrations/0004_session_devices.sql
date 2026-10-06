ALTER TABLE sessions ADD COLUMN device_label TEXT;
ALTER TABLE sessions ADD COLUMN user_agent TEXT;
ALTER TABLE sessions ADD COLUMN ip_created TEXT;
ALTER TABLE sessions ADD COLUMN security_version INTEGER NOT NULL DEFAULT 0;
