use super::*;
use crate::warehouse::{self, WarehouseAccessMode};
use serde_json::json;
use tempfile::tempdir;

fn create(session: &mut WarehouseSession) -> ApplicationListItem {
    super::super::create(
        session,
        CreateApplicationRequest {
            company_name: "单格测试公司".into(),
            position_name: "工程师".into(),
            company_type: "private".into(),
            industry: "软件".into(),
            position_category: "研发".into(),
            work_location: "上海".into(),
        },
    )
    .unwrap()
    .record
}
fn request(record: &ApplicationListItem, key: &str, value: Value) -> Request {
    Request {
        version: 1,
        id: record.id.clone(),
        revision: record.revision,
        key: key.into(),
        value,
    }
}

#[test]
fn metadata_save_and_undo_leave_workflow_files_and_other_fields_untouched() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let before = create(&mut session);
    let path = dir
        .path()
        .join(&before.folder_relative_path)
        .join("resume.pdf");
    std::fs::write(&path, b"synthetic resume").unwrap();
    let applied = apply(
        &mut session,
        request(&before, "companyName", json!(" 新公司 ")),
    )
    .unwrap();
    assert!(applied.changed);
    assert_eq!(applied.record.company_name, "新公司");
    assert!(applied.record.folder_normalization_pending);
    assert_eq!(
        applied.record.folder_relative_path,
        before.folder_relative_path
    );
    assert_eq!(
        applied.record.status_updated_at_utc,
        before.status_updated_at_utc
    );
    assert_eq!(applied.record.created_at_utc, before.created_at_utc);
    assert_eq!(applied.record.application_date, before.application_date);
    assert_eq!(applied.record.position_name, before.position_name);
    let undo = apply(
        &mut session,
        request(&applied.record, "companyName", applied.previous_value),
    )
    .unwrap();
    assert_eq!(undo.record.company_name, before.company_name);
    assert_eq!(undo.record.revision, before.revision + 2);
    assert_eq!(std::fs::read(&path).unwrap(), b"synthetic resume");
    let no_op = apply(
        &mut session,
        request(&undo.record, "companyName", json!(before.company_name)),
    )
    .unwrap();
    assert!(!no_op.changed);
    assert_eq!(no_op.record.revision, undo.record.revision);
    assert_eq!(no_op.record.updated_at_utc, undo.record.updated_at_utc);
}

#[test]
fn conflict_readonly_deleted_and_invalid_keys_never_write() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let before = create(&mut session);
    let saved = apply(&mut session, request(&before, "notes", json!("新备注"))).unwrap();
    let later = apply(
        &mut session,
        request(&saved.record, "industry", json!("银行")),
    )
    .unwrap();
    assert!(matches!(
        apply(
            &mut session,
            request(&saved.record, "notes", saved.previous_value)
        ),
        Err(CoreError::RevisionConflict)
    ));
    for (key, value) in [
        ("folderRelativePath", json!("../outside")),
        ("createdAtUtc", json!("2000")),
        ("currentStageName", json!("offer")),
        ("notes=?1; DELETE FROM applications", json!("x")),
        ("notes", Value::Null),
        ("applicationUrl", json!("file:///resume.pdf")),
        ("companyName", json!(" ")),
        ("applicationDate", json!("2026-02-30")),
    ] {
        assert!(apply(&mut session, request(&later.record, key, value)).is_err());
    }
    assert_eq!(
        load_record(session.connection(), &before.id)
            .unwrap()
            .revision,
        later.record.revision
    );
    let mut readonly = warehouse::open(dir.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert!(
        apply(
            &mut readonly,
            request(&later.record, "notes", json!("forbidden"))
        )
        .is_err()
    );
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET deleted_at_utc='2026-09-03T00:00:00Z' WHERE id=?1",
            [&before.id],
        )
        .unwrap();
    assert!(matches!(
        apply(
            &mut session,
            request(&later.record, "notes", json!("forbidden"))
        ),
        Err(CoreError::NotFound)
    ));
}

#[test]
fn custom_values_are_typed_clearable_and_isolated_with_transaction_rollback() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let before = create(&mut session);
    let fields = save_field_definition(
        &mut session,
        FieldDefinitionRequest {
            id: None,
            revision: None,
            display_name: "优先级".into(),
            field_type: "number".into(),
            config: json!({}),
        },
    )
    .unwrap();
    let id = &fields[0].id;
    let key = format!("custom:{id}");
    let custom = apply(&mut session, request(&before, &key, json!(12.5))).unwrap();
    assert!(custom.previous_value.is_null());
    assert!(
        apply(
            &mut session,
            request(&custom.record, &key, json!("wrong type"))
        )
        .is_err()
    );
    let tags = apply(
        &mut session,
        request(&custom.record, "tags", json!(["重点", "远程"])),
    )
    .unwrap();
    assert_eq!(tags.record.custom_fields.get(id), Some(&json!(12.5)));
    session.connection_mut().unwrap().execute_batch("CREATE TRIGGER cell_failure BEFORE UPDATE OF revision ON applications BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
    assert!(
        apply(
            &mut session,
            request(&tags.record, "tags", json!(["不应保存"]))
        )
        .is_err()
    );
    assert!(apply(&mut session, request(&tags.record, &key, Value::Null)).is_err());
    let unchanged = load_record(session.connection(), &before.id).unwrap();
    assert_eq!(unchanged.revision, tags.record.revision);
    assert_eq!(unchanged.tags[0].id, tags.record.tags[0].id);
    assert_eq!(unchanged.custom_fields, tags.record.custom_fields);
    session
        .connection_mut()
        .unwrap()
        .execute_batch("DROP TRIGGER cell_failure")
        .unwrap();
    let cleared = apply(&mut session, request(&tags.record, &key, Value::Null)).unwrap();
    assert!(!cleared.record.custom_fields.contains_key(id));
    assert_eq!(cleared.record.tags[0].id, tags.record.tags[0].id);
    let restored = apply(
        &mut session,
        request(&cleared.record, &key, cleared.previous_value),
    )
    .unwrap();
    assert_eq!(restored.record.custom_fields.get(id), Some(&json!(12.5)));
}

#[test]
fn all_standard_cell_types_roundtrip_and_archived_metadata_is_editable() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let mut record = create(&mut session);
    for (key, value) in [
        ("companyType", json!("bank")),
        ("industry", json!("金融")),
        ("positionName", json!("分析师")),
        ("positionCategory", json!("研究")),
        ("workLocation", json!("北京")),
        ("applicationDate", json!("2026-09-03")),
        ("applicationUrl", json!("https://example.com/apply")),
        ("announcementUrl", json!("https://example.com/news")),
        ("companyUrl", json!("https://example.com")),
        ("positionUrl", json!("https://example.com/job")),
        ("positionDescription", json!("多行\n介绍")),
        ("notes", json!("中文\n😀")),
    ] {
        let applied = apply(&mut session, request(&record, key, value.clone())).unwrap();
        assert_eq!(serde_json::to_value(&applied.record).unwrap()[key], value);
        record = applied.record;
    }
    record = set_archived(&mut session, &record.id, true).unwrap().record;
    let changed = apply(
        &mut session,
        request(&record, "applicationDate", Value::Null),
    )
    .unwrap();
    assert!(changed.record.archived_at_utc.is_some());
    assert!(changed.record.application_date.is_none());
    assert_eq!(
        changed.record.status_updated_at_utc,
        record.status_updated_at_utc
    );
    drop(session);
    let reopened = warehouse::open(dir.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(
        load_record(reopened.connection(), &record.id)
            .unwrap()
            .notes,
        "中文\n😀"
    );
}

#[test]
fn dto_version_unknown_fields_and_size_are_rejected() {
    assert!(
        serde_json::from_value::<Request>(
            json!({"version":1,"id":"x","revision":1,"key":"notes","value":"x","path":"outside"})
        )
        .is_err()
    );
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let record = create(&mut session);
    let mut edit = request(&record, "notes", json!("x"));
    edit.version = 2;
    assert!(apply(&mut session, edit).is_err());
    assert!(
        apply(
            &mut session,
            request(&record, "notes", json!("x".repeat(100_001)))
        )
        .is_err()
    );
    assert!(
        apply(
            &mut session,
            request(&record, "tags", json!(["x".repeat(41)]))
        )
        .is_err()
    );
    assert_eq!(
        load_record(session.connection(), &record.id)
            .unwrap()
            .revision,
        record.revision
    );
}
