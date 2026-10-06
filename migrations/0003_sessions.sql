CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    methods TEXT NOT NULL,
    auth_time BIGINT NOT NULL,
    last_used_at BIGINT NOT NULL,
    idle_expires_at BIGINT NOT NULL,
    absolute_expires_at BIGINT NOT NULL,
    revoked_at BIGINT,
    revoke_reason TEXT
);

CREATE TABLE refresh_tokens (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL,
    replaced_at BIGINT,
    replaced_by TEXT
);
