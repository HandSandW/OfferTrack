-- Separate from record-folder moves: both paths are relative to one application.
CREATE TABLE document_renames (
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL CHECK (version = 1),
    application_id TEXT NOT NULL,
    document_id TEXT NOT NULL,
    folder_relative_path TEXT NOT NULL,
    source_relative_path TEXT NOT NULL,
    target_relative_path TEXT NOT NULL,
    file_identity TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    completed_at_utc TEXT,
    outcome TEXT CHECK (outcome IN ('completed', 'cancelled'))
);
CREATE UNIQUE INDEX idx_document_rename_pending
ON document_renames(document_id) WHERE completed_at_utc IS NULL;
