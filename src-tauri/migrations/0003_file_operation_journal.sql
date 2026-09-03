-- Durable intent for moves crossing the filesystem / SQLite boundary.
CREATE TABLE file_operations (
    id TEXT PRIMARY KEY NOT NULL,
    operation_kind TEXT NOT NULL CHECK (operation_kind IN ('trash', 'restore', 'normalize')),
    application_id TEXT NOT NULL,
    trash_id TEXT NOT NULL,
    source_relative_path TEXT NOT NULL,
    target_relative_path TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    completed_at_utc TEXT,
    outcome TEXT
);
