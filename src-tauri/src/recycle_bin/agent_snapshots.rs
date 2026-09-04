//! Explicitly confirmed cleanup of UUID directories in recycle-bin/agent-snapshots only.
use crate::{
    database_backup,
    error::{AppErrorPayload, CoreError, file_error},
    warehouse::WarehouseSession,
};
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    id: String,
    identity: String,
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
    pub skipped_count: usize,
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
    pub skipped_count: usize,
}

fn items(session: &WarehouseSession) -> Result<(Vec<Item>, usize), CoreError> {
    let base = session.root().join("recycle-bin/agent-snapshots");
    let _guards = database_backup::guard_chain(session.root(), &base)?;
    let mut result = Vec::new();
    let mut skipped = 0;
    for entry in fs::read_dir(base).map_err(file_error)? {
        let entry = entry.map_err(file_error)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let valid_id = Uuid::parse_str(&name).is_ok_and(|id| id.to_string() == name);
        if !valid_id {
            skipped += 1;
            continue;
        }
        let identity = (|| {
            let handle = database_backup::open_guard(&entry.path(), true)?;
            #[cfg(windows)]
            {
                crate::copying::directory_identity_from_handle(&handle)
            }
            #[cfg(not(windows))]
            {
                let _ = handle;
                Err(CoreError::FileOperation)
            }
        })();
        match identity {
            Ok(identity) => result.push(Item { id: name, identity }),
            Err(_) => skipped += 1,
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((result, skipped))
}

pub fn prepare(session: &WarehouseSession) -> Result<(Confirmation, Challenge), CoreError> {
    if !session.is_writable() {
        return Err(CoreError::ReadOnlyWarehouse);
    }
    let (items, skipped_count) = items(session)?;
    let token = Uuid::new_v4().to_string();
    let challenge = Challenge {
        confirmation_token: token.clone(),
        item_ids: items.iter().map(|item| item.id.clone()).collect(),
        skipped_count,
    };
    Ok((
        Confirmation {
            root: session.root().to_owned(),
            warehouse_id: session.summary().warehouse_id,
            token,
            expires: Instant::now() + Duration::from_secs(60),
            items,
        },
        challenge,
    ))
}

pub fn purge(
    session: &WarehouseSession,
    confirmation: Confirmation,
    token: &str,
) -> Result<Purged, CoreError> {
    if !session.is_writable() {
        return Err(CoreError::ReadOnlyWarehouse);
    }
    if session.root() != confirmation.root
        || session.summary().warehouse_id != confirmation.warehouse_id
        || confirmation.token != token
        || Instant::now() >= confirmation.expires
    {
        return Err(CoreError::InvalidConfirmation);
    }
    let (current, skipped_count) = items(session)?;
    if current != confirmation.items {
        return Err(CoreError::InvalidConfirmation);
    }
    let mut result = Purged {
        deleted_ids: Vec::new(),
        failed: Vec::new(),
        skipped_count,
    };
    for item in current {
        let path = session
            .root()
            .join("recycle-bin/agent-snapshots")
            .join(&item.id);
        match super::remove_tree_in_area(
            session.root(),
            &path,
            super::TrashArea::AgentSnapshots,
            Some(&item.identity),
        ) {
            Ok(()) => result.deleted_ids.push(item.id),
            Err(error) => result.failed.push(Failure {
                id: item.id,
                error: error.into(),
            }),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warehouse;

    #[test]
    fn cleanup_is_confirmed_and_confined_to_agent_snapshot_recycle() {
        let temp = tempfile::tempdir().unwrap();
        let session = warehouse::create(temp.path()).unwrap();
        let id = Uuid::new_v4().to_string();
        let target = session.root().join("recycle-bin/agent-snapshots").join(&id);
        fs::create_dir(&target).unwrap();
        fs::write(target.join("manifest.json"), b"synthetic").unwrap();
        fs::write(session.root().join("applications/keep.pdf"), b"keep").unwrap();

        let (confirmation, challenge) = prepare(&session).unwrap();
        assert_eq!(challenge.item_ids, vec![id.clone()]);
        let result = purge(&session, confirmation, &challenge.confirmation_token).unwrap();
        assert_eq!(result.deleted_ids, vec![id]);
        assert!(!target.exists());
        assert!(session.root().join("applications/keep.pdf").exists());
        assert!(session.root().join("recycle-bin/agent-snapshots").is_dir());
    }

    #[test]
    fn wrong_token_or_changed_set_never_deletes() {
        let temp = tempfile::tempdir().unwrap();
        let session = warehouse::create(temp.path()).unwrap();
        let base = session.root().join("recycle-bin/agent-snapshots");
        let first = base.join(Uuid::new_v4().to_string());
        fs::create_dir(&first).unwrap();
        let (confirmation, _) = prepare(&session).unwrap();
        assert!(matches!(
            purge(&session, confirmation, "wrong"),
            Err(CoreError::InvalidConfirmation)
        ));
        let (confirmation, challenge) = prepare(&session).unwrap();
        fs::create_dir(base.join(Uuid::new_v4().to_string())).unwrap();
        assert!(matches!(
            purge(&session, confirmation, &challenge.confirmation_token),
            Err(CoreError::InvalidConfirmation)
        ));
        assert!(first.exists());
    }
}
