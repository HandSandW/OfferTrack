ALTER TABLE tasks ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0);
ALTER TABLE tasks ADD COLUMN remind_at_utc TEXT;
ALTER TABLE reminder_rules ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0);

CREATE TABLE reminder_actions (
    reminder_key TEXT PRIMARY KEY NOT NULL,
    fingerprint TEXT NOT NULL,
    until_utc TEXT,
    updated_at_utc TEXT NOT NULL
);

INSERT INTO reminder_rules (id, stable_key, display_name, threshold_json, created_at_utc, updated_at_utc)
VALUES
 ('missing_resume', 'missing_resume', '创建后尚无简历', '{"value":3}', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('preparing_idle', 'preparing_idle', '准备投递且没有更新', '{"value":7}', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('stage_idle', 'stage_idle', '进行中没有状态变化', '{"value":7}', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('result_idle', 'result_idle', '待结果长期未变化', '{"value":10}', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('due_soon', 'due_soon', '即将到期', '{"value":3}', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('due_urgent', 'due_urgent', '临近截止或面试', '{"value":24}', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('overdue', 'overdue', '已经逾期', '{"value":0}', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'));
