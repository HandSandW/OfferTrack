use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use chrono::{SecondsFormat, Utc};
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::CoreError,
    migrations::{self, CURRENT_SCHEMA_VERSION},
    storage::{StorageLocationInspector, StorageWarning, SystemStorageLocationInspector},
};

pub const WAREHOUSE_FORMAT_VERSION: u32 = 1;
const DESCRIPTOR_FILE: &str = "warehouse.json";
const DATABASE_FILE: &str = "offertrack.sqlite";
const LOCK_FILE: &str = ".offertrack.lock";
const REQUIRED_DIRECTORIES: &[&str] = &[
    "applications",
    "recycle-bin/records",
    "recycle-bin/documents",
    "recycle-bin/backups",
    "backups/database",
    "agent-access",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WarehouseAccessMode {
    Write,
    ReadOnly,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WarehouseDescriptor {
    format_version: u32,
    warehouse_id: Uuid,
    created_at_utc: String,
    capabilities: WarehouseCapabilities,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WarehouseCapabilities {
    database_schema_version: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseSummary {
    pub warehouse_id: Uuid,
    pub format_version: u32,
    pub display_path: String,
    pub access_mode: WarehouseAccessMode,
    pub warnings: Vec<StorageWarning>,
}

pub struct WarehouseSession {
    root: PathBuf,
    descriptor: WarehouseDescriptor,
    access_mode: WarehouseAccessMode,
    warnings: Vec<StorageWarning>,
    _lock: Option<File>,
    _connection: Connection,
}

impl WarehouseSession {
    pub fn summary(&self) -> WarehouseSummary {
        WarehouseSummary {
            warehouse_id: self.descriptor.warehouse_id,
            format_version: self.descriptor.format_version,
            display_path: self.root.to_string_lossy().into_owned(),
            access_mode: self.access_mode,
            warnings: self.warnings.clone(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn connection(&self) -> &Connection {
        &self._connection
    }

    pub fn connection_mut(&mut self) -> Result<&mut Connection, CoreError> {
        if self.access_mode == WarehouseAccessMode::ReadOnly {
            return Err(CoreError::ReadOnlyWarehouse);
        }
        Ok(&mut self._connection)
    }

    pub fn is_writable(&self) -> bool {
        self.access_mode == WarehouseAccessMode::Write
    }
}

pub fn create(path: &Path) -> Result<WarehouseSession, CoreError> {
    fs::create_dir_all(path).map_err(|_| CoreError::Storage)?;
    let root = fs::canonicalize(path).map_err(|_| CoreError::Storage)?;
    let mut entries = fs::read_dir(&root).map_err(|_| CoreError::Storage)?;
    if entries
        .next()
        .transpose()
        .map_err(|_| CoreError::Storage)?
        .is_some()
    {
        return Err(CoreError::WarehouseNotEmpty);
    }

    let lock = acquire_write_lock(&root)?;
    for directory in REQUIRED_DIRECTORIES {
        fs::create_dir_all(root.join(directory)).map_err(|_| CoreError::Storage)?;
    }

    let descriptor = WarehouseDescriptor {
        format_version: WAREHOUSE_FORMAT_VERSION,
        warehouse_id: Uuid::new_v4(),
        created_at_utc: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        capabilities: WarehouseCapabilities {
            database_schema_version: CURRENT_SCHEMA_VERSION,
        },
    };
    write_descriptor(&root, &descriptor)?;

    let mut connection =
        Connection::open(root.join(DATABASE_FILE)).map_err(|_| CoreError::DatabaseInvalid)?;
    configure_writable_database(&connection)?;
    migrations::migrate(&mut connection)?;

    Ok(build_session(
        root,
        descriptor,
        WarehouseAccessMode::Write,
        Some(lock),
        connection,
    ))
}

pub fn open(path: &Path, access_mode: WarehouseAccessMode) -> Result<WarehouseSession, CoreError> {
    let root = fs::canonicalize(path).map_err(|_| CoreError::Storage)?;
    if !root.is_dir() {
        return Err(CoreError::Storage);
    }

    let mut descriptor = read_descriptor(&root)?;
    validate_descriptor(&descriptor)?;
    let lock = match access_mode {
        WarehouseAccessMode::Write => Some(acquire_write_lock(&root)?),
        WarehouseAccessMode::ReadOnly => None,
    };

    let database_path = root.join(DATABASE_FILE);
    if !database_path.is_file() {
        return Err(CoreError::DatabaseMissing);
    }

    let mut connection = match access_mode {
        WarehouseAccessMode::Write => {
            let connection =
                Connection::open(&database_path).map_err(|_| CoreError::DatabaseInvalid)?;
            configure_writable_database(&connection)?;
            connection
        }
        WarehouseAccessMode::ReadOnly => Connection::open_with_flags(
            &database_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| CoreError::DatabaseInvalid)?,
    };

    match access_mode {
        WarehouseAccessMode::Write => {
            migrations::migrate(&mut connection)?;
            if descriptor.capabilities.database_schema_version != CURRENT_SCHEMA_VERSION {
                descriptor.capabilities.database_schema_version = CURRENT_SCHEMA_VERSION;
                write_descriptor(&root, &descriptor)?;
            }
        }
        WarehouseAccessMode::ReadOnly => migrations::validate_schema(&connection)?,
    }

    let mut session = build_session(root, descriptor, access_mode, lock, connection);
    crate::recycle_bin::recover_moves(&mut session)?;
    crate::copying::recover(&mut session)?;
    Ok(session)
}

fn build_session(
    root: PathBuf,
    descriptor: WarehouseDescriptor,
    access_mode: WarehouseAccessMode,
    lock: Option<File>,
    connection: Connection,
) -> WarehouseSession {
    let warnings = SystemStorageLocationInspector.inspect(&root);
    WarehouseSession {
        root,
        descriptor,
        access_mode,
        warnings,
        _lock: lock,
        _connection: connection,
    }
}

fn acquire_write_lock(root: &Path) -> Result<File, CoreError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(LOCK_FILE))
        .map_err(|_| CoreError::Storage)?;
    file.try_lock_exclusive()
        .map_err(|_| CoreError::WarehouseLocked)?;
    Ok(file)
}

fn configure_writable_database(connection: &Connection) -> Result<(), CoreError> {
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;",
        )
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn write_descriptor(root: &Path, descriptor: &WarehouseDescriptor) -> Result<(), CoreError> {
    let bytes = serde_json::to_vec_pretty(descriptor).map_err(|_| CoreError::Storage)?;
    fs::write(root.join(DESCRIPTOR_FILE), bytes).map_err(|_| CoreError::Storage)
}

fn read_descriptor(root: &Path) -> Result<WarehouseDescriptor, CoreError> {
    let path = root.join(DESCRIPTOR_FILE);
    if !path.is_file() {
        return Err(CoreError::MetadataMissing);
    }
    let bytes = fs::read(path).map_err(|_| CoreError::MetadataInvalid)?;
    serde_json::from_slice(&bytes).map_err(|_| CoreError::MetadataInvalid)
}

fn validate_descriptor(descriptor: &WarehouseDescriptor) -> Result<(), CoreError> {
    if descriptor.format_version != WAREHOUSE_FORMAT_VERSION
        || descriptor.capabilities.database_schema_version > CURRENT_SCHEMA_VERSION
    {
        return Err(CoreError::UnsupportedFormat);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_and_reopens_a_versioned_warehouse() {
        let parent = tempdir().expect("create temporary parent");
        let root = parent.path().join("warehouse");

        let first = create(&root).expect("create warehouse");
        let warehouse_id = first.descriptor.warehouse_id;
        assert_eq!(first.descriptor.format_version, WAREHOUSE_FORMAT_VERSION);
        assert!(root.join(DATABASE_FILE).is_file());
        assert!(root.join("applications").is_dir());
        drop(first);

        let reopened = open(&root, WarehouseAccessMode::Write).expect("reopen warehouse");
        assert_eq!(reopened.descriptor.warehouse_id, warehouse_id);
    }

    #[test]
    fn refuses_to_initialize_a_non_empty_directory() {
        let root = tempdir().expect("create temporary directory");
        fs::write(root.path().join("keep.txt"), "user data").expect("write fixture");

        let result = create(root.path());
        assert!(matches!(result, Err(CoreError::WarehouseNotEmpty)));
        assert!(root.path().join("keep.txt").is_file());
    }

    #[test]
    fn detects_a_second_writer_and_allows_read_only_access() {
        let parent = tempdir().expect("create temporary parent");
        let root = parent.path().join("warehouse");
        let writer = create(&root).expect("create warehouse");

        let second_writer = open(&root, WarehouseAccessMode::Write);
        assert!(matches!(second_writer, Err(CoreError::WarehouseLocked)));

        let reader = open(&root, WarehouseAccessMode::ReadOnly)
            .expect("read-only opening remains available");
        assert_eq!(reader.access_mode, WarehouseAccessMode::ReadOnly);
        drop(writer);
    }

    #[test]
    fn rejects_a_folder_without_warehouse_metadata() {
        let root = tempdir().expect("create temporary directory");
        let result = open(root.path(), WarehouseAccessMode::ReadOnly);
        assert!(matches!(result, Err(CoreError::MetadataMissing)));
    }
}
