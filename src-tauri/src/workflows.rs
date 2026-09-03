//! Record-scoped order and independent template editing. No filesystem writes.
use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{
    applications,
    auxiliary_states::{self, Owner},
    domain::{
        ApplicationDetail, ReorderWorkflowRequest, UpdateWorkflowTemplateRequest, WorkflowStage,
        WorkflowTemplate, WorkflowTemplateDetail,
    },
    error::CoreError,
    warehouse::WarehouseSession,
};

const MAX_STAGES: usize = 100;

pub fn get_template(
    session: &WarehouseSession,
    id: &str,
) -> Result<WorkflowTemplateDetail, CoreError> {
    get_template_from_connection(session.connection(), id)
}

pub(crate) fn get_template_from_connection(
    connection: &Connection,
    id: &str,
) -> Result<WorkflowTemplateDetail, CoreError> {
    let mut template = connection.query_row(
        "SELECT id, name, description, is_default, revision FROM workflow_templates WHERE id = ?1",
        [id], |row| Ok(WorkflowTemplate { id: row.get(0)?, name: row.get(1)?,
            description: row.get(2)?, is_default: row.get(3)?, revision: row.get(4)?, stage_count: 0 }),
    ).optional().map_err(|_| CoreError::DatabaseInvalid)?.ok_or(CoreError::NotFound)?;
    let mut statement = connection.prepare(
        "SELECT id, stable_key, display_name, stage_kind, display_order, color, is_terminal, terminal_outcome
         FROM workflow_stages WHERE template_id = ?1 ORDER BY display_order, rowid"
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    let stages = statement
        .query_map([id], |row| {
            Ok(WorkflowStage {
                id: row.get(0)?,
                stable_key: row.get(1)?,
                display_name: row.get(2)?,
                stage_kind: row.get(3)?,
                display_order: row.get(4)?,
                color: row.get(5)?,
                is_terminal: row.get(6)?,
                terminal_outcome: row.get(7)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    template.stage_count = stages.len() as i64;
    Ok(WorkflowTemplateDetail {
        template,
        stages,
        auxiliary_states: auxiliary_states::load(connection, Owner::Template(id))?,
    })
}

fn require_template_revision(
    detail: &WorkflowTemplateDetail,
    revision: i64,
) -> Result<(), CoreError> {
    if detail.template.revision != revision {
        Err(CoreError::RevisionConflict)
    } else {
        Ok(())
    }
}

/// Anchor the starting point and terminal outcomes, but let intermediate stages
/// move freely. The full permutation prevents omissions and cross-record IDs.
pub(crate) fn validate_order(stages: &[WorkflowStage]) -> Result<(), CoreError> {
    if stages.len() < 4
        || stages.len() > MAX_STAGES
        || stages.first().map(|s| s.stable_key.as_str()) != Some("preparing")
        || stages[stages.len() - 2].stable_key != "offer"
        || stages[stages.len() - 1].stable_key != "failed_terminal"
        || !stages.iter().any(|s| s.stable_key == "applied")
        || stages[..stages.len() - 2]
            .iter()
            .any(|s| s.is_terminal || s.terminal_outcome.is_some())
        || !stages[stages.len() - 2].is_terminal
        || stages[stages.len() - 2].terminal_outcome.as_deref() != Some("offer")
        || !stages[stages.len() - 1].is_terminal
        || stages[stages.len() - 1].terminal_outcome.as_deref() != Some("failed")
        || stages.iter().map(|s| &s.id).collect::<HashSet<_>>().len() != stages.len()
        || stages
            .iter()
            .map(|s| &s.stable_key)
            .collect::<HashSet<_>>()
            .len()
            != stages.len()
    {
        return Err(CoreError::Validation);
    }
    Ok(())
}

pub fn reorder_record(
    session: &mut WarehouseSession,
    request: ReorderWorkflowRequest,
) -> Result<ApplicationDetail, CoreError> {
    let now = applications::now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    applications::require_record_revision(&transaction, &request.application_id, request.revision)?;
    let source = applications::load_stages(&transaction, &request.application_id)?;
    if source.len() != request.stage_ids.len() {
        return Err(CoreError::Validation);
    }
    let by_id: HashMap<_, _> = source.iter().map(|s| (s.id.as_str(), s)).collect();
    let ordered: Vec<WorkflowStage> = request
        .stage_ids
        .iter()
        .map(|id| {
            by_id
                .get(id.as_str())
                .map(|s| (*s).clone())
                .ok_or(CoreError::Validation)
        })
        .collect::<Result<_, _>>()?;
    validate_order(&ordered)?;
    for (index, stage) in ordered.iter().enumerate() {
        transaction.execute(
            "UPDATE workflow_stages SET display_order = ?1, updated_at_utc = ?2 WHERE id = ?3 AND application_id = ?4",
            params![(index as i64 + 1) * 10, now, stage.id, request.application_id],
        ).map_err(|_| CoreError::DatabaseInvalid)?;
    }
    transaction
        .execute(
            "UPDATE applications SET revision = revision + 1, updated_at_utc = ?1 WHERE id = ?2",
            params![now, request.application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    applications::get(session, &request.application_id)
}

pub fn update_template(
    session: &mut WarehouseSession,
    request: UpdateWorkflowTemplateRequest,
) -> Result<WorkflowTemplateDetail, CoreError> {
    applications::validate_required_text(&request.name)?;
    if request.description.chars().count() > 10_000 || request.stages.len() > MAX_STAGES {
        return Err(CoreError::Validation);
    }
    let now = applications::now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let source = get_template_from_connection(&transaction, &request.id)?;
    require_template_revision(&source, request.revision)?;
    let by_id: HashMap<_, _> = source.stages.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut ordered = Vec::new();
    for (index, edit) in request.stages.iter().enumerate() {
        applications::validate_required_text(&edit.display_name)?;
        if !applications::is_hex_color(&edit.color) {
            return Err(CoreError::Validation);
        }
        let mut stage = if let Some(id) = &edit.id {
            (*by_id.get(id.as_str()).ok_or(CoreError::Validation)?).clone()
        } else {
            WorkflowStage {
                id: Uuid::new_v4().to_string(),
                stable_key: format!("custom_{}", Uuid::new_v4().simple()),
                display_name: String::new(),
                color: String::new(),
                display_order: 0,
                stage_kind: "custom".into(),
                is_terminal: false,
                terminal_outcome: None,
            }
        };
        stage.display_name = edit.display_name.trim().into();
        stage.color = edit.color.clone();
        stage.display_order = (index as i64 + 1) * 10;
        ordered.push(stage);
    }
    validate_order(&ordered)?;
    let kept: HashSet<_> = ordered.iter().map(|s| s.id.as_str()).collect();
    for old in &source.stages {
        if !kept.contains(old.id.as_str()) {
            // Built-in stages remain stable even after their display name changes.
            if !old.stable_key.starts_with("custom_") {
                return Err(CoreError::Validation);
            }
            transaction
                .execute(
                    "DELETE FROM workflow_stages WHERE id = ?1 AND template_id = ?2",
                    params![old.id, request.id],
                )
                .map_err(|_| CoreError::DatabaseInvalid)?;
        }
    }
    for stage in ordered {
        if by_id.contains_key(stage.id.as_str()) {
            transaction.execute(
                "UPDATE workflow_stages SET display_name = ?1, color = ?2, display_order = ?3, updated_at_utc = ?4
                 WHERE id = ?5 AND template_id = ?6",
                params![stage.display_name, stage.color, stage.display_order, now, stage.id, request.id],
            ).map_err(|_| CoreError::DatabaseInvalid)?;
        } else {
            transaction.execute(
                "INSERT INTO workflow_stages (id, template_id, stable_key, display_name, stage_kind, display_order, color,
                     is_terminal, terminal_outcome, created_at_utc, updated_at_utc)
                 VALUES (?1, ?2, ?3, ?4, 'custom', ?5, ?6, 0, NULL, ?7, ?7)",
                params![stage.id, request.id, stage.stable_key, stage.display_name, stage.display_order, stage.color, now],
            ).map_err(|_| CoreError::DatabaseInvalid)?;
        }
    }
    transaction.execute(
        "UPDATE workflow_templates SET name = ?1, description = ?2, revision = revision + 1, updated_at_utc = ?3 WHERE id = ?4",
        params![request.name.trim(), request.description, now, request.id],
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    get_template(session, &request.id)
}

pub fn duplicate_template(
    session: &mut WarehouseSession,
    id: &str,
    revision: i64,
    name: &str,
) -> Result<WorkflowTemplateDetail, CoreError> {
    applications::validate_required_text(name)?;
    let now = applications::now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let source = get_template_from_connection(&transaction, id)?;
    require_template_revision(&source, revision)?;
    validate_order(&source.stages)?;
    let target = Uuid::new_v4().to_string();
    transaction.execute(
        "INSERT INTO workflow_templates (id, name, description, is_default, created_at_utc, updated_at_utc)
         VALUES (?1, ?2, ?3, 0, ?4, ?4)",
        params![target, name.trim(), source.template.description, now],
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    auxiliary_states::clone_into_new_owner(
        &transaction,
        Owner::Template(id),
        Owner::Template(&target),
    )?;
    for stage in source.stages {
        transaction.execute(
            "INSERT INTO workflow_stages (id, template_id, stable_key, display_name, stage_kind, display_order, color,
                 is_terminal, terminal_outcome, created_at_utc, updated_at_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![Uuid::new_v4().to_string(), target, stage.stable_key, stage.display_name, stage.stage_kind,
                stage.display_order, stage.color, stage.is_terminal, stage.terminal_outcome, now],
        ).map_err(|_| CoreError::DatabaseInvalid)?;
    }
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    get_template(session, &target)
}

pub fn set_default(
    session: &mut WarehouseSession,
    id: &str,
    revision: i64,
) -> Result<WorkflowTemplateDetail, CoreError> {
    let now = applications::now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let source = get_template_from_connection(&transaction, id)?;
    require_template_revision(&source, revision)?;
    validate_order(&source.stages)?;
    if !source.template.is_default {
        transaction.execute(
            "UPDATE workflow_templates SET is_default = 0, revision = revision + 1, updated_at_utc = ?1 WHERE is_default = 1",
            [&now],
        ).map_err(|_| CoreError::DatabaseInvalid)?;
        transaction.execute(
            "UPDATE workflow_templates SET is_default = 1, revision = revision + 1, updated_at_utc = ?1 WHERE id = ?2",
            params![now, id],
        ).map_err(|_| CoreError::DatabaseInvalid)?;
    }
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    get_template(session, id)
}

/// A rank-based visual indicator, not a prediction of hiring success. Failure
/// retains the position of the last reached stage; only Offer reaches 100.
pub(crate) fn progress(stages: &[WorkflowStage], current_id: Option<&str>) -> i64 {
    let ordered: Vec<_> = stages
        .iter()
        .filter(|s| s.stable_key != "failed_terminal")
        .collect();
    let Some(index) = ordered
        .iter()
        .position(|s| Some(s.id.as_str()) == current_id)
    else {
        return 0;
    };
    if ordered[index].stable_key == "offer" {
        return 100;
    }
    if ordered.len() <= 1 {
        return 0;
    }
    (index as i64 * 100 / (ordered.len() as i64 - 1)).min(99)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{ChangeStageRequest, CreateApplicationRequest, TemplateStageEdit},
        warehouse::{self, WarehouseAccessMode},
    };
    use tempfile::tempdir;

    fn default_template(session: &WarehouseSession) -> WorkflowTemplateDetail {
        let id = applications::list_workflow_templates(session)
            .unwrap()
            .into_iter()
            .find(|t| t.is_default)
            .unwrap()
            .id;
        get_template(session, &id).unwrap()
    }
    fn edit_request(detail: &WorkflowTemplateDetail) -> UpdateWorkflowTemplateRequest {
        UpdateWorkflowTemplateRequest {
            id: detail.template.id.clone(),
            revision: detail.template.revision,
            name: detail.template.name.clone(),
            description: detail.template.description.clone(),
            stages: detail
                .stages
                .iter()
                .map(|s| TemplateStageEdit {
                    id: Some(s.id.clone()),
                    display_name: s.display_name.clone(),
                    color: s.color.clone(),
                })
                .collect(),
        }
    }
    fn create_record(session: &mut WarehouseSession, name: &str) -> ApplicationDetail {
        applications::create(
            session,
            CreateApplicationRequest {
                company_name: name.into(),
                position_name: "测试岗位".into(),
                company_type: "private".into(),
                industry: "".into(),
                position_category: "".into(),
                work_location: "".into(),
            },
        )
        .unwrap()
    }

    #[test]
    fn template_edits_and_copies_are_independent_and_persist_after_reopening() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let before = create_record(&mut session, "旧记录");
        let template = default_template(&session);
        let copy = duplicate_template(
            &mut session,
            &template.template.id,
            template.template.revision,
            "独立模板",
        )
        .unwrap();
        assert!(!copy.template.is_default);
        assert_ne!(copy.template.id, template.template.id);
        assert!(
            copy.stages
                .iter()
                .all(|stage| !template.stages.iter().any(|s| s.id == stage.id))
        );
        let mut edit = edit_request(&copy);
        edit.name = "修改后的模板".into();
        edit.description = "此模板仅供以后新建记录使用".into();
        edit.stages[4].display_name = "技术面试".into();
        edit.stages[4].color = "#123456".into();
        edit.stages.swap(3, 4);
        edit.stages.insert(
            5,
            TemplateStageEdit {
                id: None,
                display_name: "主管沟通".into(),
                color: "#654321".into(),
            },
        );
        let saved = update_template(&mut session, edit).unwrap();
        assert_eq!(saved.template.revision, 2);
        assert_eq!(saved.stages[3].stable_key, "interview");
        assert_eq!(saved.stages[3].display_name, "技术面试");
        assert_eq!(saved.stages[3].id, copy.stages[4].id);
        assert_eq!(saved.stages[5].stage_kind, "custom");
        assert!(!saved.stages[5].is_terminal);
        let chosen =
            set_default(&mut session, &saved.template.id, saved.template.revision).unwrap();
        assert_eq!(chosen.template.revision, 3);
        assert_eq!(
            get_template(&session, &template.template.id)
                .unwrap()
                .template
                .revision,
            2
        );
        let after = create_record(&mut session, "新记录");
        assert_eq!(after.record.current_stage_name, "准备投递");
        assert_eq!(after.stages[3].display_name, "技术面试");
        assert!(
            after
                .stages
                .iter()
                .all(|s| !chosen.stages.iter().any(|t| s.id == t.id))
        );
        let old = applications::get(&session, &before.record.id).unwrap();
        assert_eq!(old.record.revision, before.record.revision);
        assert_eq!(old.stages.len(), before.stages.len());
        assert_eq!(old.stages[4].display_name, "面试考核");
        // Removing a custom template stage cannot cascade to copied records.
        let mut removal = edit_request(&chosen);
        removal.stages.remove(5);
        update_template(&mut session, removal).unwrap();
        assert!(
            applications::get(&session, &after.record.id)
                .unwrap()
                .stages
                .iter()
                .any(|s| s.display_name == "主管沟通")
        );
        drop(session);
        let reopened = warehouse::open(directory.path(), WarehouseAccessMode::Write).unwrap();
        let current = default_template(&reopened);
        assert_eq!(current.template.name, "修改后的模板");
        assert_eq!(current.template.description, "此模板仅供以后新建记录使用");
        assert_eq!(current.stages[3].color, "#123456");
        assert_eq!(
            applications::list_workflow_templates(&reopened)
                .unwrap()
                .iter()
                .filter(|t| t.is_default)
                .count(),
            1
        );
    }

    #[test]
    fn template_commands_reject_stale_versions_unknown_ids_and_builtin_removal() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let template = default_template(&session);
        let saved = update_template(&mut session, edit_request(&template)).unwrap();
        assert!(matches!(
            update_template(&mut session, edit_request(&template)),
            Err(CoreError::RevisionConflict)
        ));
        assert!(matches!(
            duplicate_template(&mut session, &template.template.id, 1, "过期副本"),
            Err(CoreError::RevisionConflict)
        ));
        assert!(matches!(
            set_default(&mut session, &template.template.id, 1),
            Err(CoreError::RevisionConflict)
        ));
        assert!(matches!(
            set_default(&mut session, "missing", 1),
            Err(CoreError::NotFound)
        ));
        let unrelated = duplicate_template(
            &mut session,
            &saved.template.id,
            saved.template.revision,
            "另一个模板",
        )
        .unwrap();
        let mut invalid = edit_request(&saved);
        invalid.stages[3].id = Some(unrelated.stages[3].id.clone());
        assert!(matches!(
            update_template(&mut session, invalid),
            Err(CoreError::Validation)
        ));
        let mut invalid = edit_request(&saved);
        invalid.stages[3].id = invalid.stages[2].id.clone();
        assert!(update_template(&mut session, invalid).is_err());
        let mut invalid = edit_request(&saved);
        invalid.stages.remove(4);
        assert!(update_template(&mut session, invalid).is_err());
        let mut invalid = edit_request(&saved);
        invalid.stages.swap(0, 1);
        assert!(update_template(&mut session, invalid).is_err());
        let mut invalid = edit_request(&saved);
        invalid.stages[0].color = "red;".into();
        assert!(update_template(&mut session, invalid).is_err());
        let final_template = get_template(&session, &saved.template.id).unwrap();
        assert_eq!(final_template.template.revision, saved.template.revision);
        assert_eq!(final_template.stages.len(), saved.stages.len());
        assert!(final_template.template.is_default);
    }

    #[test]
    fn template_update_copy_and_default_failure_roll_back_atomically() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let template = default_template(&session);
        let copy = duplicate_template(&mut session, &template.template.id, 1, "副本").unwrap();
        session
            .connection_mut()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_template_update BEFORE UPDATE ON workflow_templates
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            )
            .unwrap();
        let mut edit = edit_request(&copy);
        edit.stages[2].display_name = "不应写入".into();
        edit.stages.insert(
            3,
            TemplateStageEdit {
                id: None,
                display_name: "新增".into(),
                color: "#123456".into(),
            },
        );
        assert!(update_template(&mut session, edit).is_err());
        let after = get_template(&session, &copy.template.id).unwrap();
        assert_eq!(after.stages[2].display_name, copy.stages[2].display_name);
        assert_eq!(after.stages.len(), copy.stages.len());
        session
            .connection_mut()
            .unwrap()
            .execute_batch(
                "DROP TRIGGER fail_template_update;
             CREATE TRIGGER fail_template_default BEFORE UPDATE ON workflow_templates
             WHEN NEW.is_default = 1 AND OLD.is_default = 0
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            )
            .unwrap();
        assert!(set_default(&mut session, &copy.template.id, 1).is_err());
        assert_eq!(default_template(&session).template.id, template.template.id);
        session
            .connection_mut()
            .unwrap()
            .execute_batch(
                "DROP TRIGGER fail_template_default;
             CREATE TRIGGER fail_template_stage BEFORE INSERT ON workflow_stages
             WHEN NEW.stable_key = 'interview'
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            )
            .unwrap();
        assert!(duplicate_template(&mut session, &template.template.id, 1, "失败副本").is_err());
        assert_eq!(
            applications::list_workflow_templates(&session)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn record_order_preserves_stage_identity_history_and_failure_position() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = create_record(&mut session, "排序记录");
        let other = create_record(&mut session, "独立记录");
        let current = applications::change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id.clone(),
                revision: record.record.revision,
                stage_id: record.stages[4].id.clone(),
                stage_state: "awaitingResult".into(),
                notes: "等待结果".into(),
            },
        )
        .unwrap();
        let mut stage_ids: Vec<_> = current.stages.iter().map(|s| s.id.clone()).collect();
        stage_ids.swap(3, 4);
        let reordered = reorder_record(
            &mut session,
            ReorderWorkflowRequest {
                application_id: record.record.id.clone(),
                revision: current.record.revision,
                stage_ids,
            },
        )
        .unwrap();
        assert_eq!(
            reordered.record.current_stage_id,
            current.record.current_stage_id
        );
        assert_eq!(
            reordered.record.status_updated_at_utc,
            current.record.status_updated_at_utc
        );
        assert_eq!(
            reordered.record.application_date,
            current.record.application_date
        );
        assert_eq!(reordered.history[0].id, current.history[0].id);
        assert_eq!(reordered.record.current_stage_progress, 3 * 100 / 7);
        assert_eq!(
            applications::get(&session, &other.record.id)
                .unwrap()
                .stages[4]
                .id,
            other.stages[4].id
        );
        let failed = applications::change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id.clone(),
                revision: reordered.record.revision,
                stage_id: record.stages[8].id.clone(),
                stage_state: "pending".into(),
                notes: "".into(),
            },
        )
        .unwrap();
        assert_eq!(
            failed.record.current_stage_progress,
            reordered.record.current_stage_progress
        );
        let offer = applications::change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id.clone(),
                revision: failed.record.revision,
                stage_id: record.stages[7].id.clone(),
                stage_state: "pending".into(),
                notes: "".into(),
            },
        )
        .unwrap();
        assert_eq!(offer.record.current_stage_progress, 100);
        assert_eq!(
            applications::list(&session, crate::domain::ApplicationScope::Active)
                .unwrap()
                .iter()
                .find(|r| r.id == offer.record.id)
                .unwrap()
                .current_stage_progress,
            100
        );
        drop(session);
        let reopened = warehouse::open(directory.path(), WarehouseAccessMode::Write).unwrap();
        assert_eq!(
            applications::get(&reopened, &record.record.id)
                .unwrap()
                .stages[3]
                .id,
            current.stages[4].id
        );
    }

    #[test]
    fn reorder_requires_a_full_owned_permutation_and_rolls_back_on_error() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = create_record(&mut session, "排序校验");
        let other = create_record(&mut session, "另外记录");
        let ids: Vec<_> = record.stages.iter().map(|s| s.id.clone()).collect();
        let mut omitted = ids.clone();
        omitted.pop();
        let mut repeated = ids.clone();
        repeated[2] = ids[3].clone();
        let mut foreign = ids.clone();
        foreign[2] = other.stages[2].id.clone();
        let mut bad_start = ids.clone();
        bad_start.swap(0, 1);
        let mut bad_end = ids.clone();
        bad_end.swap(6, 7);
        for invalid in [omitted, repeated, foreign, bad_start, bad_end] {
            assert!(
                reorder_record(
                    &mut session,
                    ReorderWorkflowRequest {
                        application_id: record.record.id.clone(),
                        revision: 1,
                        stage_ids: invalid,
                    }
                )
                .is_err()
            );
        }
        assert!(matches!(
            reorder_record(
                &mut session,
                ReorderWorkflowRequest {
                    application_id: record.record.id.clone(),
                    revision: 0,
                    stage_ids: ids.clone(),
                }
            ),
            Err(CoreError::RevisionConflict)
        ));
        session.connection_mut().unwrap().execute_batch(
            "CREATE TRIGGER fail_record_revision BEFORE UPDATE ON applications BEGIN SELECT RAISE(ABORT, 'injected'); END;"
        ).unwrap();
        let mut moved = ids.clone();
        moved.swap(2, 3);
        assert!(
            reorder_record(
                &mut session,
                ReorderWorkflowRequest {
                    application_id: record.record.id.clone(),
                    revision: 1,
                    stage_ids: moved,
                }
            )
            .is_err()
        );
        let after = applications::get(&session, &record.record.id).unwrap();
        assert_eq!(after.record.revision, 1);
        assert_eq!(
            after
                .stages
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
    }

    #[test]
    fn read_only_sessions_cannot_edit_templates_or_record_order() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = create_record(&mut session, "只读记录");
        let template = default_template(&session);
        drop(session);
        let mut read_only =
            warehouse::open(directory.path(), WarehouseAccessMode::ReadOnly).unwrap();
        assert!(matches!(
            update_template(&mut read_only, edit_request(&template)),
            Err(CoreError::ReadOnlyWarehouse)
        ));
        assert!(matches!(
            duplicate_template(&mut read_only, &template.template.id, 1, "禁止副本"),
            Err(CoreError::ReadOnlyWarehouse)
        ));
        assert!(matches!(
            set_default(&mut read_only, &template.template.id, 1),
            Err(CoreError::ReadOnlyWarehouse)
        ));
        assert!(matches!(
            reorder_record(
                &mut read_only,
                ReorderWorkflowRequest {
                    application_id: record.record.id,
                    revision: 1,
                    stage_ids: record.stages.iter().map(|s| s.id.clone()).collect(),
                }
            ),
            Err(CoreError::ReadOnlyWarehouse)
        ));
    }

    #[test]
    fn appending_many_record_stages_keeps_terminals_last_and_template_reusable() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let mut record = create_record(&mut session, "长流程记录");
        for index in 0..20 {
            record = applications::save_workflow_stage(
                &mut session,
                crate::domain::WorkflowStageRequest {
                    application_id: record.record.id.clone(),
                    revision: record.record.revision,
                    id: None,
                    display_name: format!("沟通阶段 {index}"),
                    color: "#123456".into(),
                    is_terminal: false,
                    terminal_outcome: None,
                },
            )
            .unwrap();
            validate_order(&record.stages).unwrap();
        }
        assert_eq!(record.stages.len(), 29);
        assert_eq!(record.stages[27].stable_key, "offer");
        assert_eq!(record.stages[28].stable_key, "failed_terminal");
        applications::save_workflow_as_template(
            &mut session,
            &record.record.id,
            "长流程模板",
            true,
        )
        .unwrap();
        let next = create_record(&mut session, "采用长流程");
        assert_eq!(next.record.current_stage_name, "准备投递");
        validate_order(&next.stages).unwrap();
        assert_eq!(next.stages.len(), 29);
    }

    #[test]
    fn long_workflows_use_rank_instead_of_sort_numbers_as_percentage() {
        let directory = tempdir().unwrap();
        let session = warehouse::create(directory.path()).unwrap();
        let mut stages = default_template(&session).stages;
        for i in 0..20 {
            let mut extra = stages[2].clone();
            extra.id = format!("test-{i}");
            extra.stable_key = format!("custom_test-{i}");
            stages.insert(5, extra);
        }
        for (i, stage) in stages.iter_mut().enumerate() {
            stage.display_order = (i as i64 + 1) * 10;
        }
        assert_eq!(progress(&stages, Some(&stages[0].id)), 0);
        assert!(progress(&stages, Some(&stages[10].id)) < 100);
        assert_eq!(progress(&stages, Some(&stages[stages.len() - 2].id)), 100);
    }
}
