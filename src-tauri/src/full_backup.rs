//! Full backups and verified migration copies. Never delete or overwrite user content.
use crate::{
    backup_archive::{self, Entry, Manifest, Preview},
    copying, database_backup as db,
    error::{CoreError, file_error},
    filesystem, migrations,
    warehouse::{self, WarehouseSession},
};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    pub path: String,
    pub preview: Preview,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub directory: String,
    pub warehouse_id: Uuid,
    pub application_count: i64,
    pub document_count: i64,
    pub includes_recycle_bin: bool,
    pub migration_backup_path: Option<String>,
}

// Unlike an OS temporary directory, retained work is not automatically deleted on Drop.
struct Staging {
    path: PathBuf,
    identity: String,
    _guard: File,
}
impl Staging {
    fn create(parent: &Path) -> Result<Self, CoreError> {
        let path = parent.join(format!(".offertrack-restoring-{}", Uuid::new_v4()));
        fs::create_dir(&path).map_err(file_error)?;
        let identity = copying::directory_identity(&path)?;
        let guard = db::open_guard(&path, true)?;
        Ok(Self {
            path,
            identity,
            _guard: guard,
        })
    }
    fn publish(self, parent: &Path) -> Result<PathBuf, CoreError> {
        let target = parent.join(format!("OfferTrack-restored-{}", Uuid::new_v4()));
        let Self {
            path,
            identity,
            _guard,
        } = self;
        drop(_guard);
        copying::rename_no_replace(&path, &target, &identity)?;
        Ok(target)
    }
}

fn external_path(path: &Path, directory: bool) -> Result<(PathBuf, Vec<File>), CoreError> {
    let canonical = db::checked_parent(path)?;
    let mut guards = Vec::new();
    for ancestor in canonical.ancestors().skip(usize::from(!directory)) {
        guards.push(db::open_guard(ancestor, true)?);
    }
    if directory && !canonical.is_dir() {
        return Err(CoreError::FileTypeMismatch);
    }
    Ok((canonical, guards))
}

pub(crate) fn outside_parent(
    path: &Path,
    active_root: Option<&Path>,
) -> Result<(PathBuf, Vec<File>), CoreError> {
    let (path, guards) = external_path(path, true)?;
    if active_root.is_some_and(|root| path.starts_with(root)) {
        return Err(CoreError::UnsafePath);
    }
    Ok((path, guards))
}

fn excluded_root(name: &str, include_trash: bool) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "offertrack.sqlite"
            | "offertrack.sqlite-wal"
            | "offertrack.sqlite-shm"
            | "offertrack.sqlite-journal"
            | "warehouse.json"
            | ".offertrack.lock"
    ) || (!include_trash && name.eq_ignore_ascii_case("recycle-bin"))
}

fn inventory(root: &Path, include_trash: bool) -> Result<Vec<Entry>, CoreError> {
    let mut entries = Vec::new();
    collect(root, root, include_trash, &mut entries)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn collect(
    root: &Path,
    directory: &Path,
    include_trash: bool,
    entries: &mut Vec<Entry>,
) -> Result<(), CoreError> {
    let _guard = db::guard_chain(root, directory)?;
    for child in fs::read_dir(directory).map_err(file_error)? {
        let child = child.map_err(file_error)?;
        let name = child
            .file_name()
            .to_str()
            .ok_or(CoreError::UnsafePath)?
            .to_owned();
        if directory == root && excluded_root(&name, include_trash) {
            continue;
        }
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| CoreError::UnsafePath)?
            .to_str()
            .ok_or(CoreError::UnsafePath)?
            .replace('\\', "/");
        if !backup_archive::valid_path(&relative) || entries.len() >= 99_999 {
            return Err(CoreError::UnsafePath);
        }
        let metadata = fs::symlink_metadata(&path).map_err(file_error)?;
        if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
            return Err(CoreError::UnsafePath);
        }
        if metadata.is_dir() {
            entries.push(Entry {
                path: relative,
                directory: true,
                size_bytes: 0,
                sha256: String::new(),
            });
            collect(root, &path, include_trash, entries)?;
        } else {
            let mut file = db::open_guard(&path, false)?;
            let (size_bytes, sha256) = db::hash(&mut file)?;
            entries.push(Entry {
                path: relative,
                directory: false,
                size_bytes,
                sha256,
            });
        }
    }
    Ok(())
}

pub(crate) fn new_output(path: &Path, publishable: bool) -> Result<File, CoreError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
        };
        options
            .access_mode(
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | if publishable { DELETE } else { 0 },
            )
            .share_mode(if publishable { FILE_SHARE_READ } else { 0 });
    }
    #[cfg(not(windows))]
    let _ = publishable;
    options.open(path).map_err(file_error)
}

pub fn create(
    session: &WarehouseSession,
    parent: &Path,
    include_trash: bool,
) -> Result<Created, CoreError> {
    create_with_reason(session, parent, include_trash, "manual")
}

fn create_with_reason(
    session: &WarehouseSession,
    parent: &Path,
    include_trash: bool,
    reason: &str,
) -> Result<Created, CoreError> {
    if !session.is_writable() {
        return Err(CoreError::ReadOnlyWarehouse);
    }
    let (parent, _parent_guards) = outside_parent(parent, Some(session.root()))?;
    db::inspect_records(session.connection())?;
    let snapshot = db::create_at(
        session.connection(),
        session.root(),
        session.summary().warehouse_id,
        reason,
    )?;
    let snapshot_path = session
        .root()
        .join("backups/database")
        .join(snapshot.id.to_string())
        .join("database.sqlite");
    let before = inventory(session.root(), include_trash)?;
    let mut entries = before.clone();
    entries.push(Entry {
        path: "offertrack.sqlite".into(),
        directory: false,
        size_bytes: snapshot.size_bytes,
        sha256: snapshot.sha256,
    });
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = Manifest {
        version: 1,
        kind: "full".into(),
        warehouse_format: warehouse::WAREHOUSE_FORMAT_VERSION,
        warehouse_id: session.summary().warehouse_id,
        schema_version: snapshot.schema_version,
        created_at_utc: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        includes_recycle_bin: include_trash,
        entries,
    };
    backup_archive::validate(&manifest)?;
    let id = Uuid::new_v4();
    let staging = parent.join(format!(".offertrack-backup-{id}.pending"));
    let target = parent.join(format!("OfferTrack-{id}.offertrack-backup"));
    let mut output = new_output(&staging, true)?;
    backup_archive::write_header(&mut output, &manifest)?;
    for entry in &manifest.entries {
        if entry.directory {
            continue;
        }
        let path = if entry.path == "offertrack.sqlite" {
            snapshot_path.clone()
        } else {
            session.root().join(&entry.path)
        };
        let _guards = db::guard_chain(session.root(), path.parent().ok_or(CoreError::UnsafePath)?)?;
        let mut input = db::open_guard(&path, false)?;
        backup_archive::transfer(&mut input, &mut output, entry)?;
        if input.metadata().map_err(file_error)?.len() != entry.size_bytes {
            return Err(CoreError::BackupInvalid);
        }
    }
    output
        .flush()
        .and_then(|_| output.sync_all())
        .map_err(file_error)?;
    if inventory(session.root(), include_trash)? != before {
        return Err(CoreError::CopyVerification);
    }
    let verified = backup_archive::verify(&mut output)?;
    copying::rename_handle_no_replace(&output, &target)?;
    Ok(Created {
        path: target.to_string_lossy().into_owned(),
        preview: verified.preview,
    })
}

pub fn preview(path: &Path) -> Result<Preview, CoreError> {
    let (path, _guards) = external_path(path, false)?;
    let mut file = db::open_guard(&path, false)?;
    Ok(backup_archive::verify(&mut file)?.preview)
}

pub fn restore(
    path: &Path,
    parent: &Path,
    expected_sha256: &str,
    active_root: Option<&Path>,
) -> Result<Restored, CoreError> {
    let (path, _source_guards) = external_path(path, false)?;
    let mut file = db::open_guard(&path, false)?;
    let verified = backup_archive::verify(&mut file)?;
    if verified.preview.sha256 != expected_sha256 {
        return Err(CoreError::RevisionConflict);
    }
    let (parent, _parent_guards) = outside_parent(parent, active_root)?;
    let staging = Staging::create(&parent)?;
    extract(&mut file, &verified, &staging.path)?;
    let database_path = staging.path.join("offertrack.sqlite");
    let connection = db::read_database(&database_path)?;
    if db::check_database(&connection)? != verified.manifest.schema_version {
        return Err(CoreError::BackupInvalid);
    }
    let (application_count, document_count) = db::inspect_records(&connection)?;
    drop(connection);
    let mut connection =
        rusqlite::Connection::open(&database_path).map_err(|_| CoreError::BackupInvalid)?;
    migrations::migrate(&mut connection)?;
    db::check_database(&connection)?;
    drop(connection);
    // Keep the warehouse identity so included historical database backups remain usable.
    warehouse::finish_restored_layout(&staging.path, verified.manifest.warehouse_id, true)?;
    let target = staging.publish(&parent)?;
    Ok(Restored {
        directory: target.to_string_lossy().into_owned(),
        warehouse_id: verified.manifest.warehouse_id,
        application_count,
        document_count,
        includes_recycle_bin: verified.manifest.includes_recycle_bin,
        migration_backup_path: None,
    })
}

fn extract(
    file: &mut File,
    verified: &backup_archive::Verified,
    root: &Path,
) -> Result<(), CoreError> {
    file.seek(SeekFrom::Start(verified.payload_offset))
        .map_err(file_error)?;
    let mut directories: Vec<_> = verified
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.directory)
        .collect();
    directories.sort_by_key(|entry| entry.path.split('/').count());
    for entry in directories {
        let path = root.join(&entry.path);
        let _guards = db::guard_chain(root, path.parent().ok_or(CoreError::UnsafePath)?)?;
        fs::create_dir(&path).map_err(file_error)?;
    }
    for entry in &verified.manifest.entries {
        if entry.directory {
            continue;
        }
        let path = root.join(&entry.path);
        let _guards = db::guard_chain(root, path.parent().ok_or(CoreError::UnsafePath)?)?;
        let mut output = new_output(&path, false)?;
        backup_archive::transfer(file, &mut output, entry)?;
        output
            .flush()
            .and_then(|_| output.sync_all())
            .map_err(file_error)?;
        if db::hash(&mut output)? != (entry.size_bytes, entry.sha256.clone()) {
            return Err(CoreError::BackupInvalid);
        }
    }
    Ok(())
}

pub fn migrate(session: &WarehouseSession, parent: &Path) -> Result<Restored, CoreError> {
    let backup = create_with_reason(session, parent, true, "beforeMigration")?;
    let mut result = restore(
        Path::new(&backup.path),
        parent,
        &backup.preview.sha256,
        Some(session.root()),
    )?;
    result.migration_backup_path = Some(backup.path);
    Ok(result)
}

#[cfg(test)]
#[path = "full_backup_tests.rs"]
mod tests;
