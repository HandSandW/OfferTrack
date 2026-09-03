//! Confirmation-bound permanent cleanup for registered document trash only.
use super::trash_path;
use crate::{
    document_files,
    error::{AppErrorPayload, CoreError},
    recycle_bin,
    warehouse::WarehouseSession,
};
use rusqlite::params;
use serde::Serialize;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    id: String,
    identity: Option<String>,
}
pub struct Confirmation {
    root: PathBuf,
    warehouse_id: Uuid,
    token: String,
    expires: Instant,
    items: Vec<Item>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Challenge {
    pub confirmation_token: String,
    pub item_ids: Vec<String>,
    pub missing_count: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub id: String,
    pub error: AppErrorPayload,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Purged {
    pub deleted_ids: Vec<String>,
    pub failed: Vec<Failure>,
}
fn db(_: rusqlite::Error) -> CoreError {
    CoreError::DatabaseInvalid
}
fn items(session: &WarehouseSession) -> Result<Vec<Item>, CoreError> {
    let mut stmt = session
        .connection()
        .prepare("SELECT id FROM document_trash WHERE state='active' ORDER BY id")
        .map_err(db)?;
    stmt.query_map([], |r| r.get::<_, String>(0))
        .map_err(db)?
        .map(|row| {
            let id = row.map_err(db)?;
            let path = trash_path(session.root(), &id)?;
            let identity = match path.try_exists().map_err(crate::error::file_error)? {
                true => Some(document_files::file_identity(&path)?),
                false => None,
            };
            Ok(Item { id, identity })
        })
        .collect()
}
pub fn prepare(session: &WarehouseSession) -> Result<(Confirmation, Challenge), CoreError> {
    if !session.is_writable() {
        return Err(CoreError::ReadOnlyWarehouse);
    }
    let items = items(session)?;
    let token = Uuid::new_v4().to_string();
    let challenge = Challenge {
        confirmation_token: token.clone(),
        item_ids: items.iter().map(|i| i.id.clone()).collect(),
        missing_count: items.iter().filter(|i| i.identity.is_none()).count(),
    };
    Ok((
        Confirmation {
            root: session.root().into(),
            warehouse_id: session.summary().warehouse_id,
            token,
            expires: Instant::now() + Duration::from_secs(60),
            items,
        },
        challenge,
    ))
}
pub fn purge(
    session: &mut WarehouseSession,
    confirmation: Confirmation,
    token: &str,
) -> Result<Purged, CoreError> {
    if !session.is_writable() {
        return Err(CoreError::ReadOnlyWarehouse);
    }
    super::recover(session)?;
    if session.root() != confirmation.root
        || session.summary().warehouse_id != confirmation.warehouse_id
        || token != confirmation.token
        || Instant::now() >= confirmation.expires
    {
        return Err(CoreError::InvalidConfirmation);
    }
    if items(session)? != confirmation.items {
        return Err(CoreError::InvalidConfirmation);
    }
    let mut result = Purged {
        deleted_ids: Vec::new(),
        failed: Vec::new(),
    };
    for item in confirmation.items {
        let journal = Uuid::new_v4().to_string();
        session.connection_mut()?.execute("INSERT INTO document_purges(id,version,trash_id,created_at_utc) VALUES(?1,1,?2,?3)",params![journal,item.id,chrono::Utc::now().to_rfc3339()]).map_err(db)?;
        let deleted = match item.identity.as_deref() {
            None => Ok(()),
            Some(identity) => recycle_bin::remove_document_file(
                session.root(),
                &trash_path(session.root(), &item.id)?,
                identity,
            ),
        };
        match deleted {
            Ok(()) => {
                let tx = session.connection_mut()?.transaction().map_err(db)?;
                tx.execute(
                    "UPDATE document_trash SET state='purged' WHERE id=?1 AND state='active'",
                    [&item.id],
                )
                .map_err(db)?;
                tx.execute("UPDATE document_purges SET completed_at_utc=?1,outcome='completed' WHERE id=?2",params![chrono::Utc::now().to_rfc3339(),journal]).map_err(db)?;
                tx.commit().map_err(|_| CoreError::DocumentTrashRecovery)?;
                result.deleted_ids.push(item.id);
            }
            Err(error) => {
                session.connection_mut()?.execute("UPDATE document_purges SET completed_at_utc=?1,outcome='cancelled' WHERE id=?2",params![chrono::Utc::now().to_rfc3339(),journal]).map_err(db)?;
                result.failed.push(Failure {
                    id: item.id,
                    error: error.into(),
                });
            }
        }
    }
    Ok(result)
}
pub fn recover(session: &mut WarehouseSession) -> Result<(), CoreError> {
    if !session.is_writable() {
        return Ok(());
    }
    let ids = {
        let mut stmt=session.connection().prepare("SELECT p.id,p.trash_id FROM document_purges p WHERE p.completed_at_utc IS NULL ORDER BY p.created_at_utc,p.id").map_err(db)?;
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?
    };
    for (journal, id) in ids {
        let exists = trash_path(session.root(), &id)?
            .try_exists()
            .map_err(crate::error::file_error)?;
        let tx = session.connection_mut()?.transaction().map_err(db)?;
        if !exists {
            tx.execute(
                "UPDATE document_trash SET state='purged' WHERE id=?1 AND state='active'",
                [&id],
            )
            .map_err(db)?;
        }
        tx.execute(
            "UPDATE document_purges SET completed_at_utc=?1,outcome=?2 WHERE id=?3",
            params![
                chrono::Utc::now().to_rfc3339(),
                if exists { "cancelled" } else { "completed" },
                journal
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
    }
    Ok(())
}
