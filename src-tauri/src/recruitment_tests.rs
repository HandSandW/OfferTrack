use super::*;
use crate::{
    applications,
    domain::CreateApplicationRequest,
    overview,
    warehouse::{self, WarehouseAccessMode},
};

fn application(s: &mut WarehouseSession) -> crate::domain::ApplicationDetail {
    applications::create(
        s,
        CreateApplicationRequest {
            company_name: "日程示例".into(),
            position_name: "工程师".into(),
            company_type: "private".into(),
            industry: "软件".into(),
            position_category: "研发".into(),
            work_location: "厦门".into(),
        },
    )
    .unwrap()
}
fn request(id: &str) -> SaveEvent {
    SaveEvent {
        id: None,
        revision: None,
        application_id: id.into(),
        event_type: "assessment".into(),
        title: "完成测评".into(),
        notes: "虚构详细说明".into(),
        starts_at_utc: Some("2026-09-03T13:00:00+08:00".into()),
        deadline_at_utc: Some("2026-09-04T12:00:00+08:00".into()),
        interview_round_id: None,
        location: "线上".into(),
        meeting_url: Some("https://example.com/meeting".into()),
        result: "等待结果".into(),
    }
}
fn now() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339("2026-09-03T12:00:00+08:00").unwrap()
}
fn round(s: &mut WarehouseSession, id: &str) {
    s.connection_mut().unwrap().execute("INSERT INTO interview_rounds(id,application_id,sequence_number,display_name,state,scheduled_at_utc,created_at_utc,updated_at_utc) VALUES ('round',?1,1,'HR 面','awaitingParticipation','2026-09-03T05:00:00Z','2026-09-03T00:00:00Z','2026-09-03T00:00:00Z')",[id]).unwrap();
}

#[test]
fn event_roundtrip_revision_completion_readonly_and_failure_are_atomic() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = application(&mut s);
    let mut req = request(&a.record.id);
    let e = save(&mut s, &req).unwrap();
    assert_eq!(e.starts_at_utc.as_deref(), Some("2026-09-03T05:00:00.000Z"));
    req.id = Some(e.id.clone());
    req.revision = Some(e.revision);
    req.title = "改名测评".into();
    s.connection_mut().unwrap().execute_batch("CREATE TRIGGER event_fail BEFORE UPDATE ON recruitment_events BEGIN SELECT RAISE(ABORT,'injected');END;").unwrap();
    assert!(save(&mut s, &req).is_err());
    assert_eq!(list(s.connection()).unwrap()[0].title, e.title);
    assert!(complete(&mut s, &e.id, 1, true).is_err());
    assert!(!list(s.connection()).unwrap()[0].finished);
    s.connection_mut()
        .unwrap()
        .execute_batch("DROP TRIGGER event_fail")
        .unwrap();
    let edited = save(&mut s, &req).unwrap();
    assert!(matches!(
        save(&mut s, &req),
        Err(CoreError::RevisionConflict)
    ));
    let done = complete(&mut s, &e.id, edited.revision, true).unwrap();
    assert!(done.finished);
    assert_eq!(done.notes, req.notes);
    assert_eq!(
        complete(&mut s, &e.id, done.revision, true)
            .unwrap()
            .revision,
        done.revision
    );
    let reopened = complete(&mut s, &e.id, done.revision, false).unwrap();
    assert!(!reopened.finished);
    assert_eq!(reopened.created_at_utc, e.created_at_utc);
    drop(s);
    let mut readonly = warehouse::open(root.path(), WarehouseAccessMode::ReadOnly).unwrap();
    assert_eq!(list(readonly.connection()).unwrap()[0].result, req.result);
    assert!(matches!(
        save(&mut readonly, &req),
        Err(CoreError::ReadOnlyWarehouse)
    ));
    assert!(matches!(
        complete(&mut readonly, &e.id, reopened.revision, true),
        Err(CoreError::ReadOnlyWarehouse)
    ));
}

#[test]
fn rejects_invalid_input_foreign_or_duplicate_rounds_and_unknown_dto_fields() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = application(&mut s);
    let b = application(&mut s);
    round(&mut s, &a.record.id);
    let base = request(&a.record.id);
    let mut bad = base.clone();
    bad.meeting_url = Some("javascript:alert(1)".into());
    assert!(save(&mut s, &bad).is_err());
    bad = base.clone();
    bad.deadline_at_utc = Some("2020-01-01T00:00:00Z".into());
    assert!(save(&mut s, &bad).is_err());
    bad = base.clone();
    bad.starts_at_utc = Some("not-a-date".into());
    assert!(save(&mut s, &bad).is_err());
    bad = base.clone();
    bad.application_id = "missing".into();
    assert!(save(&mut s, &bad).is_err());
    bad = base.clone();
    bad.revision = Some(1);
    assert!(save(&mut s, &bad).is_err());
    let mut linked = base.clone();
    linked.event_type = "interview".into();
    linked.interview_round_id = Some("round".into());
    linked.starts_at_utc = None;
    linked.result.clear();
    linked.application_id = b.record.id;
    assert!(save(&mut s, &linked).is_err());
    linked.application_id = a.record.id;
    let e = save(&mut s, &linked).unwrap();
    assert!(save(&mut s, &linked).is_err());
    assert!(complete(&mut s, &e.id, 1, true).is_err());
    let mut json = serde_json::json!({"applicationId":"id","eventType":"other","title":"x","notes":"","location":"","result":"","deletePath":"outside"});
    assert!(serde_json::from_value::<SaveEvent>(json.clone()).is_err());
    json.as_object_mut().unwrap().remove("deletePath");
    assert!(serde_json::from_value::<SaveEvent>(json).is_ok());
    assert_eq!(list(s.connection()).unwrap().len(), 1);
}

#[test]
fn linked_round_is_single_source_and_cannot_be_deleted_until_unlinked() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = application(&mut s);
    round(&mut s, &a.record.id);
    let mut req = request(&a.record.id);
    req.event_type = "interview".into();
    req.interview_round_id = Some("round".into());
    req.starts_at_utc = None;
    req.result.clear();
    req.deadline_at_utc = None;
    let e = save(&mut s, &req).unwrap();
    let data = overview::get(&s, now()).unwrap();
    assert!(data.interviews.is_empty());
    assert_eq!(data.schedule.len(), 1);
    assert_eq!(data.schedule[0].source_id, e.id);
    assert_eq!(
        data.reminders
            .iter()
            .filter(|r| r.source_kind == "event")
            .count(),
        1
    );
    assert!(matches!(
        applications::delete_interview_round(&mut s, &a.record.id, "round", a.record.revision),
        Err(CoreError::EventRoundInUse)
    ));
    s.connection_mut().unwrap().execute_batch("UPDATE interview_rounds SET scheduled_at_utc=NULL,result='轮次结果',state='completed' WHERE id='round'").unwrap();
    let live = list(s.connection()).unwrap().remove(0);
    assert!(live.finished);
    assert!(live.starts_at_utc.is_none());
    assert_eq!(live.result, "轮次结果");
    assert_ne!(live.source_version, e.source_version);
    assert!(
        overview::get(&s, now())
            .unwrap()
            .reminders
            .iter()
            .all(|r| r.source_kind != "event")
    );
    req.id = Some(e.id.clone());
    req.revision = Some(e.revision);
    req.interview_round_id = None;
    req.starts_at_utc = Some("2026-09-04T00:00:00Z".into());
    save(&mut s, &req).unwrap();
    applications::delete_interview_round(&mut s, &a.record.id, "round", a.record.revision).unwrap();
    assert_eq!(list(s.connection()).unwrap()[0].id, e.id);
}

#[test]
fn event_visibility_and_reminders_follow_source_archive_delete_restore_and_terminal() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = application(&mut s);
    let e = save(&mut s, &request(&a.record.id)).unwrap();
    for column in ["archived_at_utc", "deleted_at_utc"] {
        s.connection_mut()
            .unwrap()
            .execute(
                &format!("UPDATE applications SET {column}='2026-09-03T00:00:00Z' WHERE id=?1"),
                [&a.record.id],
            )
            .unwrap();
        let visible = list(s.connection()).unwrap();
        assert_eq!(visible.len(), usize::from(column != "deleted_at_utc"));
        assert!(overview::get(&s, now()).unwrap().schedule.is_empty());
        if column == "deleted_at_utc" {
            assert!(complete(&mut s, &e.id, 1, true).is_err());
        }
        s.connection_mut()
            .unwrap()
            .execute(
                &format!("UPDATE applications SET {column}=NULL WHERE id=?1"),
                [&a.record.id],
            )
            .unwrap();
        assert_eq!(overview::get(&s, now()).unwrap().schedule.len(), 1);
    }
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET current_stage_state='failed' WHERE id=?1",
            [&a.record.id],
        )
        .unwrap();
    assert_eq!(list(s.connection()).unwrap().len(), 1);
    assert!(overview::get(&s, now()).unwrap().schedule.is_empty());
}

#[test]
fn deadlines_deduplicate_equal_instants_and_feedback_does_not_change_due_counts() {
    let root = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = application(&mut s);
    let mut req = request(&a.record.id);
    req.deadline_at_utc = req.starts_at_utc.clone();
    let e = save(&mut s, &req).unwrap();
    let data = overview::get(&s, now()).unwrap();
    assert_eq!(
        data.reminders
            .iter()
            .filter(|r| r.source_kind == "event")
            .count(),
        1
    );
    assert_eq!(data.due_metrics[1].keys, vec![format!("event:{}", e.id)]);
    // At the running clock this synthetic event may be past or future; acknowledge only a current reminder.
    let current = overview::get(&s, chrono::Local::now().fixed_offset()).unwrap();
    let current_item = current
        .reminders
        .iter()
        .find(|r| r.source_kind == "event")
        .unwrap();
    overview::respond(&mut s, &current_item.key, &current_item.fingerprint, false).unwrap();
    let after = overview::get(&s, chrono::Local::now().fixed_offset()).unwrap();
    assert!(after.reminders.iter().all(|r| r.key != current_item.key));
    assert_eq!(after.due_metrics[0].keys, current.due_metrics[0].keys);
    assert!(!list(s.connection()).unwrap()[0].finished);
    let mut rules = tasks::rules(s.connection()).unwrap();
    for r in &mut rules {
        r.enabled = false;
    }
    tasks::save_rules(&mut s, &rules).unwrap();
    assert!(overview::get(&s, now()).unwrap().reminders.is_empty());
    assert_eq!(
        overview::get(&s, now()).unwrap().due_metrics[1].keys.len(),
        1
    );
}

#[test]
fn schema_eight_upgrade_preserves_legacy_events_and_snapshot_restores_new_event_metadata() {
    let root = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(root.path()).unwrap();
    let a = application(&mut s);
    crate::migrations::fixture_remove_migration_nine(s.connection());
    s.connection_mut().unwrap().execute("INSERT INTO recruitment_events(id,application_id,event_type,title,notes,starts_at_utc,ends_at_utc,completed_at_utc,created_at_utc,updated_at_utc) VALUES ('legacy',?1,'other','旧事件','旧说明','2026-09-01T00:00:00Z','2026-09-02T00:00:00Z','2026-09-02T00:00:00Z','2026-09-01T00:00:00Z','2026-09-02T00:00:00Z')",[&a.record.id]).unwrap();
    drop(s);
    let mut s = warehouse::open(root.path(), WarehouseAccessMode::Write).unwrap();
    let old = list(s.connection()).unwrap().remove(0);
    assert_eq!(old.title, "旧事件");
    assert!(old.finished);
    assert_eq!(old.revision, 1);
    let end: String = s
        .connection()
        .query_row(
            "SELECT ends_at_utc FROM recruitment_events WHERE id='legacy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(end, "2026-09-02T00:00:00Z");
    let e = save(&mut s, &request(&a.record.id)).unwrap();
    let backup = crate::database_backup::create(&s).unwrap().backup;
    complete(&mut s, &e.id, e.revision, true).unwrap();
    let restored = crate::database_backup::restore(
        &s,
        &backup.id.to_string(),
        false,
        &backup.sha256,
        output.path(),
    )
    .unwrap();
    let copy = warehouse::open(
        std::path::Path::new(&restored.directory),
        WarehouseAccessMode::ReadOnly,
    )
    .unwrap();
    let from_backup = list(copy.connection())
        .unwrap()
        .into_iter()
        .find(|r| r.id == e.id)
        .unwrap();
    assert!(!from_backup.finished);
    assert_eq!(from_backup.meeting_url, e.meeting_url);
    assert_eq!(from_backup.notes, e.notes);
    assert!(
        list(s.connection())
            .unwrap()
            .iter()
            .find(|r| r.id == e.id)
            .unwrap()
            .finished
    );
}
