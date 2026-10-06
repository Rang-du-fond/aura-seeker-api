CREATE TABLE email_challenges (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    purpose TEXT NOT NULL,
    user_id TEXT REFERENCES users (id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    attempts INTEGER NOT NULL,
    expires_at BIGINT NOT NULL,
    consumed_at BIGINT,
    ip TEXT,
    created_at BIGINT NOT NULL
);
