//! Only desktop commands can change this warehouse-scoped permission.
use crate::{applications::now_utc, error::CoreError, warehouse::WarehouseSession};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

const KEY: &str = "agent_access_v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub version: u32,
    pub enabled: bool,
    pub revision: i64,
}

pub fn get(connection: &Connection) -> Result<Permission, CoreError> {
    let raw: Option<String> = connection
        .query_row("SELECT value_json FROM settings WHERE key=?1", [KEY], |r| {
            r.get(0)
        })
        .optional()
        .map_err(super::db)?;
    let Some(raw) = raw else {
        return Ok(Permission {
            version: 1,
            enabled: false,
            revision: 0,
        });
    };
    let value: Permission = serde_json::from_str(&raw).map_err(|_| CoreError::DatabaseInvalid)?;
    if value.version != 1 || value.revision < 1 {
        return Err(CoreError::DatabaseInvalid);
    }
    Ok(value)
}

pub fn require(connection: &Connection) -> Result<Permission, CoreError> {
    let permission = get(connection)?;
    if !permission.enabled {
        return Err(CoreError::AgentWriteDisabled);
    }
    Ok(permission)
}

pub fn set(
    session: &mut WarehouseSession,
    enabled: bool,
    revision: i64,
) -> Result<Permission, CoreError> {
    let tx = session.connection_mut()?.transaction().map_err(super::db)?;
    let old = get(&tx)?;
    if revision != old.revision {
        return Err(CoreError::RevisionConflict);
    }
    if enabled == old.enabled {
        return Ok(old);
    }
    let value = Permission {
        version: 1,
        enabled,
        revision: revision.checked_add(1).ok_or(CoreError::Validation)?,
    };
    let now = now_utc();
    tx.execute("INSERT INTO settings (key,value_json,updated_at_utc) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at_utc=excluded.updated_at_utc",
        params![KEY, serde_json::to_string(&value).map_err(|_| CoreError::Validation)?, now]).map_err(super::db)?;
    tx.execute("INSERT INTO agent_audit_log (id,operation,entity_type,request_version,change_summary_json,occurred_at_utc,outcome) VALUES (?1,'permission','settings',1,?2,?3,'committed')",
        params![uuid::Uuid::new_v4().to_string(), serde_json::json!({"version":1,"actor":"desktop_user","before":old,"after":value}).to_string(), now]).map_err(super::db)?;
    tx.commit().map_err(super::db)?;
    Ok(value)
}
