use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::error::CoreError;

pub const CURRENT_SCHEMA_VERSION: i64 = 1;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_core_schema",
    sql: include_str!("../migrations/0001_initial_core.sql"),
}];

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
mod tests {
    use super::*;

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
        assert_eq!(migration_count, 1);
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
}
