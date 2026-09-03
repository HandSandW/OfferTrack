//! Crash-safe publication of new record directories. Existing source data is
//! never modified. Incomplete work is cancelled, not replayed from stale data.
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chrono::{SecondsFormat, Utc};
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{error::CoreError, filesystem, recycle_bin, warehouse::WarehouseSession};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FileDigest {
    directory: bool,
    size: u64,
    sha256: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    entries: BTreeMap<String, FileDigest>,
}

pub(crate) struct PendingCreation {
    id: String,
    root: PathBuf,
    pub(crate) target_relative: String,
}

impl PendingCreation {
    pub(crate) fn begin(
        session: &mut WarehouseSession,
        id: &str,
        target_relative: &str,
        now: &str,
    ) -> Result<Self, CoreError> {
        session.connection_mut()?;
        recover(session)?;
        Uuid::parse_str(id).map_err(|_| CoreError::Validation)?;
        let operation = Self {
            id: id.to_owned(),
            root: session.root().to_owned(),
            target_relative: target_relative.to_owned(),
        };
        let target = operation.target()?;
        let staging = operation.staging()?;
        if target.try_exists().map_err(|_| CoreError::FileOperation)?
            || staging.try_exists().map_err(|_| CoreError::FileOperation)?
        {
            return Err(CoreError::FileOperation);
        }
        // FULL synchronous SQLite persists the intent before the first mkdir.
        session
            .connection_mut()?
            .execute(
                "INSERT INTO record_creations
             (application_id, target_relative_path, state, created_at_utc)
             VALUES (?1, ?2, 'copying', ?3)",
                params![id, target_relative, now],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let _parents = lock_ancestors(
            &operation.root,
            staging.parent().ok_or(CoreError::UnsafePath)?,
        )?;
        fs::create_dir(&staging).map_err(|_| CoreError::FileOperation)?;
        let identity = directory_identity(&staging)?;
        session
            .connection_mut()?
            .execute(
                "UPDATE record_creations SET directory_identity = ?1 WHERE application_id = ?2",
                params![identity, id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        Ok(operation)
    }

    fn staging(&self) -> Result<PathBuf, CoreError> {
        Uuid::parse_str(&self.id).map_err(|_| CoreError::UnsafePath)?;
        filesystem::trash_folder(
            &self.root,
            &format!("recycle-bin/records/.copying-{}", self.id),
        )
    }

    fn target(&self) -> Result<PathBuf, CoreError> {
        filesystem::application_folder(&self.root, &self.target_relative)
    }

    pub(crate) fn copy_and_verify(
        &self,
        session: &mut WarehouseSession,
        source: Option<&Path>,
    ) -> Result<(), CoreError> {
        let staging = self.staging()?;
        let _parents = lock_ancestors(&self.root, staging.parent().ok_or(CoreError::UnsafePath)?)?;
        let expected = if let Some(source) = source {
            let _source_parents =
                lock_ancestors(&self.root, source.parent().ok_or(CoreError::UnsafePath)?)?;
            let expected = manifest(source)?;
            copy_contents(source, &staging)?;
            if manifest(source)? != expected {
                return Err(CoreError::CopyVerification);
            }
            expected
        } else {
            Manifest {
                version: 1,
                entries: BTreeMap::new(),
            }
        };
        verify(&staging, &expected)?;
        let json = serde_json::to_string(&expected).map_err(|_| CoreError::DatabaseInvalid)?;
        session
            .connection_mut()?
            .execute(
                "UPDATE record_creations SET state = 'verified', manifest_json = ?1
             WHERE application_id = ?2 AND state = 'copying'",
                params![json, self.id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        Ok(())
    }

    /// Call inside the same transaction that inserts the new record. A crash
    /// after rename but before commit leaves an uncommitted journal for recovery.
    pub(crate) fn publish(&self, transaction: &Transaction<'_>) -> Result<(), CoreError> {
        let (identity, json): (String, String) = transaction
            .query_row(
                "SELECT directory_identity, manifest_json FROM record_creations
             WHERE application_id = ?1 AND state = 'verified'",
                [&self.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let expected: Manifest =
            serde_json::from_str(&json).map_err(|_| CoreError::DatabaseInvalid)?;
        let staging = self.staging()?;
        let target = self.target()?;
        let _parents =
            lock_move_ancestors(&self.root, staging.parent().ok_or(CoreError::UnsafePath)?)?;
        let _target_parents =
            lock_move_ancestors(&self.root, target.parent().ok_or(CoreError::UnsafePath)?)?;
        if directory_identity(&staging)? != identity {
            return Err(CoreError::CopyRecovery);
        }
        verify(&staging, &expected)?;
        rename_no_replace(&staging, &target, &identity)?;
        transaction
            .execute(
                "UPDATE record_creations SET state = 'completed', completed_at_utc = ?1
             WHERE application_id = ?2",
                params![now(), self.id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        Ok(())
    }

    pub(crate) fn cancel(&self, session: &mut WarehouseSession) -> Result<(), CoreError> {
        recover_one(session, &self.id)
    }
}

pub(crate) fn recover(session: &mut WarehouseSession) -> Result<(), CoreError> {
    if session.connection_mut().is_err() {
        return Ok(());
    }
    let ids = {
        let mut statement = session.connection().prepare(
            "SELECT application_id FROM record_creations WHERE state IN ('copying', 'verified')"
        ).map_err(|_| CoreError::DatabaseInvalid)?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| CoreError::DatabaseInvalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CoreError::DatabaseInvalid)?
    };
    for id in ids {
        recover_one(session, &id).map_err(|_| CoreError::CopyRecovery)?;
    }
    Ok(())
}

fn recover_one(session: &mut WarehouseSession, id: &str) -> Result<(), CoreError> {
    session.connection_mut()?;
    let (target_relative, identity, state): (String, Option<String>, String) = session.connection().query_row(
        "SELECT target_relative_path, directory_identity, state FROM record_creations WHERE application_id = ?1",
        [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    if state == "completed" || state == "cancelled" {
        return Ok(());
    }
    let exists: bool = session
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM applications WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if exists {
        return Err(CoreError::CopyRecovery);
    }
    let operation = PendingCreation {
        id: id.to_owned(),
        root: session.root().to_owned(),
        target_relative,
    };
    let staging = operation.staging()?;
    let target = operation.target()?;
    let _parents = lock_move_ancestors(
        &operation.root,
        staging.parent().ok_or(CoreError::UnsafePath)?,
    )?;
    let _target_parents = lock_move_ancestors(
        &operation.root,
        target.parent().ok_or(CoreError::UnsafePath)?,
    )?;
    if let Some(identity) = identity {
        if staging.try_exists().map_err(|_| CoreError::FileOperation)? {
            if directory_identity(&staging)? != identity {
                return Err(CoreError::CopyRecovery);
            }
            // A conflicting target belongs to somebody else. Never modify it.
        } else if target.try_exists().map_err(|_| CoreError::FileOperation)? {
            if directory_identity(&target)? != identity {
                return Err(CoreError::CopyRecovery);
            }
            rename_no_replace(&target, &staging, &identity)?;
        }
        if staging.try_exists().map_err(|_| CoreError::FileOperation)? {
            recycle_bin::remove_staging_directory(&operation.root, &staging, &identity)?;
        }
    } else if staging.try_exists().map_err(|_| CoreError::FileOperation)? {
        // Crash between mkdir and identity persistence: ownership cannot be
        // proven. Retain the directory and pending intent, fail closed.
        return Err(CoreError::CopyRecovery);
    }
    session.connection_mut()?.execute(
        "UPDATE record_creations SET state = 'cancelled', completed_at_utc = ?1 WHERE application_id = ?2",
        params![now(), id],
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(())
}

fn manifest(root: &Path) -> Result<Manifest, CoreError> {
    let mut entries = BTreeMap::new();
    scan_manifest(root, root, &mut entries)?;
    Ok(Manifest {
        version: 1,
        entries,
    })
}

fn scan_manifest(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<String, FileDigest>,
) -> Result<(), CoreError> {
    let _directory = open_checked(directory)?;
    for entry in fs::read_dir(directory).map_err(|_| CoreError::FileOperation)? {
        let path = entry.map_err(|_| CoreError::FileOperation)?.path();
        let mut file = open_checked(&path)?;
        let metadata = file.metadata().map_err(|_| CoreError::FileOperation)?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| CoreError::UnsafePath)?
            .to_str()
            .ok_or(CoreError::Validation)?
            .replace('\\', "/");
        if metadata.is_dir() {
            entries.insert(
                relative,
                FileDigest {
                    directory: true,
                    size: 0,
                    sha256: String::new(),
                },
            );
            scan_manifest(root, &path, entries)?;
        } else if metadata.is_file() {
            let mut digest = Sha256::new();
            let mut buffer = [0_u8; 65536];
            let mut size = 0_u64;
            loop {
                let count = file
                    .read(&mut buffer)
                    .map_err(|_| CoreError::FileOperation)?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
                size += count as u64;
            }
            entries.insert(
                relative,
                FileDigest {
                    directory: false,
                    size,
                    sha256: format!("{:x}", digest.finalize()),
                },
            );
        } else {
            return Err(CoreError::UnsafePath);
        }
    }
    Ok(())
}

fn verify(path: &Path, expected: &Manifest) -> Result<(), CoreError> {
    if expected.version != 1 || &manifest(path)? != expected {
        Err(CoreError::CopyVerification)
    } else {
        Ok(())
    }
}

fn copy_contents(source: &Path, target: &Path) -> Result<(), CoreError> {
    let _source = open_checked(source)?;
    let _target = open_checked(target)?;
    for entry in fs::read_dir(source).map_err(|_| CoreError::FileOperation)? {
        let entry = entry.map_err(|_| CoreError::FileOperation)?;
        let mut input = open_checked(&entry.path())?;
        let metadata = input.metadata().map_err(|_| CoreError::FileOperation)?;
        let destination = target.join(entry.file_name());
        if metadata.is_dir() {
            fs::create_dir(&destination).map_err(|_| CoreError::FileOperation)?;
            copy_contents(&entry.path(), &destination)?;
        } else if metadata.is_file() {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(0);
            }
            let mut output = options
                .open(destination)
                .map_err(|_| CoreError::FileOperation)?;
            std::io::copy(&mut input, &mut output).map_err(|_| CoreError::FileOperation)?;
            output
                .flush()
                .and_then(|_| output.sync_all())
                .map_err(|_| CoreError::FileOperation)?;
        } else {
            return Err(CoreError::UnsafePath);
        }
    }
    Ok(())
}

fn lock_ancestors(root: &Path, parent: &Path) -> Result<Vec<File>, CoreError> {
    lock_ancestor_chain(root, parent, false)
}

pub(crate) fn lock_move_ancestors(root: &Path, parent: &Path) -> Result<Vec<File>, CoreError> {
    lock_ancestor_chain(root, parent, true)
}

fn lock_ancestor_chain(root: &Path, parent: &Path, moving: bool) -> Result<Vec<File>, CoreError> {
    filesystem::validate_no_reparse(root, parent)?;
    let mut handles = vec![open_checked(root)?];
    let mut current = root.to_owned();
    for component in parent
        .strip_prefix(root)
        .map_err(|_| CoreError::UnsafePath)?
        .components()
    {
        current.push(component);
        handles.push(open_checked_with_sharing(
            &current,
            moving && current == parent,
        )?);
    }
    Ok(handles)
}

fn open_checked(path: &Path) -> Result<File, CoreError> {
    open_checked_with_sharing(path, false)
}

fn open_checked_with_sharing(path: &Path, _moving: bool) -> Result<File, CoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CoreError::FileOperation)?;
    if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | if _moving { FILE_SHARE_WRITE } else { 0 })
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(crate::error::file_error)?;
    let metadata = file.metadata().map_err(|_| CoreError::FileOperation)?;
    if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    Ok(file)
}

#[cfg(windows)]
pub(crate) fn directory_identity(path: &Path) -> Result<String, CoreError> {
    directory_identity_from_handle(&open_checked(path)?)
}

#[cfg(windows)]
pub(crate) fn directory_identity_from_handle(file: &File) -> Result<String, CoreError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let metadata = file.metadata().map_err(|_| CoreError::FileOperation)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || filesystem::is_reparse_point(&metadata)
    {
        return Err(CoreError::UnsafePath);
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: a live owned handle and an appropriately sized writable struct.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(CoreError::FileOperation);
    }
    Ok(format!(
        "{}:{}:{}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    ))
}

#[cfg(not(windows))]
pub(crate) fn directory_identity(_path: &Path) -> Result<String, CoreError> {
    Err(CoreError::FileOperation)
}

#[cfg(windows)]
pub(crate) fn rename_no_replace(
    source: &Path,
    target: &Path,
    identity: &str,
) -> Result<(), CoreError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        FILE_SHARE_READ,
    };
    let handle = fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(source)
        .map_err(|_| CoreError::FileOperation)?;
    let metadata = handle.metadata().map_err(|_| CoreError::FileOperation)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || filesystem::is_reparse_point(&metadata)
    {
        return Err(CoreError::UnsafePath);
    }
    if directory_identity_from_handle(&handle)? != identity {
        return Err(CoreError::CopyRecovery);
    }
    rename_handle_no_replace(&handle, target)
}

/// Publish the exact already-verified object through its live DELETE-capable handle.
#[cfg(windows)]
pub(crate) fn rename_handle_no_replace(handle: &File, target: &Path) -> Result<(), CoreError> {
    use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
    let target_parent =
        open_checked_with_sharing(target.parent().ok_or(CoreError::UnsafePath)?, true)?;
    // Native rename uses the already-open destination directory and one leaf
    // name, never re-resolves an absolute target path. Direct move parents must
    // share writes (required by Windows for a rename), but not deletion; all
    // higher ancestors remain locked against writes and replacement. This is
    // the ordinary access-checked FileRenameInformation class, not a bypass.
    let filename: Vec<u16> = target
        .file_name()
        .ok_or(CoreError::UnsafePath)?
        .encode_wide()
        .collect();
    if filename.contains(&0) || filename.len() > 32767 {
        return Err(CoreError::UnsafePath);
    }
    let name_offset = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    let byte_len = std::mem::size_of::<FILE_RENAME_INFORMATION>() + filename.len() * 2;
    // Use usize storage to give the flexible Windows structure proper alignment.
    let mut buffer = vec![0_usize; byte_len.div_ceil(std::mem::size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let mut io_status = IO_STATUS_BLOCK::default();
    // SAFETY: buffer is aligned and larger than FILE_RENAME_INFORMATION plus the UTF-16
    // name. It stays live through the call; zero flags mean no replacement.
    let result = unsafe {
        (*info).RootDirectory = target_parent.as_raw_handle();
        (*info).FileNameLength = (filename.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            filename.as_ptr(),
            buffer
                .as_mut_ptr()
                .cast::<u8>()
                .add(name_offset)
                .cast::<u16>(),
            filename.len(),
        );
        NtSetInformationFile(
            handle.as_raw_handle(),
            &mut io_status,
            info.cast(),
            byte_len as u32,
            FileRenameInformation,
        )
    };
    if result != 0 {
        Err(CoreError::FileOperation)
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
pub(crate) fn rename_handle_no_replace(_handle: &File, _target: &Path) -> Result<(), CoreError> {
    Err(CoreError::FileOperation)
}

#[cfg(not(windows))]
pub(crate) fn rename_no_replace(
    _source: &Path,
    _target: &Path,
    _identity: &str,
) -> Result<(), CoreError> {
    Err(CoreError::FileOperation)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::warehouse::{self, WarehouseAccessMode};
    use tempfile::tempdir;

    fn begin(session: &mut WarehouseSession) -> PendingCreation {
        let id = Uuid::new_v4().to_string();
        PendingCreation::begin(session, &id, &format!("applications/test-{id}"), &now()).unwrap()
    }

    fn state(session: &WarehouseSession, operation: &PendingCreation) -> String {
        session
            .connection()
            .query_row(
                "SELECT state FROM record_creations WHERE application_id = ?1",
                [&operation.id],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn manifest_detects_same_size_corruption_and_empty_directory_changes() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("resume.pdf"), b"abc").unwrap();
        fs::create_dir(directory.path().join("empty")).unwrap();
        let expected = manifest(directory.path()).unwrap();
        assert_eq!(
            expected.entries["resume.pdf"].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::write(directory.path().join("resume.pdf"), b"abd").unwrap();
        assert!(matches!(
            verify(directory.path(), &expected),
            Err(CoreError::CopyVerification)
        ));
        fs::write(directory.path().join("resume.pdf"), b"abc").unwrap();
        fs::rename(
            directory.path().join("empty"),
            directory.path().join("renamed"),
        )
        .unwrap();
        assert!(matches!(
            verify(directory.path(), &expected),
            Err(CoreError::CopyVerification)
        ));
    }

    #[test]
    fn partial_large_copy_is_cancelled_on_reopen_without_touching_source() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let source = directory.path().join("applications/source");
        fs::create_dir(&source).unwrap();
        let data = vec![42_u8; 2 * 1024 * 1024];
        fs::write(source.join("large.pdf"), &data).unwrap();
        let operation = begin(&mut session);
        // Simulate process loss after only the first block reached the target.
        fs::write(
            operation.staging().unwrap().join("large.pdf"),
            &data[..65536],
        )
        .unwrap();
        drop(session);
        let mut reopened = warehouse::open(directory.path(), WarehouseAccessMode::Write).unwrap();
        assert_eq!(state(&reopened, &operation), "cancelled");
        assert!(!operation.staging().unwrap().exists());
        assert!(!operation.target().unwrap().exists());
        assert_eq!(fs::read(source.join("large.pdf")).unwrap(), data);
        recover(&mut reopened).unwrap();
    }

    #[test]
    fn crash_after_validation_before_publication_cancels_the_copy() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let operation = begin(&mut session);
        operation.copy_and_verify(&mut session, None).unwrap();
        drop(session);
        let reopened = warehouse::open(directory.path(), WarehouseAccessMode::Write).unwrap();
        assert_eq!(state(&reopened, &operation), "cancelled");
        assert!(!operation.staging().unwrap().exists());
        assert!(!operation.target().unwrap().exists());
    }

    #[test]
    fn crash_after_rename_before_commit_moves_back_then_cancels() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let operation = begin(&mut session);
        operation.copy_and_verify(&mut session, None).unwrap();
        {
            let transaction = session.connection_mut().unwrap().transaction().unwrap();
            operation.publish(&transaction).unwrap();
            assert!(operation.target().unwrap().exists());
            // Uncommitted journal completion rolls back when the process exits.
        }
        assert_eq!(state(&session, &operation), "verified");
        drop(session);
        let mut reopened = warehouse::open(directory.path(), WarehouseAccessMode::Write).unwrap();
        assert_eq!(state(&reopened, &operation), "cancelled");
        assert!(!operation.target().unwrap().exists());
        assert!(!operation.staging().unwrap().exists());
        recover(&mut reopened).unwrap();
    }

    #[test]
    fn destination_collision_preserves_unrelated_data() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let operation = begin(&mut session);
        operation.copy_and_verify(&mut session, None).unwrap();
        let target = operation.target().unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), b"unrelated").unwrap();
        {
            let transaction = session.connection_mut().unwrap().transaction().unwrap();
            assert!(matches!(
                operation.publish(&transaction),
                Err(CoreError::FileOperation)
            ));
        }
        operation.cancel(&mut session).unwrap();
        assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"unrelated");
        assert_eq!(state(&session, &operation), "cancelled");
    }

    #[test]
    fn changed_staging_content_is_rejected_before_publication() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let operation = begin(&mut session);
        operation.copy_and_verify(&mut session, None).unwrap();
        fs::write(
            operation.staging().unwrap().join("unexpected.txt"),
            b"changed",
        )
        .unwrap();
        {
            let transaction = session.connection_mut().unwrap().transaction().unwrap();
            assert!(matches!(
                operation.publish(&transaction),
                Err(CoreError::CopyVerification)
            ));
        }
        assert!(!operation.target().unwrap().exists());
        operation.cancel(&mut session).unwrap();
    }

    #[test]
    fn changed_directory_identity_is_not_deleted() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let operation = begin(&mut session);
        let staging = operation.staging().unwrap();
        fs::rename(&staging, directory.path().join("retained-original")).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("keep.txt"), b"unrelated").unwrap();
        assert!(matches!(
            operation.cancel(&mut session),
            Err(CoreError::CopyRecovery)
        ));
        assert_eq!(fs::read(staging.join("keep.txt")).unwrap(), b"unrelated");
        assert_eq!(state(&session, &operation), "copying");
    }

    #[test]
    fn missing_identity_retains_data_and_allows_read_only_open() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let operation = begin(&mut session);
        session
            .connection_mut()
            .unwrap()
            .execute(
                "UPDATE record_creations SET directory_identity = NULL WHERE application_id = ?1",
                [&operation.id],
            )
            .unwrap();
        drop(session);
        assert!(matches!(
            warehouse::open(directory.path(), WarehouseAccessMode::Write),
            Err(CoreError::CopyRecovery)
        ));
        let read_only = warehouse::open(directory.path(), WarehouseAccessMode::ReadOnly).unwrap();
        assert_eq!(state(&read_only, &operation), "copying");
        assert!(operation.staging().unwrap().exists());
    }

    #[test]
    fn occupied_file_retains_pending_intent_until_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let operation = begin(&mut session);
        let path = operation.staging().unwrap().join("resume.pdf");
        fs::write(&path, b"test").unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(operation.cancel(&mut session).is_err());
        assert_eq!(state(&session, &operation), "copying");
        assert!(path.exists());
        drop(held);
        operation.cancel(&mut session).unwrap();
        assert!(!operation.staging().unwrap().exists());
    }

    #[test]
    fn unsafe_journal_path_cannot_move_or_delete_unrelated_data() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let operation = begin(&mut session);
        let outside = directory.path().join("keep.txt");
        fs::write(&outside, b"safe").unwrap();
        session.connection_mut().unwrap().execute(
            "UPDATE record_creations SET target_relative_path = '../outside' WHERE application_id = ?1", [&operation.id]).unwrap();
        assert!(matches!(
            operation.cancel(&mut session),
            Err(CoreError::UnsafePath)
        ));
        assert_eq!(fs::read(outside).unwrap(), b"safe");
        assert!(operation.staging().unwrap().exists());
    }

    #[test]
    fn source_junction_is_rejected_without_copying_external_content() {
        use std::process::Command;
        let directory = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("keep.txt"), b"external").unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let source = directory.path().join("applications/source");
        fs::create_dir(&source).unwrap();
        let junction = source.join("junction");
        let result = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
            .env("OFFERTRACK_TEST_LINK", &junction).env("OFFERTRACK_TEST_TARGET", outside.path()).output().unwrap();
        assert!(result.status.success(), "junction fixture must succeed");
        let operation = begin(&mut session);
        assert!(matches!(
            operation.copy_and_verify(&mut session, Some(&source)),
            Err(CoreError::UnsafePath)
        ));
        operation.cancel(&mut session).unwrap();
        assert_eq!(
            fs::read(outside.path().join("keep.txt")).unwrap(),
            b"external"
        );
        assert!(!operation.target().unwrap().exists());
    }

    #[test]
    fn process_exit_mid_publish_recovers_sqlite_and_files() {
        const CHILD_ROOT: &str = "OFFERTRACK_COPY_CRASH_TEST_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let mut session =
                warehouse::open(Path::new(&root), WarehouseAccessMode::Write).unwrap();
            let operation = begin(&mut session);
            let source = session.root().join("applications/source");
            operation
                .copy_and_verify(&mut session, Some(&source))
                .unwrap();
            let transaction = session.connection_mut().unwrap().transaction().unwrap();
            transaction.execute("INSERT INTO applications (id, short_id, created_at_utc,
                created_timezone_offset_minutes, folder_relative_path, status_updated_at_utc, updated_at_utc)
                VALUES (?1, 'CRASH1', ?2, 0, ?3, ?2, ?2)",
                params![operation.id, now(), operation.target_relative]).unwrap();
            operation.publish(&transaction).unwrap();
            // Deliberately skip Rust destructors, including Transaction::drop.
            std::process::exit(77);
        }
        let directory = tempdir().unwrap();
        let session = warehouse::create(directory.path()).unwrap();
        let source = session.root().join("applications/source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("resume.pdf"), b"original").unwrap();
        drop(session);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "copying::tests::process_exit_mid_publish_recovers_sqlite_and_files",
            ])
            .env(CHILD_ROOT, directory.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "child must reach the injected crash"
        );
        let reopened = warehouse::open(directory.path(), WarehouseAccessMode::Write).unwrap();
        let count: i64 = reopened
            .connection()
            .query_row("SELECT COUNT(*) FROM applications", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            fs::read_dir(directory.path().join("applications"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            fs::read_dir(directory.path().join("recycle-bin/records"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(fs::read(source.join("resume.pdf")).unwrap(), b"original");
    }
}
