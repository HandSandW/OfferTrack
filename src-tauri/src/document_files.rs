//! Safe, journaled operations for one indexed attachment.
//!
//! Callers identify files by application/document IDs. The only user supplied
//! path fragment is a validated leaf name; relative paths loaded from SQLite
//! are revalidated before any filesystem access.
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    applications, copying,
    error::{CoreError, file_error},
    filesystem,
    warehouse::WarehouseSession,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameDocumentRequest {
    pub application_id: String,
    pub document_id: String,
    pub expected_relative_path: String,
    pub new_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDirectories {
    pub version: u32,
    pub directories: Vec<filesystem::ScannedDirectory>,
}

struct Intent {
    id: String,
    application_id: String,
    document_id: String,
    folder: String,
    source: String,
    target: String,
    identity: String,
    created: String,
}

pub fn list_directories(
    session: &WarehouseSession,
    application_id: &str,
) -> Result<ApplicationDirectories, CoreError> {
    let folder = application_folder_relative(session, application_id)?;
    Ok(ApplicationDirectories {
        version: 1,
        directories: filesystem::scan_application_directories(session.root(), &folder)?,
    })
}

pub fn rename(
    session: &mut WarehouseSession,
    request: RenameDocumentRequest,
) -> Result<crate::domain::ApplicationDetail, CoreError> {
    session.connection_mut()?;
    recover(session)?;
    validate_leaf_name(&request.new_name)?;
    let (folder, current) = session
        .connection()
        .query_row(
            "SELECT a.folder_relative_path, d.relative_path
         FROM documents d JOIN applications a ON a.id = d.application_id
         WHERE a.id = ?1 AND d.id = ?2 AND a.deleted_at_utc IS NULL",
            params![request.application_id, request.document_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    if current != request.expected_relative_path {
        return Err(CoreError::RevisionConflict);
    }
    let parent = safe_document_relative(&current)?
        .parent()
        .unwrap_or(Path::new(""));
    let target_relative = parent
        .join(&request.new_name)
        .to_string_lossy()
        .replace('\\', "/");
    if target_relative == current {
        return applications::get(session, &request.application_id);
    }
    if target_relative.eq_ignore_ascii_case(&current) {
        return Err(CoreError::DocumentNameConflict);
    }
    let conflict: bool = session
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE application_id = ?1
         AND id <> ?2 AND relative_path = ?3 COLLATE NOCASE)",
            params![request.application_id, request.document_id, target_relative],
            |row| row.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if conflict {
        return Err(CoreError::DocumentNameConflict);
    }

    let root = filesystem::application_folder(session.root(), &folder)?;
    let source = checked_file_path(session.root(), &root, &current)?;
    let target = checked_target_path(session.root(), &root, &target_relative)?;
    if target.try_exists().map_err(file_error)? {
        return Err(CoreError::DocumentNameConflict);
    }
    let _source_parents = copying::lock_move_ancestors(
        session.root(),
        source.parent().ok_or(CoreError::UnsafePath)?,
    )?;
    let _target_parents = copying::lock_move_ancestors(
        session.root(),
        target.parent().ok_or(CoreError::UnsafePath)?,
    )?;
    let identity = file_identity(&source)?;
    let intent = Intent {
        id: Uuid::new_v4().to_string(),
        application_id: request.application_id.clone(),
        document_id: request.document_id,
        folder,
        source: current,
        target: target_relative,
        identity,
        created: now(),
    };
    intent.persist(session)?;
    // Hold the exact object until its index and journal commit together.
    let _renamed = match rename_file_no_replace(&source, &target, &intent.identity) {
        Ok(handle) => handle,
        Err(error) => {
            intent.finish(session, false)?;
            return Err(error);
        }
    };
    intent
        .finish(session, true)
        .map_err(|_| CoreError::DocumentRenameRecovery)?;
    applications::get(session, &request.application_id)
}

pub fn recover(session: &mut WarehouseSession) -> Result<(), CoreError> {
    if !session.is_writable() {
        return Ok(());
    }
    let pending = {
        let mut statement = session
            .connection()
            .prepare(
                "SELECT id, application_id, document_id, folder_relative_path,
             source_relative_path, target_relative_path, file_identity, created_at_utc
             FROM document_renames WHERE completed_at_utc IS NULL ORDER BY created_at_utc, id",
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        statement
            .query_map([], |row| {
                Ok(Intent {
                    id: row.get(0)?,
                    application_id: row.get(1)?,
                    document_id: row.get(2)?,
                    folder: row.get(3)?,
                    source: row.get(4)?,
                    target: row.get(5)?,
                    identity: row.get(6)?,
                    created: row.get(7)?,
                })
            })
            .map_err(|_| CoreError::DatabaseInvalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CoreError::DatabaseInvalid)?
    };
    for intent in pending {
        let root = filesystem::application_folder(session.root(), &intent.folder)?;
        let source = checked_target_path(session.root(), &root, &intent.source)?;
        let target = checked_target_path(session.root(), &root, &intent.target)?;
        if source.parent() != target.parent() || source == target {
            return Err(CoreError::DocumentRenameRecovery);
        }
        let _parents = copying::lock_move_ancestors(
            session.root(),
            source.parent().ok_or(CoreError::UnsafePath)?,
        )?;
        let moved = match (
            source.try_exists().map_err(file_error)?,
            target.try_exists().map_err(file_error)?,
        ) {
            (true, false) => false,
            (false, true) => true,
            _ => return Err(CoreError::DocumentRenameRecovery),
        };
        let handle = open_identity_file(if moved { &target } else { &source })?;
        if identity_from_handle(&handle)? != intent.identity {
            return Err(CoreError::DocumentRenameRecovery);
        }
        intent.finish(session, moved)?;
    }
    Ok(())
}

impl Intent {
    fn persist(&self, session: &mut WarehouseSession) -> Result<(), CoreError> {
        session.connection_mut()?.execute(
            "INSERT INTO document_renames (id, version, application_id, document_id,
             folder_relative_path, source_relative_path, target_relative_path, file_identity, created_at_utc)
             VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![self.id, self.application_id, self.document_id, self.folder,
                    self.source, self.target, self.identity, self.created],
        ).map(|_| ()).map_err(|_| CoreError::DatabaseInvalid)
    }
    fn finish(&self, session: &mut WarehouseSession, moved: bool) -> Result<(), CoreError> {
        let transaction = session
            .connection_mut()?
            .transaction()
            .map_err(|_| CoreError::DatabaseInvalid)?;
        if moved {
            let display_name = Path::new(&self.target)
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or(CoreError::UnsafePath)?;
            let changed = transaction
                .execute(
                    "UPDATE documents SET relative_path = ?1, display_name = ?2,
                 missing_at_utc = NULL, last_observed_at_utc = ?3, media_type = ?7
                 WHERE id = ?4 AND application_id = ?5 AND relative_path = ?6",
                    params![
                        self.target,
                        display_name,
                        self.created,
                        self.document_id,
                        self.application_id,
                        self.source,
                        filesystem::media_type_for_path(Path::new(&self.target))
                    ],
                )
                .map_err(|_| CoreError::DatabaseInvalid)?;
            if changed != 1 {
                return Err(CoreError::DocumentRenameRecovery);
            }
            let changed = transaction
                .execute(
                    "UPDATE applications SET updated_at_utc = ?1, revision = revision + 1
                 WHERE id = ?2 AND folder_relative_path = ?3 AND deleted_at_utc IS NULL",
                    params![self.created, self.application_id, self.folder],
                )
                .map_err(|_| CoreError::DatabaseInvalid)?;
            if changed != 1 {
                return Err(CoreError::DocumentRenameRecovery);
            }
        }
        transaction
            .execute(
                "UPDATE document_renames SET completed_at_utc = ?1, outcome = ?2 WHERE id = ?3",
                params![
                    now(),
                    if moved { "completed" } else { "cancelled" },
                    self.id
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        transaction.commit().map_err(|_| CoreError::DatabaseInvalid)
    }
}

pub(crate) fn application_folder_relative(
    session: &WarehouseSession,
    id: &str,
) -> Result<String, CoreError> {
    session.connection().query_row(
        "SELECT folder_relative_path FROM applications WHERE id = ?1 AND deleted_at_utc IS NULL",
        [id], |row| row.get(0)
    ).optional().map_err(|_| CoreError::DatabaseInvalid)?.ok_or(CoreError::NotFound)
}
pub(crate) fn safe_document_relative(value: &str) -> Result<&Path, CoreError> {
    let path = Path::new(value);
    if value.is_empty()
        || value
            .split(['/', '\\'])
            .any(|part| !valid_windows_leaf(part))
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        Err(CoreError::UnsafePath)
    } else {
        Ok(path)
    }
}
fn validate_leaf_name(value: &str) -> Result<(), CoreError> {
    let path = safe_document_relative(value)?;
    if path.components().count() != 1 || value != value.trim() || !valid_windows_leaf(value) {
        Err(CoreError::Validation)
    } else {
        Ok(())
    }
}
fn valid_windows_leaf(value: &str) -> bool {
    !value.is_empty()
        && !value.ends_with(['.', ' '])
        && value.encode_utf16().count() <= 255
        && !filesystem::is_windows_reserved_name(value)
        && !value.chars().any(|c| {
            c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
}
pub(crate) fn checked_target_path(
    root: &Path,
    folder: &Path,
    relative: &str,
) -> Result<PathBuf, CoreError> {
    let path = folder.join(safe_document_relative(relative)?);
    filesystem::validate_no_reparse(root, &path)?;
    Ok(path)
}
fn checked_file_path(root: &Path, folder: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    let path = checked_target_path(root, folder, relative)?;
    let metadata = fs::symlink_metadata(&path).map_err(file_error)?;
    if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    if !metadata.is_file() {
        return Err(CoreError::FileTypeMismatch);
    }
    Ok(path)
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(windows)]
pub(crate) fn file_identity(path: &Path) -> Result<String, CoreError> {
    identity_from_handle(&open_identity_file(path)?)
}
#[cfg(windows)]
pub(crate) fn open_identity_file(path: &Path) -> Result<fs::File, CoreError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_READ,
    };
    let file = fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(file_error)?;
    identity_from_handle(&file)?;
    Ok(file)
}
#[cfg(windows)]
pub(crate) fn identity_from_handle(file: &fs::File) -> Result<String, CoreError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let metadata = file.metadata().map_err(file_error)?;
    if !metadata.is_file() || filesystem::is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the owned handle remains live and info is a correctly sized writable struct.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(CoreError::FileOperation);
    }
    Ok(format!(
        "{}:{}:{}",
        info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
    ))
}
#[cfg(windows)]
pub(crate) fn rename_file_no_replace(
    source: &Path,
    target: &Path,
    identity: &str,
) -> Result<fs::File, CoreError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_READ,
    };
    let handle = fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(source)
        .map_err(file_error)?;
    if identity_from_handle(&handle)? != identity {
        return Err(CoreError::DocumentRenameRecovery);
    }
    copying::rename_handle_no_replace(&handle, target)?;
    Ok(handle)
}
#[cfg(not(windows))]
pub(crate) fn file_identity(_path: &Path) -> Result<String, CoreError> {
    Err(CoreError::FileOperation)
}
#[cfg(not(windows))]
pub(crate) fn open_identity_file(_path: &Path) -> Result<fs::File, CoreError> {
    Err(CoreError::FileOperation)
}
#[cfg(not(windows))]
pub(crate) fn identity_from_handle(_file: &fs::File) -> Result<String, CoreError> {
    Err(CoreError::FileOperation)
}
#[cfg(not(windows))]
pub(crate) fn rename_file_no_replace(
    _source: &Path,
    _target: &Path,
    _identity: &str,
) -> Result<fs::File, CoreError> {
    // First release supports Windows only. Do not substitute a racy check/rename.
    Err(CoreError::FileOperation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{applications, domain::CreateApplicationRequest, warehouse};
    use tempfile::tempdir;
    fn record() -> (
        tempfile::TempDir,
        WarehouseSession,
        crate::domain::ApplicationDetail,
    ) {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let detail = applications::create(
            &mut session,
            CreateApplicationRequest {
                company_name: "测试".into(),
                position_name: "研发".into(),
                company_type: "private".into(),
                industry: String::new(),
                position_category: String::new(),
                work_location: String::new(),
            },
        )
        .unwrap();
        (dir, session, detail)
    }
    #[test]
    fn rename_is_id_scoped_and_never_overwrites() {
        let (dir, mut session, detail) = record();
        let root = dir.path().join(&detail.record.folder_relative_path);
        fs::create_dir(root.join("材料")).unwrap();
        fs::write(root.join("材料/简历.pdf"), b"mine").unwrap();
        fs::write(root.join("材料/占用.pdf"), b"other").unwrap();
        let documents = applications::scan_documents(&mut session, &detail.record.id).unwrap();
        let source = documents
            .iter()
            .find(|d| d.display_name == "简历.pdf")
            .unwrap();
        let conflict = RenameDocumentRequest {
            application_id: detail.record.id.clone(),
            document_id: source.id.clone(),
            expected_relative_path: source.relative_path.clone(),
            new_name: "占用.pdf".into(),
        };
        assert!(matches!(
            rename(&mut session, conflict),
            Err(CoreError::DocumentNameConflict)
        ));
        assert_eq!(fs::read(root.join("材料/简历.pdf")).unwrap(), b"mine");
        assert_eq!(fs::read(root.join("材料/占用.pdf")).unwrap(), b"other");
        let renamed = rename(
            &mut session,
            RenameDocumentRequest {
                application_id: detail.record.id.clone(),
                document_id: source.id.clone(),
                expected_relative_path: source.relative_path.clone(),
                new_name: "正式简历.pdf".into(),
            },
        )
        .unwrap();
        assert_eq!(fs::read(root.join("材料/正式简历.pdf")).unwrap(), b"mine");
        assert_eq!(
            renamed
                .documents
                .iter()
                .find(|d| d.id == source.id)
                .unwrap()
                .relative_path,
            "材料/正式简历.pdf"
        );
    }
    #[test]
    fn directories_include_empty_nested_and_hidden_folders_without_index_writes() {
        let (dir, session, detail) = record();
        let root = dir.path().join(&detail.record.folder_relative_path);
        fs::create_dir_all(root.join("材料/空目录")).unwrap();
        fs::create_dir_all(root.join("有文件")).unwrap();
        fs::write(root.join("有文件/a.txt"), b"a").unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();
        let before = session.connection().total_changes();
        let result = list_directories(&session, &detail.record.id).unwrap();
        assert!(
            result
                .directories
                .iter()
                .any(|d| d.relative_path == "材料/空目录" && d.empty)
        );
        assert!(
            result
                .directories
                .iter()
                .any(|d| d.relative_path == "有文件" && !d.empty)
        );
        assert!(
            result
                .directories
                .iter()
                .any(|d| d.relative_path == ".hidden" && d.empty)
        );
        assert_eq!(session.connection().total_changes(), before);
    }

    fn with_file() -> (
        tempfile::TempDir,
        WarehouseSession,
        crate::domain::ApplicationDetail,
        RenameDocumentRequest,
    ) {
        let (dir, mut session, detail) = record();
        fs::write(
            dir.path()
                .join(&detail.record.folder_relative_path)
                .join("resume.pdf"),
            b"original attachment",
        )
        .unwrap();
        let doc = applications::scan_documents(&mut session, &detail.record.id)
            .unwrap()
            .remove(0);
        let request = RenameDocumentRequest {
            application_id: detail.record.id.clone(),
            document_id: doc.id,
            expected_relative_path: doc.relative_path,
            new_name: "renamed.pdf".into(),
        };
        (dir, session, detail, request)
    }
    fn pending(
        session: &mut WarehouseSession,
        detail: &crate::domain::ApplicationDetail,
        request: &RenameDocumentRequest,
    ) -> Intent {
        let source = session
            .root()
            .join(&detail.record.folder_relative_path)
            .join(&request.expected_relative_path);
        let intent = Intent {
            id: Uuid::new_v4().to_string(),
            application_id: detail.record.id.clone(),
            document_id: request.document_id.clone(),
            folder: detail.record.folder_relative_path.clone(),
            source: request.expected_relative_path.clone(),
            target: request.new_name.clone(),
            identity: file_identity(&source).unwrap(),
            created: now(),
        };
        intent.persist(session).unwrap();
        intent
    }
    #[test]
    fn rejects_path_names_devices_stale_request_and_readonly_before_moving() {
        for name in [
            "",
            "..",
            "../bad.pdf",
            "dir/file.pdf",
            "dir\\file.pdf",
            "C:\\bad.pdf",
            "file:stream",
            "CON.pdf",
            "COM¹.txt",
            "CON .txt",
            "bad.",
            " bad.pdf",
            "bad\0.pdf",
        ] {
            assert!(validate_leaf_name(name).is_err(), "accepted {name:?}");
        }
        assert!(validate_leaf_name(&format!("{}.pdf", "a".repeat(240))).is_ok());
        assert!(validate_leaf_name(&"a".repeat(256)).is_err());
        let (dir, mut session, detail, mut request) = with_file();
        request.expected_relative_path = "old.pdf".into();
        assert!(matches!(
            rename(&mut session, request),
            Err(CoreError::RevisionConflict)
        ));
        drop(session);
        let mut readonly =
            warehouse::open(dir.path(), warehouse::WarehouseAccessMode::ReadOnly).unwrap();
        let doc = applications::get(&readonly, &detail.record.id)
            .unwrap()
            .documents
            .remove(0);
        assert!(matches!(
            rename(
                &mut readonly,
                RenameDocumentRequest {
                    application_id: detail.record.id.clone(),
                    document_id: doc.id,
                    expected_relative_path: doc.relative_path,
                    new_name: "next.pdf".into()
                }
            ),
            Err(CoreError::ReadOnlyWarehouse)
        ));
        assert_eq!(
            fs::read(
                dir.path()
                    .join(&detail.record.folder_relative_path)
                    .join("resume.pdf")
            )
            .unwrap(),
            b"original attachment"
        );
    }
    #[test]
    fn refuses_missing_files_cross_record_ids_and_reserved_missing_index_names() {
        let (dir, mut session, detail, mut request) = with_file();
        let root = dir.path().join(&detail.record.folder_relative_path);
        request.application_id = "another-record".into();
        assert!(matches!(
            rename(&mut session, request),
            Err(CoreError::NotFound)
        ));
        fs::write(root.join("gone.pdf"), b"reserved index").unwrap();
        applications::scan_documents(&mut session, &detail.record.id).unwrap();
        fs::rename(root.join("gone.pdf"), dir.path().join("moved-gone.pdf")).unwrap();
        let docs = applications::scan_documents(&mut session, &detail.record.id).unwrap();
        let doc = docs
            .iter()
            .find(|d| d.relative_path == "resume.pdf")
            .unwrap();
        assert!(matches!(
            rename(
                &mut session,
                RenameDocumentRequest {
                    application_id: detail.record.id.clone(),
                    document_id: doc.id.clone(),
                    expected_relative_path: doc.relative_path.clone(),
                    new_name: "gone.pdf".into()
                }
            ),
            Err(CoreError::DocumentNameConflict)
        ));
        fs::rename(root.join("resume.pdf"), dir.path().join("moved-resume.pdf")).unwrap();
        assert!(matches!(
            rename(
                &mut session,
                RenameDocumentRequest {
                    application_id: detail.record.id,
                    document_id: doc.id.clone(),
                    expected_relative_path: doc.relative_path.clone(),
                    new_name: "next.pdf".into()
                }
            ),
            Err(CoreError::FileMissing)
        ));
        assert!(!root.join("next.pdf").exists());
    }
    #[test]
    fn failed_index_commit_recovers_on_reopen_once_without_changing_document_id() {
        let (dir, mut session, detail, request) = with_file();
        let id = request.document_id.clone();
        let root = dir.path().join(&detail.record.folder_relative_path);
        session.connection().execute_batch("CREATE TRIGGER fail_rename BEFORE UPDATE OF relative_path ON documents BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
        assert!(matches!(
            rename(&mut session, request),
            Err(CoreError::DocumentRenameRecovery)
        ));
        assert!(!root.join("resume.pdf").exists());
        assert_eq!(
            fs::read(root.join("renamed.pdf")).unwrap(),
            b"original attachment"
        );
        assert_eq!(
            applications::get(&session, &detail.record.id)
                .unwrap()
                .documents[0]
                .relative_path,
            "resume.pdf"
        );
        assert!(matches!(
            crate::database_backup::inspect_records(session.connection()),
            Err(CoreError::BackupPendingOperations)
        ));
        session
            .connection()
            .execute_batch("DROP TRIGGER fail_rename")
            .unwrap();
        drop(session);
        let mut reopened =
            warehouse::open(dir.path(), warehouse::WarehouseAccessMode::Write).unwrap();
        let result = applications::get(&reopened, &detail.record.id).unwrap();
        assert_eq!(result.documents[0].id, id);
        assert_eq!(result.documents[0].relative_path, "renamed.pdf");
        assert_eq!(result.record.revision, detail.record.revision + 1);
        recover(&mut reopened).unwrap();
        assert_eq!(
            applications::get(&reopened, &detail.record.id)
                .unwrap()
                .record
                .revision,
            result.record.revision
        );
        assert!(crate::database_backup::inspect_records(reopened.connection()).is_ok());
    }
    #[test]
    fn interrupted_intent_cancels_before_move_and_never_guesses_between_copies() {
        let (dir, mut session, detail, request) = with_file();
        let root = dir.path().join(&detail.record.folder_relative_path);
        let intent = pending(&mut session, &detail, &request);
        recover(&mut session).unwrap();
        let outcome: String = session
            .connection()
            .query_row(
                "SELECT outcome FROM document_renames WHERE id=?1",
                [&intent.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(outcome, "cancelled");
        let _intent = pending(&mut session, &detail, &request);
        fs::copy(root.join("resume.pdf"), root.join("renamed.pdf")).unwrap();
        assert!(matches!(
            recover(&mut session),
            Err(CoreError::DocumentRenameRecovery)
        ));
        assert_eq!(
            fs::read(root.join("resume.pdf")).unwrap(),
            b"original attachment"
        );
        assert_eq!(
            fs::read(root.join("renamed.pdf")).unwrap(),
            b"original attachment"
        );
        drop(session);
        let readonly =
            warehouse::open(dir.path(), warehouse::WarehouseAccessMode::ReadOnly).unwrap();
        let before = readonly.connection().total_changes();
        let report = crate::file_health::recovery_diagnostics(&readonly).unwrap();
        assert_eq!(report.total_pending, 1);
        assert_eq!(report.items[0].kind, "documentRename");
        assert_eq!(
            report.items[0].source.state,
            crate::file_health::PathState::Available
        );
        assert_eq!(readonly.connection().total_changes(), before);
    }
    #[test]
    fn same_content_different_file_identity_does_not_complete_interrupted_rename() {
        let (dir, mut session, detail, request) = with_file();
        let root = dir.path().join(&detail.record.folder_relative_path);
        let _intent = pending(&mut session, &detail, &request);
        fs::rename(root.join("resume.pdf"), root.join("original-kept.pdf")).unwrap();
        fs::write(root.join("renamed.pdf"), b"original attachment").unwrap();
        assert!(matches!(
            recover(&mut session),
            Err(CoreError::DocumentRenameRecovery)
        ));
        assert_eq!(
            applications::get(&session, &detail.record.id)
                .unwrap()
                .documents[0]
                .relative_path,
            "resume.pdf"
        );
        assert_eq!(
            fs::read(root.join("original-kept.pdf")).unwrap(),
            b"original attachment"
        );
    }
    #[cfg(windows)]
    #[test]
    fn occupied_files_and_racing_target_are_preserved() {
        use std::os::windows::fs::OpenOptionsExt;
        let (dir, mut session, detail, request) = with_file();
        let root = dir.path().join(&detail.record.folder_relative_path);
        let source = root.join("resume.pdf");
        let target = root.join("renamed.pdf");
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&source)
            .unwrap();
        assert!(matches!(
            rename(&mut session, request),
            Err(CoreError::FileBusy)
        ));
        drop(lock);
        let identity = file_identity(&source).unwrap();
        fs::write(&target, b"racing target").unwrap();
        assert!(rename_file_no_replace(&source, &target, &identity).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"original attachment");
        assert_eq!(fs::read(&target).unwrap(), b"racing target");
    }
    #[cfg(windows)]
    #[test]
    fn rejects_junctions_and_tampered_paths_without_touching_outside() {
        let (dir, mut session, detail, mut request) = with_file();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("private.pdf"), b"outside").unwrap();
        let root = dir.path().join(&detail.record.folder_relative_path);
        let link = root.join("junction");
        let result = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
            .env("OFFERTRACK_TEST_LINK", &link).env("OFFERTRACK_TEST_TARGET", outside.path()).output().unwrap();
        assert!(result.status.success());
        assert!(matches!(
            list_directories(&session, &detail.record.id),
            Err(CoreError::UnsafePath)
        ));
        session
            .connection()
            .execute(
                "UPDATE documents SET relative_path='junction/private.pdf' WHERE id=?1",
                [&request.document_id],
            )
            .unwrap();
        request.expected_relative_path = "junction/private.pdf".into();
        assert!(matches!(
            rename(&mut session, request),
            Err(CoreError::UnsafePath)
        ));
        assert_eq!(
            fs::read(outside.path().join("private.pdf")).unwrap(),
            b"outside"
        );
        assert!(!outside.path().join("renamed.pdf").exists());
        fs::remove_dir(&link).unwrap(); // Synthetic junction only; never remove its target.
        assert!(checked_target_path(dir.path(), &root, "../outside").is_err());
    }
}
