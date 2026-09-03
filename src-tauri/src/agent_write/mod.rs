//! Controlled, metadata-only writes. No recovery, file moves or deletion here.
mod dto;
mod operations;
pub(crate) mod schema;
pub(crate) mod settings;
pub use dto::*;

use crate::{
    agent_access, applications, database_backup, error::CoreError, warehouse::WarehouseSession,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const AUDIT_LIMIT: usize = 16 * 1024 * 1024;
pub(super) fn db(_: rusqlite::Error) -> CoreError {
    CoreError::DatabaseInvalid
}

pub fn validate(request: &Request) -> Result<(), CoreError> {
    if request.version != 1 {
        return Err(CoreError::AgentVersion);
    }
    if request.actions.is_empty()
        || request.actions.len() > 50
        || request.source.trim().is_empty()
        || request.source.chars().count() > 200
        || request.source.chars().any(char::is_control)
        || request.request_id.is_nil()
        || request.warehouse_id.is_nil()
    {
        return Err(CoreError::Validation);
    }
    agent_access::encode_with_limit(request, 64 * 1024)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    request_sha256: String,
    actor: String,
    transport: String,
    source_unverified: String,
    response: Applied,
    changes: Vec<Value>,
}

fn receipt(
    connection: &Connection,
    request: &Request,
    hash: &str,
) -> Result<Option<Applied>, CoreError> {
    let item: Option<(String, i64)> = connection.query_row(
        "SELECT operation,length(CAST(change_summary_json AS BLOB)) FROM agent_audit_log WHERE id=?1",
        [request.request_id.to_string()], |r| Ok((r.get(0)?,r.get(1)?))
    ).optional().map_err(db)?;
    let Some((operation, length)) = item else {
        return Ok(None);
    };
    if operation != "write" {
        return Err(CoreError::AgentRequestConflict);
    }
    if length < 0 || length as usize > AUDIT_LIMIT {
        return Err(CoreError::AgentLimit);
    }
    let raw: String = connection
        .query_row(
            "SELECT change_summary_json FROM agent_audit_log WHERE id=?1",
            [request.request_id.to_string()],
            |r| r.get(0),
        )
        .map_err(db)?;
    let saved: Receipt = serde_json::from_str(&raw).map_err(|_| CoreError::DatabaseInvalid)?;
    if saved.version != 1
        || saved.request_sha256 != hash
        || saved.response.warehouse_id != request.warehouse_id
        || saved.response.request_id != request.request_id
    {
        return Err(CoreError::AgentRequestConflict);
    }
    Ok(Some(saved.response))
}

pub fn apply(
    session: &mut WarehouseSession,
    request: &Request,
    transport: &str,
) -> Result<Applied, CoreError> {
    validate(request)?;
    if session.summary().warehouse_id != request.warehouse_id {
        return Err(CoreError::AgentWarehouseChanged);
    }
    if !session.is_writable() {
        return Err(CoreError::ReadOnlyWarehouse);
    }
    if !["cli", "mcp"].contains(&transport) {
        return Err(CoreError::Validation);
    }
    let permission = settings::require(session.connection())?;
    let hash = format!("{:x}", Sha256::digest(agent_access::encode(request)?));
    if let Some(saved) = receipt(session.connection(), request, &hash)? {
        return Ok(saved);
    }
    database_backup::inspect_records(session.connection())?;
    // Validate every action by running the exact transaction body and rolling it back.
    // No external effect occurs in operations, including folder normalization.
    {
        let tx = session.connection_mut()?.transaction().map_err(db)?;
        let (_, changes) = operations::run(&tx, &request.actions)?;
        agent_access::encode_with_limit(&changes, AUDIT_LIMIT / 2)?;
        tx.rollback().map_err(db)?;
    }
    let backup = database_backup::create_at(
        session.connection(),
        session.root(),
        request.warehouse_id,
        "beforeAgentWrite",
    )?;
    let tx = session
        .connection_mut()?
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    if settings::require(&tx)? != permission {
        return Err(CoreError::RevisionConflict);
    }
    if let Some(saved) = receipt(&tx, request, &hash)? {
        return Ok(saved);
    }
    let (results, changes) = operations::run(&tx, &request.actions)?;
    let response = Applied {
        version: 1,
        warehouse_id: request.warehouse_id,
        request_id: request.request_id,
        backup_id: backup.id,
        committed_at_utc: applications::now_utc(),
        results,
        snapshot_refresh_required: true,
    };
    let receipt = Receipt {
        version: 1,
        request_sha256: hash,
        actor: "agent".into(),
        transport: transport.into(),
        source_unverified: request.source.clone(),
        response: response.clone(),
        changes,
    };
    let bytes = agent_access::encode_with_limit(&receipt, AUDIT_LIMIT)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| CoreError::DatabaseInvalid)?;
    tx.execute("INSERT INTO agent_audit_log (id,operation,entity_type,request_version,change_summary_json,occurred_at_utc,outcome) VALUES (?1,'write','batch',1,?2,?3,'committed')",
        params![request.request_id.to_string(), text, response.committed_at_utc]).map_err(db)?;
    // The receipt and all entity changes commit together. Never run fallible reads after commit.
    tx.commit().map_err(db)?;
    Ok(response)
}

/// Safe cross-process entry. Keeps the shared reader's path/identity guards;
/// acquires the SAME exclusive warehouse lock as desktop, without recovery/migration.
pub fn execute(
    path: &std::path::Path,
    request: &Request,
    transport: &str,
) -> Result<Value, CoreError> {
    validate(request)?;
    let mut access = agent_access::reader::open(path)?;
    if access.session.summary().warehouse_id != request.warehouse_id {
        return Err(CoreError::AgentWarehouseChanged);
    }
    settings::require(access.session.connection())?;
    access.acquire_writer()?;
    let result = apply(&mut access.session, request, transport)?;
    // The immutable receipt describes the commit; this separate observation can change on retries.
    // Never use ? after committing the business transaction to report a derived failure as rejection.
    let snapshot_status = agent_access::freshness::check(&access.session, true);
    let mut response = json!(result);
    response["snapshot_status"] = json!(snapshot_status);
    Ok(response)
}

#[derive(Serialize)]
pub struct AuditItem {
    pub id: String,
    pub operation: String,
    pub occurred_at_utc: String,
    pub outcome: String,
}

pub fn audit_list(session: &WarehouseSession) -> Result<Vec<AuditItem>, CoreError> {
    let mut stmt = session.connection().prepare("SELECT id,operation,occurred_at_utc,outcome FROM agent_audit_log ORDER BY occurred_at_utc DESC,id DESC LIMIT 50").map_err(db)?;
    stmt.query_map([], |r| {
        Ok(AuditItem {
            id: r.get(0)?,
            operation: r.get(1)?,
            occurred_at_utc: r.get(2)?,
            outcome: r.get(3)?,
        })
    })
    .map_err(db)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(db)
}

pub fn audit_detail(session: &WarehouseSession, id: &str) -> Result<Value, CoreError> {
    let length: Option<i64> = session
        .connection()
        .query_row(
            "SELECT length(CAST(change_summary_json AS BLOB)) FROM agent_audit_log WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    let length = length.ok_or(CoreError::NotFound)?;
    if length < 0 || length as usize > AUDIT_LIMIT {
        return Err(CoreError::AgentLimit);
    }
    let raw: String = session
        .connection()
        .query_row(
            "SELECT change_summary_json FROM agent_audit_log WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(db)?;
    serde_json::from_str(&raw).map_err(|_| CoreError::DatabaseInvalid)
}

#[cfg(test)]
mod tests;
