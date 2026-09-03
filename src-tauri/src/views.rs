//! Saved application views. Metadata operations never write records or files.
use crate::{
    applications::now_utc,
    applications::validate_required_text,
    domain::{SavedView, SavedViewChange, SavedViewRequest, ViewMetadataRequest},
    error::CoreError,
    warehouse::WarehouseSession,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;
use uuid::Uuid;

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedView> {
    fn json(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Value> {
        let raw: String = row.get(index)?;
        serde_json::from_str(&raw).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
    }
    let group: Option<String> = row.get(5)?;
    Ok(SavedView {
        id: row.get(0)?,
        name: row.get(1)?,
        layout: json(row, 2)?,
        sort: json(row, 3)?,
        filter: json(row, 4)?,
        group: group
            .map(|raw| {
                serde_json::from_str(&raw).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .transpose()?,
        is_default: row.get(6)?,
        revision: row.get(7)?,
    })
}
fn get(connection: &Connection, id: &str) -> Result<SavedView, CoreError> {
    connection.query_row("SELECT id, name, layout_json, sort_json, filter_json, group_json, is_default, revision FROM views WHERE id = ?1 AND view_kind = 'applications'",
        [id], map_row).optional().map_err(|_| CoreError::DatabaseInvalid)?.ok_or(CoreError::NotFound)
}
pub fn list(session: &WarehouseSession) -> Result<Vec<SavedView>, CoreError> {
    list_connection(session.connection())
}
fn list_connection(connection: &Connection) -> Result<Vec<SavedView>, CoreError> {
    let mut statement = connection.prepare("SELECT id, name, layout_json, sort_json, filter_json, group_json, is_default, revision FROM views WHERE view_kind = 'applications' ORDER BY is_default DESC, name, id")
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let views = statement
        .query_map([], map_row)
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for view in &views {
        validate_view(&as_request(view)).map_err(|_| CoreError::DatabaseInvalid)?;
    }
    Ok(views)
}
fn as_request(view: &SavedView) -> SavedViewRequest {
    SavedViewRequest {
        id: Some(view.id.clone()),
        revision: Some(view.revision),
        name: view.name.clone(),
        layout: view.layout.clone(),
        sort: view.sort.clone(),
        filter: view.filter.clone(),
        group: view.group.clone(),
        is_default: view.is_default,
    }
}
fn require_revision(view: &SavedView, revision: i64) -> Result<(), CoreError> {
    if view.revision != revision {
        Err(CoreError::RevisionConflict)
    } else {
        Ok(())
    }
}
fn clear_old_default(transaction: &Transaction<'_>, id: &str, now: &str) -> Result<(), CoreError> {
    transaction.execute("UPDATE views SET is_default = 0, revision = revision + 1, updated_at_utc = ?1 WHERE view_kind = 'applications' AND is_default = 1 AND id != ?2",
        params![now, id]).map(|_| ()).map_err(|_| CoreError::DatabaseInvalid)
}
fn finish(connection: &Connection, id: &str) -> Result<SavedViewChange, CoreError> {
    Ok(SavedViewChange {
        view: get(connection, id)?,
        views: list_connection(connection)?,
    })
}
pub fn save(
    session: &mut WarehouseSession,
    request: SavedViewRequest,
) -> Result<SavedViewChange, CoreError> {
    validate_required_text(&request.name)?;
    validate_view(&request)?;
    let now = now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let existing = request
        .id
        .as_ref()
        .map(|id| get(&transaction, id))
        .transpose()?;
    if let Some(view) = &existing {
        require_revision(view, request.revision.ok_or(CoreError::Validation)?)?;
    } else if request.revision.is_some() {
        return Err(CoreError::Validation);
    }
    let id = request.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    if request.is_default {
        clear_old_default(&transaction, &id, &now)?;
    }
    if existing.is_some() {
        transaction.execute("UPDATE views SET name = ?1, layout_json = ?2, sort_json = ?3, filter_json = ?4, group_json = ?5, is_default = ?6,
            revision = revision + 1, updated_at_utc = ?7 WHERE id = ?8 AND view_kind = 'applications'",
            params![request.name.trim(), request.layout.to_string(), request.sort.to_string(), request.filter.to_string(), request.group.map(|v| v.to_string()), request.is_default, now, id])
            .map_err(|_| CoreError::DatabaseInvalid)?;
    } else {
        transaction.execute("INSERT INTO views (id, name, view_kind, layout_json, sort_json, filter_json, group_json, is_default, created_at_utc, updated_at_utc)
            VALUES (?1, ?2, 'applications', ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![id, request.name.trim(), request.layout.to_string(), request.sort.to_string(), request.filter.to_string(), request.group.map(|v| v.to_string()), request.is_default, now])
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    // Validate the response inside the transaction: malformed existing metadata
    // must not turn a committed write into an apparent failure and unsafe retry.
    let result = finish(&transaction, &id)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}
pub fn metadata(
    session: &mut WarehouseSession,
    request: ViewMetadataRequest,
) -> Result<SavedViewChange, CoreError> {
    validate_required_text(&request.name)?;
    let now = now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let source = get(&transaction, &request.id)?;
    require_revision(&source, request.revision)?;
    if request.is_default {
        clear_old_default(&transaction, &request.id, &now)?;
    }
    transaction.execute("UPDATE views SET name = ?1, is_default = ?2, updated_at_utc = ?3, revision = revision + 1 WHERE id = ?4 AND view_kind = 'applications'",
        params![request.name.trim(), request.is_default, now, request.id]).map_err(|_| CoreError::DatabaseInvalid)?;
    let result = finish(&transaction, &request.id)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}
pub fn duplicate(
    session: &mut WarehouseSession,
    id: &str,
    revision: i64,
    name: &str,
) -> Result<SavedViewChange, CoreError> {
    validate_required_text(name)?;
    let now = now_utc();
    let target = Uuid::new_v4().to_string();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let source = get(&transaction, id)?;
    require_revision(&source, revision)?;
    validate_view(&as_request(&source))?;
    transaction.execute("INSERT INTO views (id, name, view_kind, layout_json, sort_json, filter_json, group_json, is_default, created_at_utc, updated_at_utc)
        SELECT ?1, ?2, view_kind, layout_json, sort_json, filter_json, group_json, 0, ?3, ?3 FROM views WHERE id = ?4 AND view_kind = 'applications'",
        params![target, name.trim(), now, id]).map_err(|_| CoreError::DatabaseInvalid)?;
    let result = finish(&transaction, &target)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}
pub fn delete(
    session: &mut WarehouseSession,
    id: &str,
    revision: i64,
) -> Result<Vec<SavedView>, CoreError> {
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    require_revision(&get(&transaction, id)?, revision)?;
    transaction
        .execute(
            "DELETE FROM views WHERE id = ?1 AND view_kind = 'applications'",
            [id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let result = list_connection(&transaction)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}

fn validate_view(request: &SavedViewRequest) -> Result<(), CoreError> {
    let columns = request
        .layout
        .get("columns")
        .and_then(Value::as_array)
        .ok_or(CoreError::Validation)?;
    let mut keys = std::collections::HashSet::new();
    for column in columns {
        let key = column
            .get("key")
            .and_then(Value::as_str)
            .ok_or(CoreError::Validation)?;
        if key.is_empty()
            || !keys.insert(key)
            || !column
                .get("width")
                .and_then(Value::as_i64)
                .is_some_and(|width| (80..=600).contains(&width))
            || !column.get("visible").is_some_and(Value::is_boolean)
            || !column.get("pinned").is_some_and(Value::is_boolean)
        {
            return Err(CoreError::Validation);
        }
    }
    for rule in request.sort.as_array().ok_or(CoreError::Validation)? {
        if !rule.get("key").is_some_and(Value::is_string)
            || !matches!(
                rule.get("direction").and_then(Value::as_str),
                Some("asc" | "desc")
            )
        {
            return Err(CoreError::Validation);
        }
    }
    if !request.filter.get("search").is_some_and(Value::is_string) {
        return Err(CoreError::Validation);
    }
    if let Some(state) = request.filter.get("businessState")
        && !matches!(
            state.as_str(),
            Some("preparing" | "inProgress" | "awaitingResult" | "ended")
        )
    {
        return Err(CoreError::Validation);
    }
    for key in ["companyTypes", "stages"] {
        if !request
            .filter
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().all(Value::is_string))
        {
            return Err(CoreError::Validation);
        }
    }
    if let Some(group) = &request.group
        && !matches!(
            group.as_str(),
            Some("companyType" | "currentStageName" | "workLocation")
        )
    {
        return Err(CoreError::Validation);
    }
    Ok(())
}
