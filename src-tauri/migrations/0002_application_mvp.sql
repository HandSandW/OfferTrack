ALTER TABLE applications ADD COLUMN industry TEXT NOT NULL DEFAULT '';
ALTER TABLE applications ADD COLUMN position_category TEXT NOT NULL DEFAULT '';
ALTER TABLE applications ADD COLUMN work_location TEXT NOT NULL DEFAULT '';
ALTER TABLE applications ADD COLUMN folder_normalization_pending INTEGER NOT NULL DEFAULT 0 CHECK (folder_normalization_pending IN (0, 1));

ALTER TABLE tags ADD COLUMN scope TEXT NOT NULL DEFAULT 'record';

ALTER TABLE interview_rounds ADD COLUMN result TEXT NOT NULL DEFAULT '';

ALTER TABLE documents ADD COLUMN modified_at_utc TEXT;

INSERT OR IGNORE INTO workflow_templates (
    id, name, description, is_default, created_at_utc, updated_at_utc
) VALUES (
    '00000000-0000-0000-0000-000000000001',
    '默认招聘流程',
    'OfferTrack 内置流程；修改模板只影响以后创建的投递。',
    1,
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);

INSERT OR IGNORE INTO workflow_stages (
    id, application_id, template_id, stable_key, display_name, stage_kind,
    display_order, color, is_terminal, terminal_outcome, created_at_utc, updated_at_utc
) VALUES
    ('00000000-0000-0000-0000-000000000101', NULL, '00000000-0000-0000-0000-000000000001', 'preparing', '准备投递', 'application', 10, '#64748b', 0, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('00000000-0000-0000-0000-000000000102', NULL, '00000000-0000-0000-0000-000000000001', 'applied', '已投递', 'application', 20, '#2563eb', 0, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('00000000-0000-0000-0000-000000000103', NULL, '00000000-0000-0000-0000-000000000001', 'assessment', '在线测评', 'assessment', 30, '#7c3aed', 0, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('00000000-0000-0000-0000-000000000104', NULL, '00000000-0000-0000-0000-000000000001', 'written_exam', '现场/远程笔试', 'written_exam', 40, '#9333ea', 0, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('00000000-0000-0000-0000-000000000105', NULL, '00000000-0000-0000-0000-000000000001', 'interview', '面试考核', 'interview', 50, '#db2777', 0, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('00000000-0000-0000-0000-000000000106', NULL, '00000000-0000-0000-0000-000000000001', 'interview_passed', '面试通过', 'interview', 60, '#ea580c', 0, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('00000000-0000-0000-0000-000000000107', NULL, '00000000-0000-0000-0000-000000000001', 'signing', '待签约', 'signing', 70, '#ca8a04', 0, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('00000000-0000-0000-0000-000000000108', NULL, '00000000-0000-0000-0000-000000000001', 'offer', 'offer✅️', 'terminal', 80, '#16a34a', 1, 'offer', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('00000000-0000-0000-0000-000000000109', NULL, '00000000-0000-0000-0000-000000000001', 'failed_terminal', '已挂', 'terminal', 90, '#dc2626', 1, 'failed', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

INSERT OR IGNORE INTO settings (key, value_json, updated_at_utc)
VALUES ('applications.page_size', '50', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
