//! Derived checkpoint, not business truth. No caller-selected paths or file deletion.
use std::{collections::BTreeMap, io::Read};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{MAX_BYTES, collect, dto, encode, snapshot};
use crate::{
    applications::now_utc,
    database_backup as db,
    error::{AppErrorPayload, CoreError, file_error},
    full_backup,
    warehouse::WarehouseSession,
};

const KEY: &str = "agent_snapshot_v1";
const FIXED_RELATIVE_PATH: &str = "agent-access/snapshot";
const META_LIMIT: usize = 16 * 1024;
const FILES: [&str; 7] = [
    "applications.jsonl",
    "tasks.jsonl",
    "events.jsonl",
    "fields.json",
    "summary.json",
    "schema.json",
    "README.md",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    pub relative_path: String,
    pub generated_at_utc: String,
    pub application_count: usize,
    pub content_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: u32,
    warehouse_id: String,
    snapshot: Info,
    manifest_sha256: String,
}

#[derive(Serialize)]
pub struct Report {
    pub version: u32,
    pub warehouse_id: String,
    pub checked_at_utc: String,
    /// current means both content equality and file integrity at this check, not live sync.
    pub state: &'static str,
    pub snapshot: Option<Info>,
    pub published: bool,
    pub error: Option<AppErrorPayload>,
    pub warnings: Vec<String>,
}

fn invalid(_: impl std::fmt::Debug) -> CoreError {
    CoreError::MetadataInvalid
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn fingerprint(data: &dto::Dataset) -> Result<String, CoreError> {
    // The generation clock is intentionally excluded. Indexed file observations, revisions,
    // long texts and schema participate; UI preferences/permission/audit/backup clocks do not.
    Ok(digest(&encode(&serde_json::json!({
        "version":data.version, "warehouse_id":data.warehouse_id,
        "warehouse_format_version":data.warehouse_format_version,
        "applications":data.applications, "tasks":data.tasks, "events":data.events,
        "fields":data.fields, "summary":data.summary, "schema":dto::schema()
    }))?))
}

fn load(session: &WarehouseSession) -> Result<Option<Checkpoint>, CoreError> {
    let row: Option<(i64, Option<String>)> = session.connection().query_row(
        "SELECT length(CAST(value_json AS BLOB)), CASE WHEN length(CAST(value_json AS BLOB))<=?2 THEN value_json END FROM settings WHERE key=?1",
        params![KEY, META_LIMIT as i64], |r| Ok((r.get(0)?,r.get(1)?))
    ).optional().map_err(invalid)?;
    let Some((length, raw)) = row else {
        return Ok(None);
    };
    if length < 0 || length > META_LIMIT as i64 {
        return Err(CoreError::AgentLimit);
    }
    let checkpoint: Checkpoint =
        serde_json::from_str(&raw.ok_or(CoreError::MetadataInvalid)?).map_err(invalid)?;
    let legacy_path_valid = || {
        let Some(name) = checkpoint
            .snapshot
            .relative_path
            .strip_prefix("agent-access/")
        else {
            return false;
        };
        let Some(suffix) = name.strip_prefix("snapshot-") else {
            return false;
        };
        name.is_ascii()
            && name.len() <= 120
            && !name.contains(['/', '\\', ':'])
            && suffix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && suffix.len() >= 37
            && uuid::Uuid::parse_str(&suffix[suffix.len() - 36..]).is_ok()
    };
    if !matches!(checkpoint.version, 1 | 2) {
        return Err(CoreError::AgentVersion);
    }
    if !((checkpoint.version == 1 && legacy_path_valid())
        || (checkpoint.version == 2 && checkpoint.snapshot.relative_path == FIXED_RELATIVE_PATH))
        || chrono::DateTime::parse_from_rfc3339(&checkpoint.snapshot.generated_at_utc).is_err()
        || ![
            &checkpoint.manifest_sha256,
            &checkpoint.snapshot.content_sha256,
        ]
        .iter()
        .all(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(CoreError::MetadataInvalid);
    }
    Ok(Some(checkpoint))
}

pub(super) fn remember(
    session: &WarehouseSession,
    info: Info,
    manifest: &[u8],
) -> Result<(), CoreError> {
    if !session.is_writable() {
        return Err(CoreError::ReadOnlyWarehouse);
    }
    // Validate before updating. Unknown future/corrupt metadata must never be silently overwritten.
    load(session)?;
    let checkpoint = Checkpoint {
        version: 2,
        warehouse_id: session.summary().warehouse_id.to_string(),
        snapshot: info,
        manifest_sha256: digest(manifest),
    };
    let bytes = encode(&checkpoint)?;
    let tx = session
        .connection()
        .unchecked_transaction()
        .map_err(invalid)?;
    tx.execute("INSERT INTO settings (key,value_json,updated_at_utc) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at_utc=excluded.updated_at_utc",
        params![KEY, std::str::from_utf8(&bytes).map_err(invalid)?, now_utc()]).map_err(invalid)?;
    tx.commit().map_err(invalid)
}

pub(super) fn validate_checkpoint(session: &WarehouseSession) -> Result<(), CoreError> {
    load(session).map(|_| ())
}

#[derive(Deserialize)]
struct Entry {
    size_bytes: u64,
    sha256: String,
}
#[derive(Deserialize)]
struct Manifest {
    version: u32,
    warehouse_id: String,
    content_sha256: String,
    files: BTreeMap<String, Entry>,
}

fn verify(session: &WarehouseSession, checkpoint: &Checkpoint) -> Result<(), CoreError> {
    let (root, _ancestors) = full_backup::outside_parent(session.root(), None)?;
    let directory = root.join(&checkpoint.snapshot.relative_path);
    let _guards = db::guard_chain(&root, &directory)?;
    let mut manifest_file = db::open_guard(&directory.join("manifest.json"), false)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut manifest_file)
        .take(META_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(file_error)?;
    if bytes.len() > META_LIMIT || digest(&bytes) != checkpoint.manifest_sha256 {
        return Err(CoreError::MetadataInvalid);
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(invalid)?;
    if manifest.version != 1
        || manifest.warehouse_id != checkpoint.warehouse_id
        || manifest.content_sha256 != checkpoint.snapshot.content_sha256
        || manifest.files.len() != FILES.len()
        || FILES.iter().any(|name| !manifest.files.contains_key(*name))
    {
        return Err(CoreError::MetadataInvalid);
    }
    let mut held_files = Vec::new();
    for name in FILES {
        let entry = &manifest.files[name];
        if entry.size_bytes > MAX_BYTES as u64 {
            return Err(CoreError::AgentLimit);
        }
        let mut file = db::open_guard(&directory.join(name), false)?;
        if file.metadata().map_err(file_error)?.len() != entry.size_bytes {
            return Err(CoreError::MetadataInvalid);
        }
        let mut hash = Sha256::new();
        let mut remaining = entry.size_bytes;
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            let cap = remaining.min(buffer.len() as u64) as usize;
            let n = file.read(&mut buffer[..cap]).map_err(file_error)?;
            if n == 0 {
                return Err(CoreError::MetadataInvalid);
            }
            hash.update(&buffer[..n]);
            remaining -= n as u64;
        }
        if format!("{:x}", hash.finalize()) != entry.sha256 {
            return Err(CoreError::MetadataInvalid);
        }
        held_files.push(file);
    }
    Ok(())
}

/// Always returns a derived-status report. A failed refresh must not turn a committed write into failure.
pub fn check(session: &WarehouseSession, refresh: bool) -> Report {
    let mut report = Report {
        version: 1,
        warehouse_id: session.summary().warehouse_id.to_string(),
        checked_at_utc: now_utc(),
        state: "missing",
        snapshot: None,
        published: false,
        error: None,
        warnings: Vec::new(),
    };
    if let Err(error) = check_inner(session, refresh, &mut report) {
        report.state = "error";
        report.error = Some(error.into());
    }
    report.checked_at_utc = now_utc();
    report
}

fn check_inner(
    session: &WarehouseSession,
    refresh: bool,
    report: &mut Report,
) -> Result<(), CoreError> {
    let checkpoint = load(session)?;
    if let Some(checkpoint) = &checkpoint
        && checkpoint.warehouse_id == report.warehouse_id
    {
        report.snapshot = Some(checkpoint.snapshot.clone());
    }
    let data = collect(session)?;
    let hash = fingerprint(&data)?;
    if let Some(checkpoint) = checkpoint {
        let identity_matches = checkpoint.warehouse_id == report.warehouse_id;
        report.state = "stale";
        let verified = if identity_matches {
            verify(session, &checkpoint)
        } else {
            Err(CoreError::AgentWarehouseChanged)
        };
        let fixed_layout =
            checkpoint.version == 2 && checkpoint.snapshot.relative_path == FIXED_RELATIVE_PATH;
        if identity_matches
            && fixed_layout
            && hash == checkpoint.snapshot.content_sha256
            && verified.is_ok()
        {
            report.state = "current";
            if refresh
                && session.is_writable()
                && snapshot::retire_legacy_layout(session.root()).is_err()
            {
                report.warnings.push(
                    "固定快照有效，但旧预览版代际尚未全部移入固定回收区；请关闭占用后重试检查。"
                        .into(),
                );
            }
            return Ok(());
        }
        if let Err(error) = verified {
            // Unsafe paths/permissions are surfaced, never repaired through or deleted.
            report.error = Some(error.into());
        } else if !fixed_layout {
            report.warnings.push(
                "检测到旧预览版 Agent 快照布局；以写入方式检查后会迁移到固定 agent-access/snapshot。"
                    .into(),
            );
        }
    }
    if refresh && session.is_writable() {
        let created = snapshot::create_from_data(session, data)?;
        report.snapshot = Some(created.info);
        report.published = true;
        report.warnings = created.created.warnings;
        // Publication already succeeded even if recording its checkpoint failed.
        if let Some(error) = created.checkpoint_error {
            return Err(error);
        }
        report.state = "current";
        report.error = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
