use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::error::CoreError;

pub const CURRENT_SCHEMA_VERSION: i64 = 9;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    // Published migrations remain immutable; new capabilities append a version.
    Migration {
        version: 1,
        name: "initial_core_schema",
        sql: include_str!("../migrations/0001_initial_core.sql"),
    },
    Migration {
        version: 2,
        name: "application_management_mvp",
        sql: include_str!("../migrations/0002_application_mvp.sql"),
    },
    Migration {
        version: 3,
        name: "file_operation_journal",
        sql: include_str!("../migrations/0003_file_operation_journal.sql"),
    },
    Migration {
        version: 4,
        name: "record_creation_journal",
        sql: include_str!("../migrations/0004_record_creation_journal.sql"),
    },
    Migration {
        version: 5,
        name: "workflow_template_revision",
        sql: include_str!("../migrations/0005_workflow_template_revision.sql"),
    },
    Migration {
        version: 6,
        name: "auxiliary_states",
        sql: include_str!("../migrations/0006_auxiliary_states.sql"),
    },
    Migration {
        version: 7,
        name: "metadata_revisions",
        sql: include_str!("../migrations/0007_metadata_revisions.sql"),
    },
    Migration {
        version: 8,
        name: "tasks_and_reminders",
        sql: include_str!("../migrations/0008_tasks_and_reminders.sql"),
    },
    Migration {
        version: 9,
        name: "recruitment_schedule",
        sql: include_str!("../migrations/0009_recruitment_schedule.sql"),
    },
];

pub fn configure_connection(connection: &Connection) -> Result<(), CoreError> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub fn migrate(connection: &mut Connection) -> Result<(), CoreError> {
    configure_connection(connection)?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at_utc TEXT NOT NULL
            );",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;

    for migration in MIGRATIONS {
        let applied = connection
            .query_row(
                "SELECT version FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| CoreError::DatabaseInvalid)?
            .is_some();

        if !applied {
            apply_migration(connection, migration)?;
        }
    }

    validate_schema(connection)
}

fn apply_migration(connection: &mut Connection, migration: &Migration) -> Result<(), CoreError> {
    let transaction = connection
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .execute_batch(migration.sql)
        .map_err(|_| CoreError::DatabaseInvalid)?;
    record_migration(&transaction, migration)?;
    transaction.commit().map_err(|_| CoreError::DatabaseInvalid)
}

fn record_migration(transaction: &Transaction<'_>, migration: &Migration) -> Result<(), CoreError> {
    transaction
        .execute(
            "INSERT INTO schema_migrations (version, name, applied_at_utc)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            (migration.version, migration.name),
        )
        .map(|_| ())
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub fn validate_schema(connection: &Connection) -> Result<(), CoreError> {
    configure_connection(connection)?;
    let version = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .unwrap_or_default();

    if version == CURRENT_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(CoreError::DatabaseInvalid)
    }
}

#[cfg(test)]
pub(crate) fn fixture_remove_migration_eight(connection: &Connection) {
    // Synthetic fixture only: reconstruct the actual historical schema for upgrade tests.
    fixture_remove_migration_nine(connection);
    connection
        .execute_batch(
            "ALTER TABLE tasks DROP COLUMN revision; ALTER TABLE tasks DROP COLUMN remind_at_utc;
        ALTER TABLE reminder_rules DROP COLUMN revision; DELETE FROM reminder_rules;
        DROP TABLE reminder_actions; DELETE FROM schema_migrations WHERE version=8;",
        )
        .unwrap();
}

#[cfg(test)]
pub(crate) fn fixture_remove_migration_nine(connection: &Connection) {
    connection.execute_batch("DROP TRIGGER event_round_owner_insert; DROP TRIGGER event_round_owner_update;
        DROP INDEX idx_event_interview; ALTER TABLE recruitment_events DROP COLUMN interview_round_id;
        ALTER TABLE recruitment_events DROP COLUMN revision; ALTER TABLE recruitment_events DROP COLUMN deadline_at_utc;
        ALTER TABLE recruitment_events DROP COLUMN location; ALTER TABLE recruitment_events DROP COLUMN meeting_url;
        ALTER TABLE recruitment_events DROP COLUMN result; DELETE FROM schema_migrations WHERE version=9;").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_nine_rolls_back_all_added_columns_when_index_creation_fails() {
        let mut connection = Connection::open_in_memory().unwrap();
        configure_connection(&connection).unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,name TEXT NOT NULL,applied_at_utc TEXT NOT NULL)").unwrap();
        for migration in &MIGRATIONS[..8] {
            apply_migration(&mut connection, migration).unwrap();
        }
        connection
            .execute_batch("CREATE INDEX idx_event_interview ON recruitment_events(title)")
            .unwrap();
        assert!(migrate(&mut connection).is_err());
        assert!(
            connection
                .prepare("SELECT revision FROM recruitment_events")
                .is_err()
        );
        assert!(
            connection
                .prepare("SELECT interview_round_id FROM recruitment_events")
                .is_err()
        );
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 8);
        connection
            .execute_batch("DROP INDEX idx_event_interview")
            .unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        assert!(
            connection
                .prepare("SELECT revision,interview_round_id FROM recruitment_events")
                .is_ok()
        );
    }

    #[test]
    fn migration_eight_preserves_tasks_and_rolls_back_on_rule_conflict() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY,name TEXT NOT NULL,applied_at_utc TEXT NOT NULL)").unwrap();
        for migration in &MIGRATIONS[..7] {
            apply_migration(&mut connection, migration).unwrap();
        }
        connection.execute_batch("INSERT INTO tasks (id,title,notes,completed_at_utc,created_at_utc,updated_at_utc) VALUES ('old','保留待办','保留备注','2026-08-01T00:00:00Z','created','updated');
            INSERT INTO reminder_rules (id,stable_key,display_name,threshold_json,created_at_utc,updated_at_utc) VALUES ('conflict','overdue','外部规则','{}','created','updated');").unwrap();
        assert!(migrate(&mut connection).is_err());
        assert!(connection.prepare("SELECT revision FROM tasks").is_err());
        assert!(
            connection
                .prepare("SELECT * FROM reminder_actions")
                .is_err()
        );
        connection
            .execute("DELETE FROM reminder_rules WHERE id='conflict'", [])
            .unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let task: (String,String,String,i64,Option<String>) = connection.query_row("SELECT title,notes,completed_at_utc,revision,remind_at_utc FROM tasks WHERE id='old'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
        assert_eq!(
            task,
            (
                "保留待办".into(),
                "保留备注".into(),
                "2026-08-01T00:00:00Z".into(),
                1,
                None
            )
        );
        assert_eq!(crate::tasks::rules(&connection).unwrap().len(), 7);
    }

    #[test]
    fn migration_seven_preserves_metadata_and_rolls_back_duplicate_default_failure() {
        let mut connection = Connection::open_in_memory().unwrap();
        configure_connection(&connection).unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_utc TEXT NOT NULL);").unwrap();
        for migration in &MIGRATIONS[..6] {
            apply_migration(&mut connection, migration).unwrap();
        }
        connection.execute_batch("INSERT INTO views (id, name, view_kind, layout_json, sort_json, filter_json, is_default, created_at_utc, updated_at_utc)
            VALUES ('original', '保留视图', 'applications', '{\"columns\":[]}', '[]', '{\"search\":\"原始筛选\",\"companyTypes\":[],\"stages\":[]}', 1, 'created', 'updated'),
                   ('duplicate', '重复默认', 'applications', '{}', '[]', '{}', 1, 'created', 'updated');
            INSERT INTO field_definitions (id, key, display_name, field_type, config_json, display_order, created_at_utc, updated_at_utc)
            VALUES ('field', 'stable-key', '保留字段', 'text', '{\"future\":true}', 42, 'created', 'updated');").unwrap();
        assert!(migrate(&mut connection).is_err());
        assert!(connection.prepare("SELECT revision FROM views").is_err());
        assert!(
            connection
                .prepare("SELECT revision FROM field_definitions")
                .is_err()
        );
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 6);
        connection
            .execute("UPDATE views SET is_default = 0 WHERE id = 'duplicate'", [])
            .unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let view: (String, String, String, i64) = connection.query_row("SELECT name, filter_json, updated_at_utc, revision FROM views WHERE id = 'original'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!(view.0, "保留视图");
        assert!(view.1.contains("原始筛选"));
        assert_eq!((view.2.as_str(), view.3), ("updated", 1));
        let field: (String, String, String, i64, i64) = connection.query_row("SELECT key, display_name, config_json, display_order, revision FROM field_definitions WHERE id = 'field'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap();
        assert_eq!(
            field,
            (
                "stable-key".into(),
                "保留字段".into(),
                "{\"future\":true}".into(),
                42,
                1
            )
        );
        assert!(
            connection
                .execute("UPDATE views SET is_default = 1 WHERE id = 'duplicate'", [])
                .is_err()
        );
    }

    #[test]
    fn initial_migration_is_idempotent() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");

        migrate(&mut connection).expect("first migration succeeds");
        migrate(&mut connection).expect("second migration succeeds");

        let migration_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("count migrations");
        assert_eq!(migration_count, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn migration_creates_every_initial_core_table() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");
        migrate(&mut connection).expect("migration succeeds");

        for table in [
            "applications",
            "field_definitions",
            "field_values",
            "tags",
            "application_tags",
            "workflow_templates",
            "workflow_stages",
            "workflow_states",
            "workflow_events",
            "interview_rounds",
            "documents",
            "tasks",
            "recruitment_events",
            "reminder_rules",
            "views",
            "trash_entries",
            "agent_audit_log",
            "settings",
            "file_operations",
            "record_creations",
        ] {
            let found: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("query sqlite schema");
            assert_eq!(found, 1, "missing core table {table}");
        }
    }

    #[test]
    fn migration_five_preserves_templates_and_rejects_duplicate_defaults_without_partial_changes() {
        let mut connection = Connection::open_in_memory().unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
            version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_utc TEXT NOT NULL);",
            )
            .unwrap();
        for migration in &MIGRATIONS[..4] {
            apply_migration(&mut connection, migration).unwrap();
        }
        connection.execute_batch(
            "UPDATE workflow_templates SET name = '用户自定义名称';
             UPDATE workflow_stages SET display_name = '个性化面试' WHERE stable_key = 'interview';
             INSERT INTO workflow_templates (id, name, is_default, created_at_utc, updated_at_utc)
             VALUES ('second-default', '重复默认', 1, '2026-09-03T00:00:00Z', '2026-09-03T00:00:00Z');"
        ).unwrap();
        // Invalid pre-existing data cannot cause a partial ALTER TABLE commit.
        assert!(migrate(&mut connection).is_err());
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 4);
        assert!(
            connection
                .prepare("SELECT revision FROM workflow_templates")
                .is_err()
        );
        connection
            .execute(
                "UPDATE workflow_templates SET is_default = 0 WHERE id = 'second-default'",
                [],
            )
            .unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let (name, revision): (String, i64) = connection
            .query_row(
                "SELECT name, revision FROM workflow_templates WHERE is_default = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "用户自定义名称");
        assert_eq!(revision, 1);
        let name: String = connection
            .query_row(
                "SELECT display_name FROM workflow_stages WHERE stable_key = 'interview'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "个性化面试");
        assert!(
            connection
                .execute(
                    "UPDATE workflow_templates SET is_default = 1 WHERE id = 'second-default'",
                    []
                )
                .is_err()
        );
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM workflow_templates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn migration_six_backfills_history_and_rolls_back_on_snapshot_failure() {
        let mut connection = Connection::open_in_memory().unwrap();
        configure_connection(&connection).unwrap();
        connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_utc TEXT NOT NULL)").unwrap();
        for migration in &MIGRATIONS[..5] {
            apply_migration(&mut connection, migration).unwrap();
        }
        connection.execute_batch(
            "INSERT INTO applications (id, short_id, created_at_utc, created_timezone_offset_minutes, folder_relative_path, status_updated_at_utc, updated_at_utc)
             VALUES ('legacy-record', 'LEGACY', '2026-09-03T00:00:00Z', 480, 'applications/legacy', '2026-09-03T00:00:00Z', '2026-09-03T00:00:00Z');
             INSERT INTO workflow_events (id, application_id, stage_name_snapshot, previous_state, next_state, occurred_at_utc, actor_type)
             VALUES ('legacy-event', 'legacy-record', '历史面试阶段', 'pending', 'awaitingResult', '2026-09-03T01:00:00Z', 'user'),
                    ('unknown-event', 'legacy-record', '外部状态', NULL, 'legacy-unknown', '2026-09-03T02:00:00Z', 'user');
             CREATE TRIGGER reject_history_update BEFORE UPDATE ON workflow_events BEGIN SELECT RAISE(ABORT, 'injected'); END;"
        ).unwrap();
        assert!(migrate(&mut connection).is_err());
        assert!(connection.prepare("SELECT * FROM workflow_states").is_err());
        assert!(
            connection
                .prepare("SELECT next_state_name_snapshot FROM workflow_events")
                .is_err()
        );
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 5);
        connection
            .execute_batch("DROP TRIGGER reject_history_update")
            .unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let snapshots: (String, String, String, String) = connection.query_row(
            "SELECT previous_state_name_snapshot, next_state_name_snapshot, next_state_kind_snapshot, stage_name_snapshot FROM workflow_events WHERE id = 'legacy-event'", [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!(
            snapshots,
            (
                "尚未开始".into(),
                "待结果".into(),
                "awaitingResult".into(),
                "历史面试阶段".into()
            )
        );
        let unknown: (String, Option<String>) = connection.query_row("SELECT next_state_name_snapshot, next_state_kind_snapshot FROM workflow_events WHERE id = 'unknown-event'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(unknown, ("legacy-unknown".into(), None));
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM workflow_states", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 12);
        let revision: i64 = connection
            .query_row(
                "SELECT revision FROM applications WHERE id = 'legacy-record'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revision, 1);
        let foreign_keys: i64 = connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(foreign_keys, 0);
    }

    #[test]
    fn migration_four_preserves_existing_settings_and_move_journal() {
        let mut connection = Connection::open_in_memory().unwrap();
        configure_connection(&connection).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
            version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at_utc TEXT NOT NULL);",
            )
            .unwrap();
        for migration in &MIGRATIONS[..3] {
            apply_migration(&mut connection, migration).unwrap();
        }
        connection
            .execute(
                "UPDATE settings SET value_json = '100' WHERE key = 'applications.page_size'",
                [],
            )
            .unwrap();
        connection
            .execute_batch(
                "INSERT INTO file_operations (id, operation_kind, application_id,
            trash_id, source_relative_path, target_relative_path, created_at_utc)
            VALUES ('test-move', 'normalize', 'test-record', '', 'applications/source',
                    'applications/target', '2026-09-03T00:00:00Z');",
            )
            .unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let size: String = connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = 'applications.page_size'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(size, "100");
        let count: i64 = connection.query_row("SELECT COUNT(*) FROM file_operations WHERE id = 'test-move' AND completed_at_utc IS NULL", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1);
        validate_schema(&connection).unwrap();
    }
}
