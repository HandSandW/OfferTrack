use super::*;
use crate::{domain::*, warehouse};
use std::fs;

fn record(session: &mut WarehouseSession, name: &str) -> ApplicationDetail {
    applications::create(
        session,
        CreateApplicationRequest {
            company_name: name.into(),
            position_name: "研发岗位".into(),
            company_type: "private".into(),
            industry: "软件".into(),
            position_category: String::new(),
            work_location: String::new(),
        },
    )
    .unwrap()
}
fn request(records: &[&ApplicationDetail], action: Action) -> Request {
    Request {
        version: 1,
        targets: records
            .iter()
            .map(|r| Target {
                id: r.record.id.clone(),
                revision: r.record.revision,
            })
            .collect(),
        action,
    }
}
fn save(session: &mut WarehouseSession, request: &Request) -> Applied {
    let reviewed = preview(session, request).unwrap();
    apply(session, request, &reviewed.fingerprint).unwrap()
}
fn snapshot(session: &WarehouseSession, id: &str) -> serde_json::Value {
    serde_json::to_value(applications::get(session, id).unwrap()).unwrap()
}

#[test]
fn preview_is_rolled_back_and_batch_backup_contains_previous_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let a = record(&mut session, "示例甲");
    let b = record(&mut session, "示例乙");
    let before = snapshot(&session, &a.record.id);
    let req = request(
        &[&a, &b],
        Action::AddTags {
            tags: vec!["重点".into(), "重点".into(), "校招".into()],
        },
    );
    let first = preview(&mut session, &req).unwrap();
    assert_eq!(first.changed_count, 2);
    assert_eq!(
        first.fingerprint,
        preview(&mut session, &req).unwrap().fingerprint
    );
    assert_eq!(before, snapshot(&session, &a.record.id));
    assert!(database_backup::catalog(&session).unwrap().items.is_empty());
    let applied = apply(&mut session, &req, &first.fingerprint).unwrap();
    let catalog = database_backup::catalog(&session).unwrap();
    assert_eq!(catalog.items.len(), 1);
    let backup_id = applied.backup_id.unwrap();
    let verified = database_backup::preview(&session, &backup_id.to_string(), false).unwrap();
    assert_eq!(verified.backup.reason, "beforeBatch");
    let old = database_backup::read_database(
        &temp
            .path()
            .join("backups/database")
            .join(backup_id.to_string())
            .join("database.sqlite"),
    )
    .unwrap();
    assert_eq!(
        applications::load_record(&old, &a.record.id)
            .unwrap()
            .revision,
        a.record.revision
    );
    assert!(
        applications::load_record(&old, &a.record.id)
            .unwrap()
            .tags
            .is_empty()
    );
    let after = applications::get(&session, &a.record.id).unwrap();
    assert_eq!(after.record.tags.len(), 2);
    assert_eq!(
        after.record.status_updated_at_utc,
        a.record.status_updated_at_utc
    );
    let unchanged = save(&mut session, &request(&[&after], req.action));
    assert_eq!(unchanged.changed_count, 0);
    assert!(unchanged.backup_id.is_none());
}

#[test]
fn one_stale_target_or_fingerprint_mismatch_never_partially_commits() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let a = record(&mut session, "示例甲");
    let b = record(&mut session, "示例乙");
    let req = request(&[&a, &b], Action::Archive { archived: true });
    let reviewed = preview(&mut session, &req).unwrap();
    assert!(matches!(
        apply(&mut session, &req, "wrong"),
        Err(CoreError::RevisionConflict)
    ));
    applications::set_archived(&mut session, &b.record.id, true).unwrap();
    assert!(matches!(
        apply(&mut session, &req, &reviewed.fingerprint),
        Err(CoreError::RevisionConflict)
    ));
    assert_eq!(
        snapshot(&session, &a.record.id),
        serde_json::to_value(&a).unwrap()
    );
    assert!(database_backup::catalog(&session).unwrap().items.is_empty());
    let fresh = applications::get(&session, &b.record.id).unwrap();
    save(
        &mut session,
        &request(&[&a, &fresh], Action::Archive { archived: false }),
    );
    assert!(
        applications::get(&session, &b.record.id)
            .unwrap()
            .record
            .archived_at_utc
            .is_none()
    );
}

#[test]
fn backup_failure_and_readonly_mode_block_batch_without_touching_files() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let a = record(&mut session, "备份故障示例");
    let path = temp
        .path()
        .join(&a.record.folder_relative_path)
        .join("resume.pdf");
    fs::write(&path, b"synthetic resume").unwrap();
    let req = request(&[&a], Action::Archive { archived: true });
    let reviewed = preview(&mut session, &req).unwrap();
    fs::rename(
        temp.path().join("backups/database"),
        temp.path().join("database-kept"),
    )
    .unwrap();
    fs::write(temp.path().join("backups/database"), b"blocked").unwrap();
    assert!(apply(&mut session, &req, &reviewed.fingerprint).is_err());
    assert_eq!(
        snapshot(&session, &a.record.id),
        serde_json::to_value(&a).unwrap()
    );
    let mut readonly =
        warehouse::open(temp.path(), warehouse::WarehouseAccessMode::ReadOnly).unwrap();
    assert!(matches!(
        apply(&mut readonly, &req, &reviewed.fingerprint),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"synthetic resume");
}

#[test]
fn progress_uses_shared_history_dates_terminal_semantics_and_skips_noops() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let a = record(&mut session, "进度甲");
    let b = record(&mut session, "进度乙");
    let stage = |key: &str| Action::Stage {
        stage_key: key.into(),
        state_key: "awaitingResult".into(),
    };
    save(&mut session, &request(&[&a, &b], stage("applied")));
    let a = applications::get(&session, &a.record.id).unwrap();
    assert!(a.record.application_date.is_some());
    assert_eq!(a.history.len(), 2); // Creation already records the initial stage.
    assert_eq!(
        save(&mut session, &request(&[&a], stage("applied"))).changed_count,
        0
    );
    // Once intentionally cleared, the first-application date is not filled again.
    let mut edit: UpdateApplicationRequest =
        serde_json::from_value(serde_json::to_value(&a.record).unwrap()).unwrap();
    edit.tags = vec![];
    edit.application_date = None;
    let a = applications::update(&mut session, edit).unwrap();
    save(&mut session, &request(&[&a], stage("interview")));
    let a = applications::get(&session, &a.record.id).unwrap();
    save(&mut session, &request(&[&a], stage("failed_terminal")));
    let failed = applications::get(&session, &a.record.id).unwrap();
    assert_eq!(failed.record.current_stage_id, a.record.current_stage_id);
    assert_eq!(failed.record.current_stage_state, "failed");
    assert_eq!(
        failed.record.current_stage_progress,
        a.record.current_stage_progress
    );
    save(&mut session, &request(&[&failed], stage("applied")));
    let a = applications::get(&session, &a.record.id).unwrap();
    assert!(a.record.application_date.is_none());
    save(&mut session, &request(&[&a], stage("offer")));
    let a = applications::get(&session, &a.record.id).unwrap();
    assert_eq!(a.record.current_stage_progress, 100);
    assert_eq!(a.record.current_stage_state, "completed");
}

fn extended_template(
    session: &mut WarehouseSession,
    source: &ApplicationDetail,
) -> WorkflowTemplateDetail {
    let source = applications::save_workflow_stage(
        session,
        WorkflowStageRequest {
            application_id: source.record.id.clone(),
            revision: source.record.revision,
            id: None,
            display_name: "主管沟通".into(),
            color: "#123456".into(),
            is_terminal: false,
            terminal_outcome: None,
        },
    )
    .unwrap();
    let mut states = source
        .auxiliary_states
        .iter()
        .map(|s| AuxiliaryStateEdit {
            id: Some(s.id.clone()),
            display_name: s.display_name.clone(),
            semantic_kind: s.semantic_kind.clone(),
        })
        .collect::<Vec<_>>();
    states.push(AuxiliaryStateEdit {
        id: None,
        display_name: "等主管反馈".into(),
        semantic_kind: "awaitingResult".into(),
    });
    let source = auxiliary_states::update_record(
        session,
        UpdateAuxiliaryStatesRequest {
            owner_id: source.record.id.clone(),
            revision: source.record.revision,
            states,
        },
    )
    .unwrap();
    let template =
        applications::save_workflow_as_template(session, &source.record.id, "批量测试模板", false)
            .unwrap();
    workflows::get_template(
        session,
        &template
            .iter()
            .find(|t| t.name == "批量测试模板")
            .unwrap()
            .id,
    )
    .unwrap()
}

#[test]
fn template_append_preserves_local_definitions_rounds_history_and_independent_ids() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let source = record(&mut session, "模板来源");
    let a = record(&mut session, "目标甲");
    let b = record(&mut session, "目标乙");
    let template = extended_template(&mut session, &source);
    let stage = a
        .stages
        .iter()
        .find(|s| s.stable_key == "interview")
        .unwrap();
    let a = applications::change_stage(
        &mut session,
        ChangeStageRequest {
            application_id: a.record.id.clone(),
            revision: a.record.revision,
            stage_id: stage.id.clone(),
            stage_state: "awaitingResult".into(),
            notes: "保留历史".into(),
        },
    )
    .unwrap();
    let stage = a
        .stages
        .iter()
        .find(|s| s.stable_key == "interview")
        .unwrap();
    let a = applications::save_workflow_stage(
        &mut session,
        WorkflowStageRequest {
            application_id: a.record.id.clone(),
            revision: a.record.revision,
            id: Some(stage.id.clone()),
            display_name: "当地公司面试".into(),
            color: "#654321".into(),
            is_terminal: false,
            terminal_outcome: None,
        },
    )
    .unwrap();
    let before = snapshot(&session, &a.record.id);
    let req = request(
        &[&a, &b],
        Action::AppendTemplate {
            template_id: template.template.id.clone(),
            revision: template.template.revision,
        },
    );
    let reviewed = preview(&mut session, &req).unwrap();
    assert_eq!(reviewed.items[0].changes.len(), 2);
    assert_eq!(snapshot(&session, &a.record.id), before);
    apply(&mut session, &req, &reviewed.fingerprint).unwrap();
    let after = applications::get(&session, &a.record.id).unwrap();
    let other = applications::get(&session, &b.record.id).unwrap();
    assert_eq!(after.record.current_stage_name, "当地公司面试");
    assert_eq!(
        after.record.status_updated_at_utc,
        a.record.status_updated_at_utc
    );
    assert_eq!(
        serde_json::to_value(&after.history).unwrap(),
        before["history"]
    );
    assert_eq!(
        serde_json::to_value(&after.interview_rounds).unwrap(),
        before["interviewRounds"]
    );
    let key = template
        .stages
        .iter()
        .find(|s| s.display_name == "主管沟通")
        .unwrap()
        .stable_key
        .clone();
    assert_ne!(
        after
            .stages
            .iter()
            .find(|s| s.stable_key == key)
            .unwrap()
            .id,
        other
            .stages
            .iter()
            .find(|s| s.stable_key == key)
            .unwrap()
            .id
    );
    assert_ne!(
        after.auxiliary_states.last().unwrap().id,
        other.auxiliary_states.last().unwrap().id
    );
    assert_eq!(
        save(&mut session, &request(&[&after], req.action)).changed_count,
        0
    );
}

#[test]
fn template_conflicts_rollback_earlier_targets_and_old_template_revision_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let source = record(&mut session, "模板来源");
    let a = record(&mut session, "目标甲");
    let b = record(&mut session, "目标乙");
    let template = extended_template(&mut session, &source);
    let b = applications::save_workflow_stage(
        &mut session,
        WorkflowStageRequest {
            application_id: b.record.id.clone(),
            revision: b.record.revision,
            id: None,
            display_name: "主管沟通".into(),
            color: "#123456".into(),
            is_terminal: false,
            terminal_outcome: None,
        },
    )
    .unwrap();
    let before = snapshot(&session, &a.record.id);
    let req = request(
        &[&a, &b],
        Action::AppendTemplate {
            template_id: template.template.id.clone(),
            revision: template.template.revision,
        },
    );
    assert!(matches!(
        preview(&mut session, &req),
        Err(CoreError::BatchConflict)
    ));
    assert_eq!(snapshot(&session, &a.record.id), before);
    let req = request(
        &[&a],
        Action::AppendTemplate {
            template_id: template.template.id.clone(),
            revision: template.template.revision - 1,
        },
    );
    assert!(matches!(
        preview(&mut session, &req),
        Err(CoreError::RevisionConflict)
    ));
}

#[test]
fn limits_duplicate_targets_unknown_fields_and_deleted_records_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let a = record(&mut session, "范围测试");
    let mut req = request(&[&a, &a], Action::Archive { archived: true });
    assert!(matches!(
        preview(&mut session, &req),
        Err(CoreError::Validation)
    ));
    req.targets.clear();
    assert!(preview(&mut session, &req).is_err());
    req.targets = (0..201)
        .map(|i| Target {
            id: i.to_string(),
            revision: 1,
        })
        .collect();
    assert!(matches!(
        preview(&mut session, &req),
        Err(CoreError::Validation)
    ));
    let req = request(&[&a], Action::Archive { archived: true });
    let mut value = serde_json::to_value(&req).unwrap();
    value["path"] = "/arbitrary".into();
    assert!(serde_json::from_value::<Request>(value).is_err());
    crate::recycle_bin::move_application_to_trash(&mut session, &a.record.id).unwrap();
    assert!(matches!(
        preview(&mut session, &req),
        Err(CoreError::NotFound)
    ));
}
