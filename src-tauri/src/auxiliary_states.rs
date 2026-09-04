//! Scoped state definitions. Keys survive renames/copies; IDs and ownership do not.
use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::{
    applications,
    domain::{
        ApplicationDetail, AuxiliaryState, UpdateAuxiliaryStatesRequest, WorkflowTemplateDetail,
    },
    error::CoreError,
    warehouse::WarehouseSession,
    workflows,
};

#[derive(Clone, Copy)]
pub(crate) enum Owner<'a> {
    Application(&'a str),
    Template(&'a str),
}

impl<'a> Owner<'a> {
    fn columns(self) -> (Option<&'a str>, Option<&'a str>) {
        match self {
            Self::Application(id) => (Some(id), None),
            Self::Template(id) => (None, Some(id)),
        }
    }
}

pub(crate) fn load(
    connection: &Connection,
    owner: Owner<'_>,
) -> Result<Vec<AuxiliaryState>, CoreError> {
    let (application, template) = owner.columns();
    let mut statement = connection.prepare(
        "SELECT s.id, s.stable_key, s.display_name, s.semantic_kind, s.display_order,
            EXISTS(SELECT 1 FROM applications WHERE id = s.application_id AND current_stage_state = s.stable_key)
            OR EXISTS(SELECT 1 FROM interview_rounds WHERE application_id = s.application_id AND state = s.stable_key)
            OR EXISTS(SELECT 1 FROM workflow_events WHERE application_id = s.application_id
                AND (previous_state = s.stable_key OR next_state = s.stable_key))
         FROM workflow_states s WHERE s.application_id = ?1 OR s.template_id = ?2
         ORDER BY s.display_order, s.id"
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map(params![application, template], |row| {
            Ok(AuxiliaryState {
                id: row.get(0)?,
                stable_key: row.get(1)?,
                display_name: row.get(2)?,
                semantic_kind: row.get(3)?,
                display_order: row.get(4)?,
                in_use: row.get(5)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub(crate) fn require_state(
    connection: &Connection,
    application: &str,
    key: &str,
) -> Result<(), CoreError> {
    if key.is_empty() {
        return Ok(());
    }
    let valid: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM workflow_states WHERE application_id = ?1 AND stable_key = ?2)",
        params![application, key], |row| row.get(0),
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    if valid {
        Ok(())
    } else {
        Err(CoreError::Validation)
    }
}

pub(crate) fn describe(
    connection: &Connection,
    application: &str,
    key: &str,
) -> Result<(String, Option<String>), CoreError> {
    if key.is_empty() {
        return Ok((String::new(), None));
    }
    Ok(connection.query_row("SELECT display_name, semantic_kind FROM workflow_states WHERE application_id = ?1 AND stable_key = ?2",
        params![application, key], |row| Ok((row.get(0)?, Some(row.get(1)?))))
        .optional().map_err(|_| CoreError::DatabaseInvalid)?.unwrap_or_else(|| (key.into(), None)))
}

/// Only called inside a transaction, for a newly-created destination whose
/// seed definitions have no history. Not an "apply to existing records" API.
pub(crate) fn clone_into_new_owner(
    connection: &Transaction<'_>,
    source: Owner<'_>,
    target: Owner<'_>,
) -> Result<(), CoreError> {
    let (application, template) = target.columns();
    let empty: bool = connection.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM workflow_stages WHERE application_id = ?1 OR template_id = ?2)
         AND NOT EXISTS(SELECT 1 FROM workflow_events WHERE application_id = ?1)
         AND NOT EXISTS(SELECT 1 FROM interview_rounds WHERE application_id = ?1)
         AND (EXISTS(SELECT 1 FROM applications WHERE id = ?1 AND revision = 1 AND current_stage_id IS NULL)
              OR EXISTS(SELECT 1 FROM workflow_templates WHERE id = ?2 AND revision = 1))",
        params![application, template], |row| row.get(0),
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    if !empty {
        return Err(CoreError::Validation);
    }
    let states = load(connection, source)?;
    if states.len() < 6 {
        return Err(CoreError::Validation);
    }
    connection
        .execute(
            "DELETE FROM workflow_states WHERE application_id = ?1 OR template_id = ?2",
            params![application, template],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for state in states {
        insert(
            connection,
            target,
            &AuxiliaryState {
                id: Uuid::new_v4().to_string(),
                in_use: false,
                ..state
            },
        )?;
    }
    Ok(())
}

fn insert(
    connection: &Connection,
    owner: Owner<'_>,
    state: &AuxiliaryState,
) -> Result<(), CoreError> {
    let (application, template) = owner.columns();
    connection.execute(
        "INSERT INTO workflow_states (id, application_id, template_id, stable_key, display_name, semantic_kind, display_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![state.id, application, template, state.stable_key, state.display_name, state.semantic_kind, state.display_order],
    ).map(|_| ()).map_err(|_| CoreError::DatabaseInvalid)
}

fn update(
    connection: &Connection,
    owner: Owner<'_>,
    request: &UpdateAuxiliaryStatesRequest,
) -> Result<(), CoreError> {
    if !(6..=100).contains(&request.states.len()) {
        return Err(CoreError::Validation);
    }
    let source = load(connection, owner)?;
    let by_id: HashMap<_, _> = source.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut kept = HashSet::new();
    let mut names = HashSet::new();
    let mut ordered = Vec::new();
    for (index, edit) in request.states.iter().enumerate() {
        applications::validate_required_text(&edit.display_name)?;
        if !names.insert(edit.display_name.trim().to_lowercase()) {
            return Err(CoreError::Validation);
        }
        let mut state = if let Some(id) = &edit.id {
            if !kept.insert(id.as_str()) {
                return Err(CoreError::Validation);
            }
            let existing = by_id.get(id.as_str()).ok_or(CoreError::Validation)?;
            // Classifications are stable, even if the displayed label changes.
            if existing.semantic_kind != edit.semantic_kind
                || (existing.stable_key == "failed"
                    && existing.display_name != edit.display_name.trim())
            {
                return Err(CoreError::Validation);
            }
            (*existing).clone()
        } else {
            if ![
                "pending",
                "awaitingParticipation",
                "awaitingCompletion",
                "awaitingResult",
                "completed",
            ]
            .contains(&edit.semantic_kind.as_str())
            {
                return Err(CoreError::Validation);
            }
            AuxiliaryState {
                id: Uuid::new_v4().to_string(),
                stable_key: format!("custom_{}", Uuid::new_v4().simple()),
                display_name: String::new(),
                semantic_kind: edit.semantic_kind.clone(),
                display_order: 0,
                in_use: false,
            }
        };
        state.display_name = edit.display_name.trim().into();
        state.display_order = (index as i64 + 1) * 10;
        ordered.push(state);
    }
    for state in &source {
        if !kept.contains(state.id.as_str()) {
            if !state.stable_key.starts_with("custom_") || state.in_use {
                return Err(CoreError::Validation);
            }
            connection
                .execute("DELETE FROM workflow_states WHERE id = ?1", [&state.id])
                .map_err(|_| CoreError::DatabaseInvalid)?;
        }
    }
    for state in ordered {
        if by_id.contains_key(state.id.as_str()) {
            connection.execute("UPDATE workflow_states SET display_name = ?1, display_order = ?2 WHERE id = ?3",
                params![state.display_name, state.display_order, state.id]).map_err(|_| CoreError::DatabaseInvalid)?;
        } else {
            insert(connection, owner, &state)?;
        }
    }
    Ok(())
}

pub fn update_record(
    session: &mut WarehouseSession,
    request: UpdateAuxiliaryStatesRequest,
) -> Result<ApplicationDetail, CoreError> {
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    applications::require_record_revision(&transaction, &request.owner_id, request.revision)?;
    update(
        &transaction,
        Owner::Application(&request.owner_id),
        &request,
    )?;
    transaction
        .execute(
            "UPDATE applications SET revision = revision + 1, updated_at_utc = ?1 WHERE id = ?2",
            params![applications::now_utc(), request.owner_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    applications::get(session, &request.owner_id)
}

pub fn update_template(
    session: &mut WarehouseSession,
    request: UpdateAuxiliaryStatesRequest,
) -> Result<WorkflowTemplateDetail, CoreError> {
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let revision: i64 = transaction
        .query_row(
            "SELECT revision FROM workflow_templates WHERE id = ?1",
            [&request.owner_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    if revision != request.revision {
        return Err(CoreError::RevisionConflict);
    }
    update(&transaction, Owner::Template(&request.owner_id), &request)?;
    transaction.execute("UPDATE workflow_templates SET revision = revision + 1, updated_at_utc = ?1 WHERE id = ?2",
        params![applications::now_utc(), request.owner_id]).map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    workflows::get_template(session, &request.owner_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            AuxiliaryStateEdit, ChangeStageRequest, CreateApplicationRequest, DuplicateMode,
            InterviewRoundRequest,
        },
        warehouse::{self, WarehouseAccessMode},
    };
    use tempfile::tempdir;

    fn create(session: &mut WarehouseSession) -> ApplicationDetail {
        let request: CreateApplicationRequest = serde_json::from_value(
            serde_json::json!({"companyName": "虚构公司", "positionName": "开发岗位"}),
        )
        .unwrap();
        applications::create(session, request).unwrap()
    }
    fn edits(
        owner: &str,
        revision: i64,
        states: &[AuxiliaryState],
    ) -> UpdateAuxiliaryStatesRequest {
        UpdateAuxiliaryStatesRequest {
            owner_id: owner.into(),
            revision,
            states: states
                .iter()
                .map(|s| AuxiliaryStateEdit {
                    id: Some(s.id.clone()),
                    display_name: s.display_name.clone(),
                    semantic_kind: s.semantic_kind.clone(),
                })
                .collect(),
        }
    }
    fn edit_record(detail: &ApplicationDetail) -> UpdateAuxiliaryStatesRequest {
        edits(
            &detail.record.id,
            detail.record.revision,
            &detail.auxiliary_states,
        )
    }
    fn add_custom(session: &mut WarehouseSession, detail: &ApplicationDetail) -> ApplicationDetail {
        let mut request = edit_record(detail);
        request.states.push(AuxiliaryStateEdit {
            id: None,
            display_name: "等待主管反馈".into(),
            semantic_kind: "awaitingResult".into(),
        });
        update_record(session, request).unwrap()
    }
    fn transition(
        session: &mut WarehouseSession,
        detail: &ApplicationDetail,
        stage: &str,
        state: &str,
    ) -> Result<ApplicationDetail, CoreError> {
        applications::change_stage(
            session,
            ChangeStageRequest {
                application_id: detail.record.id.clone(),
                revision: detail.record.revision,
                stage_id: detail
                    .stages
                    .iter()
                    .find(|s| s.stable_key == stage)
                    .unwrap()
                    .id
                    .clone(),
                stage_state: state.into(),
                notes: String::new(),
            },
        )
    }

    #[test]
    fn record_names_order_and_history_snapshots_are_independent_and_persistent() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let first = create(&mut session);
        let other = create(&mut session);
        let mut edit = edit_record(&first);
        edit.states[0].display_name = "准备启动".into();
        edit.states.swap(0, 2);
        let edited = update_record(&mut session, edit.clone()).unwrap();
        assert_eq!(edited.record.current_state_name, "");
        assert_eq!(
            edited.record.status_updated_at_utc,
            first.record.status_updated_at_utc
        );
        assert_eq!(edited.history[0].next_state_name_snapshot, "");
        assert_eq!(edited.auxiliary_states[0].stable_key, "awaitingCompletion");
        assert_eq!(
            applications::get(&session, &other.record.id)
                .unwrap()
                .record
                .current_state_name,
            ""
        );
        assert!(matches!(
            update_record(&mut session, edit),
            Err(CoreError::RevisionConflict)
        ));
        let added = add_custom(&mut session, &edited);
        let key = added.auxiliary_states.last().unwrap().stable_key.clone();
        let waiting = transition(&mut session, &added, "interview", &key).unwrap();
        assert_eq!(
            waiting.record.current_state_kind.as_deref(),
            Some("awaitingResult")
        );
        assert_eq!(waiting.history[0].previous_state_name_snapshot, None);
        assert_eq!(waiting.history[0].next_state_name_snapshot, "等待主管反馈");
        assert_eq!(
            waiting.history[0].next_state_kind_snapshot.as_deref(),
            Some("awaitingResult")
        );
        let mut rename = edit_record(&waiting);
        rename.states.last_mut().unwrap().display_name = "待主管通知".into();
        let renamed = update_record(&mut session, rename).unwrap();
        assert_eq!(renamed.record.current_state_name, "待主管通知");
        assert_eq!(renamed.history[0].next_state_name_snapshot, "等待主管反馈");
        let completed = transition(&mut session, &renamed, "interview", "completed").unwrap();
        assert_eq!(
            completed.history[0].previous_state_name_snapshot.as_deref(),
            Some("待主管通知")
        );
        assert_eq!(
            completed.history[1].next_state_name_snapshot,
            "等待主管反馈"
        );
        assert_eq!(
            applications::list(&session, crate::domain::ApplicationScope::Active)
                .unwrap()
                .iter()
                .find(|r| r.id == first.record.id)
                .unwrap()
                .current_state_name,
            "已完成"
        );
        drop(session);
        let reopened = warehouse::open(dir.path(), WarehouseAccessMode::Write).unwrap();
        let restored = applications::get(&reopened, &first.record.id).unwrap();
        assert_eq!(
            restored.auxiliary_states.last().unwrap().display_name,
            "待主管通知"
        );
        assert_eq!(restored.history[1].next_state_name_snapshot, "等待主管反馈");
    }

    #[test]
    fn state_deletion_protects_current_round_and_historical_references() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let initial = create(&mut session);
        let added = add_custom(&mut session, &initial);
        let mut remove = edit_record(&added);
        remove.states.pop();
        let removed = update_record(&mut session, remove).unwrap();
        assert_eq!(removed.auxiliary_states.len(), 6);
        let added = add_custom(&mut session, &removed);
        let key = added.auxiliary_states.last().unwrap().stable_key.clone();
        let round = applications::save_interview_round(
            &mut session,
            InterviewRoundRequest {
                application_id: added.record.id.clone(),
                revision: added.record.revision,
                id: None,
                display_name: "主管面".into(),
                state: key.clone(),
                scheduled_at_utc: None,
                completed_at_utc: None,
                result: String::new(),
                notes: String::new(),
            },
        )
        .unwrap();
        assert!(round.auxiliary_states.last().unwrap().in_use);
        let mut remove = edit_record(&round);
        remove.states.pop();
        assert!(matches!(
            update_record(&mut session, remove),
            Err(CoreError::Validation)
        ));
        let waiting = transition(&mut session, &round, "interview", &key).unwrap();
        let mut remove = edit_record(&waiting);
        remove.states.pop();
        assert!(update_record(&mut session, remove).is_err());
        let finished = transition(&mut session, &waiting, "interview", "completed").unwrap();
        let without_round = applications::delete_interview_round(
            &mut session,
            &finished.record.id,
            &finished.interview_rounds[0].id,
            finished.record.revision,
        )
        .unwrap();
        let mut remove = edit_record(&without_round);
        remove.states.pop();
        assert!(update_record(&mut session, remove).is_err());
        assert!(
            applications::get(&session, &finished.record.id)
                .unwrap()
                .auxiliary_states
                .last()
                .unwrap()
                .in_use
        );
    }

    #[test]
    fn record_template_new_record_and_duplicate_palettes_never_share_ids() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let first = create(&mut session);
        let added = add_custom(&mut session, &first);
        let templates = applications::save_workflow_as_template(
            &mut session,
            &added.record.id,
            "个性化模板",
            true,
        )
        .unwrap();
        let template_id = &templates.iter().find(|t| t.is_default).unwrap().id;
        let template = workflows::get_template(&session, template_id).unwrap();
        let clone = workflows::duplicate_template(
            &mut session,
            template_id,
            template.template.revision,
            "模板副本",
        )
        .unwrap();
        let new = create(&mut session);
        let copied =
            applications::duplicate(&mut session, &added.record.id, DuplicateMode::FullRecord)
                .unwrap();
        let original = &added.auxiliary_states.last().unwrap();
        let mut ids = HashSet::from([original.id.clone()]);
        for states in [
            &template.auxiliary_states,
            &clone.auxiliary_states,
            &new.auxiliary_states,
            &copied.auxiliary_states,
        ] {
            assert_eq!(states.last().unwrap().stable_key, original.stable_key);
            assert!(ids.insert(states.last().unwrap().id.clone()));
        }
        assert_eq!(copied.record.current_stage_state, "");
        let mut edit = edits(
            template_id,
            template.template.revision,
            &template.auxiliary_states,
        );
        edit.states.last_mut().unwrap().display_name = "新模板命名".into();
        let saved = update_template(&mut session, edit.clone()).unwrap();
        assert!(matches!(
            update_template(&mut session, edit),
            Err(CoreError::RevisionConflict)
        ));
        let newest = create(&mut session);
        assert_eq!(
            newest.auxiliary_states.last().unwrap().display_name,
            "新模板命名"
        );
        for id in [&new.record.id, &copied.record.id, &added.record.id] {
            assert_eq!(
                applications::get(&session, id)
                    .unwrap()
                    .auxiliary_states
                    .last()
                    .unwrap()
                    .display_name,
                "等待主管反馈"
            );
        }
        let mut remove = edits(
            template_id,
            saved.template.revision,
            &saved.auxiliary_states,
        );
        remove.states.pop();
        assert_eq!(
            update_template(&mut session, remove)
                .unwrap()
                .auxiliary_states
                .len(),
            6
        );
        assert_eq!(
            applications::get(&session, &newest.record.id)
                .unwrap()
                .auxiliary_states
                .len(),
            7
        );
    }

    #[test]
    fn invalid_definitions_foreign_states_and_terminal_forgery_are_rejected() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let first = create(&mut session);
        let other = create(&mut session);
        let valid = edit_record(&first);
        let mut invalid = valid.clone();
        invalid.states[0].id = Some(other.auxiliary_states[0].id.clone());
        assert!(update_record(&mut session, invalid).is_err());
        let mut invalid = valid.clone();
        invalid.states[1] = invalid.states[0].clone();
        assert!(update_record(&mut session, invalid).is_err());
        let mut invalid = valid.clone();
        invalid.states.remove(0);
        invalid.states.push(AuxiliaryStateEdit {
            id: None,
            display_name: "替换系统状态".into(),
            semantic_kind: "pending".into(),
        });
        assert!(update_record(&mut session, invalid).is_err());
        for name in [" ".to_string(), "待结果".into(), "名".repeat(201)] {
            let mut invalid = valid.clone();
            invalid.states[0].display_name = name;
            assert!(update_record(&mut session, invalid).is_err());
        }
        let mut invalid = valid.clone();
        invalid.states[0].semantic_kind = "completed".into();
        assert!(update_record(&mut session, invalid).is_err());
        let mut invalid = valid.clone();
        invalid.states[5].display_name = "自动失败".into();
        assert!(update_record(&mut session, invalid).is_err());
        for kind in ["failed", "offer", "unknown"] {
            let mut invalid = valid.clone();
            invalid.states.push(AuxiliaryStateEdit {
                id: None,
                display_name: "伪造终态".into(),
                semantic_kind: kind.into(),
            });
            assert!(update_record(&mut session, invalid).is_err());
        }
        let mut too_many = valid.clone();
        for n in 0..95 {
            too_many.states.push(AuxiliaryStateEdit {
                id: None,
                display_name: format!("状态{n}"),
                semantic_kind: "pending".into(),
            });
        }
        assert!(update_record(&mut session, too_many).is_err());
        assert!(transition(&mut session, &first, "interview", "failed").is_err());
        let added = add_custom(&mut session, &first);
        let key = &added.auxiliary_states.last().unwrap().stable_key;
        assert!(transition(&mut session, &other, "interview", key).is_err());
        let failed = transition(&mut session, &added, "failed_terminal", "pending").unwrap();
        assert_eq!(failed.record.current_stage_state, "failed");
        assert_eq!(
            failed.record.current_stage_id,
            added.record.current_stage_id
        );
        let offer = transition(&mut session, &failed, "offer", "pending").unwrap();
        assert_eq!(offer.record.current_stage_state, "completed");
        assert_eq!(offer.record.current_stage_progress, 100);
    }

    #[test]
    fn definition_and_history_failures_roll_back_and_read_only_cannot_write() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let first = create(&mut session);
        let mut edit = edit_record(&first);
        edit.states[0].display_name = "失败时不得保存".into();
        session.connection().execute_batch("CREATE TRIGGER fail_state_revision BEFORE UPDATE OF revision ON applications BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
        assert!(update_record(&mut session, edit.clone()).is_err());
        assert_eq!(
            applications::get(&session, &first.record.id)
                .unwrap()
                .record
                .current_state_name,
            ""
        );
        session.connection().execute_batch("DROP TRIGGER fail_state_revision; CREATE TRIGGER fail_state_snapshot BEFORE UPDATE OF next_state_name_snapshot ON workflow_events BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
        assert!(transition(&mut session, &first, "applied", "awaitingResult").is_err());
        let unchanged = applications::get(&session, &first.record.id).unwrap();
        assert_eq!(unchanged.record.revision, first.record.revision);
        assert_eq!(unchanged.record.application_date, None);
        assert_eq!(unchanged.history.len(), 1);
        session
            .connection()
            .execute_batch("DROP TRIGGER fail_state_snapshot")
            .unwrap();
        let template_id = applications::list_workflow_templates(&session).unwrap()[0]
            .id
            .clone();
        let template = workflows::get_template(&session, &template_id).unwrap();
        let mut template_edit = edits(
            &template_id,
            template.template.revision,
            &template.auxiliary_states,
        );
        template_edit.states[0].display_name = "模板失败不得保存".into();
        session.connection().execute_batch("CREATE TRIGGER fail_template_revision BEFORE UPDATE OF revision ON workflow_templates BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
        assert!(update_template(&mut session, template_edit.clone()).is_err());
        assert_eq!(
            workflows::get_template(&session, &template_id)
                .unwrap()
                .auxiliary_states[0]
                .display_name,
            "尚未开始"
        );
        session
            .connection()
            .execute_batch("DROP TRIGGER fail_template_revision")
            .unwrap();
        let mut read_only = warehouse::open(dir.path(), WarehouseAccessMode::ReadOnly).unwrap();
        assert!(update_record(&mut read_only, edit).is_err());
        assert!(update_template(&mut read_only, template_edit).is_err());
    }

    #[test]
    fn copying_definitions_rejects_existing_targets_and_rolls_back_new_record_failure() {
        let dir = tempdir().unwrap();
        let mut session = warehouse::create(dir.path()).unwrap();
        let initial = create(&mut session);
        let added = add_custom(&mut session, &initial);
        let target = create(&mut session);
        {
            let transaction = session.connection_mut().unwrap().transaction().unwrap();
            assert!(
                clone_into_new_owner(
                    &transaction,
                    Owner::Application(&added.record.id),
                    Owner::Application(&target.record.id)
                )
                .is_err()
            );
        }
        assert_eq!(
            applications::get(&session, &target.record.id)
                .unwrap()
                .auxiliary_states
                .len(),
            6
        );
        // Any custom state insert now fails. Neither the new record nor the
        // default-template switch may survive this injected failure.
        session.connection().execute_batch("CREATE TRIGGER fail_cloned_state BEFORE INSERT ON workflow_states WHEN NEW.stable_key LIKE 'custom_%' BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
        assert!(
            applications::duplicate(&mut session, &added.record.id, DuplicateMode::FullRecord)
                .is_err()
        );
        assert_eq!(
            applications::list(&session, crate::domain::ApplicationScope::Active)
                .unwrap()
                .len(),
            2
        );
        assert!(
            applications::save_workflow_as_template(
                &mut session,
                &added.record.id,
                "失败模板",
                true
            )
            .is_err()
        );
        let templates = applications::list_workflow_templates(&session).unwrap();
        assert_eq!(templates.len(), 1);
        assert!(templates[0].is_default);
        assert_eq!(templates[0].revision, 1);
        assert_eq!(
            applications::get(&session, &added.record.id)
                .unwrap()
                .auxiliary_states
                .len(),
            7
        );
        session
            .connection()
            .execute_batch("DROP TRIGGER fail_cloned_state")
            .unwrap();
    }

    #[test]
    fn definition_dto_rejects_client_injected_keys_and_scope_fields() {
        let mut request = serde_json::json!({"ownerId": "test", "revision": 1, "states": [{"id": null, "displayName": "自定义", "semanticKind": "pending"}]});
        assert!(serde_json::from_value::<UpdateAuxiliaryStatesRequest>(request.clone()).is_ok());
        request["states"][0]["stableKey"] = "failed".into();
        assert!(serde_json::from_value::<UpdateAuxiliaryStatesRequest>(request.clone()).is_err());
        request["states"][0]
            .as_object_mut()
            .unwrap()
            .remove("stableKey");
        request["applyToAll"] = true.into();
        assert!(serde_json::from_value::<UpdateAuxiliaryStatesRequest>(request).is_err());
    }
}
