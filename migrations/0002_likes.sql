CREATE TABLE places_liked_by (
    resource_id TEXT NOT NULL REFERENCES places (id) ON DELETE CASCADE,
    value TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (resource_id, value)
);
