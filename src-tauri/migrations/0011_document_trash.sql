-- Metadata is retained independently from the live path index. A new file at
-- the old path must receive a new document ID, not revive a trashed attachment.
CREATE TABLE document_trash (
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL CHECK (version = 1),
    document_id TEXT NOT NULL,
    application_id TEXT NOT NULL REFERENCES applications(id),
    relative_path TEXT NOT NULL,
    display_name TEXT NOT NULL,
    media_type TEXT,
    size_bytes INTEGER,
    content_hash TEXT,
    discovered_at_utc TEXT NOT NULL,
    last_observed_at_utc TEXT NOT NULL,
    deleted_at_utc TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'active', 'cancelled', 'restored', 'purged'))
);
CREATE INDEX idx_document_trash_state ON document_trash(state, deleted_at_utc);
CREATE TABLE document_moves (
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL CHECK (version = 1),
    trash_id TEXT NOT NULL REFERENCES document_trash(id),
    kind TEXT NOT NULL CHECK (kind IN ('trash', 'restore')),
    folder_relative_path TEXT NOT NULL,
    document_relative_path TEXT NOT NULL,
    file_identity TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    completed_at_utc TEXT,
    outcome TEXT CHECK (outcome IN ('completed', 'cancelled'))
);
CREATE UNIQUE INDEX idx_document_move_pending ON document_moves(trash_id) WHERE completed_at_utc IS NULL;
-- Recovery reconciles completion only. It NEVER resumes permanent deletion.
CREATE TABLE document_purges (
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL CHECK (version = 1),
    trash_id TEXT NOT NULL REFERENCES document_trash(id),
    created_at_utc TEXT NOT NULL,
    completed_at_utc TEXT,
    outcome TEXT CHECK (outcome IN ('completed', 'cancelled'))
);
