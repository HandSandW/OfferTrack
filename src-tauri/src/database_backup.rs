//! Versioned, database-only snapshots. Never delete source or backup files.
use crate::{
    copying,
    error::{CoreError, file_error},
    filesystem, migrations,
    warehouse::{self, WarehouseSession},
};
use chrono::{Local, SecondsFormat, Utc};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

const VERSION: u32 = 1;
const DATABASE: &str = "database.sqlite";
const MANIFEST: &str = "manifest.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatabaseBackup {
    pub version: u32,
    pub kind: String,
    pub id: Uuid,
    pub warehouse_id: Uuid,
    pub schema_version: i64,
    pub created_at_utc: String,
    pub local_date: String,
    pub reason: String,
    pub size_bytes: u64,
    pub sha256: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupItem {
    #[serde(flatten)]
    pub backup: DatabaseBackup,
    pub recycled: bool,
}
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupCatalog {
    pub items: Vec<BackupItem>,
    pub incomplete_count: usize,
    pub invalid_count: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPreview {
    pub backup: DatabaseBackup,
    pub application_count: i64,
    pub document_count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCreated {
    pub backup: DatabaseBackup,
    pub retention_warning: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseRestore {
    pub directory: String,
    pub application_count: i64,
    pub document_count: i64,
}

pub fn catalog(session: &WarehouseSession) -> Result<BackupCatalog, CoreError> {
    catalog_at(session.root(), session.summary().warehouse_id)
}

fn catalog_at(root: &Path, warehouse_id: Uuid) -> Result<BackupCatalog, CoreError> {
    let mut result = BackupCatalog::default();
    for (base, recycled) in [("backups/database", false), ("recycle-bin/backups", true)] {
        let directory = root.join(base);
        let _guards = guard_chain(root, &directory)?;
        for entry in fs::read_dir(&directory).map_err(file_error)? {
            let entry = entry.map_err(file_error)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".pending-") {
                result.incomplete_count += 1;
                continue;
            }
            let Ok(id) = parse_id(&name) else {
                result.invalid_count += 1;
                continue;
            };
            match read_manifest(root, &entry.path(), id, warehouse_id) {
                Ok(backup) => result.items.push(BackupItem { backup, recycled }),
                Err(_) => result.invalid_count += 1,
            }
        }
    }
    result
        .items
        .sort_by(|a, b| b.backup.created_at_utc.cmp(&a.backup.created_at_utc));
    Ok(result)
}

pub fn create(session: &WarehouseSession) -> Result<BackupCreated, CoreError> {
    if !session.is_writable() {
        return Err(CoreError::ReadOnlyWarehouse);
    }
    let backup = create_at(
        session.connection(),
        session.root(),
        session.summary().warehouse_id,
        "manual",
    )?;
    let retention_warning = rotate(session).is_err();
    Ok(BackupCreated {
        backup,
        retention_warning,
    })
}

pub fn ensure_daily(session: &mut WarehouseSession) -> Result<(), CoreError> {
    if !session.is_writable() {
        return Ok(());
    }
    let today = Local::now().format("%Y-%m-%d").to_string();
    if session.last_backup_date.as_deref() == Some(&today) {
        return Ok(());
    }
    let existing = catalog(session)?.items.into_iter().any(|item| {
        !item.recycled
            && item.backup.reason == "daily"
            && item.backup.local_date == today
            && verified(session, &item.backup.id.to_string(), false).is_ok()
    });
    if !existing {
        create_at(
            session.connection(),
            session.root(),
            session.summary().warehouse_id,
            "daily",
        )?;
    }
    // A retention failure must not turn a successfully published snapshot into a reported failure.
    let _ = rotate(session);
    session.last_backup_date = Some(today);
    Ok(())
}

pub(crate) fn before_upgrade(
    connection: &Connection,
    root: &Path,
    warehouse_id: Uuid,
) -> Result<(), CoreError> {
    let version = schema_version(connection)?;
    if version > migrations::CURRENT_SCHEMA_VERSION {
        return Err(CoreError::UnsupportedFormat);
    }
    if version < migrations::CURRENT_SCHEMA_VERSION {
        create_at(connection, root, warehouse_id, "beforeUpgrade")?;
    }
    Ok(())
}

pub(crate) fn create_at(
    connection: &Connection,
    root: &Path,
    warehouse_id: Uuid,
    reason: &str,
) -> Result<DatabaseBackup, CoreError> {
    let base = root.join("backups/database");
    let _guards = guard_chain(root, &base)?;
    let id = Uuid::new_v4();
    let staging = base.join(format!(".pending-{id}"));
    fs::create_dir(&staging).map_err(file_error)?;
    let identity = copying::directory_identity(&staging)?;
    let guard = open_guard(&staging, true)?;
    // The pending directory is a durable operation record. Failures leave it intact.
    let database = staging.join(DATABASE);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&database)
        .map_err(file_error)?
        .sync_all()
        .map_err(file_error)?;
    let mut destination = Connection::open(&database).map_err(|_| CoreError::BackupInvalid)?;
    {
        let backup =
            Backup::new(connection, &mut destination).map_err(|_| CoreError::BackupInvalid)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match backup.step(128).map_err(|_| CoreError::BackupInvalid)? {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => return Err(CoreError::BackupInvalid),
            }
            if Instant::now() > deadline {
                return Err(CoreError::OperationBusy);
            }
        }
    }
    destination
        .execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
        .map_err(|_| CoreError::BackupInvalid)?;
    let version = check_database(&destination)?;
    drop(destination);
    OpenOptions::new()
        .write(true)
        .open(&database)
        .map_err(file_error)?
        .sync_all()
        .map_err(file_error)?;
    let mut file = open_guard(&database, false)?;
    let (size_bytes, sha256) = hash(&mut file)?;
    let backup = DatabaseBackup {
        version: VERSION,
        kind: "database".into(),
        id,
        warehouse_id,
        schema_version: version,
        created_at_utc: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        local_date: Local::now().format("%Y-%m-%d").to_string(),
        reason: reason.into(),
        size_bytes,
        sha256,
    };
    write_new(
        &staging.join(MANIFEST),
        &serde_json::to_vec_pretty(&backup).map_err(|_| CoreError::BackupInvalid)?,
    )?;
    drop(file);
    drop(guard);
    copying::rename_no_replace(&staging, &base.join(id.to_string()), &identity)?;
    Ok(backup)
}

pub fn preview(
    session: &WarehouseSession,
    id: &str,
    recycled: bool,
) -> Result<BackupPreview, CoreError> {
    let source = verified(session, id, recycled)?;
    preview_verified(&source)
}

struct Verified {
    backup: DatabaseBackup,
    database: PathBuf,
    _guards: Vec<File>,
}

fn verified(session: &WarehouseSession, id: &str, recycled: bool) -> Result<Verified, CoreError> {
    let id = parse_id(id)?;
    let base = if recycled {
        "recycle-bin/backups"
    } else {
        "backups/database"
    };
    let directory = session.root().join(base).join(id.to_string());
    verified_directory(
        session.root(),
        &directory,
        Some((id, session.summary().warehouse_id)),
    )
}

fn verified_directory(
    root: &Path,
    directory: &Path,
    expected: Option<(Uuid, Uuid)>,
) -> Result<Verified, CoreError> {
    let mut guards = guard_chain(root, directory)?;
    // Snapshots are self-contained: never let SQLite read an unverified WAL or journal.
    for entry in fs::read_dir(directory).map_err(file_error)? {
        let name = entry.map_err(file_error)?.file_name();
        if name != MANIFEST && name != DATABASE {
            return Err(CoreError::BackupInvalid);
        }
    }
    // Keep the manifest frozen along with the database through verification/copy.
    guards.push(open_guard(&directory.join(MANIFEST), false)?);
    let backup = read_manifest_unbound(root, directory)?;
    if expected.is_some_and(|(id, warehouse)| backup.id != id || backup.warehouse_id != warehouse) {
        return Err(CoreError::BackupInvalid);
    }
    let database = directory.join(DATABASE);
    let mut file = open_guard(&database, false)?;
    if hash(&mut file)? != (backup.size_bytes, backup.sha256.clone()) {
        return Err(CoreError::BackupInvalid);
    }
    guards.push(file);
    let connection = read_database(&database)?;
    if check_database(&connection)? != backup.schema_version {
        return Err(CoreError::BackupInvalid);
    }
    drop(connection);
    Ok(Verified {
        backup,
        database,
        _guards: guards,
    })
}

fn preview_verified(source: &Verified) -> Result<BackupPreview, CoreError> {
    let connection = read_database(&source.database)?;
    let (application_count, document_count) = inspect_records(&connection)?;
    Ok(BackupPreview {
        backup: source.backup.clone(),
        application_count,
        document_count,
    })
}

pub(crate) fn inspect_records(connection: &Connection) -> Result<(i64, i64), CoreError> {
    for (table, condition) in [
        ("file_operations", "completed_at_utc IS NULL"),
        ("record_creations", "state IN ('copying','verified')"),
    ] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .map_err(|_| CoreError::BackupInvalid)?;
        if exists
            && connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {condition}"),
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|_| CoreError::BackupInvalid)?
                > 0
        {
            return Err(CoreError::BackupPendingOperations);
        }
    }
    let count = |table: &str| {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|_| CoreError::BackupInvalid)
    };
    Ok((count("applications")?, count("documents")?))
}

pub fn restore(
    session: &WarehouseSession,
    id: &str,
    recycled: bool,
    expected_sha256: &str,
    parent: &Path,
) -> Result<DatabaseRestore, CoreError> {
    let source = verified(session, id, recycled)?;
    if source.backup.sha256 != expected_sha256 {
        return Err(CoreError::RevisionConflict);
    }
    restore_verified(&source, parent, Some(session.root()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalPreview {
    #[serde(flatten)]
    pub preview: BackupPreview,
    pub fingerprint: String,
}

/// Import the existing two-file format, not an arbitrary SQLite database.
fn external_source(directory: &Path) -> Result<Verified, CoreError> {
    let directory = checked_parent(directory)?;
    let mut ancestors = Vec::new();
    for ancestor in directory.ancestors() {
        ancestors.push(open_guard(ancestor, true)?);
    }
    let mut source = verified_directory(&directory, &directory, None)?;
    source._guards.extend(ancestors);
    Ok(source)
}

fn fingerprint(source: &Verified) -> Result<String, CoreError> {
    let bytes = serde_json::to_vec(&source.backup).map_err(|_| CoreError::BackupInvalid)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn preview_external(directory: &Path) -> Result<ExternalPreview, CoreError> {
    let source = external_source(directory)?;
    Ok(ExternalPreview {
        preview: preview_verified(&source)?,
        fingerprint: fingerprint(&source)?,
    })
}

pub fn restore_external(
    directory: &Path,
    parent: &Path,
    expected: &str,
    active_root: Option<&Path>,
) -> Result<DatabaseRestore, CoreError> {
    let source = external_source(directory)?;
    if fingerprint(&source)? != expected {
        return Err(CoreError::RevisionConflict);
    }
    restore_verified(&source, parent, active_root)
}

fn restore_verified(
    source: &Verified,
    parent: &Path,
    active_root: Option<&Path>,
) -> Result<DatabaseRestore, CoreError> {
    let preview = preview_verified(source)?;
    // Only create a fresh generated child. Never write into the selected parent itself or the source warehouse.
    let parent = checked_parent(parent)?;
    if active_root.is_some_and(|root| parent.starts_with(root))
        || parent.starts_with(source.database.parent().ok_or(CoreError::UnsafePath)?)
    {
        return Err(CoreError::UnsafePath);
    }
    let guard = open_guard(&parent, true)?;
    let restore_id = Uuid::new_v4();
    let staging = parent.join(format!(".offertrack-restoring-{restore_id}"));
    let target = parent.join(format!("OfferTrack-restored-{restore_id}"));
    fs::create_dir(&staging).map_err(file_error)?;
    let identity = copying::directory_identity(&staging)?;
    let staging_guard = open_guard(&staging, true)?;
    let restored_db = staging.join("offertrack.sqlite");
    let mut input = open_guard(&source.database, false)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&restored_db)
        .map_err(file_error)?;
    std::io::copy(&mut input, &mut output).map_err(file_error)?;
    output.sync_all().map_err(file_error)?;
    drop(output);
    let mut copied = open_guard(&restored_db, false)?;
    if hash(&mut copied)? != (source.backup.size_bytes, source.backup.sha256.clone()) {
        return Err(CoreError::BackupInvalid);
    }
    drop(copied);
    let mut database = Connection::open(&restored_db).map_err(|_| CoreError::BackupInvalid)?;
    migrations::migrate(&mut database)?;
    check_database(&database)?;
    drop(database);
    warehouse::prepare_restored_layout(&staging)?;
    drop(staging_guard);
    copying::rename_no_replace(&staging, &target, &identity)?;
    drop(guard);
    Ok(DatabaseRestore {
        directory: target.to_string_lossy().into_owned(),
        application_count: preview.application_count,
        document_count: preview.document_count,
    })
}

fn rotate(session: &WarehouseSession) -> Result<(), CoreError> {
    let daily: Vec<_> = catalog(session)?
        .items
        .into_iter()
        .filter(|item| !item.recycled && item.backup.reason == "daily")
        .collect();
    let mut keep = BTreeSet::new();
    let mut days = BTreeSet::new();
    let mut months = BTreeSet::new();
    for item in &daily {
        let date = &item.backup.local_date;
        if days.len() < 30 && days.insert(date.clone()) {
            keep.insert(item.backup.id);
        }
        if months.len() < 12 && months.insert(date[..7].to_owned()) {
            keep.insert(item.backup.id);
        }
    }
    for item in daily {
        if keep.contains(&item.backup.id) {
            continue;
        }
        let source = session
            .root()
            .join("backups/database")
            .join(item.backup.id.to_string());
        let target = session
            .root()
            .join("recycle-bin/backups")
            .join(item.backup.id.to_string());
        let _source_guards = guard_chain(
            session.root(),
            source.parent().ok_or(CoreError::UnsafePath)?,
        )?;
        let _target_guards = guard_chain(
            session.root(),
            target.parent().ok_or(CoreError::UnsafePath)?,
        )?;
        let checked = verified(session, &item.backup.id.to_string(), false)?;
        let identity = copying::directory_identity(&source)?;
        drop(checked);
        // Atomic relocation is the only authoritative state change; no database row to desynchronize.
        copying::rename_no_replace(&source, &target, &identity)?;
    }
    Ok(())
}

fn parse_id(value: &str) -> Result<Uuid, CoreError> {
    let id = Uuid::parse_str(value).map_err(|_| CoreError::UnsafePath)?;
    if id.to_string() != value {
        return Err(CoreError::UnsafePath);
    }
    Ok(id)
}

fn read_manifest(
    root: &Path,
    directory: &Path,
    id: Uuid,
    warehouse_id: Uuid,
) -> Result<DatabaseBackup, CoreError> {
    let backup = read_manifest_unbound(root, directory)?;
    if backup.id != id || backup.warehouse_id != warehouse_id {
        return Err(CoreError::BackupInvalid);
    }
    Ok(backup)
}

fn read_manifest_unbound(root: &Path, directory: &Path) -> Result<DatabaseBackup, CoreError> {
    let _guards = guard_chain(root, directory)?;
    let mut file = open_guard(&directory.join(MANIFEST), false)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(16_385)
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    if bytes.len() > 16_384 {
        return Err(CoreError::BackupInvalid);
    }
    let backup: DatabaseBackup =
        serde_json::from_slice(&bytes).map_err(|_| CoreError::BackupInvalid)?;
    if backup.version != VERSION
        || backup.kind != "database"
        || !(1..=migrations::CURRENT_SCHEMA_VERSION).contains(&backup.schema_version)
        || chrono::DateTime::parse_from_rfc3339(&backup.created_at_utc).is_err()
        || chrono::NaiveDate::parse_from_str(&backup.local_date, "%Y-%m-%d").is_err()
        || backup.local_date.len() != 10
        || !backup.local_date.is_ascii()
        || backup.sha256.len() != 64
        || !backup.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || !matches!(
            backup.reason.as_str(),
            "manual"
                | "daily"
                | "beforeUpgrade"
                | "beforeMigration"
                | "beforeBatch"
                | "beforeAgentWrite"
        )
    {
        return Err(CoreError::BackupInvalid);
    }
    Ok(backup)
}

pub(crate) fn read_database(path: &Path) -> Result<Connection, CoreError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| CoreError::BackupInvalid)?;
    connection
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(|_| CoreError::BackupInvalid)?;
    Ok(connection)
}

fn schema_version(connection: &Connection) -> Result<i64, CoreError> {
    connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .map_err(|_| CoreError::BackupInvalid)
}
pub(crate) fn check_database(connection: &Connection) -> Result<i64, CoreError> {
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| CoreError::BackupInvalid)?;
    if integrity != "ok" {
        return Err(CoreError::BackupInvalid);
    }
    let foreign_keys: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|_| CoreError::BackupInvalid)?;
    let version = schema_version(connection)?;
    if foreign_keys != 0 || !(1..=migrations::CURRENT_SCHEMA_VERSION).contains(&version) {
        return Err(CoreError::BackupInvalid);
    }
    Ok(version)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(file_error)?;
    file.write_all(bytes).map_err(file_error)?;
    file.sync_all().map_err(file_error)
}
pub(crate) fn hash(file: &mut File) -> Result<(u64, String), CoreError> {
    file.seek(SeekFrom::Start(0)).map_err(file_error)?;
    let mut digest = Sha256::new();
    let mut size = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(file_error)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((size, format!("{:x}", digest.finalize())))
}
pub(crate) fn open_guard(path: &Path, directory: bool) -> Result<File, CoreError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | if directory { FILE_SHARE_WRITE } else { 0 })
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(file_error)?;
    let metadata = file.metadata().map_err(file_error)?;
    if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err(CoreError::FileTypeMismatch);
    }
    Ok(file)
}
pub(crate) fn guard_chain(root: &Path, directory: &Path) -> Result<Vec<File>, CoreError> {
    filesystem::validate_no_reparse(root, directory)?;
    let mut guards = vec![open_guard(root, true)?];
    let mut current = root.to_owned();
    for component in directory
        .strip_prefix(root)
        .map_err(|_| CoreError::UnsafePath)?
        .components()
    {
        current.push(component);
        guards.push(open_guard(&current, true)?);
    }
    Ok(guards)
}
pub(crate) fn checked_parent(path: &Path) -> Result<PathBuf, CoreError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(CoreError::UnsafePath);
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(file_error)?;
        if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
            return Err(CoreError::UnsafePath);
        }
    }
    fs::canonicalize(path).map_err(file_error)
}

#[cfg(test)]
#[path = "database_backup_tests.rs"]
mod tests;
