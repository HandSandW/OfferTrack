//! Read-only observations. Never recover, rename, create, or remove anything.
use crate::{
    error::{CoreError, file_error},
    filesystem,
    warehouse::WarehouseSession,
};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PathState {
    Available,
    Missing,
    WrongType,
    Busy,
    AccessDenied,
    Unsafe,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathObservation {
    pub relative_path: Option<String>,
    pub state: PathState,
}
fn state_for_error(error: CoreError) -> PathState {
    match error {
        CoreError::FileMissing => PathState::Missing,
        CoreError::FileBusy => PathState::Busy,
        CoreError::FileAccessDenied => PathState::AccessDenied,
        CoreError::FileTypeMismatch => PathState::WrongType,
        CoreError::UnsafePath => PathState::Unsafe,
        _ => PathState::Unavailable,
    }
}
fn observe(root: &Path, relative: &str, trash: bool) -> PathObservation {
    let resolved = if trash {
        filesystem::trash_folder(root, relative)
    } else {
        filesystem::application_folder(root, relative)
    };
    let path = match resolved {
        Ok(path) => path,
        Err(error) => {
            return PathObservation {
                relative_path: None,
                state: state_for_error(error),
            };
        }
    };
    // This probes only the directory, not file contents, locks, or identity.
    let state = match fs::symlink_metadata(&path) {
        Ok(metadata)
            if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) =>
        {
            PathState::Unsafe
        }
        Ok(metadata) if !metadata.is_dir() => PathState::WrongType,
        Ok(_) => match fs::read_dir(&path) {
            Ok(_) => PathState::Available,
            Err(e) => state_for_error(file_error(e)),
        },
        Err(e) => state_for_error(file_error(e)),
    };
    PathObservation {
        relative_path: Some(relative.into()),
        state,
    }
}

pub(crate) fn observe_file_path(
    root: &Path,
    path: Result<std::path::PathBuf, CoreError>,
) -> PathObservation {
    let path = match path {
        Ok(path) => path,
        Err(error) => {
            return PathObservation {
                relative_path: None,
                state: state_for_error(error),
            };
        }
    };
    let state = match fs::symlink_metadata(&path) {
        Ok(metadata)
            if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) =>
        {
            PathState::Unsafe
        }
        Ok(metadata) if metadata.is_file() => PathState::Available,
        Ok(_) => PathState::WrongType,
        Err(error) => state_for_error(file_error(error)),
    };
    PathObservation {
        relative_path: path
            .strip_prefix(root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/")),
        state,
    }
}

pub fn inspect_application(
    session: &WarehouseSession,
    id: &str,
) -> Result<PathObservation, CoreError> {
    let relative: String = session.connection().query_row(
        "SELECT folder_relative_path FROM applications WHERE id = ?1 AND deleted_at_utc IS NULL",
        [id], |r| r.get(0)).optional().map_err(|_| CoreError::DatabaseInvalid)?.ok_or(CoreError::NotFound)?;
    Ok(observe(session.root(), &relative, false))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingFileOperation {
    pub id: String,
    pub kind: String,
    pub source: PathObservation,
    pub target: PathObservation,
    pub identity_recorded: Option<bool>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryDiagnostics {
    pub version: u32,
    pub total_pending: i64,
    pub items: Vec<PendingFileOperation>,
}
pub fn recovery_diagnostics(session: &WarehouseSession) -> Result<RecoveryDiagnostics, CoreError> {
    let connection = session.connection();
    let total_pending = connection
        .query_row(
            "SELECT
        (SELECT COUNT(*) FROM record_creations WHERE state IN ('copying', 'verified')) +
        (SELECT COUNT(*) FROM file_operations WHERE completed_at_utc IS NULL) +
        (SELECT COUNT(*) FROM document_renames WHERE completed_at_utc IS NULL) +
        (SELECT COUNT(*) FROM document_moves WHERE completed_at_utc IS NULL) +
        (SELECT COUNT(*) FROM document_purges WHERE completed_at_utc IS NULL)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let mut items = Vec::new();
    let mut statement = connection.prepare("SELECT application_id, target_relative_path, directory_identity IS NOT NULL
        FROM record_creations WHERE state IN ('copying', 'verified') ORDER BY created_at_utc, application_id LIMIT 100")
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for row in rows {
        let (id, target, identity) = row.map_err(|_| CoreError::DatabaseInvalid)?;
        let source = if uuid::Uuid::parse_str(&id).is_ok() {
            observe(
                session.root(),
                &format!("recycle-bin/records/.copying-{id}"),
                true,
            )
        } else {
            PathObservation {
                relative_path: None,
                state: PathState::Unsafe,
            }
        };
        items.push(PendingFileOperation {
            id,
            kind: "creation".into(),
            source,
            target: observe(session.root(), &target, false),
            identity_recorded: Some(identity),
        });
    }
    let mut statement = connection
        .prepare(
            "SELECT id,trash_id,kind,folder_relative_path,document_relative_path
         FROM document_moves WHERE completed_at_utc IS NULL ORDER BY created_at_utc,id LIMIT 100",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for row in rows {
        let (id, trash_id, kind, folder, relative) = row.map_err(|_| CoreError::DatabaseInvalid)?;
        let live = observe_document(session.root(), &folder, &relative);
        let recycled = observe_file_path(
            session.root(),
            crate::document_trash::trash_path(session.root(), &trash_id),
        );
        let (source, target) = if kind == "trash" {
            (live, recycled)
        } else {
            (recycled, live)
        };
        items.push(PendingFileOperation {
            id,
            kind: if kind == "trash" {
                "documentTrash"
            } else {
                "documentRestore"
            }
            .into(),
            source,
            target,
            identity_recorded: Some(true),
        });
    }
    let mut statement=connection.prepare("SELECT id,trash_id FROM document_purges WHERE completed_at_utc IS NULL ORDER BY created_at_utc,id LIMIT 100").map_err(|_|CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for row in rows {
        let (id, trash_id) = row.map_err(|_| CoreError::DatabaseInvalid)?;
        let source = observe_file_path(
            session.root(),
            crate::document_trash::trash_path(session.root(), &trash_id),
        );
        items.push(PendingFileOperation {
            id,
            kind: "documentPurge".into(),
            source,
            target: PathObservation {
                relative_path: None,
                state: PathState::Unavailable,
            },
            identity_recorded: Some(true),
        });
    }
    let mut statement = connection
        .prepare(
            "SELECT id, operation_kind, source_relative_path, target_relative_path
        FROM file_operations WHERE completed_at_utc IS NULL ORDER BY created_at_utc, id LIMIT 100",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for row in rows {
        let (id, kind, source, target) = row.map_err(|_| CoreError::DatabaseInvalid)?;
        items.push(PendingFileOperation {
            id,
            source: observe(session.root(), &source, kind == "restore"),
            target: observe(session.root(), &target, kind == "trash"),
            kind,
            identity_recorded: None,
        });
    }
    let mut statement = connection.prepare(
        "SELECT id, folder_relative_path, source_relative_path, target_relative_path
         FROM document_renames WHERE completed_at_utc IS NULL ORDER BY created_at_utc, id LIMIT 100"
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for row in rows {
        let (id, folder, source, target) = row.map_err(|_| CoreError::DatabaseInvalid)?;
        items.push(PendingFileOperation {
            id,
            kind: "documentRename".into(),
            source: observe_document(session.root(), &folder, &source),
            target: observe_document(session.root(), &folder, &target),
            identity_recorded: Some(true),
        });
    }
    Ok(RecoveryDiagnostics {
        version: 1,
        total_pending,
        items,
    })
}

fn observe_document(root: &Path, folder: &str, relative: &str) -> PathObservation {
    let path = filesystem::application_folder(root, folder)
        .and_then(|folder| crate::document_files::checked_target_path(root, &folder, relative));
    let path = match path {
        Ok(path) => path,
        Err(error) => {
            return PathObservation {
                relative_path: None,
                state: state_for_error(error),
            };
        }
    };
    let state = match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) =>
        {
            PathState::Unsafe
        }
        Ok(metadata) if metadata.is_file() => PathState::Available,
        Ok(_) => PathState::WrongType,
        Err(error) => state_for_error(file_error(error)),
    };
    PathObservation {
        relative_path: Some(format!("{folder}/{relative}")),
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        applications,
        domain::CreateApplicationRequest,
        warehouse::{self, WarehouseAccessMode},
    };
    use tempfile::tempdir;
    fn request() -> CreateApplicationRequest {
        CreateApplicationRequest {
            company_name: "测试".into(),
            position_name: "研发".into(),
            company_type: "private".into(),
            industry: String::new(),
            position_category: String::new(),
            work_location: String::new(),
        }
    }
    #[test]
    fn missing_folder_is_reported_without_recreation_and_reappearance_restores_same_index_ids() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let folder = dir.path().join(&record.record.folder_relative_path);
        fs::write(folder.join("resume.pdf"), b"fixture").unwrap();
        let original = applications::scan_documents(&mut session, &record.record.id).unwrap();
        fs::rename(&folder, dir.path().join("temporarily-moved")).unwrap();
        assert_eq!(
            inspect_application(&session, &record.record.id)
                .unwrap()
                .state,
            PathState::Missing
        );
        let missing = applications::scan_documents(&mut session, &record.record.id).unwrap();
        assert!(missing[0].missing);
        assert_eq!(missing[0].id, original[0].id);
        assert!(!folder.exists());
        fs::rename(dir.path().join("temporarily-moved"), &folder).unwrap();
        let recovered = applications::scan_documents(&mut session, &record.record.id).unwrap();
        assert!(!recovered[0].missing);
        assert_eq!(recovered[0].id, original[0].id);
        assert_eq!(fs::read(folder.join("resume.pdf")).unwrap(), b"fixture");
    }
    #[test]
    fn wrong_type_scan_failure_preserves_index_and_does_not_overwrite_replacement() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let folder = dir.path().join(&record.record.folder_relative_path);
        fs::write(folder.join("resume.pdf"), b"fixture").unwrap();
        applications::scan_documents(&mut session, &record.record.id).unwrap();
        fs::rename(&folder, dir.path().join("original")).unwrap();
        fs::write(&folder, b"unrelated replacement").unwrap();
        assert!(matches!(
            applications::scan_documents(&mut session, &record.record.id),
            Err(CoreError::FileTypeMismatch)
        ));
        assert!(
            !applications::get(&session, &record.record.id)
                .unwrap()
                .documents[0]
                .missing
        );
        assert_eq!(
            inspect_application(&session, &record.record.id)
                .unwrap()
                .state,
            PathState::WrongType
        );
        assert_eq!(fs::read(folder).unwrap(), b"unrelated replacement");
    }
    #[test]
    fn recovery_diagnostics_are_read_only_and_redact_unsafe_paths() {
        let dir = tempdir().unwrap();
        let session = warehouse::create(dir.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let staging = dir
            .path()
            .join(format!("recycle-bin/records/.copying-{id}"));
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("keep.txt"), b"keep").unwrap();
        session.connection().execute("INSERT INTO record_creations (application_id, target_relative_path, state, created_at_utc) VALUES (?1, 'applications/../outside', 'copying', 'now')", [&id]).unwrap();
        session.connection().execute_batch("INSERT INTO file_operations (id, operation_kind, application_id, trash_id, source_relative_path, target_relative_path, created_at_utc) VALUES ('pending', 'normalize', 'record', '', 'applications/a', 'applications/b', 'now')").unwrap();
        drop(session);
        let readonly = warehouse::open(dir.path(), WarehouseAccessMode::ReadOnly).unwrap();
        let before = readonly.connection().total_changes();
        for _ in 0..2 {
            let report = recovery_diagnostics(&readonly).unwrap();
            assert_eq!(report.total_pending, 2);
            assert_eq!(report.version, 1);
            assert_eq!(report.items[0].identity_recorded, Some(false));
            assert_eq!(report.items[0].source.state, PathState::Available);
            assert_eq!(report.items[0].target.state, PathState::Unsafe);
            assert!(report.items[0].target.relative_path.is_none());
            let json = serde_json::to_string(&report).unwrap();
            assert!(!json.contains("../outside"));
            assert!(!json.contains("manifest"));
        }
        assert_eq!(readonly.connection().total_changes(), before);
        assert_eq!(fs::read(staging.join("keep.txt")).unwrap(), b"keep");
    }

    #[test]
    fn diagnostics_limit_rows_but_report_the_total_pending_count() {
        let dir = tempdir().unwrap();
        let session = warehouse::create(dir.path()).unwrap();
        session.connection().execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 101)
            INSERT INTO file_operations (id, operation_kind, application_id, trash_id, source_relative_path, target_relative_path, created_at_utc)
            SELECT 'pending-' || x, 'normalize', 'record', '', 'applications/a', 'applications/b', 'now' FROM n;").unwrap();
        let report = recovery_diagnostics(&session).unwrap();
        assert_eq!(report.total_pending, 101);
        assert_eq!(report.items.len(), 100);
    }

    #[cfg(windows)]
    #[test]
    fn occupied_source_copy_reports_busy_and_retains_original_without_partial_record() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let file = dir
            .path()
            .join(&record.record.folder_relative_path)
            .join("resume.pdf");
        fs::write(&file, b"source fixture").unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&file)
            .unwrap();
        assert!(matches!(
            applications::duplicate(
                &mut session,
                &record.record.id,
                crate::domain::DuplicateMode::FullRecord
            ),
            Err(CoreError::FileBusy)
        ));
        drop(lock);
        assert_eq!(fs::read(file).unwrap(), b"source fixture");
        assert_eq!(
            applications::list(&session, crate::domain::ApplicationScope::Active)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(recovery_diagnostics(&session).unwrap().total_pending, 0);
    }

    #[cfg(windows)]
    #[test]
    fn occupied_directory_does_not_mark_index_missing_and_normalization_can_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let folder = dir.path().join(&record.record.folder_relative_path);
        fs::write(folder.join("resume.pdf"), b"fixture").unwrap();
        applications::scan_documents(&mut session, &record.record.id).unwrap();
        let exclusive = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&folder)
            .unwrap();
        assert!(applications::scan_documents(&mut session, &record.record.id).is_err());
        assert!(
            !applications::get(&session, &record.record.id)
                .unwrap()
                .documents[0]
                .missing
        );
        assert_eq!(
            inspect_application(&session, &record.record.id)
                .unwrap()
                .state,
            PathState::Busy
        );
        drop(exclusive);
        let arbitrary = dir.path().join("applications/manual");
        fs::rename(&folder, &arbitrary).unwrap();
        session.connection().execute("UPDATE applications SET folder_relative_path = 'applications/manual', folder_normalization_pending = 1 WHERE id = ?1", [&record.record.id]).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&arbitrary)
            .unwrap();
        let pending =
            applications::retry_folder_normalization(&mut session, &record.record.id).unwrap();
        assert!(pending.record.folder_normalization_pending);
        assert_eq!(pending.record.folder_relative_path, "applications/manual");
        drop(lock);
        let normalized =
            applications::retry_folder_normalization(&mut session, &record.record.id).unwrap();
        assert!(!normalized.record.folder_normalization_pending);
        assert_eq!(
            normalized.record.folder_relative_path,
            record.record.folder_relative_path
        );
        assert_eq!(fs::read(folder.join("resume.pdf")).unwrap(), b"fixture");
    }

    #[test]
    fn missing_document_paths_are_distinct_from_unknown_ids_and_recheck_reappeared_files() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let folder = dir.path().join(&record.record.folder_relative_path);
        let file = folder.join("resume.pdf");
        fs::write(&file, b"fixture").unwrap();
        let docs = applications::scan_documents(&mut session, &record.record.id).unwrap();
        fs::rename(&file, folder.join("moved.pdf")).unwrap();
        applications::scan_documents(&mut session, &record.record.id).unwrap();
        assert!(matches!(
            crate::platform::document_path(
                session.connection(),
                session.root(),
                &record.record.id,
                &docs[0].id
            ),
            Err(CoreError::FileMissing)
        ));
        assert!(matches!(
            crate::platform::document_path(
                session.connection(),
                session.root(),
                &record.record.id,
                "unknown"
            ),
            Err(CoreError::NotFound)
        ));
        fs::rename(folder.join("moved.pdf"), &file).unwrap();
        assert!(
            crate::platform::document_path(
                session.connection(),
                session.root(),
                &record.record.id,
                &docs[0].id
            )
            .is_ok()
        );
        // Pure path resolution must not write back the previously missing index.
        assert!(
            applications::get(&session, &record.record.id)
                .unwrap()
                .documents
                .iter()
                .find(|d| d.id == docs[0].id)
                .unwrap()
                .missing
        );
    }

    #[cfg(windows)]
    #[test]
    fn nested_junction_is_rejected_without_following_or_modifying_existing_index() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let folder = dir.path().join(&record.record.folder_relative_path);
        fs::write(folder.join("resume.pdf"), b"fixture").unwrap();
        fs::write(outside.path().join("private.txt"), b"outside").unwrap();
        applications::scan_documents(&mut session, &record.record.id).unwrap();
        let link = folder.join("junction");
        let result = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
            .env("OFFERTRACK_TEST_LINK", &link).env("OFFERTRACK_TEST_TARGET", outside.path()).output().unwrap();
        assert!(result.status.success(), "junction fixture must succeed");
        assert!(matches!(
            applications::scan_documents(&mut session, &record.record.id),
            Err(CoreError::UnsafePath)
        ));
        let docs = applications::get(&session, &record.record.id)
            .unwrap()
            .documents;
        assert_eq!(docs.len(), 1);
        assert!(!docs[0].missing);
        assert_eq!(
            fs::read(outside.path().join("private.txt")).unwrap(),
            b"outside"
        );
        fs::remove_dir(&link).unwrap(); // Remove only the synthetic junction, never its target.
    }
}
