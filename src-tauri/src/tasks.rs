use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::CoreError, warehouse::WarehouseSession};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub revision: i64,
    pub application_id: Option<String>,
    pub application_label: Option<String>,
    pub application_archived: bool,
    pub title: String,
    pub notes: String,
    pub priority: String,
    pub due_at_utc: Option<String>,
    pub remind_at_utc: Option<String>,
    pub completed_at_utc: Option<String>,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveTask {
    pub id: Option<String>,
    pub revision: Option<i64>,
    pub application_id: Option<String>,
    pub title: String,
    pub notes: String,
    pub priority: String,
    pub due_at_utc: Option<String>,
    pub remind_at_utc: Option<String>,
}

pub fn timestamp(value: &str) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| CoreError::Validation)
}

pub(crate) fn normalized(value: &Option<String>) -> Result<Option<String>, CoreError> {
    value
        .as_deref()
        .map(|v| timestamp(v).map(|d| d.to_rfc3339_opts(SecondsFormat::Millis, true)))
        .transpose()
}

pub fn list(connection: &Connection) -> Result<Vec<Task>, CoreError> {
    let mut statement = connection.prepare(
        "SELECT t.id,t.revision,t.application_id,
         CASE WHEN a.id IS NOT NULL THEN a.company_name || ' · ' || a.position_name END,
         a.archived_at_utc IS NOT NULL,t.title,t.notes,t.priority,t.due_at_utc,t.remind_at_utc,
         t.completed_at_utc,t.created_at_utc,t.updated_at_utc
         FROM tasks t LEFT JOIN applications a ON a.id=t.application_id
         WHERE t.deleted_at_utc IS NULL AND (t.application_id IS NULL OR (a.id IS NOT NULL AND a.deleted_at_utc IS NULL))
         ORDER BY t.created_at_utc DESC,t.id",
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([], |row| {
            Ok(Task {
                id: row.get(0)?,
                revision: row.get(1)?,
                application_id: row.get(2)?,
                application_label: row.get(3)?,
                application_archived: row.get(4)?,
                title: row.get(5)?,
                notes: row.get(6)?,
                priority: row.get(7)?,
                due_at_utc: row.get(8)?,
                remind_at_utc: row.get(9)?,
                completed_at_utc: row.get(10)?,
                created_at_utc: row.get(11)?,
                updated_at_utc: row.get(12)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub fn save(session: &mut WarehouseSession, request: &SaveTask) -> Result<Task, CoreError> {
    let tx = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let result = save_in_transaction(&tx, request)?;
    tx.commit().map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}

pub(crate) fn save_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    request: &SaveTask,
) -> Result<Task, CoreError> {
    if request.title.trim().is_empty()
        || request.title.chars().count() > 200
        || request.notes.chars().count() > 100_000
        || !["low", "normal", "high"].contains(&request.priority.as_str())
        || request.id.is_some() != request.revision.is_some()
    {
        return Err(CoreError::Validation);
    }
    let due = normalized(&request.due_at_utc)?;
    let reminder = normalized(&request.remind_at_utc)?;
    if let Some(id) = &request.application_id {
        let valid: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM applications WHERE id=?1 AND deleted_at_utc IS NULL)",
                [id],
                |r| r.get(0),
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        if !valid {
            return Err(CoreError::Validation);
        }
    }
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let id = request
        .id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if request.id.is_some() {
        if !list(tx)?
            .iter()
            .any(|t| t.id == id && Some(t.revision) == request.revision)
        {
            return Err(CoreError::RevisionConflict);
        }
        let changed = tx.execute("UPDATE tasks SET application_id=?1,title=?2,notes=?3,priority=?4,due_at_utc=?5,remind_at_utc=?6,updated_at_utc=?7,revision=revision+1 WHERE id=?8 AND revision=?9 AND deleted_at_utc IS NULL",
            params![request.application_id, request.title.trim(), request.notes, request.priority, due, reminder, now, id, request.revision]).map_err(|_| CoreError::DatabaseInvalid)?;
        if changed != 1 {
            return Err(CoreError::RevisionConflict);
        }
    } else {
        tx.execute("INSERT INTO tasks (id,application_id,title,notes,priority,due_at_utc,remind_at_utc,created_at_utc,updated_at_utc) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)",
            params![id, request.application_id, request.title.trim(), request.notes, request.priority, due, reminder, now]).map_err(|_| CoreError::DatabaseInvalid)?;
    }
    let result = list(tx)?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or(CoreError::DatabaseInvalid)?;
    Ok(result)
}

pub fn complete(
    session: &mut WarehouseSession,
    id: &str,
    revision: i64,
    completed: bool,
) -> Result<Task, CoreError> {
    let tx = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let old = list(&tx)?
        .into_iter()
        .find(|t| t.id == id && t.revision == revision)
        .ok_or(CoreError::RevisionConflict)?;
    if old.completed_at_utc.is_some() == completed {
        return Ok(old);
    }
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    tx.execute("UPDATE tasks SET completed_at_utc=?1,updated_at_utc=?2,revision=revision+1 WHERE id=?3 AND revision=?4", params![completed.then_some(&now), now, id, revision]).map_err(|_| CoreError::DatabaseInvalid)?;
    let result = list(&tx)?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or(CoreError::DatabaseInvalid)?;
    tx.commit().map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReminderRule {
    pub key: String,
    pub label: String,
    pub enabled: bool,
    pub value: i64,
    pub revision: i64,
}

pub const RULE_KEYS: [&str; 7] = [
    "missing_resume",
    "preparing_idle",
    "stage_idle",
    "result_idle",
    "due_soon",
    "due_urgent",
    "overdue",
];

pub fn rules(connection: &Connection) -> Result<Vec<ReminderRule>, CoreError> {
    RULE_KEYS.iter().map(|key| {
        let rule = connection.query_row("SELECT display_name,is_enabled,threshold_json,revision FROM reminder_rules WHERE stable_key=?1", [key], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
        }).optional().map_err(|_| CoreError::DatabaseInvalid)?.ok_or(CoreError::DatabaseInvalid)?;
        let value = serde_json::from_str::<serde_json::Value>(&rule.2).map_err(|_| CoreError::DatabaseInvalid)?["value"].as_i64().ok_or(CoreError::DatabaseInvalid)?;
        if (*key == "overdue" && value != 0) || (*key != "overdue" && !(1..=8760).contains(&value)) { return Err(CoreError::DatabaseInvalid); }
        Ok(ReminderRule { key: key.to_string(), label: rule.0, enabled: rule.1, value, revision: rule.3 })
    }).collect()
}

pub fn save_rules(
    session: &mut WarehouseSession,
    edits: &[ReminderRule],
) -> Result<Vec<ReminderRule>, CoreError> {
    let tx = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let old = rules(&tx)?;
    if edits.len() != RULE_KEYS.len() {
        return Err(CoreError::Validation);
    }
    for rule in &old {
        let matches: Vec<_> = edits.iter().filter(|e| e.key == rule.key).collect();
        if matches.len() != 1 {
            return Err(CoreError::Validation);
        }
        let edit = matches[0];
        if edit.revision != rule.revision {
            return Err(CoreError::RevisionConflict);
        }
        if edit.label != rule.label
            || (edit.key == "overdue" && edit.value != 0)
            || (edit.key != "overdue" && !(1..=8760).contains(&edit.value))
        {
            return Err(CoreError::Validation);
        }
        if edit.enabled != rule.enabled || edit.value != rule.value {
            tx.execute("UPDATE reminder_rules SET is_enabled=?1,threshold_json=?2,revision=revision+1,updated_at_utc=?3 WHERE stable_key=?4", params![edit.enabled, serde_json::json!({"value": edit.value}).to_string(), Utc::now().to_rfc3339(), edit.key]).map_err(|_| CoreError::DatabaseInvalid)?;
        }
    }
    let result = rules(&tx)?;
    tx.commit().map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}
