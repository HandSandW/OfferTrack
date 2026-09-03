//! Phase-two acceptance journey through the same services used by Tauri.
//! Only synthetic data in a dedicated temporary warehouse; no direct SQL edits.
use std::fs;

use crate::{applications as app, domain::*, recycle_bin, warehouse};

#[test]
fn mvp_journey_from_preparing_to_offer_files_copy_archive_restore_and_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let mut record = app::create(
        &mut session,
        CreateApplicationRequest {
            company_name: "验收示例公司".into(),
            position_name: "开发工程师".into(),
            company_type: "private".into(),
            industry: "软件".into(),
            position_category: "研发".into(),
            work_location: "厦门".into(),
        },
    )
    .unwrap();
    assert_eq!(record.record.current_stage_name, "准备投递");
    assert!(record.record.application_date.is_none());
    let id = record.record.id.clone();
    let created = record.record.created_at_utc.clone();
    let folder = temp.path().join(&record.record.folder_relative_path);
    fs::create_dir(folder.join("材料")).unwrap();
    fs::write(folder.join("材料/resume.pdf"), b"synthetic version one").unwrap();
    let docs = app::scan_documents(&mut session, &id).unwrap();
    assert_eq!(docs.len(), 1);
    // External rename/edit/move out and back is observed, not treated as deletion of the record.
    fs::rename(
        folder.join("材料/resume.pdf"),
        folder.join("材料/final.pdf"),
    )
    .unwrap();
    fs::write(folder.join("材料/final.pdf"), b"synthetic final").unwrap();
    let docs = app::scan_documents(&mut session, &id).unwrap();
    assert_eq!(docs.iter().filter(|doc| !doc.missing).count(), 1);
    fs::rename(
        folder.join("材料/final.pdf"),
        temp.path().join("outside.pdf"),
    )
    .unwrap();
    assert!(
        app::scan_documents(&mut session, &id)
            .unwrap()
            .iter()
            .all(|doc| doc.missing)
    );
    fs::rename(
        temp.path().join("outside.pdf"),
        folder.join("材料/final.pdf"),
    )
    .unwrap();
    app::scan_documents(&mut session, &id).unwrap();
    let mut edit: UpdateApplicationRequest =
        serde_json::from_value(serde_json::to_value(&record.record).unwrap()).unwrap();
    edit.notes = "已阅读招聘公告".into();
    edit.application_url = Some("https://example.com/jobs".into());
    edit.tags = vec!["重点关注".into()];
    record = app::update(&mut session, edit).unwrap();
    let stages = record.stages.clone();
    for stage in stages.iter().filter(|stage| {
        stage.stable_key != "preparing" && stage.terminal_outcome.as_deref() != Some("failed")
    }) {
        record = app::change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: id.clone(),
                stage_id: stage.id.clone(),
                stage_state: "completed".into(),
                revision: record.record.revision,
                notes: format!("完成 {}", stage.display_name),
            },
        )
        .unwrap();
        if stage.stable_key == "applied" {
            assert!(record.record.application_date.is_some());
        }
    }
    assert_eq!(record.record.current_stage_progress, 100);
    assert_eq!(record.record.created_at_utc, created);
    for name in ["技术一面", "HR 面"] {
        record = app::save_interview_round(
            &mut session,
            InterviewRoundRequest {
                application_id: id.clone(),
                revision: record.record.revision,
                id: None,
                display_name: name.into(),
                state: "completed".into(),
                scheduled_at_utc: None,
                completed_at_utc: None,
                result: "通过".into(),
                notes: "虚构验收数据".into(),
            },
        )
        .unwrap();
    }
    let copied = app::duplicate(&mut session, &id, DuplicateMode::FullRecord).unwrap();
    assert_ne!(copied.record.id, id);
    assert_ne!(
        copied.record.folder_relative_path,
        record.record.folder_relative_path
    );
    assert_eq!(copied.record.current_stage_name, "准备投递");
    assert!(copied.record.application_date.is_none());
    assert_eq!(copied.interview_rounds.len(), 2);
    let copy_path = temp
        .path()
        .join(&copied.record.folder_relative_path)
        .join("材料/final.pdf");
    assert_eq!(fs::read(&copy_path).unwrap(), b"synthetic final");
    fs::write(&copy_path, b"independent copy").unwrap();
    assert_eq!(
        fs::read(folder.join("材料/final.pdf")).unwrap(),
        b"synthetic final"
    );
    app::set_page_size(&mut session, 100).unwrap();
    app::save_view(&mut session, SavedViewRequest {
        id: None, revision: None, name: "验收视图".into(), is_default: true,
        layout: serde_json::json!({"columns":[{"key":"companyName","width":240,"visible":true,"pinned":true}]}),
        sort: serde_json::json!([{"key":"createdAtUtc","direction":"desc"}]),
        filter: serde_json::json!({"search":"验收","companyTypes":["private"],"stages":[]}),
        group: Some(serde_json::json!("companyType")),
    }).unwrap();
    app::set_archived(&mut session, &id, true).unwrap();
    assert_eq!(
        app::list(&session, ApplicationScope::Archived)
            .unwrap()
            .len(),
        1
    );
    app::set_archived(&mut session, &id, false).unwrap();
    recycle_bin::move_application_to_trash(&mut session, &id).unwrap();
    assert!(!folder.exists());
    let restored = recycle_bin::restore_application(&mut session, &id).unwrap();
    assert!(!restored.renamed);
    drop(session);
    let reopened = warehouse::open(temp.path(), warehouse::WarehouseAccessMode::Write).unwrap();
    let final_record = app::get(&reopened, &id).unwrap();
    assert_eq!(final_record.record.current_stage_progress, 100);
    assert_eq!(final_record.record.notes, "已阅读招聘公告");
    assert_eq!(final_record.record.tags[0].name, "重点关注");
    assert_eq!(final_record.interview_rounds.len(), 2);
    assert_eq!(final_record.history.len(), record.history.len());
    assert_eq!(
        app::list(&reopened, ApplicationScope::Active)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(app::page_size(&reopened).unwrap(), 100);
    let views = app::list_views(&reopened).unwrap();
    assert!(views[0].is_default);
    assert_eq!(views[0].layout["columns"][0]["width"], 240);
    assert_eq!(
        fs::read(folder.join("材料/final.pdf")).unwrap(),
        b"synthetic final"
    );
}
