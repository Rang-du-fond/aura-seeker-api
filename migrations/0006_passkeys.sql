CREATE TABLE passkeys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    passkey TEXT NOT NULL,
    label TEXT,
    created_at BIGINT NOT NULL,
    last_used_at BIGINT
);
