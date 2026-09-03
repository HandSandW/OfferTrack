use std::{fs, path::Path};

use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

pub mod backups;

use crate::{
    domain::{EmptyTrashResult, TrashEntry},
    error::CoreError,
    filesystem,
    warehouse::WarehouseSession,
};

pub fn list(session: &WarehouseSession) -> Result<Vec<TrashEntry>, CoreError> {
    let mut statement = session
        .connection()
        .prepare(
            "SELECT a.id, a.company_name, a.position_name, t.deleted_at_utc,
                    COALESCE(t.original_relative_path, ''), COALESCE(t.trash_relative_path, '')
             FROM trash_entries t
             JOIN applications a ON a.id = t.entity_id
             WHERE t.entity_type = 'application' AND t.restored_at_utc IS NULL
                   AND t.permanently_deleted_at_utc IS NULL
             ORDER BY t.deleted_at_utc DESC",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([], |row| {
            Ok(TrashEntry {
                application_id: row.get(0)?,
                company_name: row.get(1)?,
                position_name: row.get(2)?,
                deleted_at_utc: row.get(3)?,
                original_relative_path: row.get(4)?,
                trash_relative_path: row.get(5)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub fn move_application_to_trash(
    session: &mut WarehouseSession,
    application_id: &str,
) -> Result<(), CoreError> {
    session.connection_mut()?;
    recover_moves(session)?;
    let warehouse_root = session.root().to_path_buf();
    let (original_relative, already_deleted) = session
        .connection()
        .query_row(
            "SELECT folder_relative_path, deleted_at_utc IS NOT NULL
             FROM applications WHERE id = ?1",
            [application_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    if already_deleted {
        return Err(CoreError::NotFound);
    }
    let source = filesystem::application_folder(&warehouse_root, &original_relative)?;
    validate_movable_directory(&source)?;
    let trash_name = format!("{}-{}", application_id, Uuid::new_v4().simple());
    let trash_relative = format!("recycle-bin/records/{trash_name}");
    let target = filesystem::trash_folder(&warehouse_root, &trash_relative)?;
    let operation = MoveIntent::new(
        "trash",
        application_id,
        &Uuid::new_v4().to_string(),
        &original_relative,
        &trash_relative,
    );
    execute_move(session, operation, &source, &target)
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub application_id: String,
    pub folder_relative_path: String,
    pub renamed: bool,
}

pub fn restore_application(
    session: &mut WarehouseSession,
    application_id: &str,
) -> Result<RestoreResult, CoreError> {
    session.connection_mut()?;
    recover_moves(session)?;
    let warehouse_root = session.root().to_path_buf();
    let (trash_id, original_relative, trash_relative) = session
        .connection()
        .query_row(
            "SELECT id, original_relative_path, trash_relative_path
             FROM trash_entries
             WHERE entity_type = 'application' AND entity_id = ?1
                   AND restored_at_utc IS NULL AND permanently_deleted_at_utc IS NULL
             ORDER BY deleted_at_utc DESC LIMIT 1",
            [application_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    let source = filesystem::trash_folder(&warehouse_root, &trash_relative)?;
    validate_movable_directory(&source)?;
    let original_target = filesystem::application_folder(&warehouse_root, &original_relative)?;
    let target = if original_target
        .try_exists()
        .map_err(crate::error::file_error)?
    {
        let name = original_target
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(CoreError::UnsafePath)?;
        original_target.with_file_name(format!("{name}__restored__{}", Uuid::new_v4().simple()))
    } else {
        original_target
    };
    let target_relative = format!(
        "applications/{}",
        target
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(CoreError::UnsafePath)?
    );
    let operation = MoveIntent::new(
        "restore",
        application_id,
        &trash_id,
        &trash_relative,
        &target_relative,
    );
    execute_move(session, operation, &source, &target)?;
    Ok(RestoreResult {
        application_id: application_id.into(),
        renamed: target_relative != original_relative,
        folder_relative_path: target_relative,
    })
}

struct MoveIntent {
    id: String,
    kind: String,
    application_id: String,
    trash_id: String,
    source: String,
    target: String,
    created: String,
}

impl MoveIntent {
    fn new(kind: &str, application_id: &str, trash_id: &str, source: &str, target: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            kind: kind.into(),
            application_id: application_id.into(),
            trash_id: trash_id.into(),
            source: source.into(),
            target: target.into(),
            created: now_utc(),
        }
    }

    fn persist(&self, session: &mut WarehouseSession) -> Result<(), CoreError> {
        session
            .connection_mut()?
            .execute(
                "INSERT INTO file_operations (id, operation_kind, application_id, trash_id,
                source_relative_path, target_relative_path, created_at_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    self.id,
                    self.kind,
                    self.application_id,
                    self.trash_id,
                    self.source,
                    self.target,
                    self.created
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        Ok(())
    }

    fn finish(&self, session: &mut WarehouseSession, moved: bool) -> Result<(), CoreError> {
        let transaction = session
            .connection_mut()?
            .transaction()
            .map_err(|_| CoreError::DatabaseInvalid)?;
        if moved {
            if self.kind == "trash" {
                transaction
                    .execute(
                        "UPDATE applications SET deleted_at_utc = ?1, updated_at_utc = ?1,
                        revision = revision + 1 WHERE id = ?2",
                        params![self.created, self.application_id],
                    )
                    .map_err(|_| CoreError::DatabaseInvalid)?;
                transaction
                    .execute(
                        "INSERT INTO trash_entries (id, entity_type, entity_id,
                        original_relative_path, trash_relative_path, manifest_json, deleted_at_utc)
                     VALUES (?1, 'application', ?2, ?3, ?4, '{\"version\":1}', ?5)",
                        params![
                            self.trash_id,
                            self.application_id,
                            self.source,
                            self.target,
                            self.created
                        ],
                    )
                    .map_err(|_| CoreError::DatabaseInvalid)?;
            } else if self.kind == "normalize" {
                transaction.execute(
                    "UPDATE applications SET folder_relative_path = ?1, folder_normalization_pending = 0 WHERE id = ?2",
                    params![self.target, self.application_id],
                ).map_err(|_| CoreError::DatabaseInvalid)?;
            } else {
                transaction
                    .execute(
                        "UPDATE applications SET deleted_at_utc = NULL, folder_relative_path = ?1,
                        updated_at_utc = ?2, revision = revision + 1 WHERE id = ?3",
                        params![self.target, self.created, self.application_id],
                    )
                    .map_err(|_| CoreError::DatabaseInvalid)?;
                transaction
                    .execute(
                        "UPDATE trash_entries SET restored_at_utc = ?1 WHERE id = ?2",
                        params![self.created, self.trash_id],
                    )
                    .map_err(|_| CoreError::DatabaseInvalid)?;
            }
        }
        transaction
            .execute(
                "UPDATE file_operations SET completed_at_utc = ?1, outcome = ?2 WHERE id = ?3",
                params![
                    now_utc(),
                    if moved { "completed" } else { "cancelled" },
                    self.id
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        transaction.commit().map_err(|_| CoreError::DatabaseInvalid)
    }
}

fn execute_move(
    session: &mut WarehouseSession,
    operation: MoveIntent,
    source: &Path,
    target: &Path,
) -> Result<(), CoreError> {
    if target.try_exists().map_err(|_| CoreError::FileOperation)? {
        return Err(CoreError::FileOperation);
    }
    // Intent is durable before touching files. If SQLite commit fails or the
    // process stops after rename, reopening the warehouse finishes this intent.
    operation.persist(session)?;
    if let Err(error) = fs::rename(source, target) {
        operation.finish(session, false)?;
        return Err(crate::error::file_error(error));
    }
    operation.finish(session, true)
}

pub fn recover_moves(session: &mut WarehouseSession) -> Result<(), CoreError> {
    if !session.is_writable() {
        return Ok(());
    }
    let pending = {
        let mut statement = session.connection().prepare(
            "SELECT id, operation_kind, application_id, trash_id, source_relative_path,
                target_relative_path, created_at_utc FROM file_operations WHERE completed_at_utc IS NULL",
        ).map_err(|_| CoreError::DatabaseInvalid)?;
        statement
            .query_map([], |row| {
                Ok(MoveIntent {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    application_id: row.get(2)?,
                    trash_id: row.get(3)?,
                    source: row.get(4)?,
                    target: row.get(5)?,
                    created: row.get(6)?,
                })
            })
            .map_err(|_| CoreError::DatabaseInvalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CoreError::DatabaseInvalid)?
    };
    for operation in pending {
        let (source, target) = if operation.kind == "trash" {
            (
                filesystem::application_folder(session.root(), &operation.source)?,
                filesystem::trash_folder(session.root(), &operation.target)?,
            )
        } else if operation.kind == "normalize" {
            (
                filesystem::application_folder(session.root(), &operation.source)?,
                filesystem::application_folder(session.root(), &operation.target)?,
            )
        } else {
            (
                filesystem::trash_folder(session.root(), &operation.source)?,
                filesystem::application_folder(session.root(), &operation.target)?,
            )
        };
        match (source.try_exists(), target.try_exists()) {
            (Ok(true), Ok(false)) => operation.finish(session, false)?,
            (Ok(false), Ok(true)) => {
                validate_movable_directory(&target)?;
                operation.finish(session, true)?;
            }
            // Never guess which copy is authoritative or overwrite either side.
            _ => return Err(CoreError::FileOperation),
        }
    }
    Ok(())
}

pub fn normalize_record_folder(
    session: &mut WarehouseSession,
    application_id: &str,
    source_relative: &str,
    target_relative: &str,
) -> Result<(), CoreError> {
    session.connection_mut()?;
    recover_moves(session)?;
    let source = filesystem::application_folder(session.root(), source_relative)?;
    let target = filesystem::application_folder(session.root(), target_relative)?;
    validate_movable_directory(&source)?;
    execute_move(
        session,
        MoveIntent::new(
            "normalize",
            application_id,
            "",
            source_relative,
            target_relative,
        ),
        &source,
        &target,
    )
}

pub fn active_item_count(session: &WarehouseSession) -> Result<i64, CoreError> {
    session
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM trash_entries
             WHERE restored_at_utc IS NULL AND permanently_deleted_at_utc IS NULL",
            [],
            |row| row.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub fn active_item_ids(session: &WarehouseSession) -> Result<Vec<String>, CoreError> {
    let mut statement = session.connection().prepare(
        "SELECT id FROM trash_entries WHERE restored_at_utc IS NULL AND permanently_deleted_at_utc IS NULL ORDER BY id",
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([], |row| row.get(0))
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub fn empty(session: &mut WarehouseSession) -> Result<EmptyTrashResult, CoreError> {
    session.connection_mut()?;
    recover_moves(session)?;
    let warehouse_root = session.root().to_path_buf();
    let items = {
        let mut statement = session
            .connection()
            .prepare(
                "SELECT id, entity_id, trash_relative_path FROM trash_entries
                 WHERE restored_at_utc IS NULL AND permanently_deleted_at_utc IS NULL",
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|_| CoreError::DatabaseInvalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CoreError::DatabaseInvalid)?
    };
    let mut deleted = Vec::new();
    let mut failed_application_ids = Vec::new();
    for (trash_id, application_id, relative) in items {
        let Some(relative) = relative else {
            failed_application_ids.push(application_id);
            continue;
        };
        let path = match filesystem::trash_folder(&warehouse_root, &relative) {
            Ok(path) => path,
            Err(_) => {
                failed_application_ids.push(application_id);
                continue;
            }
        };
        if !relative.starts_with("recycle-bin/records/")
            || Path::new(&relative).components().count() != 3
            || (!matches!(path.try_exists(), Ok(false))
                && remove_tree_without_reparse(&warehouse_root, &path).is_err())
        {
            failed_application_ids.push(application_id);
        } else {
            deleted.push(trash_id);
        }
    }
    let now = now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for trash_id in &deleted {
        transaction
            .execute(
                "UPDATE trash_entries SET permanently_deleted_at_utc = ?1 WHERE id = ?2",
                params![now, trash_id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(EmptyTrashResult {
        deleted_count: deleted.len(),
        failed_application_ids,
    })
}

fn validate_movable_directory(path: &Path) -> Result<(), CoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CoreError::FileOperation)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || filesystem::is_reparse_point(&metadata)
    {
        Err(CoreError::UnsafePath)
    } else {
        Ok(())
    }
}

fn remove_tree_without_reparse(warehouse_root: &Path, path: &Path) -> Result<(), CoreError> {
    remove_tree_with_identity(warehouse_root, path, None)
}

fn remove_tree_with_identity(
    warehouse_root: &Path,
    path: &Path,
    expected_identity: Option<&str>,
) -> Result<(), CoreError> {
    remove_tree_in_area(warehouse_root, path, TrashArea::Records, expected_identity)
}

// Closed internal allowlist: never accept a caller-provided deletion root.
enum TrashArea {
    Records,
    Backups,
    Documents,
}

fn remove_tree_in_area(
    warehouse_root: &Path,
    path: &Path,
    area: TrashArea,
    expected_identity: Option<&str>,
) -> Result<(), CoreError> {
    let recycle_root = warehouse_root.join("recycle-bin").join(match area {
        TrashArea::Records => "records",
        TrashArea::Backups => "backups",
        TrashArea::Documents => "documents",
    });
    // Do not canonicalize a redirected recycle-bin and accidentally grant its
    // destination deletion authority. Freeze Windows ancestors while deleting.
    filesystem::validate_no_reparse(warehouse_root, path)?;
    #[cfg(windows)]
    let _ancestors = [
        warehouse_root.to_path_buf(),
        warehouse_root.join("recycle-bin"),
        recycle_root.clone(),
    ]
    .iter()
    .map(|ancestor| lock_entry(ancestor, false))
    .collect::<Result<Vec<_>, _>>()?;
    let canonical_recycle = fs::canonicalize(&recycle_root).map_err(|_| CoreError::UnsafePath)?;
    let canonical_parent = fs::canonicalize(path.parent().ok_or(CoreError::UnsafePath)?)
        .map_err(|_| CoreError::UnsafePath)?;
    if canonical_parent != canonical_recycle || path == recycle_root {
        return Err(CoreError::UnsafePath);
    }
    remove_entry_with_identity(path, expected_identity)
}

pub(crate) fn remove_document_file(
    warehouse_root: &Path,
    path: &Path,
    expected_identity: &str,
) -> Result<(), CoreError> {
    remove_tree_in_area(
        warehouse_root,
        path,
        TrashArea::Documents,
        Some(expected_identity),
    )
}

pub(crate) fn remove_staging_directory(
    warehouse_root: &Path,
    path: &Path,
    expected_identity: &str,
) -> Result<(), CoreError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(CoreError::UnsafePath)?;
    if !name.starts_with(".copying-") {
        return Err(CoreError::UnsafePath);
    }
    remove_tree_with_identity(warehouse_root, path, Some(expected_identity))
}

#[cfg(not(windows))]
fn remove_entry_with_identity(
    _path: &Path,
    _expected_identity: Option<&str>,
) -> Result<(), CoreError> {
    // A race-safe Unix implementation is deliberately not claimed by this
    // Windows-only release. Fail closed instead of path-based recursive unlink.
    Err(CoreError::FileOperation)
}

#[cfg(windows)]
fn lock_entry(path: &Path, deleting: bool) -> Result<fs::File, CoreError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        FILE_SHARE_READ,
    };
    let file = fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | if deleting { DELETE } else { 0 })
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| CoreError::FileOperation)?;
    let metadata = file.metadata().map_err(|_| CoreError::FileOperation)?;
    if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    Ok(file)
}

#[cfg(windows)]
fn remove_entry_with_identity(
    path: &Path,
    expected_identity: Option<&str>,
) -> Result<(), CoreError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    let metadata = fs::symlink_metadata(path).map_err(|_| CoreError::FileOperation)?;
    if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    // The handle excludes sharing for write/delete, preventing replacement or
    // reparse-point edits after validation. Keep it alive through recursion.
    let handle = lock_entry(path, true)?;
    if let Some(expected) = expected_identity {
        let actual = if handle
            .metadata()
            .map_err(|_| CoreError::FileOperation)?
            .is_dir()
        {
            crate::copying::directory_identity_from_handle(&handle)?
        } else {
            crate::document_files::identity_from_handle(&handle)?
        };
        if actual != expected {
            return Err(CoreError::CopyRecovery);
        }
    }
    if handle
        .metadata()
        .map_err(|_| CoreError::FileOperation)?
        .is_dir()
    {
        for entry in fs::read_dir(path).map_err(|_| CoreError::FileOperation)? {
            remove_entry_with_identity(&entry.map_err(|_| CoreError::FileOperation)?.path(), None)?;
        }
    }
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the live owned handle refers to the validated entry; info remains
    // valid for this synchronous call and its size matches FILE_DISPOSITION_INFO.
    let result = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if result == 0 {
        Err(CoreError::FileOperation)
    } else {
        Ok(())
    }
}

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{applications, domain::CreateApplicationRequest, warehouse};
    use tempfile::tempdir;

    fn request() -> CreateApplicationRequest {
        CreateApplicationRequest {
            company_name: "安全测试公司".to_owned(),
            position_name: "测试岗位".to_owned(),
            company_type: "private".to_owned(),
            industry: String::new(),
            position_category: String::new(),
            work_location: String::new(),
        }
    }

    #[test]
    fn permanent_delete_rejects_paths_outside_fixed_records_root() {
        let warehouse = tempdir().expect("warehouse");
        fs::create_dir_all(warehouse.path().join("recycle-bin/records")).expect("trash root");
        let outside = warehouse.path().join("outside");
        fs::create_dir(&outside).expect("outside fixture");
        assert!(matches!(
            remove_tree_without_reparse(warehouse.path(), &outside),
            Err(CoreError::UnsafePath)
        ));
        assert!(outside.exists());
    }

    #[cfg(windows)]
    #[test]
    fn document_cleanup_only_deletes_a_verified_direct_file_in_its_fixed_area() {
        let warehouse = tempdir().expect("warehouse");
        let base = warehouse.path().join("recycle-bin/documents");
        fs::create_dir_all(&base).unwrap();
        let id = Uuid::new_v4().to_string();
        let target = base.join(&id);
        let other = warehouse.path().join("outside.txt");
        fs::write(&target, b"trash").unwrap();
        fs::write(&other, b"safe").unwrap();
        let identity = crate::document_files::file_identity(&target).unwrap();
        remove_document_file(warehouse.path(), &target, &identity).unwrap();
        assert!(!target.exists());
        assert_eq!(fs::read(&other).unwrap(), b"safe");
        assert!(matches!(
            remove_document_file(warehouse.path(), &other, &identity),
            Err(CoreError::UnsafePath)
        ));
    }

    #[test]
    fn permanent_delete_never_deletes_the_records_root() {
        let warehouse = tempdir().expect("warehouse");
        let records = warehouse.path().join("recycle-bin/records");
        fs::create_dir_all(&records).expect("trash root");
        assert!(matches!(
            remove_tree_without_reparse(warehouse.path(), &records),
            Err(CoreError::UnsafePath)
        ));
        assert!(records.exists());
    }

    #[test]
    fn permanent_delete_removes_only_a_valid_direct_child() {
        let warehouse = tempdir().expect("warehouse");
        let records = warehouse.path().join("recycle-bin/records");
        let target = records.join("record-id");
        fs::create_dir_all(&target).expect("trash child");
        fs::write(target.join("resume.txt"), "fixture").expect("fixture file");
        remove_tree_without_reparse(warehouse.path(), &target).expect("safe removal");
        assert!(!target.exists());
        assert!(records.exists());
    }

    #[test]
    fn delete_restore_and_confirmed_empty_keep_database_and_files_consistent() {
        let directory = tempdir().expect("warehouse");
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let original = directory.path().join(&record.record.folder_relative_path);
        fs::write(original.join("resume.pdf"), b"pdf").unwrap();

        move_application_to_trash(&mut session, &record.record.id).unwrap();
        assert!(!original.exists());
        assert_eq!(list(&session).unwrap().len(), 1);
        restore_application(&mut session, &record.record.id).unwrap();
        assert!(original.exists());
        assert!(list(&session).unwrap().is_empty());

        move_application_to_trash(&mut session, &record.record.id).unwrap();
        let result = empty(&mut session).unwrap();
        assert_eq!(result.deleted_count, 1);
        assert!(result.failed_application_ids.is_empty());
        assert!(list(&session).unwrap().is_empty());
        assert!(directory.path().join("recycle-bin/records").is_dir());
    }

    #[test]
    fn restore_reports_conflict_without_overwriting_file_or_directory() {
        for occupied_by_file in [false, true] {
            let directory = tempdir().unwrap();
            let mut session = warehouse::create(directory.path()).unwrap();
            let record = applications::create(&mut session, request()).unwrap();
            let original = directory.path().join(&record.record.folder_relative_path);
            fs::write(original.join("resume.pdf"), b"original resume").unwrap();
            applications::scan_documents(&mut session, &record.record.id).unwrap();
            move_application_to_trash(&mut session, &record.record.id).unwrap();
            let occupied = if occupied_by_file {
                original.clone()
            } else {
                fs::create_dir(&original).unwrap();
                original.join("keep.txt")
            };
            fs::write(&occupied, b"never overwrite").unwrap();
            let result = restore_application(&mut session, &record.record.id).unwrap();
            assert!(result.renamed);
            assert_eq!(result.application_id, record.record.id);
            assert_ne!(
                result.folder_relative_path,
                record.record.folder_relative_path
            );
            assert_eq!(fs::read(&occupied).unwrap(), b"never overwrite");
            assert_eq!(
                fs::read(
                    directory
                        .path()
                        .join(&result.folder_relative_path)
                        .join("resume.pdf")
                )
                .unwrap(),
                b"original resume"
            );
            assert!(list(&session).unwrap().is_empty());
            drop(session);
            let reopened =
                warehouse::open(directory.path(), warehouse::WarehouseAccessMode::ReadOnly)
                    .unwrap();
            let restored = applications::get(&reopened, &record.record.id).unwrap();
            assert_eq!(
                restored.record.folder_relative_path,
                result.folder_relative_path
            );
            assert_eq!(restored.documents.len(), 1);
            let payload = serde_json::to_value(result).unwrap();
            assert_eq!(payload["renamed"], true);
            assert!(
                payload["folderRelativePath"]
                    .as_str()
                    .unwrap()
                    .starts_with("applications/")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn permanent_delete_rejects_a_symbolic_link_without_touching_its_target() {
        let warehouse = tempdir().expect("warehouse");
        let records = warehouse.path().join("recycle-bin/records");
        let outside = warehouse.path().join("outside.txt");
        let link = records.join("linked.txt");
        fs::create_dir_all(&records).unwrap();
        fs::write(&outside, "must survive").unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        assert!(matches!(
            remove_tree_without_reparse(warehouse.path(), &link),
            Err(CoreError::UnsafePath)
        ));
        assert!(outside.exists());
    }

    #[test]
    fn read_only_session_cannot_move_restore_or_purge_files() {
        let directory = tempdir().unwrap();
        let mut writer = warehouse::create(directory.path()).unwrap();
        let active = applications::create(&mut writer, request()).unwrap();
        let deleted = applications::create(&mut writer, request()).unwrap();
        move_application_to_trash(&mut writer, &deleted.record.id).unwrap();
        let trash_path = directory
            .path()
            .join(&list(&writer).unwrap()[0].trash_relative_path);
        let mut reader =
            warehouse::open(directory.path(), warehouse::WarehouseAccessMode::ReadOnly).unwrap();
        assert!(matches!(
            move_application_to_trash(&mut reader, &active.record.id),
            Err(CoreError::ReadOnlyWarehouse)
        ));
        assert!(matches!(
            restore_application(&mut reader, &deleted.record.id),
            Err(CoreError::ReadOnlyWarehouse)
        ));
        assert!(matches!(
            empty(&mut reader),
            Err(CoreError::ReadOnlyWarehouse)
        ));
        assert!(trash_path.is_dir());
        assert!(
            directory
                .path()
                .join(&active.record.folder_relative_path)
                .is_dir()
        );
    }

    #[test]
    fn reopening_recovers_a_move_interrupted_before_database_commit() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let target = "recycle-bin/records/interrupted";
        let intent = MoveIntent::new(
            "trash",
            &record.record.id,
            "trash-test",
            &record.record.folder_relative_path,
            target,
        );
        intent.persist(&mut session).unwrap();
        fs::rename(
            directory.path().join(&intent.source),
            directory.path().join(target),
        )
        .unwrap();
        drop(session);

        let mut reopened =
            warehouse::open(directory.path(), warehouse::WarehouseAccessMode::Write).unwrap();
        assert_eq!(list(&reopened).unwrap().len(), 1);
        assert!(
            applications::get(&reopened, &record.record.id)
                .unwrap()
                .record
                .deleted_at_utc
                .is_some()
        );
        recover_moves(&mut reopened).unwrap();
        assert_eq!(
            list(&reopened).unwrap().len(),
            1,
            "recovery must be idempotent"
        );
        restore_application(&mut reopened, &record.record.id).unwrap();
        assert!(
            directory
                .path()
                .join(&record.record.folder_relative_path)
                .is_dir()
        );
    }

    #[test]
    fn corrupted_trash_paths_never_delete_live_data() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = applications::create(&mut session, request()).unwrap();
        let live = directory.path().join("applications/must-survive");
        fs::create_dir(&live).unwrap();
        fs::write(live.join("data.txt"), b"protected").unwrap();
        move_application_to_trash(&mut session, &record.record.id).unwrap();
        for invalid in [
            "recycle-bin/records/../../applications/must-survive",
            "applications/must-survive",
            "recycle-bin/records",
            "recycle-bin/records/.",
        ] {
            session
                .connection_mut()
                .unwrap()
                .execute(
                    "UPDATE trash_entries SET trash_relative_path = ?1",
                    [invalid],
                )
                .unwrap();
            let result = empty(&mut session).unwrap();
            assert_eq!(result.deleted_count, 0);
            assert_eq!(result.failed_application_ids.len(), 1);
            assert!(live.join("data.txt").is_file());
        }
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        // Junctions need no Developer Mode / elevation, unlike symlinks.
        // Paths travel as env values, never as interpolated PowerShell code.
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
            .env("OFFERTRACK_TEST_LINK", link).env("OFFERTRACK_TEST_TARGET", target)
            .output().unwrap();
        assert!(
            result.status.success(),
            "junction fixture creation must succeed, never silently skip"
        );
    }

    #[cfg(windows)]
    #[test]
    fn rejects_both_ancestor_and_nested_junctions() {
        let directory = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("protected.txt"), b"outside").unwrap();
        let records = directory.path().join("recycle-bin/records");
        fs::create_dir_all(&records).unwrap();
        let child = records.join("junction");
        junction(&child, outside.path());
        assert!(matches!(
            remove_tree_without_reparse(directory.path(), &child),
            Err(CoreError::UnsafePath)
        ));
        fs::remove_dir(&child).unwrap(); // remove fixture junction itself, not its target
        fs::remove_dir(&records).unwrap();
        junction(&records, outside.path());
        assert!(matches!(
            remove_tree_without_reparse(directory.path(), &records.join("protected.txt")),
            Err(CoreError::UnsafePath)
        ));
        assert!(outside.path().join("protected.txt").is_file());
        fs::remove_dir(&records).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn locked_entry_cannot_be_renamed_between_validation_and_delete() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("locked");
        fs::create_dir(&target).unwrap();
        let handle = lock_entry(&target, false).unwrap();
        assert!(fs::rename(&target, directory.path().join("replacement")).is_err());
        drop(handle);
        assert!(fs::rename(&target, directory.path().join("replacement")).is_ok());
    }
}
