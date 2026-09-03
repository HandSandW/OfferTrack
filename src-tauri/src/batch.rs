//! Metadata-only batches: reversible preview, verified backup, one transaction.
//! No file mutation other than creating a database backup; no Agent endpoint.
use std::collections::HashSet;

use rusqlite::{Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    applications,
    auxiliary_states::{self, Owner},
    database_backup,
    domain::{ChangeStageRequest, WorkflowTemplateDetail},
    error::CoreError,
    warehouse::WarehouseSession,
    workflows,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub revision: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Archive {
        archived: bool,
    },
    AddTags {
        tags: Vec<String>,
    },
    Stage {
        #[serde(rename = "stageKey")]
        stage_key: String,
        #[serde(rename = "stateKey")]
        state_key: String,
    },
    AppendTemplate {
        #[serde(rename = "templateId")]
        template_id: String,
        revision: i64,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub targets: Vec<Target>,
    pub action: Action,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub id: String,
    pub company_name: String,
    pub position_name: String,
    pub changes: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub version: u32,
    pub fingerprint: String,
    pub items: Vec<PreviewItem>,
    pub changed_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub changed_count: usize,
    pub backup_id: Option<Uuid>,
}

/// The same SQL/validation as execution, always rolled back. Generated IDs and
/// wall-clock timestamps are not part of the confirmation fingerprint.
pub fn preview(session: &mut WarehouseSession, request: &Request) -> Result<Preview, CoreError> {
    let identity = (session.summary().warehouse_id, session.root().to_path_buf());
    let transaction = session.connection_mut()?.transaction().map_err(db)?;
    let result = run(&transaction, request, &identity)?;
    transaction.rollback().map_err(db)?;
    Ok(result)
}

pub fn apply(
    session: &mut WarehouseSession,
    request: &Request,
    expected: &str,
) -> Result<Applied, CoreError> {
    let checked = preview(session, request)?;
    if checked.fingerprint != expected {
        return Err(CoreError::RevisionConflict);
    }
    if checked.changed_count == 0 {
        return Ok(Applied {
            changed_count: 0,
            backup_id: None,
        });
    }
    // A backup failure stops the batch. A later conflict keeps the backup but
    // rolls back all metadata, including new tag/state definitions and history.
    database_backup::inspect_records(session.connection())?;
    let backup = database_backup::create_at(
        session.connection(),
        session.root(),
        session.summary().warehouse_id,
        "beforeBatch",
    )?;
    let identity = (session.summary().warehouse_id, session.root().to_path_buf());
    let transaction = session
        .connection_mut()?
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let executed = run(&transaction, request, &identity)?;
    if executed.fingerprint != expected {
        return Err(CoreError::RevisionConflict);
    }
    let result = Applied {
        changed_count: executed.changed_count,
        backup_id: Some(backup.id),
    };
    transaction.commit().map_err(db)?;
    Ok(result)
}

fn run(
    transaction: &Transaction<'_>,
    request: &Request,
    identity: &(Uuid, std::path::PathBuf),
) -> Result<Preview, CoreError> {
    if request.version != 1
        || request.targets.is_empty()
        || request.targets.len() > 200
        || request
            .targets
            .iter()
            .map(|t| &t.id)
            .collect::<HashSet<_>>()
            .len()
            != request.targets.len()
    {
        return Err(CoreError::Validation);
    }
    let template = if let Action::AppendTemplate {
        template_id,
        revision,
    } = &request.action
    {
        let detail = workflows::get_template_from_connection(transaction, template_id)?;
        if detail.template.revision != *revision {
            return Err(CoreError::RevisionConflict);
        }
        workflows::validate_order(&detail.stages)?;
        Some(detail)
    } else {
        None
    };
    let now = applications::now_utc();
    let mut before = Vec::new();
    let mut items = Vec::new();
    for target in &request.targets {
        applications::require_record_revision(transaction, &target.id, target.revision)?;
        let record = applications::load_record(transaction, &target.id)?;
        let stages = applications::load_stages(transaction, &target.id)?;
        let states = auxiliary_states::load(transaction, Owner::Application(&target.id))?;
        before.push(serde_json::json!((&record, &stages, &states)));
        let mut changes = Vec::new();
        let mut stage_changed = false;
        match &request.action {
            Action::Archive { archived } => {
                if record.archived_at_utc.is_some() != *archived {
                    transaction
                        .execute(
                            "UPDATE applications SET archived_at_utc = ?1 WHERE id = ?2",
                            params![if *archived { Some(&now) } else { None }, target.id],
                        )
                        .map_err(db)?;
                    changes.push(
                        if *archived {
                            "活跃 → 已归档"
                        } else {
                            "已归档 → 活跃"
                        }
                        .into(),
                    );
                }
            }
            Action::AddTags { tags } => {
                if tags.is_empty()
                    || tags.len() > 100
                    || tags
                        .iter()
                        .any(|t| t.trim().is_empty() || t.chars().count() > 40)
                {
                    return Err(CoreError::Validation);
                }
                let mut names: HashSet<_> =
                    record.tags.iter().map(|t| t.name.to_lowercase()).collect();
                for name in tags.iter().map(|t| t.trim()) {
                    if !names.insert(name.to_lowercase()) {
                        continue;
                    }
                    transaction.execute(
                        "INSERT OR IGNORE INTO tags (id, name, color, scope, created_at_utc, updated_at_utc)
                         VALUES (?1, ?2, '#64748b', 'record', ?3, ?3)",
                        params![Uuid::new_v4().to_string(), name, now]).map_err(db)?;
                    transaction.execute(
                        "INSERT INTO application_tags (application_id, tag_id, display_order)
                         SELECT ?1, id, (SELECT COALESCE(MAX(display_order), 0) + 1 FROM application_tags WHERE application_id = ?1)
                         FROM tags WHERE name = ?2", params![target.id, name]).map_err(db)?;
                    changes.push(format!("添加标签：{name}"));
                }
            }
            Action::Stage {
                stage_key,
                state_key,
            } => {
                let stage = stages
                    .iter()
                    .find(|s| &s.stable_key == stage_key)
                    .ok_or(CoreError::BatchConflict)?;
                let next_state = match stage_key.as_str() {
                    "failed_terminal" => "failed",
                    "offer" => "completed",
                    _ => state_key,
                };
                let next_stage = if stage_key == "failed_terminal" {
                    record.current_stage_id.as_deref()
                } else {
                    Some(stage.id.as_str())
                };
                auxiliary_states::require_state(transaction, &target.id, next_state)?;
                if next_state == "failed" && stage_key != "failed_terminal" {
                    return Err(CoreError::Validation);
                }
                if record.current_stage_id.as_deref() != next_stage
                    || record.current_stage_state != next_state
                {
                    applications::change_stage_in_transaction(
                        transaction,
                        &ChangeStageRequest {
                            application_id: target.id.clone(),
                            revision: target.revision,
                            stage_id: stage.id.clone(),
                            stage_state: next_state.into(),
                            notes: "批量更新进度".into(),
                        },
                        &now,
                    )?;
                    let after = applications::load_record(transaction, &target.id)?;
                    changes.push(format!(
                        "{} · {} → {} · {}",
                        record.current_stage_name,
                        record.current_state_name,
                        after.current_stage_name,
                        after.current_state_name
                    ));
                    if after.application_date != record.application_date {
                        changes.push(format!(
                            "投递日期：{}",
                            after.application_date.as_deref().unwrap_or("未填写")
                        ));
                    }
                    stage_changed = true;
                }
            }
            Action::AppendTemplate { .. } => {
                changes = append_template(
                    transaction,
                    &target.id,
                    template.as_ref().ok_or(CoreError::Validation)?,
                    &now,
                )?;
            }
        }
        if !changes.is_empty() && !stage_changed {
            transaction.execute("UPDATE applications SET revision = revision + 1, updated_at_utc = ?1 WHERE id = ?2",
                params![now, target.id]).map_err(db)?;
        }
        items.push(PreviewItem {
            id: target.id.clone(),
            company_name: record.company_name,
            position_name: record.position_name,
            changes,
        });
    }
    let changed_count = items.iter().filter(|i| !i.changes.is_empty()).count();
    let bytes = serde_json::to_vec(&(identity, request, before, template, &items))
        .map_err(|_| CoreError::Validation)?;
    Ok(Preview {
        version: 1,
        fingerprint: format!("{:x}", Sha256::digest(bytes)),
        items,
        changed_count,
    })
}

fn append_template(
    transaction: &Transaction<'_>,
    id: &str,
    template: &WorkflowTemplateDetail,
    now: &str,
) -> Result<Vec<String>, CoreError> {
    let mut stages = applications::load_stages(transaction, id)?;
    workflows::validate_order(&stages)?;
    let states = auxiliary_states::load(transaction, Owner::Application(id))?;
    let mut names: HashSet<_> = states
        .iter()
        .map(|s| s.display_name.to_lowercase())
        .collect();
    let mut count = states.len();
    let mut order = states.iter().map(|s| s.display_order).max().unwrap_or(0);
    let mut changes = Vec::new();
    for source in &template.auxiliary_states {
        if let Some(existing) = states.iter().find(|s| s.stable_key == source.stable_key) {
            if existing.semantic_kind != source.semantic_kind {
                return Err(CoreError::BatchConflict);
            }
            continue; // Keep record-local labels, colors, ordering and IDs.
        }
        count += 1;
        if count > 100 || !names.insert(source.display_name.to_lowercase()) {
            return Err(CoreError::BatchConflict);
        }
        order += 10;
        transaction.execute(
            "INSERT INTO workflow_states (id, application_id, stable_key, display_name, semantic_kind, display_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![Uuid::new_v4().to_string(), id, source.stable_key, source.display_name, source.semantic_kind, order]).map_err(db)?;
        changes.push(format!("追加辅助状态：{}", source.display_name));
    }
    let mut added = false;
    for source in &template.stages {
        if let Some(existing) = stages.iter().find(|s| s.stable_key == source.stable_key) {
            if existing.stage_kind != source.stage_kind
                || existing.terminal_outcome != source.terminal_outcome
                || existing.is_terminal != source.is_terminal
            {
                return Err(CoreError::BatchConflict);
            }
            continue;
        }
        if source.is_terminal
            || source.terminal_outcome.is_some()
            || stages.len() >= 100
            || stages
                .iter()
                .any(|s| s.display_name.to_lowercase() == source.display_name.to_lowercase())
        {
            return Err(CoreError::BatchConflict);
        }
        let mut next = source.clone();
        next.id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO workflow_stages (id, application_id, stable_key, display_name, stage_kind, display_order, color, is_terminal, terminal_outcome, created_at_utc, updated_at_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, 0, NULL, ?7, ?7)",
            params![next.id, id, next.stable_key, next.display_name, next.stage_kind, next.color, now]).map_err(db)?;
        changes.push(format!("追加阶段：{}（置于终态之前）", next.display_name));
        stages.insert(stages.len() - 2, next);
        added = true;
    }
    if added {
        workflows::validate_order(&stages)?;
        for (index, stage) in stages.iter().enumerate() {
            transaction.execute("UPDATE workflow_stages SET display_order = ?1 WHERE id = ?2 AND application_id = ?3",
                params![(index as i64 + 1) * 10, stage.id, id]).map_err(db)?;
        }
    }
    Ok(changes)
}

fn db(_: rusqlite::Error) -> CoreError {
    CoreError::DatabaseInvalid
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
