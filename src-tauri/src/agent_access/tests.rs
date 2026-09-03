use super::*;
use crate::{
    domain::{ChangeStageRequest, CreateApplicationRequest, FieldDefinitionRequest},
    warehouse::{self, WarehouseAccessMode},
};
use sha2::{Digest, Sha256};

pub(crate) fn record(s: &mut WarehouseSession, name: &str) -> crate::domain::ApplicationDetail {
    applications::create(
        s,
        CreateApplicationRequest {
            company_name: name.into(),
            position_name: "软件工程师".into(),
            company_type: "private".into(),
            industry: "软件".into(),
            position_category: "研发".into(),
            work_location: "厦门".into(),
        },
    )
    .unwrap()
}
fn request(value: Value) -> Request {
    serde_json::from_value(json!({"version":1,"request":value})).unwrap()
}

#[test]
fn full_text_stable_ids_typed_fields_history_and_relative_paths_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = record(&mut s, "虚构公司");
    let id = a.record.id;
    let text = "很长的岗位介绍\n\"引号\" 与备注".repeat(1500);
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET notes=?1,position_description=?1 WHERE id=?2",
            (&text, &id),
        )
        .unwrap();
    s.connection_mut().unwrap().execute("INSERT INTO interview_rounds(id,application_id,sequence_number,display_name,state,result,notes,created_at_utc,updated_at_utc) VALUES ('round',?1,1,'主管面','awaitingParticipation','完整结果','轮次备注','2026-09-03T00:00:00Z','2026-09-03T00:00:00Z')",[&id]).unwrap();
    let fields = applications::save_field_definition(
        &mut s,
        FieldDefinitionRequest {
            id: None,
            revision: None,
            display_name: "薪资期望".into(),
            field_type: "number".into(),
            config: json!({}),
        },
    )
    .unwrap();
    let field = &fields[0];
    s.connection_mut().unwrap().execute("INSERT INTO field_values(application_id,field_definition_id,value_json,updated_at_utc) VALUES (?1,?2,'12345','2026-09-03T00:00:00Z')", (&id,&field.id)).unwrap();
    let dir = s.root().join(&a.record.folder_relative_path);
    fs::create_dir(dir.join("子目录")).unwrap();
    fs::write(
        dir.join("子目录/简历.pdf"),
        b"PRIVATE PDF CONTENT NEVER READ",
    )
    .unwrap();
    applications::scan_all_documents(&mut s).unwrap();
    let data = collect(&s).unwrap();
    let app = &data.applications[0];
    assert_eq!(app.record.notes, text);
    assert_eq!(app.record.position_description, text);
    assert_eq!(app.record.custom_fields[&field.id], json!(12345));
    assert_eq!(app.interview_rounds[0].notes, "轮次备注");
    assert!(!app.history.is_empty());
    assert_eq!(
        app.documents[0].relative_path,
        format!("{}/子目录/简历.pdf", a.record.folder_relative_path)
    );
    assert_eq!(data.fields[0].key, field.key);
    let json_text = String::from_utf8(encode(&data).unwrap()).unwrap();
    assert!(!json_text.contains("PRIVATE PDF CONTENT"));
    assert!(!json_text.contains(s.root().to_str().unwrap()));
    assert!(!json_text.contains("companyName"));
    assert!(!json_text.contains("deleted_at_utc"));
    assert_schema(
        &serde_json::to_value(app).unwrap(),
        &dto::schema()["$defs"]["Application"],
    );
    let result = query(
        &s,
        request(json!({"operation":"list_applications","search":"主管面"})),
    )
    .unwrap();
    assert_eq!(result["result"]["total"], 1);
}

fn assert_schema(value: &Value, schema: &Value) {
    if let Some(options) = schema["anyOf"].as_array() {
        assert_schema(value, &options[usize::from(value.is_null())]);
    } else {
        match schema["type"].as_str() {
            Some("object") => {
                let object = value.as_object().unwrap();
                if let Some(properties) = schema["properties"].as_object() {
                    assert_eq!(
                        object.keys().collect::<Vec<_>>(),
                        properties.keys().collect::<Vec<_>>()
                    );
                    for (key, field) in properties {
                        assert_schema(&value[key], field);
                    }
                }
            }
            Some("array") => {
                for item in value.as_array().unwrap() {
                    assert_schema(item, &schema["items"]);
                }
            }
            Some("string") => assert!(value.is_string()),
            Some("integer") => assert!(value.is_i64() || value.is_u64()),
            Some("boolean") => assert!(value.is_boolean()),
            Some("null") => assert!(value.is_null()),
            _ => (),
        }
    }
}

#[test]
fn active_archive_deleted_scopes_pagination_and_terminal_counts_are_explicit() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = record(&mut s, "A");
    let b = record(&mut s, "B");
    let c = record(&mut s, "C");
    for (app, key) in [(&a, "offer"), (&b, "failed_terminal")] {
        applications::change_stage(
            &mut s,
            ChangeStageRequest {
                application_id: app.record.id.clone(),
                stage_id: app
                    .stages
                    .iter()
                    .find(|s| s.stable_key == key)
                    .unwrap()
                    .id
                    .clone(),
                stage_state: if key == "offer" {
                    "completed"
                } else {
                    "failed"
                }
                .into(),
                revision: app.record.revision,
                notes: "保留失败阶段".into(),
            },
        )
        .unwrap();
    }
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET archived_at_utc=created_at_utc WHERE id=?1",
            [&b.record.id],
        )
        .unwrap();
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET deleted_at_utc=created_at_utc WHERE id=?1",
            [&c.record.id],
        )
        .unwrap();
    let data = collect(&s).unwrap();
    assert_eq!(data.summary.active_applications, 1);
    assert_eq!(data.summary.archived_applications, 1);
    assert_eq!(data.summary.offers, 1);
    assert_eq!(data.summary.failed_applications, 1);
    let first = query(
        &s,
        request(json!({"operation":"list_applications","limit":1})),
    )
    .unwrap();
    assert_eq!(first["result"]["total"], 2);
    assert_eq!(first["result"]["next_offset"], 1);
    let last = query(
        &s,
        request(json!({"operation":"list_applications","limit":1,"offset":1})),
    )
    .unwrap();
    assert!(last["result"]["next_offset"].is_null());
    assert_ne!(
        first["result"]["items"][0]["id"],
        last["result"]["items"][0]["id"]
    );
    assert!(matches!(
        query(
            &s,
            request(json!({"operation":"get_application","id":c.record.id}))
        ),
        Err(CoreError::NotFound)
    ));
    assert!(matches!(
        query(
            &s,
            request(json!({"operation":"list_applications","limit":201}))
        ),
        Err(CoreError::Validation)
    ));
}

#[test]
fn snapshot_publishes_one_complete_generation_with_hashes_and_preserves_previous_files() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    record(&mut s, "示例");
    let first = create(&s).unwrap();
    assert!(first.root_instructions_created);
    let manifest_bytes = fs::read(Path::new(&first.path).join("manifest.json")).unwrap();
    let manifest: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest["version"], 1);
    assert_eq!(
        manifest["warehouse_id"],
        s.summary().warehouse_id.to_string()
    );
    for (name, entry) in manifest["files"].as_object().unwrap() {
        let bytes = fs::read(Path::new(&first.path).join(name)).unwrap();
        assert_eq!(entry["size_bytes"], bytes.len());
        assert_eq!(entry["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    }
    fs::write(s.root().join("AGENTS.md"), "user instructions - keep me").unwrap();
    fs::create_dir(s.root().join("agent-access/.pending-synthetic-failure")).unwrap();
    let second = create(&s).unwrap();
    assert!(!second.root_instructions_created);
    assert_ne!(first.path, second.path);
    assert!(!second.warnings.is_empty());
    assert_eq!(
        fs::read(Path::new(&first.path).join("manifest.json")).unwrap(),
        manifest_bytes
    );
    assert_eq!(
        fs::read_to_string(s.root().join("AGENTS.md")).unwrap(),
        "user instructions - keep me"
    );
    assert!(
        s.root()
            .join("agent-access/.pending-synthetic-failure")
            .is_dir()
    );
    let read = warehouse::open(s.root(), WarehouseAccessMode::ReadOnly).unwrap();
    assert!(matches!(create(&read), Err(CoreError::ReadOnlyWarehouse)));
}

#[test]
fn unsafe_indexed_paths_fail_closed_and_document_resolution_checks_live_files() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = record(&mut s, "示例");
    let file = s
        .root()
        .join(&a.record.folder_relative_path)
        .join("简历.pdf");
    fs::write(&file, b"synthetic").unwrap();
    applications::scan_all_documents(&mut s).unwrap();
    let doc = applications::get(&s, &a.record.id).unwrap().documents[0]
        .id
        .clone();
    let req = || {
        request(
            json!({"operation":"resolve_document","application_id":a.record.id,"document_id":doc}),
        )
    };
    let path = query(&s, req()).unwrap();
    assert_eq!(
        Path::new(path["result"]["resolved_path"].as_str().unwrap()),
        fs::canonicalize(&file).unwrap()
    );
    fs::rename(&file, file.with_extension("moved")).unwrap();
    assert!(matches!(query(&s, req()), Err(CoreError::FileMissing)));
    assert!(!collect(&s).unwrap().applications[0].documents[0].indexed_missing);
    for bad in [
        "../outside.pdf",
        "C:/outside.pdf",
        "resume.pdf:secret",
        "sub/NUL.pdf",
    ] {
        s.connection_mut()
            .unwrap()
            .execute(
                "UPDATE documents SET relative_path=?1 WHERE id=?2",
                (bad, &doc),
            )
            .unwrap();
        assert!(matches!(collect(&s), Err(CoreError::UnsafePath)));
        assert!(matches!(query(&s, req()), Err(CoreError::UnsafePath)));
    }
}

#[cfg(windows)]
#[test]
fn junction_snapshot_destination_is_rejected_without_touching_outside_files() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let s = warehouse::create(root.path()).unwrap();
    // All operations target this test's temporary tree; preserve the original empty directory.
    fs::rename(
        s.root().join("agent-access"),
        s.root().join("original-agent-access"),
    )
    .unwrap();
    let output = std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
        .env("OFFERTRACK_TEST_LINK",s.root().join("agent-access")).env("OFFERTRACK_TEST_TARGET",outside.path()).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(matches!(create(&s), Err(CoreError::UnsafePath)));
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn tasks_events_include_general_completed_and_archived_but_not_deleted_sources() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = record(&mut s, "归档示例");
    let b = record(&mut s, "删除示例");
    for id in [None, Some(a.record.id.clone()), Some(b.record.id.clone())] {
        let t = tasks::save(
            &mut s,
            &tasks::SaveTask {
                id: None,
                revision: None,
                application_id: id,
                title: "待办标题".into(),
                notes: "完整待办备注".into(),
                priority: "high".into(),
                due_at_utc: None,
                remind_at_utc: None,
            },
        )
        .unwrap();
        if t.application_id.is_none() {
            tasks::complete(&mut s, &t.id, t.revision, true).unwrap();
        }
    }
    for id in [&a.record.id, &b.record.id] {
        recruitment::save(
            &mut s,
            &recruitment::SaveEvent {
                id: None,
                revision: None,
                application_id: id.clone(),
                event_type: "assessment".into(),
                title: "测评标题".into(),
                notes: "完整测评备注".into(),
                starts_at_utc: Some("2026-09-03T08:00:00Z".into()),
                deadline_at_utc: None,
                interview_round_id: None,
                location: "线上".into(),
                meeting_url: Some("https://example.com/meeting".into()),
                result: "测评结果".into(),
            },
        )
        .unwrap();
    }
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET archived_at_utc=created_at_utc WHERE id=?1",
            [&a.record.id],
        )
        .unwrap();
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET deleted_at_utc=created_at_utc WHERE id=?1",
            [&b.record.id],
        )
        .unwrap();
    let data = collect(&s).unwrap();
    assert_eq!(data.tasks.len(), 2);
    assert_eq!(data.events.len(), 1);
    assert_eq!(data.summary.open_tasks, 1);
    assert_eq!(data.summary.open_events, 1);
    assert!(
        data.tasks
            .iter()
            .any(|t| t.application_id.is_none() && t.completed_at_utc.is_some())
    );
    assert_eq!(data.events[0].notes, "完整测评备注");
    assert_eq!(data.events[0].result, "测评结果");
    assert!(data.events[0].application_archived);
    let schema = dto::schema();
    assert_schema(
        &serde_json::to_value(&data.tasks[0]).unwrap(),
        &schema["$defs"]["Task"],
    );
    assert_schema(
        &serde_json::to_value(&data.events[0]).unwrap(),
        &schema["$defs"]["Event"],
    );
}

#[test]
fn projection_is_one_transaction_even_when_another_connection_commits_changes() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = record(&mut s, "事务示例");
    tasks::save(
        &mut s,
        &tasks::SaveTask {
            id: None,
            revision: None,
            application_id: Some(a.record.id),
            title: "title".into(),
            notes: "".into(),
            priority: "normal".into(),
            due_at_utc: None,
            remind_at_utc: None,
        },
    )
    .unwrap();
    let path = s.root().join("offertrack.sqlite");
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..25 {
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE applications SET notes=?1", [index.to_string()])
                .unwrap();
            tx.execute("UPDATE tasks SET notes=?1", [index.to_string()])
                .unwrap();
            tx.commit().unwrap();
        }
    });
    for _ in 0..25 {
        let data = collect(&s).unwrap();
        assert_eq!(data.applications[0].record.notes, data.tasks[0].notes);
    }
    writer.join().unwrap();
}

#[test]
fn source_budgets_fail_before_loading_text_and_do_not_leave_a_transaction_open() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    record(&mut s, "预算示例");
    assert!(matches!(
        check_budget(s.connection(), MAX_ITEMS, 1),
        Err(CoreError::AgentLimit)
    ));
    assert!(matches!(
        check_budget(s.connection(), 0, MAX_BYTES),
        Err(CoreError::AgentLimit)
    ));
    check_budget(s.connection(), MAX_ITEMS, MAX_BYTES).unwrap();
    assert!(s.connection().is_autocommit());
    assert_eq!(collect(&s).unwrap().applications.len(), 1);
    assert!(s.connection().is_autocommit());
}
