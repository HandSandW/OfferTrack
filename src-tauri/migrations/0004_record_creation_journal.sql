-- Persist intent before creating files. A record and the completed state commit
-- together; interrupted, uncommitted copies can only be cancelled, never guessed.
CREATE TABLE record_creations (
    application_id TEXT PRIMARY KEY NOT NULL,
    target_relative_path TEXT NOT NULL,
    directory_identity TEXT,
    manifest_json TEXT,
    state TEXT NOT NULL CHECK (state IN ('copying', 'verified', 'completed', 'cancelled')),
    created_at_utc TEXT NOT NULL,
    completed_at_utc TEXT
);
