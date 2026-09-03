use super::*;
use crate::{
    applications,
    domain::CreateApplicationRequest,
    warehouse::{self, WarehouseAccessMode},
};
use serde_json::json;
use std::fs;

fn fixture() -> (tempfile::TempDir, WarehouseSession, String) {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let record = applications::create(
        &mut session,
        CreateApplicationRequest {
            company_name: "快照测试公司".into(),
            position_name: "研发".into(),
            company_type: "private".into(),
            industry: "".into(),
            position_category: "".into(),
            work_location: "".into(),
        },
    )
    .unwrap();
    (root, session, record.record.id)
}
fn generations(s: &WarehouseSession) -> usize {
    fs::read_dir(s.root().join("agent-access")).unwrap().count()
}

#[test]
fn checkpoint_not_wall_clock_selects_current_generation_and_database_restore_rebuilds_it() {
    let (_temp, mut s, id) = fixture();
    let mut future_data = collect(&s).unwrap();
    future_data.generated_at_utc = "2099-01-01T00:00:00Z".into();
    let future = snapshot::create_from_data(&s, future_data).unwrap().info;
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET notes='newer content despite older clock' WHERE id=?1",
            [id],
        )
        .unwrap();
    let current = check(&s, true);
    assert_eq!(current.state, "current");
    assert!(current.snapshot.as_ref().unwrap().generated_at_utc < future.generated_at_utc);
    assert_ne!(
        current.snapshot.as_ref().unwrap().relative_path,
        future.relative_path
    );
    assert_eq!(
        check(&s, false).snapshot.unwrap().relative_path,
        current.snapshot.unwrap().relative_path
    );
    let backup = crate::database_backup::create(&s).unwrap().backup;
    let parent = tempfile::tempdir().unwrap();
    let restored = crate::database_backup::restore(
        &s,
        &backup.id.to_string(),
        false,
        &backup.sha256,
        parent.path(),
    )
    .unwrap();
    let target = std::path::Path::new(&restored.directory);
    let read = warehouse::open(target, WarehouseAccessMode::ReadOnly).unwrap();
    let stale = check(&read, false);
    assert_eq!(stale.state, "stale");
    assert!(stale.snapshot.is_none()); // old warehouse identity/path cannot be advertised as current
    assert_eq!(generations(&read), 0);
    drop(read);
    let restored = warehouse::open(target, WarehouseAccessMode::Write).unwrap();
    assert_ne!(restored.summary().warehouse_id, s.summary().warehouse_id);
    assert_eq!(check(&restored, true).state, "current");
    assert_eq!(generations(&restored), 1);
    assert_eq!(generations(&s), 2);
}

#[test]
fn automatic_refresh_tracks_content_not_clock_or_settings_and_survives_reopen() {
    let (_temp, mut s, id) = fixture();
    let before = check(&s, false);
    assert_eq!(before.state, "missing");
    assert_eq!(generations(&s), 0);
    let first = check(&s, true);
    assert!(first.published);
    assert_eq!(first.state, "current");
    let first_info = first.snapshot.unwrap();
    assert_eq!(check(&s, false).state, "current");
    crate::agent_write::settings::set(&mut s, true, 0).unwrap();
    applications::scan_all_documents(&mut s).unwrap();
    let second = check(&s, true);
    assert!(!second.published);
    assert_eq!(
        second.snapshot.unwrap().relative_path,
        first_info.relative_path
    );
    assert_eq!(generations(&s), 1);
    assert_eq!(crate::database_backup::catalog(&s).unwrap().items.len(), 0);
    applications::set_archived(&mut s, &id, true).unwrap();
    assert_eq!(check(&s, false).state, "stale");
    let third = check(&s, true);
    assert!(third.published);
    assert_eq!(third.state, "current");
    assert_eq!(generations(&s), 2);
    assert!(s.root().join(first_info.relative_path).is_dir());
    let path = s.root().to_owned();
    drop(s);
    let read = warehouse::open(&path, WarehouseAccessMode::ReadOnly).unwrap();
    let changes = read.connection().total_changes();
    let result = check(&read, true); // a caller cannot turn read-only into write permission
    assert_eq!(result.state, "current");
    assert!(!result.published);
    assert_eq!(read.connection().total_changes(), changes);
    assert_eq!(generations(&read), 2);
}

#[test]
fn stale_readonly_and_query_never_publish_and_failed_checks_preserve_last_generation() {
    let (_temp, mut s, id) = fixture();
    let first = check(&s, true).snapshot.unwrap();
    let old = fs::read(
        s.root()
            .join(&first.relative_path)
            .join("applications.jsonl"),
    )
    .unwrap();
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET notes='完整变更的长备注' WHERE id=?1",
            [&id],
        )
        .unwrap();
    let read = warehouse::open(s.root(), WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(check(&read, true).state, "stale");
    let queried = super::super::query(
        &s,
        super::super::Request {
            version: 1,
            operation: super::super::Operation::SnapshotStatus {},
        },
    )
    .unwrap();
    assert_eq!(queried["state"], "stale");
    assert_eq!(generations(&s), 1);
    assert!(
        queried["snapshot"]["relative_path"]
            .as_str()
            .unwrap()
            .starts_with("agent-access/")
    );
    drop(read);
    fs::rename(
        s.root().join("agent-access"),
        s.root().join("saved-agent-access"),
    )
    .unwrap();
    fs::write(s.root().join("agent-access"), b"occupied, do not replace").unwrap();
    let failed = check(&s, true);
    assert_eq!(failed.state, "error");
    assert!(!failed.published);
    assert_eq!(failed.snapshot.unwrap().relative_path, first.relative_path);
    assert_eq!(
        fs::read(s.root().join("agent-access")).unwrap(),
        b"occupied, do not replace"
    );
    let name = first.relative_path.strip_prefix("agent-access/").unwrap();
    assert_eq!(
        fs::read(
            s.root()
                .join("saved-agent-access")
                .join(name)
                .join("applications.jsonl")
        )
        .unwrap(),
        old
    );
    assert_eq!(
        applications::get(&s, &id).unwrap().record.notes,
        "完整变更的长备注"
    );
}

#[test]
fn tampered_generation_is_detected_then_replaced_by_new_directory_never_overwritten() {
    let (_temp, s, _) = fixture();
    let first = check(&s, true).snapshot.unwrap();
    let original = s.root().join(&first.relative_path);
    fs::write(original.join("tasks.jsonl"), b"corrupt-private-data").unwrap();
    let stale = check(&s, false);
    assert_eq!(stale.state, "stale");
    assert!(stale.error.is_some());
    let second = check(&s, true);
    assert_eq!(second.state, "current");
    assert!(second.published);
    assert_ne!(second.snapshot.unwrap().relative_path, first.relative_path);
    assert_eq!(
        fs::read(original.join("tasks.jsonl")).unwrap(),
        b"corrupt-private-data"
    );
    assert_eq!(generations(&s), 2);
}

#[test]
fn checkpoint_failure_reports_published_files_without_claiming_synced() {
    let (_temp, s, _) = fixture();
    s.connection().execute_batch("CREATE TRIGGER fail_checkpoint BEFORE INSERT ON settings WHEN NEW.key='agent_snapshot_v1' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    let report = check(&s, true);
    assert!(report.published);
    assert_eq!(report.state, "error");
    assert!(report.error.is_some());
    assert!(!report.warnings.is_empty());
    assert!(
        s.root()
            .join(report.snapshot.unwrap().relative_path)
            .join("manifest.json")
            .is_file()
    );
    assert!(load(&s).unwrap().is_none());
    s.connection()
        .execute_batch("DROP TRIGGER fail_checkpoint;")
        .unwrap();
    assert_eq!(check(&s, true).state, "current");
    assert_eq!(generations(&s), 2);
}

#[test]
fn corrupt_future_or_unsafe_checkpoint_is_not_overwritten_or_followed() {
    let (_temp, s, _) = fixture();
    check(&s, true);
    let original = serde_json::to_value(load(&s).unwrap().unwrap()).unwrap();
    for changed in [
        json!({"version":9}),
        json!("../../outside"),
        json!("agent-access/snapshot-\u{4e2d}"),
        json!("agent-access/snapshot-../outside"),
    ] {
        let mut value = original.clone();
        if changed.is_object() {
            value["version"] = json!(9);
        } else {
            value["snapshot"]["relative_path"] = changed;
        }
        s.connection()
            .execute(
                "UPDATE settings SET value_json=?1 WHERE key=?2",
                params![value.to_string(), KEY],
            )
            .unwrap();
        assert_eq!(check(&s, true).state, "error");
        assert_eq!(generations(&s), 1);
        let stored: String = s
            .connection()
            .query_row("SELECT value_json FROM settings WHERE key=?1", [KEY], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored, value.to_string());
    }
}

#[test]
fn external_document_index_changes_refresh_but_identical_scan_does_not() {
    let (_temp, mut s, id) = fixture();
    let folder = s.root().join(
        applications::get(&s, &id)
            .unwrap()
            .record
            .folder_relative_path,
    );
    fs::write(folder.join("简历.pdf"), b"synthetic").unwrap();
    applications::scan_all_documents(&mut s).unwrap();
    assert_eq!(check(&s, true).state, "current");
    applications::scan_all_documents(&mut s).unwrap();
    assert!(!check(&s, true).published);
    fs::rename(folder.join("简历.pdf"), folder.join("新简历.pdf")).unwrap();
    // No hidden scans: unindexed filesystem changes alone do not pretend to update data.
    assert_eq!(check(&s, false).state, "current");
    applications::scan_all_documents(&mut s).unwrap();
    assert_eq!(check(&s, false).state, "stale");
    assert!(check(&s, true).published);
    assert_eq!(generations(&s), 2);
}

#[cfg(windows)]
#[test]
fn generation_junction_is_not_followed_and_external_content_survives_refresh() {
    let (_temp, s, _) = fixture();
    let first = check(&s, true).snapshot.unwrap();
    let generation = s.root().join(&first.relative_path);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("sentinel"), b"unchanged").unwrap();
    fs::rename(&generation, s.root().join("saved-generation")).unwrap();
    let status = std::process::Command::new("powershell.exe").args(["-NoProfile","-Command","New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
        .env("OFFERTRACK_TEST_LINK",&generation).env("OFFERTRACK_TEST_TARGET",outside.path()).output().unwrap();
    assert!(status.status.success());
    let stale = check(&s, false);
    assert_eq!(stale.state, "stale");
    assert_eq!(stale.error.unwrap().code, "UNSAFE_PATH_REJECTED");
    let next = check(&s, true);
    assert_eq!(next.state, "current");
    assert_ne!(next.snapshot.unwrap().relative_path, first.relative_path);
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
}
