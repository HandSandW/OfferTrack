//! Recruitment event metadata. Linked interview scheduling remains owned by its round.
use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::CoreError, tasks, warehouse::WarehouseSession};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub revision: i64,
    pub application_id: Option<String>,
    pub application_label: Option<String>,
    pub application_archived: bool,
    pub application_terminal: bool,
    pub event_type: String,
    pub title: String,
    pub notes: String,
    pub starts_at_utc: Option<String>,
    pub deadline_at_utc: Option<String>,
    pub completed_at_utc: Option<String>,
    pub finished: bool,
    pub interview_round_id: Option<String>,
    pub interview_round_name: Option<String>,
    pub location: String,
    pub meeting_url: Option<String>,
    pub result: String,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub source_version: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveEvent {
    pub id: Option<String>,
    pub revision: Option<i64>,
    pub application_id: String,
    pub event_type: String,
    pub title: String,
    pub notes: String,
    pub starts_at_utc: Option<String>,
    pub deadline_at_utc: Option<String>,
    pub interview_round_id: Option<String>,
    pub location: String,
    pub meeting_url: Option<String>,
    pub result: String,
}

pub fn list(connection: &Connection) -> Result<Vec<Event>, CoreError> {
    let mut query = connection.prepare("SELECT e.id,e.revision,e.application_id,
      CASE WHEN a.id IS NOT NULL THEN a.company_name || ' · ' || a.position_name END,
      a.archived_at_utc IS NOT NULL,(COALESCE(s.is_terminal,0)=1 OR a.current_stage_state='failed'),
      e.event_type,e.title,e.notes,
      CASE WHEN i.id IS NULL THEN e.starts_at_utc ELSE i.scheduled_at_utc END,e.deadline_at_utc,
      CASE WHEN i.id IS NULL THEN e.completed_at_utc ELSE i.completed_at_utc END,
      CASE WHEN i.id IS NULL THEN e.completed_at_utc IS NOT NULL ELSE (i.completed_at_utc IS NOT NULL OR COALESCE(w.semantic_kind,'') IN ('completed','failed')) END,
      e.interview_round_id,i.display_name,e.location,e.meeting_url,
      CASE WHEN i.id IS NULL THEN e.result ELSE i.result END,e.created_at_utc,e.updated_at_utc,
      CAST(e.revision AS TEXT) || ':' || COALESCE(i.updated_at_utc,'') || ':' || COALESCE(i.state,'') || ':' || COALESCE(i.scheduled_at_utc,'') || ':' || COALESCE(i.completed_at_utc,'')
      FROM recruitment_events e LEFT JOIN applications a ON a.id=e.application_id
      LEFT JOIN workflow_stages s ON s.id=a.current_stage_id
      LEFT JOIN interview_rounds i ON i.id=e.interview_round_id
      LEFT JOIN workflow_states w ON w.application_id=i.application_id AND w.stable_key=i.state
      WHERE e.application_id IS NULL OR (a.id IS NOT NULL AND a.deleted_at_utc IS NULL)
      ORDER BY e.created_at_utc DESC,e.id").map_err(|_| CoreError::DatabaseInvalid)?;
    query
        .query_map([], |r| {
            Ok(Event {
                id: r.get(0)?,
                revision: r.get(1)?,
                application_id: r.get(2)?,
                application_label: r.get(3)?,
                application_archived: r.get(4)?,
                application_terminal: r.get::<_, Option<bool>>(5)?.unwrap_or(false),
                event_type: r.get(6)?,
                title: r.get(7)?,
                notes: r.get(8)?,
                starts_at_utc: r.get(9)?,
                deadline_at_utc: r.get(10)?,
                completed_at_utc: r.get(11)?,
                finished: r.get(12)?,
                interview_round_id: r.get(13)?,
                interview_round_name: r.get(14)?,
                location: r.get(15)?,
                meeting_url: r.get(16)?,
                result: r.get(17)?,
                created_at_utc: r.get(18)?,
                updated_at_utc: r.get(19)?,
                source_version: r.get(20)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn find(connection: &Connection, id: &str, revision: i64) -> Result<Event, CoreError> {
    list(connection)?
        .into_iter()
        .find(|e| e.id == id && e.revision == revision)
        .ok_or(CoreError::RevisionConflict)
}

pub fn save(session: &mut WarehouseSession, request: &SaveEvent) -> Result<Event, CoreError> {
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
    request: &SaveEvent,
) -> Result<Event, CoreError> {
    if request.title.trim().is_empty()
        || request.title.chars().count() > 200
        || request.notes.chars().count() > 100_000
        || request.result.chars().count() > 100_000
        || request.location.chars().count() > 1000
        || request.id.is_some() != request.revision.is_some()
        || !["assessment", "writtenExam", "interview", "signing", "other"]
            .contains(&request.event_type.as_str())
    {
        return Err(CoreError::Validation);
    }
    if let (Some(id), Some(revision)) = (&request.id, request.revision) {
        find(tx, id, revision)?;
    }
    let valid: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM applications WHERE id=?1 AND deleted_at_utc IS NULL)",
            [&request.application_id],
            |r| r.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if !valid {
        return Err(CoreError::Validation);
    }
    let url = request
        .meeting_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    if let Some(value) = &url {
        let parsed = url::Url::parse(value).map_err(|_| CoreError::Validation)?;
        if value.len() > 4096
            || value.chars().any(char::is_control)
            || !["https", "http"].contains(&parsed.scheme())
            || parsed.host_str().is_none()
        {
            return Err(CoreError::Validation);
        }
    }
    let deadline = tasks::normalized(&request.deadline_at_utc)?;
    let start = if let Some(round) = &request.interview_round_id {
        if request.event_type != "interview"
            || request.starts_at_utc.is_some()
            || !request.result.is_empty()
        {
            return Err(CoreError::Validation);
        }
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM interview_rounds i WHERE i.id=?1 AND i.application_id=?2 AND NOT EXISTS(SELECT 1 FROM recruitment_events e WHERE e.interview_round_id=i.id AND e.id<>COALESCE(?3,'')))",params![round,request.application_id,request.id],|r|r.get(0)).map_err(|_|CoreError::DatabaseInvalid)?;
        if !valid {
            return Err(CoreError::Validation);
        }
        // Existing non-null start column is kept for old-format preservation; linked reads use the round.
        tx.query_row(
            "SELECT scheduled_at_utc FROM interview_rounds WHERE id=?1",
            [round],
            |r| r.get::<_, Option<String>>(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?
    } else {
        Some(tasks::normalized(&request.starts_at_utc)?.ok_or(CoreError::Validation)?)
    };
    if let (Some(start), Some(end)) = (&start, &deadline)
        && tasks::timestamp(end)? < tasks::timestamp(start)?
    {
        return Err(CoreError::Validation);
    }
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let id = request
        .id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let stored_start = start.as_deref().unwrap_or(&now);
    if request.id.is_some() {
        tx.execute("UPDATE recruitment_events SET application_id=?1,event_type=?2,title=?3,notes=?4,starts_at_utc=?5,deadline_at_utc=?6,interview_round_id=?7,location=?8,meeting_url=?9,result=CASE WHEN ?7 IS NULL THEN ?10 ELSE result END,updated_at_utc=?11,revision=revision+1 WHERE id=?12 AND revision=?13",
            params![request.application_id,request.event_type,request.title.trim(),request.notes,stored_start,deadline,request.interview_round_id,request.location.trim(),url,request.result,now,id,request.revision]).map_err(|_|CoreError::DatabaseInvalid)?;
    } else {
        tx.execute("INSERT INTO recruitment_events (application_id,event_type,title,notes,starts_at_utc,deadline_at_utc,interview_round_id,location,meeting_url,result,created_at_utc,updated_at_utc,id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11,?12)",
            params![request.application_id,request.event_type,request.title.trim(),request.notes,stored_start,deadline,request.interview_round_id,request.location.trim(),url,request.result,now,id]).map_err(|_|CoreError::DatabaseInvalid)?;
    }
    let result = find(tx, &id, request.revision.map_or(1, |v| v + 1))?;
    Ok(result)
}

pub fn complete(
    session: &mut WarehouseSession,
    id: &str,
    revision: i64,
    completed: bool,
) -> Result<Event, CoreError> {
    let tx = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let old = find(&tx, id, revision)?;
    if old.interview_round_id.is_some() {
        return Err(CoreError::Validation);
    }
    if old.finished == completed {
        return Ok(old);
    }
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    tx.execute("UPDATE recruitment_events SET completed_at_utc=?1,updated_at_utc=?2,revision=revision+1 WHERE id=?3 AND revision=?4",params![completed.then_some(&now),now,id,revision]).map_err(|_|CoreError::DatabaseInvalid)?;
    let result = find(&tx, id, revision + 1)?;
    tx.commit().map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}

#[cfg(test)]
#[path = "recruitment_tests.rs"]
mod tests;
