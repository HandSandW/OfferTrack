CREATE TABLE workflow_states (
    id TEXT PRIMARY KEY NOT NULL,
    application_id TEXT REFERENCES applications(id) ON DELETE CASCADE,
    template_id TEXT REFERENCES workflow_templates(id) ON DELETE CASCADE,
    stable_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    semantic_kind TEXT NOT NULL CHECK (semantic_kind IN
        ('pending', 'awaitingParticipation', 'awaitingCompletion', 'awaitingResult', 'completed', 'failed')),
    display_order INTEGER NOT NULL,
    CHECK ((application_id IS NOT NULL AND template_id IS NULL)
        OR (application_id IS NULL AND template_id IS NOT NULL)),
    CHECK (semantic_kind != 'failed' OR stable_key = 'failed'),
    UNIQUE (application_id, stable_key),
    UNIQUE (template_id, stable_key)
);

-- Immutable seed values, not a shared editable palette.
CREATE VIEW builtin_workflow_states (stable_key, display_name, display_order) AS
    VALUES ('pending', '尚未开始', 10), ('awaitingParticipation', '待参加', 20),
           ('awaitingCompletion', '待完成', 30), ('awaitingResult', '待结果', 40),
           ('completed', '已完成', 50), ('failed', '未通过', 60);

INSERT INTO workflow_states
    SELECT lower(hex(randomblob(16))), a.id, NULL, b.stable_key, b.display_name, b.stable_key, b.display_order
    FROM applications a CROSS JOIN builtin_workflow_states b;
INSERT INTO workflow_states
    SELECT lower(hex(randomblob(16))), NULL, t.id, b.stable_key, b.display_name, b.stable_key, b.display_order
    FROM workflow_templates t CROSS JOIN builtin_workflow_states b;

CREATE TRIGGER seed_application_states AFTER INSERT ON applications BEGIN
    INSERT INTO workflow_states
        SELECT lower(hex(randomblob(16))), NEW.id, NULL, stable_key, display_name, stable_key, display_order
        FROM builtin_workflow_states;
END;
CREATE TRIGGER seed_template_states AFTER INSERT ON workflow_templates BEGIN
    INSERT INTO workflow_states
        SELECT lower(hex(randomblob(16))), NULL, NEW.id, stable_key, display_name, stable_key, display_order
        FROM builtin_workflow_states;
END;

ALTER TABLE workflow_events ADD COLUMN previous_state_name_snapshot TEXT;
ALTER TABLE workflow_events ADD COLUMN next_state_name_snapshot TEXT;
ALTER TABLE workflow_events ADD COLUMN previous_state_kind_snapshot TEXT;
ALTER TABLE workflow_events ADD COLUMN next_state_kind_snapshot TEXT;

-- Old versions only exposed built-ins. Unknown externally-written values stay
-- as their raw codes rather than inventing a historical name or classification.
UPDATE workflow_events SET
    previous_state_name_snapshot = COALESCE((SELECT display_name FROM builtin_workflow_states WHERE stable_key = previous_state), previous_state),
    next_state_name_snapshot = COALESCE((SELECT display_name FROM builtin_workflow_states WHERE stable_key = next_state), next_state),
    previous_state_kind_snapshot = (SELECT stable_key FROM builtin_workflow_states WHERE stable_key = previous_state),
    next_state_kind_snapshot = (SELECT stable_key FROM builtin_workflow_states WHERE stable_key = next_state);

CREATE TRIGGER snapshot_workflow_state AFTER INSERT ON workflow_events BEGIN
    UPDATE workflow_events SET
        previous_state_name_snapshot = COALESCE((SELECT display_name FROM workflow_states WHERE application_id = NEW.application_id AND stable_key = NEW.previous_state), NEW.previous_state),
        next_state_name_snapshot = COALESCE((SELECT display_name FROM workflow_states WHERE application_id = NEW.application_id AND stable_key = NEW.next_state), NEW.next_state),
        previous_state_kind_snapshot = (SELECT semantic_kind FROM workflow_states WHERE application_id = NEW.application_id AND stable_key = NEW.previous_state),
        next_state_kind_snapshot = (SELECT semantic_kind FROM workflow_states WHERE application_id = NEW.application_id AND stable_key = NEW.next_state)
    WHERE id = NEW.id;
END;
