use super::*;
use crate::{applications, domain::CreateApplicationRequest, warehouse::WarehouseAccessMode};

#[test]
fn external_snapshot_recovers_without_source_database_and_preserves_other_warehouses() {
    let source = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let export = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(source.path()).unwrap();
    let id = add_record(&mut session, "独立快照测试");
    let backup = create(&session).unwrap().backup;
    let old_dir = session
        .root()
        .join("backups/database")
        .join(backup.id.to_string());
    let directory = export.path().join("随意命名的快照副本");
    fs::create_dir(&directory).unwrap();
    fs::copy(old_dir.join(MANIFEST), directory.join(MANIFEST)).unwrap();
    fs::copy(old_dir.join(DATABASE), directory.join(DATABASE)).unwrap();
    drop(session);
    fs::write(source.path().join("offertrack.sqlite"), b"damaged source").unwrap();
    let preview = preview_external(&directory).unwrap();
    assert_eq!(preview.preview.application_count, 1);
    let active_dir = tempfile::tempdir().unwrap();
    let active = warehouse::create(active_dir.path()).unwrap();
    assert!(matches!(
        restore_external(
            &directory,
            active.root(),
            &preview.fingerprint,
            Some(active.root())
        ),
        Err(CoreError::UnsafePath)
    ));
    assert!(matches!(
        restore_external(&directory, &directory, &preview.fingerprint, None),
        Err(CoreError::UnsafePath)
    ));
    fs::write(destination.path().join("keep"), b"untouched").unwrap();
    let restored = restore_external(
        &directory,
        destination.path(),
        &preview.fingerprint,
        Some(active.root()),
    )
    .unwrap();
    let reader = warehouse::open(
        Path::new(&restored.directory),
        WarehouseAccessMode::ReadOnly,
    )
    .unwrap();
    assert_ne!(reader.summary().warehouse_id, backup.warehouse_id);
    assert_eq!(
        applications::get(&reader, &id).unwrap().record.company_name,
        "独立快照测试"
    );
    assert_eq!(
        fs::read_dir(Path::new(&restored.directory).join("applications"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read(destination.path().join("keep")).unwrap(),
        b"untouched"
    );
    assert_eq!(
        preview_external(&directory).unwrap().fingerprint,
        preview.fingerprint
    );
}

#[test]
fn external_snapshot_rejects_manifest_changes_sidecars_corruption_and_standalone_sqlite() {
    let root = tempfile::tempdir().unwrap();
    let session = warehouse::create(root.path()).unwrap();
    let backup = create(&session).unwrap().backup;
    let directory = session
        .root()
        .join("backups/database")
        .join(backup.id.to_string());
    let reviewed = preview_external(&directory).unwrap();
    let destination = tempfile::tempdir().unwrap();
    let mut modified = backup.clone();
    modified.reason = "beforeBatch".into();
    rewrite_manifest(&session, &modified);
    assert!(matches!(
        restore_external(&directory, destination.path(), &reviewed.fingerprint, None),
        Err(CoreError::RevisionConflict)
    ));
    fs::write(directory.join("database.sqlite-wal"), b"untrusted").unwrap();
    assert!(preview_external(&directory).is_err());
    assert!(preview_external(&directory.join(DATABASE)).is_err());
    assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
    let other = tempfile::tempdir().unwrap();
    fs::copy(directory.join(DATABASE), other.path().join(DATABASE)).unwrap();
    assert!(preview_external(other.path()).is_err());
    fs::copy(directory.join(MANIFEST), other.path().join(MANIFEST)).unwrap();
    fs::write(other.path().join(DATABASE), b"bad bytes").unwrap();
    assert!(matches!(
        preview_external(other.path()),
        Err(CoreError::BackupInvalid)
    ));
}

#[test]
fn external_restore_upgrades_copy_without_changing_old_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    add_record(&mut session, "旧格式测试");
    downgrade_fixture_to_six(&mut session);
    let backup = create(&session).unwrap().backup;
    let directory = session
        .root()
        .join("backups/database")
        .join(backup.id.to_string());
    drop(session);
    let preview = preview_external(&directory).unwrap();
    assert_eq!(preview.preview.backup.schema_version, 6);
    let restored = restore_external(&directory, target.path(), &preview.fingerprint, None).unwrap();
    let database =
        read_database(&Path::new(&restored.directory).join("offertrack.sqlite")).unwrap();
    assert_eq!(
        check_database(&database).unwrap(),
        migrations::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        preview_external(&directory).unwrap().fingerprint,
        preview.fingerprint
    );
}

fn add_record(session: &mut WarehouseSession, name: &str) -> String {
    applications::create(
        session,
        CreateApplicationRequest {
            company_name: name.into(),
            position_name: "测试岗位".into(),
            company_type: "private".into(),
            industry: "软件".into(),
            position_category: "研发".into(),
            work_location: "厦门".into(),
        },
    )
    .unwrap()
    .record
    .id
}
fn manifest_path(session: &WarehouseSession, id: Uuid) -> PathBuf {
    session
        .root()
        .join("backups/database")
        .join(id.to_string())
        .join(MANIFEST)
}
fn rewrite_manifest(session: &WarehouseSession, backup: &DatabaseBackup) {
    fs::write(
        manifest_path(session, backup.id),
        serde_json::to_vec(backup).unwrap(),
    )
    .unwrap();
}

#[test]
fn wal_snapshot_restore_is_independent_and_never_copies_or_overwrites_attachments() {
    let source = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    fs::write(destination.path().join("keep.txt"), b"existing content").unwrap();
    let mut session = warehouse::create(source.path()).unwrap();
    let id = add_record(&mut session, "备份中的记录");
    let record = applications::get(&session, &id).unwrap();
    let attachment = source
        .path()
        .join(&record.record.folder_relative_path)
        .join("resume.pdf");
    fs::write(&attachment, b"synthetic resume").unwrap();
    applications::scan_documents(&mut session, &id).unwrap();
    let backup = create(&session).unwrap().backup;
    add_record(&mut session, "备份后新增");
    let snapshot = preview(&session, &backup.id.to_string(), false).unwrap();
    assert_eq!(snapshot.application_count, 1);
    assert_eq!(snapshot.document_count, 1);
    let original_id = session.summary().warehouse_id;
    let reader = warehouse::open(source.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert!(matches!(create(&reader), Err(CoreError::ReadOnlyWarehouse)));
    let result = restore(
        &reader,
        &backup.id.to_string(),
        false,
        &backup.sha256,
        destination.path(),
    )
    .unwrap();
    let mut recovered =
        warehouse::open(Path::new(&result.directory), WarehouseAccessMode::Write).unwrap();
    assert_ne!(recovered.summary().warehouse_id, original_id);
    assert_eq!(
        applications::list(&recovered, crate::domain::ApplicationScope::Active)
            .unwrap()
            .len(),
        1
    );
    assert!(
        !Path::new(&result.directory)
            .join(&record.record.folder_relative_path)
            .exists()
    );
    let documents = applications::scan_documents(&mut recovered, &id).unwrap();
    assert!(documents[0].missing);
    assert_eq!(fs::read(&attachment).unwrap(), b"synthetic resume");
    assert_eq!(
        fs::read(destination.path().join("keep.txt")).unwrap(),
        b"existing content"
    );
    assert_eq!(
        applications::list(&session, crate::domain::ApplicationScope::Active)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn rejects_corrupt_missing_incompatible_and_hash_changed_backups_before_restore() {
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    add_record(&mut session, "保留原数据");
    let mut backup = create(&session).unwrap().backup;
    assert!(matches!(
        restore(
            &session,
            &backup.id.to_string(),
            false,
            "stale",
            target.path()
        ),
        Err(CoreError::RevisionConflict)
    ));
    backup.version = 999;
    rewrite_manifest(&session, &backup);
    assert!(matches!(
        preview(&session, &backup.id.to_string(), false),
        Err(CoreError::BackupInvalid)
    ));
    backup.version = VERSION;
    rewrite_manifest(&session, &backup);
    let database = manifest_path(&session, backup.id).with_file_name(DATABASE);
    fs::rename(&database, root.path().join("original-backup.sqlite")).unwrap();
    assert!(preview(&session, &backup.id.to_string(), false).is_err());
    fs::write(&database, b"not sqlite").unwrap();
    assert!(matches!(
        preview(&session, &backup.id.to_string(), false),
        Err(CoreError::BackupInvalid)
    ));
    (backup.size_bytes, backup.sha256) = hash(&mut File::open(&database).unwrap()).unwrap();
    rewrite_manifest(&session, &backup);
    assert!(matches!(
        restore(
            &session,
            &backup.id.to_string(),
            false,
            &backup.sha256,
            target.path()
        ),
        Err(CoreError::BackupInvalid)
    ));
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    assert_eq!(
        applications::list(&session, crate::domain::ApplicationScope::Active)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn rejects_arbitrary_ids_and_restore_inside_current_warehouse() {
    let root = tempfile::tempdir().unwrap();
    let session = warehouse::create(root.path()).unwrap();
    let backup = create(&session).unwrap().backup;
    for id in ["../applications", "recycle-bin", "C:/outside", ""] {
        assert!(matches!(
            preview(&session, id, false),
            Err(CoreError::UnsafePath)
        ));
    }
    for parent in [root.path().to_path_buf(), root.path().join("applications")] {
        assert!(matches!(
            restore(
                &session,
                &backup.id.to_string(),
                false,
                &backup.sha256,
                &parent
            ),
            Err(CoreError::UnsafePath)
        ));
    }
    assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".offertrack-restoring")
    }));
}

#[test]
fn daily_backup_is_once_per_local_day_across_reopen_and_rotation_retains_recoverable_files() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    add_record(&mut session, "每日快照");
    ensure_daily(&mut session).unwrap();
    ensure_daily(&mut session).unwrap();
    assert_eq!(catalog(&session).unwrap().items.len(), 1);
    drop(session);
    let session = warehouse::open(root.path(), WarehouseAccessMode::Write).unwrap();
    assert_eq!(catalog(&session).unwrap().items.len(), 1);
    let manual = create(&session).unwrap().backup;
    for day in 1..=31 {
        let mut item = create_at(
            session.connection(),
            session.root(),
            session.summary().warehouse_id,
            "daily",
        )
        .unwrap();
        item.local_date = format!("2025-01-{day:02}");
        item.created_at_utc = format!("2025-01-{day:02}T00:00:00Z");
        rewrite_manifest(&session, &item);
    }
    rotate(&session).unwrap();
    let items = catalog(&session).unwrap().items;
    assert!(
        items
            .iter()
            .any(|item| item.backup.id == manual.id && !item.recycled)
    );
    let recycled: Vec<_> = items.iter().filter(|item| item.recycled).collect();
    assert_eq!(recycled.len(), 2);
    assert_eq!(
        preview(&session, &recycled[0].backup.id.to_string(), true)
            .unwrap()
            .application_count,
        1
    );
    assert_eq!(items.len(), 33);
    // Monthly representatives survive outside the latest 30 daily snapshots.
    let mut older = Vec::new();
    for month in 1..=12 {
        let mut item = create_at(
            session.connection(),
            session.root(),
            session.summary().warehouse_id,
            "daily",
        )
        .unwrap();
        item.local_date = format!("2024-{month:02}-15");
        item.created_at_utc = format!("2024-{month:02}-15T00:00:00Z");
        rewrite_manifest(&session, &item);
        older.push(item.id);
    }
    rotate(&session).unwrap();
    let items = catalog(&session).unwrap().items;
    for (index, id) in older.into_iter().enumerate() {
        // Today and January 2025 take two monthly slots, leaving March–December 2024.
        assert_eq!(
            items
                .iter()
                .find(|item| item.backup.id == id)
                .unwrap()
                .recycled,
            index < 2
        );
    }
}

#[test]
fn incomplete_directories_are_reported_not_published_or_deleted() {
    let root = tempfile::tempdir().unwrap();
    let session = warehouse::create(root.path()).unwrap();
    let pending = root.path().join("backups/database/.pending-synthetic");
    fs::create_dir(&pending).unwrap();
    fs::write(pending.join("partial"), b"keep").unwrap();
    let invalid = root
        .path()
        .join("backups/database")
        .join(Uuid::new_v4().to_string());
    fs::create_dir(&invalid).unwrap();
    fs::write(invalid.join(MANIFEST), b"broken").unwrap();
    let catalog = catalog(&session).unwrap();
    assert!(catalog.items.is_empty());
    assert_eq!(catalog.incomplete_count, 1);
    assert_eq!(catalog.invalid_count, 1);
    assert_eq!(fs::read(pending.join("partial")).unwrap(), b"keep");
}

#[test]
fn pending_file_journals_cannot_be_silently_discarded_by_database_restore() {
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    session.connection_mut().unwrap().execute("INSERT INTO record_creations(application_id,target_relative_path,state,created_at_utc) VALUES ('pending','applications/synthetic','copying','2026-09-03T00:00:00Z')", []).unwrap();
    let backup = create(&session).unwrap().backup;
    assert!(matches!(
        restore(
            &session,
            &backup.id.to_string(),
            false,
            &backup.sha256,
            target.path()
        ),
        Err(CoreError::BackupPendingOperations)
    ));
    assert!(manifest_path(&session, backup.id).exists());
    let directory = manifest_path(&session, backup.id)
        .parent()
        .unwrap()
        .to_owned();
    assert!(matches!(
        preview_external(&directory),
        Err(CoreError::BackupPendingOperations)
    ));
    let source = external_source(&directory).unwrap();
    assert!(matches!(
        restore_external(
            &directory,
            target.path(),
            &fingerprint(&source).unwrap(),
            None
        ),
        Err(CoreError::BackupPendingOperations)
    ));
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn junction_backup_and_restore_parent_are_rejected_without_following_targets() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep.txt"), b"protected").unwrap();
    let session = warehouse::create(root.path()).unwrap();
    let backup = create(&session).unwrap().backup;
    let junction = root
        .path()
        .join("backups/database")
        .join(Uuid::new_v4().to_string());
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
        .env("OFFERTRACK_TEST_LINK", &junction)
        .env("OFFERTRACK_TEST_TARGET", outside.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(matches!(
        preview_external(&junction),
        Err(CoreError::UnsafePath)
    ));
    let directory = manifest_path(&session, backup.id)
        .parent()
        .unwrap()
        .to_owned();
    let verified = preview_external(&directory).unwrap();
    assert!(matches!(
        restore_external(&directory, &junction, &verified.fingerprint, None),
        Err(CoreError::UnsafePath)
    ));
    assert!(matches!(
        preview(
            &session,
            junction.file_name().unwrap().to_str().unwrap(),
            false
        ),
        Err(CoreError::UnsafePath)
    ));
    assert!(matches!(
        restore(
            &session,
            &backup.id.to_string(),
            false,
            &backup.sha256,
            &junction
        ),
        Err(CoreError::UnsafePath)
    ));
    assert_eq!(
        fs::read(outside.path().join("keep.txt")).unwrap(),
        b"protected"
    );
    // Remove only this synthetic junction itself; never traverse its target.
    fs::remove_dir(&junction).unwrap();
}

fn downgrade_fixture_to_six(session: &mut WarehouseSession) {
    migrations::fixture_remove_migration_eight(session.connection());
    // Reverse migration 7 after 8, producing the actual historical schema.
    session
        .connection_mut()
        .unwrap()
        .execute_batch(
            "DROP INDEX idx_views_default_per_kind;
         ALTER TABLE views DROP COLUMN revision;
         ALTER TABLE field_definitions DROP COLUMN revision;
         DELETE FROM schema_migrations WHERE version=7;",
        )
        .unwrap();
}

#[test]
fn upgrade_keeps_a_restorable_old_schema_snapshot_and_backup_failure_blocks_migration() {
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    add_record(&mut session, "升级前的记录");
    downgrade_fixture_to_six(&mut session);
    drop(session);
    let session = warehouse::open(root.path(), WarehouseAccessMode::Write).unwrap();
    let old = catalog(&session)
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.backup.reason == "beforeUpgrade")
        .unwrap()
        .backup;
    assert_eq!(old.schema_version, 6);
    assert_eq!(
        schema_version(session.connection()).unwrap(),
        migrations::CURRENT_SCHEMA_VERSION
    );
    let result = restore(
        &session,
        &old.id.to_string(),
        false,
        &old.sha256,
        target.path(),
    )
    .unwrap();
    let restored =
        warehouse::open(Path::new(&result.directory), WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(
        schema_version(restored.connection()).unwrap(),
        migrations::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        applications::list(&restored, crate::domain::ApplicationScope::Active)
            .unwrap()
            .len(),
        1
    );

    let blocked_root = tempfile::tempdir().unwrap();
    let mut blocked = warehouse::create(blocked_root.path()).unwrap();
    downgrade_fixture_to_six(&mut blocked);
    drop(blocked);
    fs::rename(
        blocked_root.path().join("backups/database"),
        blocked_root.path().join("backups/held"),
    )
    .unwrap();
    fs::write(
        blocked_root.path().join("backups/database"),
        b"do not overwrite",
    )
    .unwrap();
    assert!(warehouse::open(blocked_root.path(), WarehouseAccessMode::Write).is_err());
    let database = Connection::open(blocked_root.path().join("offertrack.sqlite")).unwrap();
    assert_eq!(schema_version(&database).unwrap(), 6);
    assert_eq!(
        fs::read(blocked_root.path().join("backups/database")).unwrap(),
        b"do not overwrite"
    );
}

#[test]
fn failed_snapshot_is_retained_as_pending_and_unverified_sidecars_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let backup = create(&session).unwrap().backup;
    fs::write(
        manifest_path(&session, backup.id).with_file_name("database.sqlite-wal"),
        b"unverified",
    )
    .unwrap();
    assert!(matches!(
        preview(&session, &backup.id.to_string(), false),
        Err(CoreError::BackupInvalid)
    ));
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE schema_migrations SET version=999 WHERE version=7",
            [],
        )
        .unwrap();
    assert!(matches!(create(&session), Err(CoreError::BackupInvalid)));
    let result = catalog(&session).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.incomplete_count, 1);
    assert_eq!(schema_version(session.connection()).unwrap(), 999);
}
