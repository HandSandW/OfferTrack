//! Shared live reader for CLI and MCP. No writer lock, scan, migration or recovery.
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

use crate::{
    database_backup,
    error::CoreError,
    filesystem, full_backup,
    warehouse::{self, WarehouseAccessMode, WarehouseSession},
};

pub(crate) struct Reader {
    // Drop the SQLite connection before releasing the protection handles.
    pub session: WarehouseSession,
    _guards: Vec<File>,
}

impl Reader {
    pub(crate) fn acquire_writer(&mut self) -> Result<(), CoreError> {
        // Desktop warehouses already have this file. Do not create a new lock
        // under an unverified/replaced entry; keep it non-replaceable until close.
        self._guards.push(database_guard(
            self.session.root(),
            &self.session.root().join(".offertrack.lock"),
        )?);
        self.session.acquire_agent_writer()
    }
}

/// Permit committed WAL changes but deny replacement while a query is running.
/// Never use immutable=1: that would hide the desktop writer's committed WAL.
fn database_guard(root: &Path, path: &Path) -> Result<File, CoreError> {
    filesystem::validate_no_reparse(root, path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(crate::error::file_error)?;
    let meta = file.metadata().map_err(crate::error::file_error)?;
    if filesystem::is_reparse_point(&meta) || meta.file_type().is_symlink() {
        return Err(CoreError::UnsafePath);
    }
    if !meta.is_file() {
        return Err(CoreError::FileTypeMismatch);
    }
    Ok(file)
}

pub(crate) fn open(path: &Path) -> Result<Reader, CoreError> {
    let root = super::checked_root(path)?;
    let (_, mut guards) = full_backup::outside_parent(&root, None)?;
    guards.push(database_backup::open_guard(
        &root.join("warehouse.json"),
        false,
    )?);
    guards.push(database_guard(&root, &root.join("offertrack.sqlite"))?);
    for name in [
        "offertrack.sqlite-wal",
        "offertrack.sqlite-shm",
        "offertrack.sqlite-journal",
    ] {
        match std::fs::symlink_metadata(root.join(name)) {
            Ok(_) => guards.push(database_guard(&root, &root.join(name))?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(crate::error::file_error(e)),
        }
    }
    let session = warehouse::open(&root, WarehouseAccessMode::ReadOnly)?;
    session
        .connection()
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(Reader {
        session,
        _guards: guards,
    })
}
