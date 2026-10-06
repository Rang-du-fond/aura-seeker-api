CREATE TABLE tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE files (
    id TEXT PRIMARY KEY,
    media_type TEXT NOT NULL,
    size BIGINT NOT NULL
);

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    email_verified_at TEXT,
    display_name TEXT,
    role TEXT NOT NULL,
    security_version INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    disabled_at TEXT
);

CREATE TABLE identities (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    provider_email TEXT,
    provider_email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    password_hash TEXT,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    UNIQUE (provider, subject)
);

CREATE TABLE places (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    latitude DOUBLE PRECISION NOT NULL,
    longitude DOUBLE PRECISION NOT NULL,
    image TEXT NOT NULL REFERENCES files (id),
    author TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    archived_at TEXT
);

CREATE TABLE places_tags (
    resource_id TEXT NOT NULL REFERENCES places (id) ON DELETE CASCADE,
    value TEXT NOT NULL REFERENCES tags (name),
    PRIMARY KEY (resource_id, value)
);
