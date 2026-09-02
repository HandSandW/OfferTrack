CREATE TABLE IF NOT EXISTS applications (
    id TEXT PRIMARY KEY NOT NULL,
    short_id TEXT NOT NULL UNIQUE,
    created_at_utc TEXT NOT NULL,
    created_timezone_offset_minutes INTEGER NOT NULL,
    application_date TEXT,
    company_name TEXT NOT NULL DEFAULT '',
    company_type TEXT NOT NULL DEFAULT 'uncategorized',
    position_name TEXT NOT NULL DEFAULT '',
    application_url TEXT,
    announcement_url TEXT,
    company_url TEXT,
    position_url TEXT,
    position_description TEXT NOT NULL DEFAULT '',
    notes TEXT NOT NULL DEFAULT '',
    folder_relative_path TEXT NOT NULL UNIQUE,
    current_stage_id TEXT,
    current_stage_state TEXT NOT NULL DEFAULT 'pending',
    status_updated_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    archived_at_utc TEXT,
    deleted_at_utc TEXT,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);

CREATE INDEX IF NOT EXISTS idx_applications_created_at
    ON applications(created_at_utc DESC);
CREATE INDEX IF NOT EXISTS idx_applications_status
    ON applications(current_stage_id, current_stage_state);
CREATE INDEX IF NOT EXISTS idx_applications_archived_deleted
    ON applications(archived_at_utc, deleted_at_utc);

CREATE TABLE IF NOT EXISTS field_definitions (
    id TEXT PRIMARY KEY NOT NULL,
    key TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    field_type TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    display_order INTEGER NOT NULL,
    is_visible INTEGER NOT NULL DEFAULT 1 CHECK (is_visible IN (0, 1)),
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS field_values (
    application_id TEXT NOT NULL,
    field_definition_id TEXT NOT NULL,
    value_json TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    PRIMARY KEY (application_id, field_definition_id),
    FOREIGN KEY (application_id) REFERENCES applications(id) ON DELETE CASCADE,
    FOREIGN KEY (field_definition_id) REFERENCES field_definitions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tags (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS application_tags (
    application_id TEXT NOT NULL,
    tag_id TEXT NOT NULL,
    display_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (application_id, tag_id),
    FOREIGN KEY (application_id) REFERENCES applications(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS workflow_templates (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_stages (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT,
    template_id TEXT,
    stable_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    stage_kind TEXT NOT NULL,
    display_order INTEGER NOT NULL,
    color TEXT NOT NULL,
    is_terminal INTEGER NOT NULL DEFAULT 0 CHECK (is_terminal IN (0, 1)),
    terminal_outcome TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    CHECK (
        (application_id IS NOT NULL AND template_id IS NULL)
        OR (application_id IS NULL AND template_id IS NOT NULL)
    ),
    FOREIGN KEY (application_id) REFERENCES applications(id) ON DELETE CASCADE,
    FOREIGN KEY (template_id) REFERENCES workflow_templates(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_workflow_stages_application
    ON workflow_stages(application_id, display_order);
CREATE INDEX IF NOT EXISTS idx_workflow_stages_template
    ON workflow_stages(template_id, display_order);

CREATE TABLE IF NOT EXISTS workflow_events (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT NOT NULL,
    stage_id TEXT,
    stage_name_snapshot TEXT NOT NULL,
    previous_state TEXT,
    next_state TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    occurred_at_utc TEXT NOT NULL,
    actor_type TEXT NOT NULL,
    FOREIGN KEY (application_id) REFERENCES applications(id) ON DELETE CASCADE,
    FOREIGN KEY (stage_id) REFERENCES workflow_stages(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_workflow_events_application_time
    ON workflow_events(application_id, occurred_at_utc DESC);

CREATE TABLE IF NOT EXISTS interview_rounds (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT NOT NULL,
    sequence_number INTEGER NOT NULL CHECK (sequence_number > 0),
    display_name TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    scheduled_at_utc TEXT,
    completed_at_utc TEXT,
    notes TEXT NOT NULL DEFAULT '',
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    UNIQUE (application_id, sequence_number),
    FOREIGN KEY (application_id) REFERENCES applications(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    display_name TEXT NOT NULL,
    media_type TEXT,
    size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
    content_hash TEXT,
    discovered_at_utc TEXT NOT NULL,
    last_observed_at_utc TEXT NOT NULL,
    missing_at_utc TEXT,
    UNIQUE (application_id, relative_path),
    FOREIGN KEY (application_id) REFERENCES applications(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_documents_application
    ON documents(application_id, relative_path);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT,
    title TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    due_at_utc TEXT,
    priority TEXT NOT NULL DEFAULT 'normal',
    completed_at_utc TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    deleted_at_utc TEXT,
    FOREIGN KEY (application_id) REFERENCES applications(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_tasks_due
    ON tasks(completed_at_utc, due_at_utc);

CREATE TABLE IF NOT EXISTS recruitment_events (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT,
    event_type TEXT NOT NULL,
    title TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    starts_at_utc TEXT NOT NULL,
    ends_at_utc TEXT,
    completed_at_utc TEXT,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL,
    FOREIGN KEY (application_id) REFERENCES applications(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_recruitment_events_start
    ON recruitment_events(starts_at_utc);

CREATE TABLE IF NOT EXISTS reminder_rules (
    id TEXT PRIMARY KEY NOT NULL,
    stable_key TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    is_enabled INTEGER NOT NULL DEFAULT 1 CHECK (is_enabled IN (0, 1)),
    threshold_json TEXT NOT NULL,
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS views (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    view_kind TEXT NOT NULL,
    layout_json TEXT NOT NULL,
    sort_json TEXT NOT NULL DEFAULT '[]',
    filter_json TEXT NOT NULL DEFAULT '[]',
    group_json TEXT,
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at_utc TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS trash_entries (
    id TEXT PRIMARY KEY NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    original_relative_path TEXT,
    trash_relative_path TEXT,
    manifest_json TEXT NOT NULL,
    deleted_at_utc TEXT NOT NULL,
    restored_at_utc TEXT,
    permanently_deleted_at_utc TEXT
);

CREATE INDEX IF NOT EXISTS idx_trash_entries_deleted
    ON trash_entries(permanently_deleted_at_utc, deleted_at_utc DESC);

CREATE TABLE IF NOT EXISTS agent_audit_log (
    id TEXT PRIMARY KEY NOT NULL,
    operation TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT,
    request_version INTEGER NOT NULL,
    change_summary_json TEXT NOT NULL,
    occurred_at_utc TEXT NOT NULL,
    outcome TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_audit_log_time
    ON agent_audit_log(occurred_at_utc DESC);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL,
    updated_at_utc TEXT NOT NULL
);
