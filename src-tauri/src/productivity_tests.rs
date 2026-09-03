use super::*;
use crate::{
    applications,
    domain::{ChangeStageRequest, CreateApplicationRequest},
    warehouse::{self, WarehouseAccessMode},
};

fn create(session: &mut WarehouseSession) -> crate::domain::ApplicationDetail {
    applications::create(
        session,
        CreateApplicationRequest {
            company_name: "统计示例公司".into(),
            position_name: "工程师".into(),
            company_type: "private".into(),
            industry: "软件".into(),
            position_category: "研发".into(),
            work_location: "厦门".into(),
        },
    )
    .unwrap()
}
fn task() -> tasks::SaveTask {
    tasks::SaveTask {
        id: None,
        revision: None,
        application_id: None,
        title: "准备作品集".into(),
        notes: "通用事项".into(),
        priority: "high".into(),
        due_at_utc: Some("2026-09-04T12:00:00+08:00".into()),
        remind_at_utc: None,
    }
}
fn clock() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-03T12:00:00+08:00").unwrap()
}

#[test]
fn rule_save_is_atomic_and_readonly_cannot_change_rules_or_responses() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let initial = tasks::rules(session.connection()).unwrap();
    let mut edits = initial.clone();
    edits[0].value = 9;
    edits[1].value = 9;
    session.connection_mut().unwrap().execute_batch("CREATE TRIGGER reject_second_rule BEFORE UPDATE ON reminder_rules WHEN OLD.stable_key='preparing_idle' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(tasks::save_rules(&mut session, &edits).is_err());
    assert_eq!(tasks::rules(session.connection()).unwrap()[0].value, 3);
    session
        .connection_mut()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_second_rule")
        .unwrap();
    tasks::save_rules(&mut session, &edits).unwrap();
    drop(session);
    let mut readonly = warehouse::open(root.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(tasks::rules(readonly.connection()).unwrap()[0].value, 9);
    assert!(matches!(
        tasks::save_rules(&mut readonly, &initial),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        respond(&mut readonly, "key", "hash", false),
        Err(CoreError::ReadOnlyWarehouse)
    ));
}

#[test]
fn interview_schedule_excludes_finished_archived_and_terminal_records() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let a = create(&mut session);
    session.connection_mut().unwrap().execute("INSERT INTO interview_rounds (id,application_id,sequence_number,display_name,state,scheduled_at_utc,created_at_utc,updated_at_utc) VALUES ('round',?1,1,'主管面','awaitingParticipation','2026-09-03T05:00:00Z','2026-09-03T00:00:00Z','2026-09-03T00:00:00Z')",[&a.record.id]).unwrap();
    let overview = get(&session, clock()).unwrap();
    assert_eq!(overview.interviews.len(), 1);
    assert!(
        overview
            .reminders
            .iter()
            .any(|r| r.source_kind == "interview" && r.severity == "urgent")
    );
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE interview_rounds SET state='completed' WHERE id='round'",
            [],
        )
        .unwrap();
    assert!(get(&session, clock()).unwrap().interviews.is_empty());
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE interview_rounds SET state='awaitingParticipation' WHERE id='round'",
            [],
        )
        .unwrap();
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET archived_at_utc='2026-09-03T00:00:00Z' WHERE id=?1",
            [&a.record.id],
        )
        .unwrap();
    assert!(get(&session, clock()).unwrap().interviews.is_empty());
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET archived_at_utc=NULL,current_stage_state='failed' WHERE id=?1",
            [a.record.id],
        )
        .unwrap();
    assert!(get(&session, clock()).unwrap().interviews.is_empty());
}

#[test]
fn database_snapshot_restores_tasks_rules_and_reminder_actions() {
    let root = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let saved = tasks::save(&mut session, &task()).unwrap();
    let data = get(&session, chrono::Local::now().fixed_offset()).unwrap();
    let item = data
        .reminders
        .iter()
        .find(|r| r.rule_key == "priority")
        .unwrap();
    respond(&mut session, &item.key, &item.fingerprint, false).unwrap();
    let mut rules = tasks::rules(session.connection()).unwrap();
    rules[0].value = 5;
    tasks::save_rules(&mut session, &rules).unwrap();
    let backup = crate::database_backup::create(&session).unwrap().backup;
    tasks::complete(&mut session, &saved.id, saved.revision, true).unwrap();
    let restored = crate::database_backup::restore(
        &session,
        &backup.id.to_string(),
        false,
        &backup.sha256,
        output.path(),
    )
    .unwrap();
    let restored = warehouse::open(
        std::path::Path::new(&restored.directory),
        WarehouseAccessMode::ReadOnly,
    )
    .unwrap();
    assert!(
        tasks::list(restored.connection()).unwrap()[0]
            .completed_at_utc
            .is_none()
    );
    assert_eq!(tasks::rules(restored.connection()).unwrap()[0].value, 5);
    assert!(
        !get(&restored, chrono::Local::now().fixed_offset())
            .unwrap()
            .reminders
            .iter()
            .any(|r| r.rule_key == "priority")
    );
    assert!(
        tasks::list(session.connection()).unwrap()[0]
            .completed_at_utc
            .is_some()
    );
}

#[test]
fn task_edit_complete_reopen_conflicts_and_readonly_preserve_data() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let record = create(&mut session);
    let first = tasks::save(&mut session, &task()).unwrap();
    assert!(first.application_id.is_none());
    assert_eq!(
        first.due_at_utc.as_deref(),
        Some("2026-09-04T04:00:00.000Z")
    );
    let mut edit = task();
    edit.id = Some(first.id.clone());
    edit.revision = Some(first.revision);
    edit.application_id = Some(record.record.id.clone());
    let changed = tasks::save(&mut session, &edit).unwrap();
    assert!(changed.application_label.unwrap().contains("示例公司"));
    assert!(matches!(
        tasks::save(&mut session, &edit),
        Err(CoreError::RevisionConflict)
    ));
    let done = tasks::complete(&mut session, &first.id, changed.revision, true).unwrap();
    assert!(done.completed_at_utc.is_some());
    assert_eq!(
        tasks::complete(&mut session, &first.id, done.revision, true)
            .unwrap()
            .revision,
        done.revision
    );
    let reopened = tasks::complete(&mut session, &first.id, done.revision, false).unwrap();
    assert!(reopened.completed_at_utc.is_none());
    drop(session);
    let mut readonly = warehouse::open(root.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(
        tasks::list(readonly.connection()).unwrap()[0].notes,
        "通用事项"
    );
    assert!(matches!(
        tasks::save(&mut readonly, &task()),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        tasks::complete(&mut readonly, &first.id, reopened.revision, true),
        Err(CoreError::ReadOnlyWarehouse)
    ));
}

#[test]
fn invalid_task_input_deleted_targets_and_sql_failure_never_commit() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    for mutate in [0, 1, 2, 3, 4] {
        let mut invalid = task();
        match mutate {
            0 => invalid.title = " ".into(),
            1 => invalid.due_at_utc = Some("2026-02-30".into()),
            2 => invalid.application_id = Some("missing".into()),
            3 => invalid.priority = "injected".into(),
            _ => invalid.revision = Some(5),
        };
        assert!(tasks::save(&mut session, &invalid).is_err());
    }
    assert!(tasks::list(session.connection()).unwrap().is_empty());
    let record = create(&mut session);
    let mut input = task();
    input.application_id = Some(record.record.id.clone());
    let saved = tasks::save(&mut session, &input).unwrap();
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET deleted_at_utc='2026-09-03T00:00:00Z' WHERE id=?1",
            [record.record.id],
        )
        .unwrap();
    assert!(tasks::list(session.connection()).unwrap().is_empty());
    input.id = Some(saved.id.clone());
    input.revision = Some(saved.revision);
    input.application_id = None;
    assert!(tasks::save(&mut session, &input).is_err());
    assert!(tasks::complete(&mut session, &saved.id, saved.revision, true).is_err());
    session.connection_mut().unwrap().execute_batch("CREATE TRIGGER fail_task BEFORE INSERT ON tasks BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(tasks::save(&mut session, &task()).is_err());
    assert!(serde_json::from_value::<tasks::SaveTask>(serde_json::json!({"id":null,"revision":null,"applicationId":null,"title":"test","notes":"","priority":"normal","dueAtUtc":null,"remindAtUtc":null,"path":"arbitrary"})).is_err());
}

#[test]
fn overview_counts_dates_scope_distribution_and_history_are_traceable() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let a = create(&mut session);
    let b = create(&mut session);
    let c = create(&mut session);
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET archived_at_utc='2026-09-03T00:00:00Z' WHERE id=?1",
            [b.record.id],
        )
        .unwrap();
    session
        .connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET deleted_at_utc='2026-09-03T00:00:00Z' WHERE id=?1",
            [c.record.id],
        )
        .unwrap();
    session.connection_mut().unwrap().execute("UPDATE applications SET created_at_utc='2026-09-02T17:00:00Z',application_date='2026-09-02' WHERE id=?1",[&a.record.id]).unwrap();
    let overview = get(&session, clock()).unwrap();
    assert_eq!(overview.metrics[0].ids, vec![a.record.id.clone()]);
    assert_eq!(
        overview.trend.last().unwrap().created_ids,
        vec![a.record.id.clone()]
    );
    assert_eq!(overview.trend[28].applied_ids, vec![a.record.id.clone()]);
    assert!(overview.funnel.iter().all(|b| b.ids.is_empty()));
    let stage = a
        .stages
        .iter()
        .find(|s| s.stable_key == "interview")
        .unwrap();
    applications::change_stage(
        &mut session,
        ChangeStageRequest {
            application_id: a.record.id.clone(),
            revision: a.record.revision,
            stage_id: stage.id.clone(),
            stage_state: "awaitingResult".into(),
            notes: "直接进入面试".into(),
        },
    )
    .unwrap();
    let overview = get(&session, clock()).unwrap();
    assert_eq!(overview.metrics[3].ids, vec![a.record.id.clone()]);
    assert_eq!(overview.funnel[3].ids, vec![a.record.id]);
    assert!(overview.funnel[0].ids.is_empty()); // Never invent skipped stages or probabilities.
    assert_eq!(overview.industries[0].label, "软件");
}

#[test]
fn reminders_use_semantic_states_resume_index_boundaries_and_rule_changes() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let a = create(&mut session);
    session.connection_mut().unwrap().execute("UPDATE applications SET created_at_utc='2026-08-31T04:00:00Z',updated_at_utc='2026-08-24T04:00:00Z',status_updated_at_utc='2026-08-24T04:00:00Z',current_stage_id=(SELECT id FROM workflow_stages WHERE application_id=?1 AND stable_key='interview'),current_stage_state='awaitingResult' WHERE id=?1",[&a.record.id]).unwrap();
    session.connection_mut().unwrap().execute("UPDATE workflow_states SET display_name='待主管回复' WHERE application_id=?1 AND stable_key='awaitingResult'",[&a.record.id]).unwrap();
    let data = get(&session, clock()).unwrap();
    assert!(
        data.reminders
            .iter()
            .any(|r| r.rule_key == "missing_resume")
    );
    assert!(data.reminders.iter().any(|r| r.rule_key == "result_idle"));
    assert!(!data.reminders.iter().any(|r| r.rule_key == "stage_idle"));
    let path = root
        .path()
        .join(&a.record.folder_relative_path)
        .join("简历.PDF");
    std::fs::write(&path, b"synthetic").unwrap();
    applications::scan_documents(&mut session, &a.record.id).unwrap();
    assert!(
        !get(&session, clock())
            .unwrap()
            .reminders
            .iter()
            .any(|r| r.rule_key == "missing_resume")
    );
    let mut rules = tasks::rules(session.connection()).unwrap();
    let old = rules.clone();
    rules
        .iter_mut()
        .find(|r| r.key == "result_idle")
        .unwrap()
        .enabled = false;
    tasks::save_rules(&mut session, &rules).unwrap();
    assert!(
        get(&session, clock())
            .unwrap()
            .reminders
            .iter()
            .any(|r| r.rule_key == "stage_idle")
    );
    assert!(matches!(
        tasks::save_rules(&mut session, &old),
        Err(CoreError::RevisionConflict)
    ));
    let mut invalid = tasks::rules(session.connection()).unwrap();
    invalid[0].value = 0;
    assert!(tasks::save_rules(&mut session, &invalid).is_err());
}

#[test]
fn deadlines_overdue_completed_and_manual_reminders_have_exact_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let mut input = task();
    input.remind_at_utc = Some("2026-09-03T12:00:00+08:00".into());
    let saved = tasks::save(&mut session, &input).unwrap();
    let data = get(&session, clock()).unwrap();
    assert!(data.reminders.iter().any(|r| r.rule_key == "due_urgent"));
    assert!(data.reminders.iter().any(|r| r.rule_key == "manual"));
    assert!(!data.reminders.iter().any(|r| r.rule_key == "overdue"));
    assert!(
        get(
            &session,
            clock() + Duration::hours(24) + Duration::milliseconds(1)
        )
        .unwrap()
        .reminders
        .iter()
        .any(|r| r.rule_key == "overdue")
    );
    tasks::complete(&mut session, &saved.id, saved.revision, true).unwrap();
    assert!(
        get(&session, clock() + Duration::days(9))
            .unwrap()
            .reminders
            .is_empty()
    );
    let mut rules = tasks::rules(session.connection()).unwrap();
    rules.iter_mut().for_each(|r| r.enabled = false);
    tasks::save_rules(&mut session, &rules).unwrap();
    tasks::complete(&mut session, &saved.id, saved.revision + 1, false).unwrap();
    assert!(
        get(&session, clock() + Duration::days(9))
            .unwrap()
            .reminders
            .iter()
            .all(|r| ["manual", "priority"].contains(&r.rule_key.as_str()))
    );
}

#[test]
fn reminder_response_is_revision_bound_persistent_and_non_destructive() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let saved = tasks::save(&mut session, &task()).unwrap();
    let data = get(&session, chrono::Local::now().fixed_offset()).unwrap();
    let item = data
        .reminders
        .iter()
        .find(|r| r.rule_key == "priority")
        .unwrap();
    assert!(matches!(
        respond(&mut session, &item.key, "stale", false),
        Err(CoreError::RevisionConflict)
    ));
    respond(&mut session, &item.key, &item.fingerprint, true).unwrap();
    assert!(
        !get(&session, chrono::Local::now().fixed_offset())
            .unwrap()
            .reminders
            .iter()
            .any(|r| r.key == item.key)
    );
    assert!(
        get(
            &session,
            chrono::Local::now().fixed_offset() + Duration::hours(25)
        )
        .unwrap()
        .reminders
        .iter()
        .any(|r| r.key == item.key)
    );
    let mut edit = task();
    edit.id = Some(saved.id);
    edit.revision = Some(saved.revision);
    edit.notes = "已跟进".into();
    tasks::save(&mut session, &edit).unwrap();
    let data = get(&session, chrono::Local::now().fixed_offset()).unwrap();
    let item = data
        .reminders
        .iter()
        .find(|r| r.rule_key == "priority")
        .unwrap();
    respond(&mut session, &item.key, &item.fingerprint, false).unwrap();
    drop(session);
    let session = warehouse::open(root.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert!(
        !get(&session, chrono::Local::now().fixed_offset())
            .unwrap()
            .reminders
            .iter()
            .any(|r| r.rule_key == "priority")
    );
    assert!(
        tasks::list(session.connection()).unwrap()[0]
            .completed_at_utc
            .is_none()
    );
}
