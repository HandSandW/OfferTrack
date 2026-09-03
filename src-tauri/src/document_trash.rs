//! ID-scoped attachment recycling. Live index rows move into a versioned
//! catalogue transactionally; filesystem moves have a separate recovery journal.
use crate::{
    applications, copying, document_files as files,
    error::{CoreError, file_error},
    filesystem,
    warehouse::WarehouseSession,
};
use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub mod cleanup;
#[cfg(test)]
mod tests;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrashRequest {
    pub application_id: String,
    pub document_id: String,
    pub expected_relative_path: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub document_id: String,
    pub application_id: String,
    pub company_name: String,
    pub position_name: String,
    pub display_name: String,
    pub original_relative_path: String,
    pub deleted_at_utc: String,
    pub parent_deleted: bool,
    pub file_state: crate::file_health::PathState,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub id: String,
    pub application_id: String,
    pub document_id: String,
    pub relative_path: String,
    pub relocated: bool,
}
#[derive(Debug)]
struct Intent {
    id: String,
    trash_id: String,
    kind: String,
    folder: String,
    relative: String,
    identity: String,
    created: String,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn db(_: rusqlite::Error) -> CoreError {
    CoreError::DatabaseInvalid
}

/// Only a canonical UUID leaf inside this fixed area, never a caller path.
pub(crate) fn trash_path(root: &Path, id: &str) -> Result<PathBuf, CoreError> {
    if !Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id) {
        return Err(CoreError::UnsafePath);
    }
    let path = root.join("recycle-bin/documents").join(id);
    filesystem::validate_no_reparse(root, &path)?;
    Ok(path)
}

pub fn list(session: &WarehouseSession) -> Result<Vec<Entry>, CoreError> {
    let mut statement = session
        .connection()
        .prepare(
            "SELECT t.id,t.document_id,t.application_id,a.company_name,a.position_name,
         t.display_name,t.relative_path,t.deleted_at_utc,a.deleted_at_utc IS NOT NULL
         FROM document_trash t JOIN applications a ON a.id=t.application_id
         WHERE t.state='active' ORDER BY t.deleted_at_utc DESC,t.id",
        )
        .map_err(db)?;
    statement
        .query_map([], |r| {
            Ok(Entry {
                id: r.get(0)?,
                document_id: r.get(1)?,
                application_id: r.get(2)?,
                company_name: r.get(3)?,
                position_name: r.get(4)?,
                display_name: r.get(5)?,
                original_relative_path: r.get(6)?,
                deleted_at_utc: r.get(7)?,
                parent_deleted: r.get(8)?,
                file_state: crate::file_health::PathState::Unavailable,
            })
        })
        .map_err(db)?
        .map(|row| {
            let mut entry = row.map_err(db)?;
            entry.file_state = crate::file_health::observe_file_path(
                session.root(),
                trash_path(session.root(), &entry.id),
            )
            .state;
            Ok(entry)
        })
        .collect()
}

pub fn trash(
    session: &mut WarehouseSession,
    request: TrashRequest,
) -> Result<crate::domain::ApplicationDetail, CoreError> {
    session.connection_mut()?;
    files::recover(session)?;
    let folder = files::application_folder_relative(session, &request.application_id)?;
    let relative: String = session
        .connection()
        .query_row(
            "SELECT relative_path FROM documents WHERE application_id=?1 AND id=?2",
            params![request.application_id, request.document_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?
        .ok_or(CoreError::NotFound)?;
    if relative != request.expected_relative_path {
        return Err(CoreError::RevisionConflict);
    }
    let application_root = filesystem::application_folder(session.root(), &folder)?;
    let source = files::checked_target_path(session.root(), &application_root, &relative)?;
    let trash_id = Uuid::new_v4().to_string();
    let target = trash_path(session.root(), &trash_id)?;
    let _source_parents = copying::lock_move_ancestors(
        session.root(),
        source.parent().ok_or(CoreError::UnsafePath)?,
    )?;
    let _target_parents = copying::lock_move_ancestors(
        session.root(),
        target.parent().ok_or(CoreError::UnsafePath)?,
    )?;
    let intent = Intent {
        id: Uuid::new_v4().to_string(),
        trash_id,
        kind: "trash".into(),
        folder,
        relative,
        identity: files::file_identity(&source)?,
        created: now(),
    };
    let tx = session.connection_mut()?.transaction().map_err(db)?;
    let changed = tx
        .execute(
            "INSERT INTO document_trash
        (id,version,document_id,application_id,relative_path,display_name,media_type,size_bytes,
         content_hash,discovered_at_utc,last_observed_at_utc,deleted_at_utc,state)
        SELECT ?1,1,id,application_id,relative_path,display_name,media_type,size_bytes,
         content_hash,discovered_at_utc,last_observed_at_utc,?2,'pending'
        FROM documents WHERE id=?3 AND application_id=?4 AND relative_path=?5",
            params![
                intent.trash_id,
                intent.created,
                request.document_id,
                request.application_id,
                intent.relative
            ],
        )
        .map_err(db)?;
    if changed != 1 {
        return Err(CoreError::RevisionConflict);
    }
    intent.persist(&tx)?;
    tx.commit().map_err(db)?;
    execute(session, &intent, &source, &target)?;
    applications::get(session, &request.application_id)
}

pub fn restore(session: &mut WarehouseSession, id: &str) -> Result<Restored, CoreError> {
    session.connection_mut()?;
    files::recover(session)?;
    let (application_id, document_id, original): (String,String,String) = session.connection().query_row(
        "SELECT application_id,document_id,relative_path FROM document_trash WHERE id=?1 AND state='active'",
        [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db)?.ok_or(CoreError::NotFound)?;
    // Deleted parents must be restored first. Archived parents remain valid.
    let folder = files::application_folder_relative(session, &application_id)?;
    let root = filesystem::application_folder(session.root(), &folder)?;
    let original_path = files::safe_document_relative(&original)?;
    let original_target = files::checked_target_path(session.root(), &root, &original)?;
    let mut relative = original.clone();
    if !original_target
        .parent()
        .ok_or(CoreError::UnsafePath)?
        .try_exists()
        .map_err(file_error)?
    {
        relative = original_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or(CoreError::UnsafePath)?
            .into();
    }
    let mut target = files::checked_target_path(session.root(), &root, &relative)?;
    let conflict: bool = session.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM documents WHERE application_id=?1 AND relative_path=?2 COLLATE NOCASE)",
        params![application_id,relative], |r|r.get(0)).map_err(db)?;
    if conflict || target.try_exists().map_err(file_error)? {
        let suffix = original_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        // Keep the generated leaf within the Windows component limit even for long originals.
        let extension = if suffix.encode_utf16().count() <= 32 && !suffix.is_empty() {
            format!(".{suffix}")
        } else {
            String::new()
        };
        relative = Path::new(&relative)
            .parent()
            .unwrap_or(Path::new(""))
            .join(format!("restored-{}{extension}", Uuid::new_v4()))
            .to_string_lossy()
            .replace('\\', "/");
        target = files::checked_target_path(session.root(), &root, &relative)?;
    }
    let source = trash_path(session.root(), id)?;
    let _source_parents = copying::lock_move_ancestors(
        session.root(),
        source.parent().ok_or(CoreError::UnsafePath)?,
    )?;
    let _target_parents = copying::lock_move_ancestors(
        session.root(),
        target.parent().ok_or(CoreError::UnsafePath)?,
    )?;
    let intent = Intent {
        id: Uuid::new_v4().to_string(),
        trash_id: id.into(),
        kind: "restore".into(),
        folder,
        relative: relative.clone(),
        identity: files::file_identity(&source)?,
        created: now(),
    };
    intent.persist(session.connection_mut()?)?;
    execute(session, &intent, &source, &target)?;
    Ok(Restored {
        id: id.into(),
        application_id,
        document_id,
        relocated: relative != original,
        relative_path: relative,
    })
}

fn execute(
    session: &mut WarehouseSession,
    intent: &Intent,
    source: &Path,
    target: &Path,
) -> Result<(), CoreError> {
    let _file = match files::rename_file_no_replace(source, target, &intent.identity) {
        Ok(file) => file,
        Err(error) => {
            intent
                .finish(session, false)
                .map_err(|_| CoreError::DocumentTrashRecovery)?;
            return Err(error);
        }
    };
    intent
        .finish(session, true)
        .map_err(|_| CoreError::DocumentTrashRecovery)
}

impl Intent {
    fn persist(&self, connection: &rusqlite::Connection) -> Result<(), CoreError> {
        connection.execute("INSERT INTO document_moves
            (id,version,trash_id,kind,folder_relative_path,document_relative_path,file_identity,created_at_utc)
            VALUES (?1,1,?2,?3,?4,?5,?6,?7)",params![self.id,self.trash_id,self.kind,self.folder,self.relative,self.identity,self.created])
            .map(|_|()).map_err(db)
    }
    fn finish(&self, session: &mut WarehouseSession, moved: bool) -> Result<(), CoreError> {
        let tx = session.connection_mut()?.transaction().map_err(db)?;
        if moved {
            let changed = tx
                .execute(
                    "UPDATE applications SET revision=revision+1,updated_at_utc=?1
                WHERE id=(SELECT application_id FROM document_trash WHERE id=?2)
                AND folder_relative_path=?3 AND deleted_at_utc IS NULL",
                    params![self.created, self.trash_id, self.folder],
                )
                .map_err(db)?;
            if changed != 1 {
                return Err(CoreError::DocumentTrashRecovery);
            }
            if self.kind == "trash" {
                let changed = tx.execute("DELETE FROM documents WHERE id=(SELECT document_id FROM document_trash WHERE id=?1)
                    AND application_id=(SELECT application_id FROM document_trash WHERE id=?1) AND relative_path=?2",
                    params![self.trash_id,self.relative]).map_err(db)?;
                if changed != 1 {
                    return Err(CoreError::DocumentTrashRecovery);
                }
            } else {
                let name = Path::new(&self.relative)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or(CoreError::UnsafePath)?;
                tx.execute("INSERT INTO documents (id,application_id,relative_path,display_name,media_type,size_bytes,content_hash,
                    discovered_at_utc,last_observed_at_utc,missing_at_utc)
                    SELECT document_id,application_id,?2,?3,?5,size_bytes,content_hash,discovered_at_utc,?4,NULL
                    FROM document_trash WHERE id=?1 AND state='active'",
                    params![self.trash_id,self.relative,name,self.created,filesystem::media_type_for_path(Path::new(&self.relative))]).map_err(db)?;
            }
        }
        let state = match (self.kind.as_str(), moved) {
            ("trash", true) => "active",
            ("trash", false) => "cancelled",
            ("restore", true) => "restored",
            ("restore", false) => "active",
            _ => return Err(CoreError::DocumentTrashRecovery),
        };
        tx.execute(
            "UPDATE document_trash SET state=?1 WHERE id=?2",
            params![state, self.trash_id],
        )
        .map_err(db)?;
        tx.execute(
            "UPDATE document_moves SET completed_at_utc=?1,outcome=?2 WHERE id=?3",
            params![
                now(),
                if moved { "completed" } else { "cancelled" },
                self.id
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
}

pub fn recover(session: &mut WarehouseSession) -> Result<(), CoreError> {
    if !session.is_writable() {
        return Ok(());
    }
    let pending = {
        let mut stmt = session.connection().prepare("SELECT id,trash_id,kind,folder_relative_path,document_relative_path,file_identity,created_at_utc
            FROM document_moves WHERE completed_at_utc IS NULL ORDER BY created_at_utc,id").map_err(db)?;
        stmt.query_map([], |r| {
            Ok(Intent {
                id: r.get(0)?,
                trash_id: r.get(1)?,
                kind: r.get(2)?,
                folder: r.get(3)?,
                relative: r.get(4)?,
                identity: r.get(5)?,
                created: r.get(6)?,
            })
        })
        .map_err(db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db)?
    };
    for intent in pending {
        let application = filesystem::application_folder(session.root(), &intent.folder)?;
        let live = files::checked_target_path(session.root(), &application, &intent.relative)?;
        let trash = trash_path(session.root(), &intent.trash_id)?;
        let _live_parents = copying::lock_move_ancestors(
            session.root(),
            live.parent().ok_or(CoreError::UnsafePath)?,
        )?;
        let _trash_parents = copying::lock_move_ancestors(
            session.root(),
            trash.parent().ok_or(CoreError::UnsafePath)?,
        )?;
        let (source, target) = if intent.kind == "trash" {
            (&live, &trash)
        } else {
            (&trash, &live)
        };
        let moved = match (
            source.try_exists().map_err(file_error)?,
            target.try_exists().map_err(file_error)?,
        ) {
            (true, false) => false,
            (false, true) => true,
            _ => return Err(CoreError::DocumentTrashRecovery),
        };
        let handle = files::open_identity_file(if moved { target } else { source })?;
        if files::identity_from_handle(&handle)? != intent.identity {
            return Err(CoreError::DocumentTrashRecovery);
        }
        intent
            .finish(session, moved)
            .map_err(|_| CoreError::DocumentTrashRecovery)?;
    }
    cleanup::recover(session)
}
