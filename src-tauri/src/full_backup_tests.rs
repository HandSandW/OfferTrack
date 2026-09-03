use super::*;
use crate::{
    applications,
    domain::{ApplicationScope, CreateApplicationRequest},
    recycle_bin,
    warehouse::WarehouseAccessMode,
};

fn add_record(session: &mut WarehouseSession, name: &str) -> (String, String) {
    let record = applications::create(
        session,
        CreateApplicationRequest {
            company_name: name.into(),
            position_name: "测试岗位".into(),
            company_type: "private".into(),
            industry: String::new(),
            position_category: String::new(),
            work_location: String::new(),
        },
    )
    .unwrap()
    .record;
    (record.id, record.folder_relative_path)
}

fn rewrite_package(path: &Path, manifest: &Manifest, payload: &[u8]) {
    // Deliberately bypass the production writer to test untrusted packages.
    let json = serde_json::to_vec(manifest).unwrap();
    let mut bytes = backup_archive::MAGIC.to_vec();
    bytes.extend_from_slice(&(json.len() as u64).to_le_bytes());
    bytes.extend(json);
    bytes.extend_from_slice(payload);
    fs::write(path, bytes).unwrap();
}

#[test]
fn empty_warehouse_round_trip_preserves_identity() {
    let source = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let session = warehouse::create(source.path()).unwrap();
    let backup = create(&session, destination.path(), true).unwrap();
    let result = restore(
        Path::new(&backup.path),
        destination.path(),
        &backup.preview.sha256,
        Some(session.root()),
    )
    .unwrap();
    let restored = warehouse::open(
        Path::new(&result.directory),
        warehouse::WarehouseAccessMode::ReadOnly,
    )
    .unwrap();
    assert_eq!(
        restored.summary().warehouse_id,
        session.summary().warehouse_id
    );
}

#[test]
fn full_round_trip_recovers_files_empty_hidden_unlinked_folders_and_trash_without_source_database()
{
    let source = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(source.path()).unwrap();
    let (id, folder) = add_record(&mut session, "活跃投递");
    fs::create_dir_all(source.path().join(&folder).join("子目录/空目录")).unwrap();
    let content = vec![0x41; 150_001];
    fs::write(
        source.path().join(&folder).join("子目录/resume.pdf"),
        &content,
    )
    .unwrap();
    applications::scan_documents(&mut session, &id).unwrap();
    let (deleted_id, deleted_folder) = add_record(&mut session, "已删除投递");
    fs::write(
        source.path().join(&deleted_folder).join("resume.docx"),
        b"trash resume",
    )
    .unwrap();
    recycle_bin::move_application_to_trash(&mut session, &deleted_id).unwrap();
    fs::create_dir_all(source.path().join("applications/.unlinked/empty")).unwrap();
    fs::write(
        source.path().join("agent-access/config.json"),
        b"synthetic configuration",
    )
    .unwrap();
    fs::write(source.path().join("notes.txt"), b"root note").unwrap();
    let backup = create(&session, destination.path(), true).unwrap();
    let original_id = session.summary().warehouse_id;
    drop(session);
    fs::rename(
        source.path().join("offertrack.sqlite"),
        source.path().join("original-database.held"),
    )
    .unwrap();
    fs::write(
        source.path().join("offertrack.sqlite"),
        b"broken source database",
    )
    .unwrap();
    let checked = preview(Path::new(&backup.path)).unwrap();
    let result = restore(
        Path::new(&backup.path),
        destination.path(),
        &checked.sha256,
        None,
    )
    .unwrap();
    let root = Path::new(&result.directory);
    let mut restored = warehouse::open(root, WarehouseAccessMode::Write).unwrap();
    assert_eq!(restored.summary().warehouse_id, original_id);
    assert_eq!(
        applications::list(&restored, ApplicationScope::Active)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fs::read(root.join(&folder).join("子目录/resume.pdf")).unwrap(),
        content
    );
    assert!(root.join(&folder).join("子目录/空目录").is_dir());
    assert!(root.join("applications/.unlinked/empty").is_dir());
    assert_eq!(
        fs::read(root.join("agent-access/config.json")).unwrap(),
        b"synthetic configuration"
    );
    assert_eq!(fs::read(root.join("notes.txt")).unwrap(), b"root note");
    assert!(!db::catalog(&restored).unwrap().items.is_empty());
    let recovered = recycle_bin::restore_application(&mut restored, &deleted_id).unwrap();
    assert_eq!(
        fs::read(
            root.join(recovered.folder_relative_path)
                .join("resume.docx")
        )
        .unwrap(),
        b"trash resume"
    );
    fs::write(
        root.join(&folder).join("子目录/resume.pdf"),
        b"independent copy",
    )
    .unwrap();
    assert_eq!(
        fs::read(source.path().join(folder).join("子目录/resume.pdf")).unwrap(),
        content
    );
    assert_eq!(
        fs::read(source.path().join("offertrack.sqlite")).unwrap(),
        b"broken source database"
    );
}

#[test]
fn excluding_trash_keeps_deleted_metadata_but_never_copies_or_deletes_trash_files() {
    let source = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(source.path()).unwrap();
    let (id, folder) = add_record(&mut session, "回收站投递");
    fs::write(
        source.path().join(folder).join("resume.pdf"),
        b"retain at source",
    )
    .unwrap();
    recycle_bin::move_application_to_trash(&mut session, &id).unwrap();
    let before = inventory(session.root(), true).unwrap();
    let backup = create(&session, destination.path(), false).unwrap();
    assert!(!backup.preview.includes_recycle_bin);
    let result = restore(
        Path::new(&backup.path),
        destination.path(),
        &backup.preview.sha256,
        Some(session.root()),
    )
    .unwrap();
    let restored =
        warehouse::open(Path::new(&result.directory), WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(recycle_bin::list(&restored).unwrap().len(), 1);
    assert_eq!(
        fs::read_dir(Path::new(&result.directory).join("recycle-bin/records"))
            .unwrap()
            .count(),
        0
    );
    let after = inventory(session.root(), true).unwrap();
    for entry in before
        .iter()
        .filter(|entry| entry.path.starts_with("recycle-bin/"))
    {
        assert!(after.contains(entry));
    }
}

#[test]
fn migration_keeps_original_lock_and_files_plus_a_valid_full_and_pre_migration_backup() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fs::write(target.path().join("keep.txt"), b"keep").unwrap();
    let mut session = warehouse::create(source.path()).unwrap();
    let (id, folder) = add_record(&mut session, "迁移投递");
    fs::write(source.path().join(&folder).join("resume.pdf"), b"original").unwrap();
    let result = migrate(&session, target.path()).unwrap();
    assert!(preview(Path::new(result.migration_backup_path.as_ref().unwrap())).is_ok());
    assert!(
        db::catalog(&session)
            .unwrap()
            .items
            .iter()
            .any(|item| item.backup.reason == "beforeMigration")
    );
    assert!(matches!(
        warehouse::open(source.path(), WarehouseAccessMode::Write),
        Err(CoreError::WarehouseLocked)
    ));
    let migrated =
        warehouse::open(Path::new(&result.directory), WarehouseAccessMode::Write).unwrap();
    assert_eq!(
        applications::get(&migrated, &id)
            .unwrap()
            .record
            .company_name,
        "迁移投递"
    );
    assert_eq!(
        fs::read(source.path().join(folder).join("resume.pdf")).unwrap(),
        b"original"
    );
    assert_eq!(fs::read(target.path().join("keep.txt")).unwrap(), b"keep");
}

#[test]
fn rejects_corruption_truncation_extra_bytes_versions_duplicate_and_unsafe_archive_paths_before_writing()
 {
    let source = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let session = warehouse::create(source.path()).unwrap();
    let created = create(&session, output.path(), true).unwrap();
    let original = fs::read(&created.path).unwrap();
    let verified = backup_archive::verify(&mut File::open(&created.path).unwrap()).unwrap();
    let payload = &original[verified.payload_offset as usize..];
    let bad = output.path().join("bad.offertrack-backup");
    for path in [
        "../outside",
        "/absolute",
        "C:/absolute",
        "a\\..\\outside",
        "folder/name:stream",
        "NUL.txt",
        "x/COM¹.pdf",
        "applications/.",
        "applications//bad",
        "applications/name.",
        "applications/name ",
    ] {
        let mut manifest = verified.manifest.clone();
        manifest.entries[0].path = path.into();
        rewrite_package(&bad, &manifest, payload);
        assert!(
            restore(
                &bad,
                target.path(),
                &created.preview.sha256,
                Some(session.root())
            )
            .is_err(),
            "{path}"
        );
    }
    for change in 0..5 {
        let mut manifest = verified.manifest.clone();
        match change {
            0 => manifest.version = 999,
            1 => manifest.schema_version = 999,
            2 => {
                let mut duplicate = manifest.entries[0].clone();
                duplicate.path = duplicate.path.to_uppercase();
                manifest.entries.push(duplicate);
            }
            3 => manifest.entries[0].path = "offertrack.sqlite-wal".into(),
            _ => manifest.entries[0].path = "warehouse.json".into(),
        }
        rewrite_package(&bad, &manifest, payload);
        assert!(preview(&bad).is_err());
    }
    for mutation in 0..3 {
        let mut bytes = original.clone();
        match mutation {
            0 => {
                let last = bytes.len() - 1;
                bytes[last] ^= 0xff;
            }
            1 => {
                bytes.pop();
            }
            _ => bytes.push(0),
        }
        fs::write(&bad, bytes).unwrap();
        assert!(preview(&bad).is_err());
    }
    fs::write(
        &bad,
        [backup_archive::MAGIC.as_slice(), &u64::MAX.to_le_bytes()].concat(),
    )
    .unwrap();
    assert!(preview(&bad).is_err());
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    assert!(matches!(
        restore(Path::new(&created.path), target.path(), "stale hash", None),
        Err(CoreError::RevisionConflict)
    ));
}

#[test]
fn invalid_database_leaves_only_an_incomplete_directory_and_no_published_warehouse() {
    use sha2::{Digest, Sha256};
    let output = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let path = output.path().join("invalid-database.offertrack-backup");
    let bytes = b"not a database";
    let manifest = Manifest {
        version: 1,
        kind: "full".into(),
        warehouse_format: 1,
        warehouse_id: Uuid::new_v4(),
        schema_version: 7,
        created_at_utc: "2026-09-03T00:00:00Z".into(),
        includes_recycle_bin: false,
        entries: vec![Entry {
            path: "offertrack.sqlite".into(),
            directory: false,
            size_bytes: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }],
    };
    rewrite_package(&path, &manifest, bytes);
    let checked = preview(&path).unwrap();
    assert!(restore(&path, target.path(), &checked.sha256, None).is_err());
    let entries: Vec<_> = fs::read_dir(target.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0]
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(".offertrack-restoring-")
    );
    assert_eq!(
        fs::read(entries[0].join("offertrack.sqlite")).unwrap(),
        bytes
    );
    assert!(path.is_file());
}

#[test]
fn read_only_inside_source_and_pending_journals_cannot_start_full_backup_or_migration() {
    let source = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(source.path()).unwrap();
    let reader = warehouse::open(source.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert!(matches!(
        create(&reader, output.path(), true),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        migrate(&reader, output.path()),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        create(&session, source.path(), true),
        Err(CoreError::UnsafePath)
    ));
    let backup = create(&session, output.path(), true).unwrap();
    assert!(matches!(
        restore(
            Path::new(&backup.path),
            &source.path().join("applications"),
            &backup.preview.sha256,
            Some(session.root())
        ),
        Err(CoreError::UnsafePath)
    ));
    session.connection_mut().unwrap().execute("INSERT INTO record_creations(application_id,target_relative_path,state,created_at_utc) VALUES ('pending','applications/synthetic','copying','2026-09-03T00:00:00Z')", []).unwrap();
    assert!(matches!(
        migrate(&session, output.path()),
        Err(CoreError::BackupPendingOperations)
    ));
}

#[cfg(windows)]
#[test]
fn occupied_source_junctions_and_publication_collisions_preserve_external_content() {
    use std::os::windows::fs::OpenOptionsExt;
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let session = warehouse::create(source.path()).unwrap();
    let file = source.path().join("applications/locked.pdf");
    fs::write(&file, b"locked").unwrap();
    let occupied = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    assert!(matches!(
        create(&session, target.path(), true),
        Err(CoreError::FileBusy)
    ));
    drop(occupied);
    let backup = create(&session, target.path(), true).unwrap();
    let junction = source.path().join("applications/junction");
    fs::write(outside.path().join("keep.txt"), b"keep").unwrap();
    let result = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
        .env("OFFERTRACK_TEST_LINK", &junction).env("OFFERTRACK_TEST_TARGET", outside.path()).output().unwrap();
    assert!(result.status.success());
    assert!(matches!(
        create(&session, target.path(), true),
        Err(CoreError::UnsafePath)
    ));
    assert!(matches!(
        restore(
            Path::new(&backup.path),
            &junction,
            &backup.preview.sha256,
            None
        ),
        Err(CoreError::UnsafePath)
    ));
    assert_eq!(fs::read(outside.path().join("keep.txt")).unwrap(), b"keep");
    fs::remove_dir(&junction).unwrap(); // Only remove the fixture link, never traverse the target.
    let pending = target.path().join("fixture.pending");
    let existing = target.path().join("existing.offertrack-backup");
    let mut file = new_output(&pending, true).unwrap();
    file.write_all(b"pending").unwrap();
    fs::write(&existing, b"existing").unwrap();
    assert!(copying::rename_handle_no_replace(&file, &existing).is_err());
    assert_eq!(fs::read(&existing).unwrap(), b"existing");
    drop(file);
    assert_eq!(fs::read(&pending).unwrap(), b"pending");
}

#[test]
fn full_restore_upgrades_the_copy_of_an_older_schema_without_upgrading_the_source() {
    let source = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(source.path()).unwrap();
    add_record(&mut session, "旧版投递");
    crate::migrations::fixture_remove_migration_eight(session.connection());
    session.connection_mut().unwrap().execute_batch("DROP INDEX idx_views_default_per_kind; ALTER TABLE views DROP COLUMN revision; ALTER TABLE field_definitions DROP COLUMN revision; DELETE FROM schema_migrations WHERE version=7;").unwrap();
    let backup = create(&session, output.path(), true).unwrap();
    assert_eq!(backup.preview.schema_version, 6);
    let result = restore(
        Path::new(&backup.path),
        output.path(),
        &backup.preview.sha256,
        None,
    )
    .unwrap();
    let restored =
        warehouse::open(Path::new(&result.directory), WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(
        db::check_database(restored.connection()).unwrap(),
        crate::migrations::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(db::check_database(session.connection()).unwrap(), 6);
}
