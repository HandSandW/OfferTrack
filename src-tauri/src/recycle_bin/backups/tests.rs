use super::*;
use crate::{database_backup, warehouse};
use std::{path::Path, time::Duration};

fn recycled(session: &WarehouseSession) -> (String, PathBuf) {
    let backup = database_backup::create(session).unwrap().backup;
    let id = backup.id.to_string();
    let target = session.root().join("recycle-bin/backups").join(&id);
    fs::rename(session.root().join("backups/database").join(&id), &target).unwrap();
    (id, target)
}

#[test]
fn cleans_only_confirmed_backup_directories_and_keeps_other_data() {
    let temp = tempfile::tempdir().unwrap();
    let session = warehouse::create(temp.path()).unwrap();
    let (id, path) = recycled(&session);
    let active = database_backup::create(&session).unwrap().backup;
    let keep = session.root().join("applications/keep.pdf");
    fs::write(&keep, b"synthetic").unwrap();
    let record_trash = session.root().join("recycle-bin/records/keep");
    fs::create_dir(&record_trash).unwrap();
    let unknown = session.root().join("recycle-bin/backups/unknown");
    fs::create_dir(&unknown).unwrap();
    let (confirmation, challenge) = prepare(&session).unwrap();
    assert_eq!(challenge.item_ids, vec![id.clone()]);
    assert_eq!(challenge.skipped_count, 1);
    let result = purge(&session, confirmation, &challenge.confirmation_token).unwrap();
    assert_eq!(result.deleted_ids, vec![id]);
    assert!(result.failed.is_empty());
    assert!(!path.exists());
    assert!(keep.exists());
    assert!(unknown.exists());
    assert!(record_trash.exists());
    assert!(session.root().join("recycle-bin/backups").is_dir());
    database_backup::preview(&session, &active.id.to_string(), false).unwrap();
}

#[test]
fn token_expiry_changed_set_replaced_identity_and_other_warehouse_reject_before_delete() {
    let temp = tempfile::tempdir().unwrap();
    let session = warehouse::create(temp.path()).unwrap();
    let (_, path) = recycled(&session);
    let (confirmation, _) = prepare(&session).unwrap();
    assert!(matches!(
        purge(&session, confirmation, "wrong"),
        Err(CoreError::InvalidConfirmation)
    ));
    let (mut confirmation, challenge) = prepare(&session).unwrap();
    confirmation.expires = Instant::now() - Duration::from_secs(1);
    assert!(matches!(
        purge(&session, confirmation, &challenge.confirmation_token),
        Err(CoreError::InvalidConfirmation)
    ));
    let (confirmation, challenge) = prepare(&session).unwrap();
    recycled(&session);
    assert!(matches!(
        purge(&session, confirmation, &challenge.confirmation_token),
        Err(CoreError::InvalidConfirmation)
    ));
    let (confirmation, challenge) = prepare(&session).unwrap();
    let held = session.root().join("held-original");
    fs::rename(&path, &held).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("keep"), b"replacement").unwrap();
    assert!(matches!(
        purge(&session, confirmation, &challenge.confirmation_token),
        Err(CoreError::InvalidConfirmation)
    ));
    assert!(held.join("database.sqlite").exists());
    assert!(path.join("keep").exists());
    let (confirmation, challenge) = prepare(&session).unwrap();
    let other = tempfile::tempdir().unwrap();
    let other_session = warehouse::create(other.path()).unwrap();
    assert!(matches!(
        purge(&other_session, confirmation, &challenge.confirmation_token),
        Err(CoreError::InvalidConfirmation)
    ));
}

#[test]
fn readonly_and_wrong_area_cannot_delete_active_backups_or_roots() {
    let temp = tempfile::tempdir().unwrap();
    let session = warehouse::create(temp.path()).unwrap();
    let (_, path) = recycled(&session);
    let (confirmation, challenge) = prepare(&session).unwrap();
    let reader = warehouse::open(temp.path(), warehouse::WarehouseAccessMode::ReadOnly).unwrap();
    assert!(matches!(
        prepare(&reader),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        purge(&reader, confirmation, &challenge.confirmation_token),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    for relative in [
        "recycle-bin/backups",
        "backups/database",
        "applications",
        "recycle-bin/records",
    ] {
        assert!(
            super::super::remove_tree_in_area(
                session.root(),
                &session.root().join(relative),
                super::super::TrashArea::Backups,
                None
            )
            .is_err()
        );
        assert!(session.root().join(relative).exists());
    }
    assert!(path.exists());
}

#[cfg(windows)]
#[test]
fn locked_files_report_partial_failure_and_remaining_directory_can_be_retried() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let session = warehouse::create(temp.path()).unwrap();
    let (id, path) = recycled(&session);
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path.join("database.sqlite"))
        .unwrap();
    let (confirmation, challenge) = prepare(&session).unwrap();
    let result = purge(&session, confirmation, &challenge.confirmation_token).unwrap();
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].id, id);
    assert!(result.deleted_ids.is_empty());
    assert!(path.join("database.sqlite").exists());
    drop(locked);
    let (confirmation, challenge) = prepare(&session).unwrap();
    assert_eq!(
        purge(&session, confirmation, &challenge.confirmation_token)
            .unwrap()
            .deleted_ids,
        vec![id]
    );
    assert!(!path.exists());
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let output = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command",
        "New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
        .env("OFFERTRACK_TEST_LINK", link).env("OFFERTRACK_TEST_TARGET", target).output().unwrap();
    assert!(output.status.success());
}

#[cfg(windows)]
#[test]
fn ancestor_and_nested_junctions_never_delete_external_content() {
    let temp = tempfile::tempdir().unwrap();
    let session = warehouse::create(temp.path()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep"), b"outside").unwrap();
    let (_, path) = recycled(&session);
    junction(&path.join("linked"), outside.path());
    let (confirmation, challenge) = prepare(&session).unwrap();
    let result = purge(&session, confirmation, &challenge.confirmation_token).unwrap();
    assert_eq!(result.failed.len(), 1);
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
    let base = session.root().join("recycle-bin/backups");
    fs::rename(&base, session.root().join("held-backups")).unwrap();
    junction(&base, outside.path());
    assert!(prepare(&session).is_err());
    assert!(outside.path().join("keep").exists());
}
