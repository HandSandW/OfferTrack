use super::{Action, Changed, db};
use crate::{
    agent_access::{self, dto},
    applications,
    domain::{ChangeStageRequest, UpdateApplicationRequest},
    error::CoreError,
    recruitment, tasks,
};
use rusqlite::{Transaction, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

fn string(value: &Value) -> Result<String, CoreError> {
    let s = value.as_str().ok_or(CoreError::Validation)?;
    if s.chars().count() > 100_000 {
        return Err(CoreError::AgentLimit);
    }
    Ok(s.into())
}
fn optional(value: &Value) -> Result<Option<String>, CoreError> {
    if value.is_null() {
        Ok(None)
    } else {
        string(value).map(Some)
    }
}

fn patch(
    tx: &Transaction<'_>,
    id: &str,
    revision: i64,
    fields: &BTreeMap<String, Value>,
) -> Result<(), CoreError> {
    if fields.is_empty() {
        return Err(CoreError::Validation);
    }
    let record = applications::load_record(tx, id)?;
    let mut edit = UpdateApplicationRequest {
        id: id.into(),
        revision,
        company_name: record.company_name,
        company_type: record.company_type,
        industry: record.industry,
        position_name: record.position_name,
        position_category: record.position_category,
        work_location: record.work_location,
        application_date: record.application_date,
        application_url: record.application_url,
        announcement_url: record.announcement_url,
        company_url: record.company_url,
        position_url: record.position_url,
        position_description: record.position_description,
        notes: record.notes,
        tags: record.tags.into_iter().map(|t| t.name).collect(),
        custom_fields: record.custom_fields,
    };
    for (key, value) in fields {
        match key.as_str() {
            "company_name" => edit.company_name = string(value)?,
            "company_type" => edit.company_type = string(value)?,
            "industry" => edit.industry = string(value)?,
            "position_name" => edit.position_name = string(value)?,
            "position_category" => edit.position_category = string(value)?,
            "work_location" => edit.work_location = string(value)?,
            "application_date" => edit.application_date = optional(value)?,
            "application_url" => edit.application_url = optional(value)?,
            "announcement_url" => edit.announcement_url = optional(value)?,
            "company_url" => edit.company_url = optional(value)?,
            "position_url" => edit.position_url = optional(value)?,
            "position_description" => edit.position_description = string(value)?,
            "notes" => edit.notes = string(value)?,
            "tags" => {
                let tags = value.as_array().ok_or(CoreError::Validation)?;
                if tags.len() > 100 {
                    return Err(CoreError::Validation);
                }
                edit.tags = tags.iter().map(string).collect::<Result<_, _>>()?;
                if edit
                    .tags
                    .iter()
                    .any(|s| s.trim().is_empty() || s.chars().count() > 40)
                {
                    return Err(CoreError::Validation);
                }
            }
            "custom_fields" => {
                // Patch semantics: unspecified IDs are preserved; null explicitly clears a value.
                for (id, value) in value.as_object().ok_or(CoreError::Validation)? {
                    edit.custom_fields.insert(id.clone(), value.clone());
                }
            }
            _ => return Err(CoreError::Validation),
        }
    }
    applications::update_in_transaction(tx, &edit)?;
    if fields.contains_key("company_name") || fields.contains_key("position_name") {
        tx.execute(
            "UPDATE applications SET folder_normalization_pending=1 WHERE id=?1",
            [id],
        )
        .map_err(db)?;
    }
    Ok(())
}

pub(super) fn run(
    tx: &Transaction<'_>,
    actions: &[Action],
) -> Result<(Vec<Changed>, Vec<Value>), CoreError> {
    agent_access::check_budget(tx, 10_000, agent_access::MAX_BYTES)?;
    // Fix all expected application revisions BEFORE any mutation. At most one
    // application edit per batch; multiple newly created tasks/events are allowed.
    let mut edited = HashSet::new();
    for action in actions {
        let target = match action {
            Action::UpdateFields {
                application_id,
                revision,
                ..
            }
            | Action::AppendNotes {
                application_id,
                revision,
                ..
            }
            | Action::ChangeStage {
                application_id,
                revision,
                ..
            } => {
                if !edited.insert(application_id) {
                    return Err(CoreError::Validation);
                }
                Some((application_id, *revision))
            }
            Action::CreateTask {
                application_id,
                application_revision,
                ..
            } => match (application_id, application_revision) {
                (None, None) => None,
                (Some(id), Some(revision)) => Some((id, *revision)),
                _ => return Err(CoreError::Validation),
            },
            Action::CreateEvent {
                application_id,
                application_revision,
                ..
            } => Some((application_id, *application_revision)),
        };
        if let Some((id, revision)) = target {
            applications::require_record_revision(tx, id, revision)?;
        }
    }
    let mut results = Vec::new();
    let mut changes = Vec::new();
    let now = applications::now_utc();
    for action in actions {
        match action {
            Action::UpdateFields {
                application_id,
                revision,
                ..
            }
            | Action::AppendNotes {
                application_id,
                revision,
                ..
            }
            | Action::ChangeStage {
                application_id,
                revision,
                ..
            } => {
                let before = applications::load_record(tx, application_id)?;
                match action {
                    Action::UpdateFields { fields, .. } => {
                        patch(tx, application_id, *revision, fields)?
                    }
                    Action::AppendNotes { text, .. } => {
                        if text.trim().is_empty() {
                            return Err(CoreError::Validation);
                        }
                        let notes = if before.notes.is_empty() {
                            text.clone()
                        } else {
                            format!("{}\n{}", before.notes, text)
                        };
                        if notes.chars().count() > 100_000 {
                            return Err(CoreError::AgentLimit);
                        }
                        tx.execute("UPDATE applications SET notes=?1,revision=revision+1,updated_at_utc=?2 WHERE id=?3 AND revision=?4",params![notes,now,application_id,revision]).map_err(db)?;
                    }
                    Action::ChangeStage {
                        stage_id,
                        state_key,
                        notes,
                        ..
                    } => applications::change_stage_with_actor(
                        tx,
                        &ChangeStageRequest {
                            application_id: application_id.clone(),
                            revision: *revision,
                            stage_id: stage_id.clone(),
                            stage_state: state_key.clone(),
                            notes: notes.clone(),
                        },
                        &now,
                        "agent",
                    )?,
                    _ => unreachable!(),
                }
                let after = applications::load_record(tx, application_id)?;
                results.push(Changed {
                    entity_type: "application".into(),
                    id: application_id.clone(),
                    revision: after.revision,
                });
                changes.push(json!({"operation":action,"before":before,"after":after}));
            }
            Action::CreateTask {
                application_id,
                title,
                notes,
                priority,
                due_at_utc,
                remind_at_utc,
                ..
            } => {
                let task = tasks::save_in_transaction(
                    tx,
                    &tasks::SaveTask {
                        id: None,
                        revision: None,
                        application_id: application_id.clone(),
                        title: title.clone(),
                        notes: notes.clone(),
                        priority: priority.clone(),
                        due_at_utc: due_at_utc.clone(),
                        remind_at_utc: remind_at_utc.clone(),
                    },
                )?;
                results.push(Changed {
                    entity_type: "task".into(),
                    id: task.id.clone(),
                    revision: task.revision,
                });
                changes
                    .push(json!({"operation":action,"before":null,"after":dto::Task::from(task)}));
            }
            Action::CreateEvent {
                application_id,
                event_type,
                title,
                starts_at_utc,
                deadline_at_utc,
                interview_round_id,
                location,
                meeting_url,
                result,
                notes,
                ..
            } => {
                let event = recruitment::save_in_transaction(
                    tx,
                    &recruitment::SaveEvent {
                        id: None,
                        revision: None,
                        application_id: application_id.clone(),
                        event_type: event_type.clone(),
                        title: title.clone(),
                        starts_at_utc: starts_at_utc.clone(),
                        deadline_at_utc: deadline_at_utc.clone(),
                        interview_round_id: interview_round_id.clone(),
                        location: location.clone(),
                        meeting_url: meeting_url.clone(),
                        result: result.clone(),
                        notes: notes.clone(),
                    },
                )?;
                results.push(Changed {
                    entity_type: "event".into(),
                    id: event.id.clone(),
                    revision: event.revision,
                });
                changes.push(
                    json!({"operation":action,"before":null,"after":dto::Event::from(event)}),
                );
            }
        }
        agent_access::encode_with_limit(&changes, super::AUDIT_LIMIT / 2)?;
    }
    Ok((results, changes))
}
