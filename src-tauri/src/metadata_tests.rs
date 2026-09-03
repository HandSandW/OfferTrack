//! Metadata regression tests use disposable warehouses, never user data.
use crate::{
    applications,
    domain::{
        CreateApplicationRequest, FieldDefinitionRequest, SavedView, SavedViewRequest,
        ViewMetadataRequest,
    },
    error::CoreError,
    views,
    warehouse::{self, WarehouseAccessMode},
};
use rusqlite::params;
use serde_json::{Value, json};
use tempfile::tempdir;

fn view_request(name: &str, is_default: bool) -> SavedViewRequest {
    SavedViewRequest {
        id: None,
        revision: None,
        name: name.into(),
        is_default,
        layout: json!({"columns": [{"key": "companyName", "width": 240, "visible": true, "pinned": true}], "future": "retained"}),
        sort: json!([]),
        filter: json!({"search": "银行", "companyTypes": ["bank"], "stages": []}),
        group: Some(json!("companyType")),
    }
}
fn meta(view: &SavedView, name: &str, is_default: bool) -> ViewMetadataRequest {
    ViewMetadataRequest {
        id: view.id.clone(),
        revision: view.revision,
        name: name.into(),
        is_default,
    }
}
fn field_request() -> FieldDefinitionRequest {
    FieldDefinitionRequest {
        id: None,
        revision: None,
        display_name: "优先级".into(),
        field_type: "select".into(),
        config: json!({"options": ["高", "低"], "future": true}),
    }
}
fn create_record(session: &mut warehouse::WarehouseSession) -> crate::domain::ApplicationDetail {
    applications::create(
        session,
        CreateApplicationRequest {
            company_name: "测试公司".into(),
            position_name: "研发".into(),
            company_type: "private".into(),
            industry: String::new(),
            position_category: String::new(),
            work_location: String::new(),
        },
    )
    .unwrap()
}

#[test]
fn views_rename_copy_update_and_delete_only_metadata_and_survive_reopen() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let record = create_record(&mut session);
    let file = dir
        .path()
        .join(&record.record.folder_relative_path)
        .join("resume.pdf");
    std::fs::write(&file, b"synthetic resume fixture").unwrap();
    let first = views::save(&mut session, view_request("原视图", true))
        .unwrap()
        .view;
    let renamed = views::metadata(&mut session, meta(&first, " 新名称 ", true))
        .unwrap()
        .view;
    assert_eq!(renamed.name, "新名称");
    assert_eq!(renamed.revision, 2);
    assert_eq!(renamed.layout, first.layout);
    assert_eq!(renamed.sort, first.sort);
    assert_eq!(renamed.filter, first.filter);
    assert_eq!(renamed.group, first.group);
    let copy = views::duplicate(&mut session, &renamed.id, 2, "副本")
        .unwrap()
        .view;
    assert_ne!(copy.id, renamed.id);
    assert_eq!(copy.layout, first.layout);
    assert_eq!(copy.filter, first.filter);
    assert_eq!(copy.group, first.group);
    assert!(!copy.is_default);
    let mut update = view_request("更新原视图", false);
    update.id = Some(renamed.id.clone());
    update.revision = Some(renamed.revision);
    update.filter["search"] = json!("临时筛选已明确保存");
    let updated = views::save(&mut session, update).unwrap().view;
    assert_eq!(updated.revision, 3);
    assert_eq!(
        views::list(&session)
            .unwrap()
            .iter()
            .find(|v| v.id == copy.id)
            .unwrap()
            .filter,
        first.filter
    );
    views::delete(&mut session, &updated.id, updated.revision).unwrap();
    drop(session);
    let reopened = warehouse::open(dir.path(), WarehouseAccessMode::Write).unwrap();
    let remaining = views::list(&reopened).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, copy.id);
    assert!(!remaining[0].is_default);
    assert_eq!(remaining[0].sort, json!([]));
    assert_eq!(
        serde_json::to_value(applications::get(&reopened, &record.record.id).unwrap()).unwrap(),
        serde_json::to_value(record).unwrap()
    );
    assert_eq!(std::fs::read(file).unwrap(), b"synthetic resume fixture");
}

#[test]
fn views_reject_stale_missing_and_foreign_kind_identifiers() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let first = views::save(&mut session, view_request("默认", true))
        .unwrap()
        .view;
    views::metadata(&mut session, meta(&first, "最新", true)).unwrap();
    let mut stale = view_request("过期", true);
    stale.id = Some(first.id.clone());
    stale.revision = Some(1);
    assert!(matches!(
        views::save(&mut session, stale.clone()),
        Err(CoreError::RevisionConflict)
    ));
    assert!(matches!(
        views::metadata(&mut session, meta(&first, "过期", false)),
        Err(CoreError::RevisionConflict)
    ));
    assert!(matches!(
        views::duplicate(&mut session, &first.id, 1, "过期副本"),
        Err(CoreError::RevisionConflict)
    ));
    assert!(matches!(
        views::delete(&mut session, &first.id, 1),
        Err(CoreError::RevisionConflict)
    ));
    stale.id = Some("missing".into());
    assert!(matches!(
        views::save(&mut session, stale.clone()),
        Err(CoreError::NotFound)
    ));
    session.connection().execute("INSERT INTO views (id, name, view_kind, layout_json, created_at_utc, updated_at_utc) VALUES ('foreign', '其它页面', 'dashboard', '{}', 'now', 'now')", []).unwrap();
    stale.id = Some("foreign".into());
    assert!(matches!(
        views::save(&mut session, stale),
        Err(CoreError::NotFound)
    ));
    assert!(matches!(
        views::delete(&mut session, "foreign", 1),
        Err(CoreError::NotFound)
    ));
    assert_eq!(views::list(&session).unwrap()[0].name, "最新");
    let total: i64 = session
        .connection()
        .query_row("SELECT COUNT(*) FROM views", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 2);
}

#[test]
fn default_switch_and_failed_create_copy_delete_are_atomic() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let first = views::save(&mut session, view_request("原默认", true))
        .unwrap()
        .view;
    let next = views::save(&mut session, view_request("下一视图", false))
        .unwrap()
        .view;
    session.connection().execute_batch("CREATE TRIGGER reject_new_default BEFORE UPDATE ON views WHEN NEW.name = '下一视图' AND NEW.is_default = 1 BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(views::metadata(&mut session, meta(&next, &next.name, true)).is_err());
    assert!(
        views::list(&session)
            .unwrap()
            .iter()
            .any(|v| v.id == first.id && v.is_default && v.revision == 1)
    );
    session
        .connection()
        .execute_batch("DROP TRIGGER reject_new_default;")
        .unwrap();
    let switched = views::metadata(&mut session, meta(&next, &next.name, true)).unwrap();
    assert_eq!(switched.view.revision, 2);
    assert_eq!(switched.views.iter().filter(|v| v.is_default).count(), 1);
    assert!(
        switched
            .views
            .iter()
            .any(|v| v.id == first.id && !v.is_default && v.revision == 2)
    );
    session.connection().execute_batch("CREATE TRIGGER reject_view_insert BEFORE INSERT ON views BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(views::save(&mut session, view_request("失败新默认", true)).is_err());
    assert!(views::duplicate(&mut session, &next.id, 2, "失败副本").is_err());
    assert!(
        views::list(&session)
            .unwrap()
            .iter()
            .any(|v| v.id == next.id && v.is_default && v.revision == 2)
    );
    session.connection().execute_batch("CREATE TRIGGER reject_view_delete BEFORE DELETE ON views BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(views::delete(&mut session, &next.id, 2).is_err());
    assert_eq!(views::list(&session).unwrap().len(), 2);
}

#[test]
fn invalid_or_corrupt_metadata_does_not_commit_a_partially_successful_write() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let first = views::save(&mut session, view_request("有效", true))
        .unwrap()
        .view;
    let mut invalid = view_request("无效", true);
    invalid.layout["columns"][0]["width"] = json!(2);
    assert!(matches!(
        views::save(&mut session, invalid),
        Err(CoreError::Validation)
    ));
    let second = views::save(&mut session, view_request("将损坏", false))
        .unwrap()
        .view;
    session
        .connection()
        .execute(
            "UPDATE views SET filter_json = 'not json' WHERE id = ?1",
            [&second.id],
        )
        .unwrap();
    assert!(views::list(&session).is_err());
    assert!(views::metadata(&mut session, meta(&first, "不能提交", false)).is_err());
    assert!(views::duplicate(&mut session, &first.id, 1, "不能提交副本").is_err());
    let current: (String, i64) = session
        .connection()
        .query_row(
            "SELECT name, revision FROM views WHERE id = ?1",
            [&first.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(current, ("有效".into(), 1));
    // Corrupt sources are reported, not silently removed or repaired.
    views::delete(&mut session, &second.id, 1).unwrap_err();
    assert!(views::delete(&mut session, &first.id, 1).is_err());
    let total: i64 = session
        .connection()
        .query_row("SELECT COUNT(*) FROM views", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 2);
}

#[test]
fn field_edits_preserve_values_keys_and_layout_and_reject_stale_or_incompatible_changes() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let record = create_record(&mut session);
    let field = applications::save_field_definition(&mut session, field_request())
        .unwrap()
        .remove(0);
    session.connection().execute("INSERT INTO field_values (application_id, field_definition_id, value_json, updated_at_utc) VALUES (?1, ?2, ?3, 'now')", params![record.record.id, field.id, json!("高").to_string()]).unwrap();
    let saved = views::save(&mut session, view_request("视图", true))
        .unwrap()
        .view;
    let mut request = field_request();
    request.id = Some(field.id.clone());
    request.revision = Some(1);
    request.display_name = "新名称".into();
    request.config = json!({"options": ["低"]});
    assert!(matches!(
        applications::save_field_definition(&mut session, request.clone()),
        Err(CoreError::Validation)
    ));
    request.field_type = "number".into();
    request.config = json!({});
    assert!(matches!(
        applications::save_field_definition(&mut session, request.clone()),
        Err(CoreError::Validation)
    ));
    request.field_type = "select".into();
    request.config = json!({"options": ["高", "低", "中"], "future": true});
    let changed = applications::save_field_definition(&mut session, request.clone())
        .unwrap()
        .remove(0);
    assert_eq!(changed.key, field.key);
    assert_eq!(changed.display_order, field.display_order);
    assert_eq!(changed.revision, 2);
    assert!(matches!(
        applications::save_field_definition(&mut session, request.clone()),
        Err(CoreError::RevisionConflict)
    ));
    request.revision = Some(2);
    session.connection().execute_batch("CREATE TRIGGER reject_field_update BEFORE UPDATE ON field_definitions BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(applications::save_field_definition(&mut session, request).is_err());
    drop(session);
    let reopened = warehouse::open(dir.path(), WarehouseAccessMode::ReadOnly).unwrap();
    let fields = applications::list_field_definitions(&reopened).unwrap();
    assert_eq!(fields[0].revision, 2);
    assert_eq!(fields[0].display_name, "新名称");
    assert_eq!(fields[0].config, changed.config);
    assert_eq!(
        applications::get(&reopened, &record.record.id)
            .unwrap()
            .record
            .custom_fields[&field.id],
        json!("高")
    );
    assert_eq!(views::list(&reopened).unwrap()[0].layout, saved.layout);
}

#[test]
fn field_validation_and_read_only_metadata_writes_cannot_change_the_warehouse() {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    for options in [
        json!([]),
        json!([""]),
        json!([1]),
        json!([" a"]),
        json!(["a", "a"]),
        json!(["a".repeat(201)]),
    ] {
        let mut request = field_request();
        request.config = json!({"options": options});
        assert!(matches!(
            applications::save_field_definition(&mut session, request),
            Err(CoreError::Validation)
        ));
    }
    let first = views::save(&mut session, view_request("只读视图", true))
        .unwrap()
        .view;
    let mut missing = field_request();
    missing.id = Some("missing".into());
    missing.revision = Some(1);
    assert!(matches!(
        applications::save_field_definition(&mut session, missing),
        Err(CoreError::NotFound)
    ));
    drop(session);
    let mut readonly = warehouse::open(dir.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert!(matches!(
        views::save(&mut readonly, view_request("拒绝", true)),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        views::metadata(&mut readonly, meta(&first, "拒绝", false)),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        views::duplicate(&mut readonly, &first.id, 1, "拒绝"),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        views::delete(&mut readonly, &first.id, 1),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        applications::save_field_definition(&mut readonly, field_request()),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert_eq!(views::list(&readonly).unwrap().len(), 1);
    assert!(
        applications::list_field_definitions(&readonly)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn metadata_dtos_reject_undeclared_scope_and_path_capabilities() {
    let mut view: Value = json!({"id": "saved", "revision": 1, "name": "名字", "isDefault": true});
    assert!(serde_json::from_value::<ViewMetadataRequest>(view.clone()).is_ok());
    view["path"] = json!("arbitrary");
    assert!(serde_json::from_value::<ViewMetadataRequest>(view).is_err());
    assert!(serde_json::from_value::<FieldDefinitionRequest>(json!({"id": null, "revision": null, "displayName": "字段", "fieldType": "text", "config": {}, "applyToAll": true})).is_err());
    assert!(serde_json::from_value::<SavedViewRequest>(json!({"id": null, "revision": null, "name": "视图", "layout": {}, "sort": [], "filter": {}, "group": null, "deleteFiles": true})).is_err());
}
