use super::*;
use crate::tasks;
use crate::{
    domain::{CreateApplicationRequest, FieldDefinitionRequest},
    warehouse::{self, WarehouseAccessMode},
};
use uuid::Uuid;

fn fixture() -> (tempfile::TempDir, WarehouseSession, String) {
    let temp = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(temp.path()).unwrap();
    let a = applications::create(
        &mut s,
        CreateApplicationRequest {
            company_name: "Agent 测试公司".into(),
            position_name: "研发".into(),
            company_type: "private".into(),
            industry: "".into(),
            position_category: "".into(),
            work_location: "".into(),
        },
    )
    .unwrap();
    (temp, s, a.record.id)
}
fn request(s: &WarehouseSession, actions: Value) -> Request {
    serde_json::from_value(json!({"version":1,"warehouse_id":s.summary().warehouse_id,"request_id":Uuid::new_v4(),"source":"synthetic-client","actions":actions})).unwrap()
}
fn append(s: &WarehouseSession, id: &str, text: &str) -> Request {
    request(
        s,
        json!([{"operation":"append_notes","application_id":id,"revision":applications::load_record(s.connection(),id).unwrap().revision,"text":text}]),
    )
}
fn count(s: &WarehouseSession) -> usize {
    database_backup::catalog(s).unwrap().items.len()
}
fn allow(s: &mut WarehouseSession) {
    let p = settings::get(s.connection()).unwrap();
    settings::set(s, true, p.revision).unwrap();
}

#[test]
fn committed_write_survives_snapshot_failure_and_identical_retry_only_repairs_snapshot() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let r = append(&s, &id, "只追加一次，即使派生刷新失败");
    let path = s.root().to_owned();
    std::fs::rename(path.join("agent-access"), path.join("saved-agent-access")).unwrap();
    std::fs::write(path.join("agent-access"), b"occupied").unwrap();
    drop(s);
    let first = execute(&path, &r, "cli").unwrap();
    assert_eq!(first["snapshot_status"]["state"], "error");
    assert_eq!(first["request_id"], r.request_id.to_string());
    let read = warehouse::open(&path, WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(
        applications::get(&read, &id).unwrap().record.notes,
        "只追加一次，即使派生刷新失败"
    );
    assert_eq!(count(&read), 1);
    let audit = audit_detail(&read, &r.request_id.to_string()).unwrap();
    assert!(audit["response"].get("snapshot_status").is_none());
    drop(read);
    std::fs::rename(path.join("agent-access"), path.join("saved-occupied-file")).unwrap();
    std::fs::rename(path.join("saved-agent-access"), path.join("agent-access")).unwrap();
    let second = execute(&path, &r, "mcp").unwrap();
    assert_eq!(second["snapshot_status"]["state"], "current");
    assert_eq!(second["backup_id"], first["backup_id"]);
    let third = execute(&path, &r, "cli").unwrap();
    assert_eq!(third["snapshot_status"]["published"], false);
    assert_eq!(
        third["snapshot_status"]["snapshot"],
        second["snapshot_status"]["snapshot"]
    );
    let read = warehouse::open(&path, WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(count(&read), 1);
    assert_eq!(
        applications::get(&read, &id).unwrap().record.notes,
        "只追加一次，即使派生刷新失败"
    );
    assert_eq!(
        audit_detail(&read, &r.request_id.to_string()).unwrap(),
        audit
    );
}

#[test]
fn permission_defaults_closed_persists_and_requires_desktop_revision_and_write_session() {
    let (_temp, mut s, id) = fixture();
    let r = append(&s, &id, "不应保存");
    assert!(!settings::get(s.connection()).unwrap().enabled);
    assert!(matches!(
        apply(&mut s, &r, "cli"),
        Err(CoreError::AgentWriteDisabled)
    ));
    assert_eq!(count(&s), 0);
    allow(&mut s);
    assert!(matches!(
        settings::set(&mut s, false, 0),
        Err(CoreError::RevisionConflict)
    ));
    let path = s.root().to_owned();
    drop(s);
    let mut read = warehouse::open(&path, WarehouseAccessMode::ReadOnly).unwrap();
    assert!(settings::get(read.connection()).unwrap().enabled);
    assert!(matches!(
        settings::set(&mut read, false, 1),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        apply(&mut read, &r, "cli"),
        Err(CoreError::ReadOnlyWarehouse)
    ));
}

#[test]
fn all_metadata_actions_share_backup_transaction_and_sensitive_audit_without_touching_files() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let old = applications::load_record(s.connection(), &id).unwrap();
    let field = applications::save_field_definition(
        &mut s,
        FieldDefinitionRequest {
            id: None,
            revision: None,
            display_name: "薪资".into(),
            field_type: "number".into(),
            config: json!({}),
        },
    )
    .unwrap()
    .into_iter()
    .next()
    .unwrap();
    let a = request(
        &s,
        json!([
            {"operation":"update_fields","application_id":id,"revision":old.revision,"fields":{"company_name":"新公司","notes":"完整备注","tags":["重点"],"custom_fields":{field.id.clone():123}}},
            {"operation":"create_task","application_id":id,"application_revision":old.revision,"title":"跟进","notes":"完整任务备注"},
            {"operation":"create_task","title":"通用求职事项"},
            {"operation":"create_event","application_id":id,"application_revision":old.revision,"event_type":"interview","title":"技术面","starts_at_utc":"2026-09-10T09:00:00+08:00","notes":"完整事件备注"}
        ]),
    );
    let applied = apply(&mut s, &a, "cli").unwrap();
    assert_eq!(applied.results.len(), 4);
    assert_eq!(count(&s), 1);
    let after = applications::load_record(s.connection(), &id).unwrap();
    assert_eq!(after.company_name, "新公司");
    assert_eq!(after.folder_relative_path, old.folder_relative_path);
    assert!(s.root().join(&old.folder_relative_path).is_dir());
    assert!(after.folder_normalization_pending);
    assert_eq!(after.custom_fields[&field.id], 123);
    assert_eq!(after.status_updated_at_utc, old.status_updated_at_utc);
    let preview = database_backup::preview(&s, &applied.backup_id.to_string(), false).unwrap();
    assert_eq!(preview.backup.reason, "beforeAgentWrite");
    let backup = rusqlite::Connection::open_with_flags(
        s.root()
            .join("backups/database")
            .join(applied.backup_id.to_string())
            .join("database.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        applications::load_record(&backup, &id)
            .unwrap()
            .company_name,
        "Agent 测试公司"
    );
    assert!(tasks::list(&backup).unwrap().is_empty());
    let audit = audit_detail(&s, &a.request_id.to_string()).unwrap();
    assert_eq!(audit["actor"], "agent");
    assert_eq!(audit["transport"], "cli");
    assert_eq!(audit["changes"][0]["before"]["notes"], "");
    assert_eq!(audit["changes"][0]["after"]["notes"], "完整备注");
    assert_eq!(
        audit["response"]["backup_id"],
        applied.backup_id.to_string()
    );
    assert_eq!(
        std::fs::read_dir(s.root().join("agent-access"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn stage_reuses_date_failure_and_history_semantics_with_agent_actor() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let stages = applications::load_stages(s.connection(), &id).unwrap();
    for (stage, state) in [("applied", "pending"), ("failed_terminal", "failed")] {
        let revision = applications::load_record(s.connection(), &id)
            .unwrap()
            .revision;
        let request = request(
            &s,
            json!([{"operation":"change_stage","application_id":id,"revision":revision,"stage_id":stages.iter().find(|s|s.stable_key==stage).unwrap().id,"state_key":state,"notes":"状态说明"}]),
        );
        apply(&mut s, &request, "mcp").unwrap();
    }
    let detail = applications::get(&s, &id).unwrap();
    assert!(detail.record.application_date.is_some());
    assert_eq!(detail.record.current_stage_state, "failed");
    assert_eq!(
        detail.record.current_stage_id.as_deref(),
        Some(
            stages
                .iter()
                .find(|s| s.stable_key == "applied")
                .unwrap()
                .id
                .as_str()
        )
    );
    assert_eq!(
        detail
            .history
            .iter()
            .filter(|e| e.actor_type == "agent")
            .count(),
        2
    );
}

#[test]
fn identical_retry_returns_receipt_without_another_backup_and_changed_payload_is_rejected() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let mut r = append(&s, &id, "只追加一次");
    let first = apply(&mut s, &r, "cli").unwrap();
    let retry = apply(&mut s, &r, "mcp").unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(retry).unwrap()
    );
    assert_eq!(count(&s), 1);
    r.source = "changed".into();
    assert!(matches!(
        apply(&mut s, &r, "cli"),
        Err(CoreError::AgentRequestConflict)
    ));
    settings::set(&mut s, false, 1).unwrap();
    assert!(matches!(
        apply(&mut s, &r, "cli"),
        Err(CoreError::AgentWriteDisabled)
    ));
    assert_eq!(
        applications::load_record(s.connection(), &id)
            .unwrap()
            .notes,
        "只追加一次"
    );
}

#[test]
fn stale_versions_invalid_fields_and_later_invalid_action_rollback_before_backup() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let revision = applications::load_record(s.connection(), &id)
        .unwrap()
        .revision;
    for actions in [
        json!([{"operation":"append_notes","application_id":id,"revision":revision-1,"text":"过期"}]),
        json!([{"operation":"update_fields","application_id":id,"revision":revision,"fields":{"folder_relative_path":"outside"}}]),
        json!([{"operation":"append_notes","application_id":id,"revision":revision,"text":"不应提交"},{"operation":"create_task","title":""}]),
        json!([{"operation":"update_fields","application_id":id,"revision":revision,"fields":{"application_url":"file:///private"}}]),
        json!([{"operation":"append_notes","application_id":id,"revision":revision,"text":"重复"},{"operation":"append_notes","application_id":id,"revision":revision,"text":"重复"}]),
    ] {
        let r = request(&s, actions);
        assert!(apply(&mut s, &r, "cli").is_err());
        assert_eq!(
            applications::load_record(s.connection(), &id)
                .unwrap()
                .revision,
            revision
        );
        assert_eq!(count(&s), 0);
        assert_eq!(audit_list(&s).unwrap().len(), 1);
    }
}

#[test]
fn backup_failure_blocks_changes_and_audit_failure_rolls_back_but_retains_backup() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let r = append(&s, &id, "修改");
    let path = s.root().join("backups/database");
    let held = s.root().join("backups/saved-database");
    std::fs::rename(&path, &held).unwrap();
    std::fs::write(&path, b"occupied").unwrap();
    assert!(apply(&mut s, &r, "cli").is_err());
    assert_eq!(
        applications::load_record(s.connection(), &id)
            .unwrap()
            .notes,
        ""
    );
    std::fs::rename(&path, s.root().join("backups/occupied-marker")).unwrap();
    std::fs::rename(&held, &path).unwrap();
    s.connection_mut().unwrap().execute_batch("CREATE TRIGGER fail_agent_audit BEFORE INSERT ON agent_audit_log WHEN NEW.operation='write' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(apply(&mut s, &r, "cli").is_err());
    assert_eq!(count(&s), 1);
    assert_eq!(
        applications::load_record(s.connection(), &id)
            .unwrap()
            .notes,
        ""
    );
    assert_eq!(audit_list(&s).unwrap().len(), 1);
}

#[test]
fn cross_process_entry_requires_same_lock_and_no_recovery_or_upgrade() {
    let (_temp, mut s, id) = fixture();
    let r = append(&s, &id, "安全写入");
    let path = s.root().to_owned();
    assert!(matches!(
        execute(&path, &r, "cli"),
        Err(CoreError::AgentWriteDisabled)
    ));
    allow(&mut s);
    assert!(matches!(
        execute(&path, &r, "cli"),
        Err(CoreError::WarehouseLocked)
    ));
    assert_eq!(count(&s), 0);
    drop(s);
    let result = execute(&path, &r, "cli").unwrap();
    assert_eq!(result["request_id"], r.request_id.to_string());
    let s = warehouse::open(&path, WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(
        applications::load_record(s.connection(), &id)
            .unwrap()
            .notes,
        "安全写入"
    );
    assert_eq!(count(&s), 1);
    let mut wrong = r.clone();
    wrong.warehouse_id = Uuid::new_v4();
    assert!(matches!(
        execute(&path, &wrong, "cli"),
        Err(CoreError::AgentWarehouseChanged)
    ));
    drop(s);
    let c = rusqlite::Connection::open(path.join("offertrack.sqlite")).unwrap();
    c.execute("DELETE FROM schema_migrations WHERE version>=9", [])
        .unwrap();
    drop(c);
    assert!(execute(&path, &r, "cli").is_err());
    let c = rusqlite::Connection::open(path.join("offertrack.sqlite")).unwrap();
    assert_eq!(
        c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        8
    );
}

#[test]
fn official_cli_supports_writes_and_retries_after_broken_stdout_without_duplicates() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("test"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let r = append(&s, &id, "中文备注");
    let path = s.root().to_owned();
    drop(s);
    let bytes = serde_json::to_vec(&r).unwrap();
    let args = || {
        [
            "--warehouse".into(),
            path.as_os_str().to_owned(),
            "write".into(),
        ]
    };
    assert_eq!(
        crate::agent_cli::run(args(), &mut bytes.as_slice(), &mut Broken),
        3
    );
    let mut out = Vec::new();
    assert_eq!(
        crate::agent_cli::run(args(), &mut bytes.as_slice(), &mut out),
        0
    );
    let value: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["data"]["request_id"], r.request_id.to_string());
    let s = warehouse::open(&path, WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(
        applications::load_record(s.connection(), &id)
            .unwrap()
            .notes,
        "中文备注"
    );
    assert_eq!(count(&s), 1);
    for bad in [
        b"{\"version\":1,\"version\":1}".as_slice(),
        b"{\"actions\":[{\"fields\":{\"notes\":\"one\",\"notes\":\"two\"}}]}",
        b"{\"operation\":\"clear_trash\"}",
    ] {
        let mut out = Vec::new();
        assert_eq!(crate::agent_cli::run(args(), &mut &*bad, &mut out), 2);
    }
}

#[test]
fn dto_rejects_arbitrary_authority_and_oversized_batches_and_source_labels() {
    let (_temp, s, id) = fixture();
    let r = append(&s, &id, "备注");
    for key in ["path", "sql", "enabled", "command"] {
        let mut v = json!(r);
        v[key] = json!(true);
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
    for action in [
        "clear_trash",
        "delete_path",
        "set_write_enabled",
        "execute_sql",
        "create_application",
    ] {
        let mut v = json!(r);
        v["actions"] = json!([{"operation":action}]);
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
    let mut large = r.clone();
    large.actions = vec![r.actions[0].clone(); 51];
    assert!(validate(&large).is_err());
    large = r.clone();
    large.version = 2;
    assert!(matches!(validate(&large), Err(CoreError::AgentVersion)));
    large = r.clone();
    large.source = "x\ny".into();
    assert!(validate(&large).is_err());
    let mut bad = json!(r);
    bad["actions"][0]["text"] = json!("x".repeat(65536));
    assert!(validate(&serde_json::from_value(bad).unwrap()).is_err());
}

#[cfg(windows)]
#[test]
fn writer_rejects_reparse_lock_and_warehouse_ancestors() {
    use std::process::Command;
    let (temp, mut s, id) = fixture();
    allow(&mut s);
    let r = append(&s, &id, "拒绝链接");
    let path = s.root().to_owned();
    drop(s);
    let outside = tempfile::tempdir().unwrap();
    let junction = outside.path().join("linked");
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null",
        ])
        .env("OFFERTRACK_TEST_LINK", &junction)
        .env("OFFERTRACK_TEST_TARGET", temp.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(execute(&junction, &r, "cli").is_err());
    std::fs::rename(path.join(".offertrack.lock"), path.join("saved-lock")).unwrap();
    let target = tempfile::tempdir().unwrap();
    std::fs::write(target.path().join("sentinel"), b"unchanged").unwrap();
    let status = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command",
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
        .env("OFFERTRACK_TEST_LINK", path.join(".offertrack.lock")).env("OFFERTRACK_TEST_TARGET", target.path()).output().unwrap();
    assert!(status.status.success());
    assert!(execute(&path, &r, "cli").is_err());
    assert_eq!(
        std::fs::read(target.path().join("sentinel")).unwrap(),
        b"unchanged"
    );
}

#[test]
fn pending_recovery_is_not_executed_and_invalid_permission_fails_closed() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let r = append(&s, &id, "不允许恢复后写入");
    let path = s.root().to_owned();
    s.connection_mut().unwrap().execute("INSERT INTO record_creations(application_id,target_relative_path,state,created_at_utc) VALUES ('pending','applications/synthetic','copying','2026-09-03T00:00:00Z')",[]).unwrap();
    drop(s);
    assert!(matches!(
        execute(&path, &r, "cli"),
        Err(CoreError::BackupPendingOperations)
    ));
    let c = rusqlite::Connection::open(path.join("offertrack.sqlite")).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT state FROM record_creations WHERE application_id='pending'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "copying"
    );
    assert_eq!(applications::load_record(&c, &id).unwrap().notes, "");
    for value in [
        "true",
        "{\"version\":2,\"enabled\":true,\"revision\":1}",
        "{\"version\":1,\"enabled\":true,\"revision\":0}",
    ] {
        c.execute(
            "UPDATE settings SET value_json=?1 WHERE key='agent_access_v1'",
            [value],
        )
        .unwrap();
        assert!(execute(&path, &r, "cli").is_err());
    }
    assert_eq!(
        std::fs::read_dir(path.join("backups/database"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn pending_document_rename_blocks_agent_without_replaying_file_operations() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let request = append(&s, &id, "不得覆盖待恢复附件");
    let path = s.root().to_owned();
    s.connection_mut().unwrap().execute_batch("INSERT INTO document_renames VALUES ('pending-rename', 1, 'record', 'document', 'applications/synthetic', 'a.pdf', 'b.pdf', 'identity', 'now', NULL, NULL)").unwrap();
    drop(s);
    assert!(matches!(
        execute(&path, &request, "cli"),
        Err(CoreError::BackupPendingOperations)
    ));
    let connection = rusqlite::Connection::open(path.join("offertrack.sqlite")).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM document_renames WHERE completed_at_utc IS NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        applications::load_record(&connection, &id).unwrap().notes,
        ""
    );
    assert!(!path.join("applications/synthetic").exists());
}

#[test]
fn pending_document_trash_or_purge_blocks_agent_without_replaying_deletion() {
    let (_temp, mut s, id) = fixture();
    allow(&mut s);
    let request = append(&s, &id, "不得越过附件恢复日志");
    let path = s.root().to_owned();
    s.connection_mut().unwrap().execute("INSERT INTO document_trash(id,version,document_id,application_id,relative_path,display_name,discovered_at_utc,last_observed_at_utc,deleted_at_utc,state) VALUES('trash',1,'document',?1,'a.pdf','a.pdf','now','now','now','pending')",[&id]).unwrap();
    s.connection_mut().unwrap().execute_batch("INSERT INTO document_moves VALUES ('move',1,'trash','trash','applications/synthetic','a.pdf','identity','now',NULL,NULL)").unwrap();
    drop(s);
    assert!(matches!(
        execute(&path, &request, "cli"),
        Err(CoreError::BackupPendingOperations)
    ));
    let connection = rusqlite::Connection::open(path.join("offertrack.sqlite")).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM document_moves WHERE completed_at_utc IS NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    connection
        .execute(
            "UPDATE document_moves SET completed_at_utc='done',outcome='cancelled' WHERE id='move'",
            [],
        )
        .unwrap();
    connection
        .execute_batch("INSERT INTO document_purges VALUES ('purge',1,'trash','now',NULL,NULL)")
        .unwrap();
    drop(connection);
    assert!(matches!(
        execute(&path, &request, "cli"),
        Err(CoreError::BackupPendingOperations)
    ));
    let connection = rusqlite::Connection::open(path.join("offertrack.sqlite")).unwrap();
    assert_eq!(
        applications::load_record(&connection, &id).unwrap().notes,
        ""
    );
    assert!(!path.join("recycle-bin/documents/trash").exists());
}
