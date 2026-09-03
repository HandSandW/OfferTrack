//! OfferTrack full archive v1: magic, little-endian manifest length, UTF-8 JSON,
//! then uncompressed file bytes in manifest order. No links or executable extraction rules.
use crate::{
    database_backup,
    error::{CoreError, file_error},
    migrations, warehouse,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};
use uuid::Uuid;

pub(crate) const MAGIC: &[u8; 16] = b"OFFERTRACK-FULL1";
const MAX_MANIFEST: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Entry {
    pub path: String,
    pub directory: bool,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Manifest {
    pub version: u32,
    pub kind: String,
    pub warehouse_format: u32,
    pub warehouse_id: Uuid,
    pub schema_version: i64,
    pub created_at_utc: String,
    pub includes_recycle_bin: bool,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub version: u32,
    pub warehouse_id: Uuid,
    pub schema_version: i64,
    pub created_at_utc: String,
    pub includes_recycle_bin: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub sha256: String,
}

pub(crate) struct Verified {
    pub manifest: Manifest,
    pub preview: Preview,
    pub payload_offset: u64,
}

pub(crate) fn valid_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 4096 || path.split('/').count() > 128 {
        return false;
    }
    path.split('/').all(|part| {
        let stem = part.split('.').next().unwrap_or("").to_uppercase();
        let numbered_device = stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            });
        !part.is_empty()
            && !matches!(part, "." | "..")
            && !part.ends_with([' ', '.'])
            && !part
                .chars()
                .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
            && !matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            )
            && !numbered_device
    })
}

pub(crate) fn validate(manifest: &Manifest) -> Result<u64, CoreError> {
    if manifest.version != 1
        || manifest.kind != "full"
        || manifest.warehouse_format != warehouse::WAREHOUSE_FORMAT_VERSION
        || !(1..=migrations::CURRENT_SCHEMA_VERSION).contains(&manifest.schema_version)
        || chrono::DateTime::parse_from_rfc3339(&manifest.created_at_utc).is_err()
        || manifest.entries.len() > MAX_ENTRIES
    {
        return Err(CoreError::BackupInvalid);
    }
    let mut names = BTreeMap::new();
    let mut total = 0u64;
    for entry in &manifest.entries {
        if !valid_path(&entry.path) {
            return Err(CoreError::UnsafePath);
        }
        let key = entry.path.to_lowercase();
        let top = key.split('/').next().unwrap_or("");
        if matches!(
            top,
            "warehouse.json"
                | ".offertrack.lock"
                | "offertrack.sqlite-wal"
                | "offertrack.sqlite-shm"
                | "offertrack.sqlite-journal"
        ) || (top == "recycle-bin" && !manifest.includes_recycle_bin)
            || names.insert(key, entry.directory).is_some()
        {
            return Err(CoreError::BackupInvalid);
        }
        if entry.directory {
            if entry.size_bytes != 0 || !entry.sha256.is_empty() {
                return Err(CoreError::BackupInvalid);
            }
        } else {
            if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(CoreError::BackupInvalid);
            }
            total = total
                .checked_add(entry.size_bytes)
                .ok_or(CoreError::BackupInvalid)?;
        }
    }
    if names.get("offertrack.sqlite") != Some(&false) {
        return Err(CoreError::BackupInvalid);
    }
    for entry in &manifest.entries {
        // Every parent must be an explicit directory; case aliases cannot redirect extraction.
        if let Some((parent, _)) = entry.path.rsplit_once('/')
            && names.get(&parent.to_lowercase()) != Some(&true)
        {
            return Err(CoreError::BackupInvalid);
        }
    }
    Ok(total)
}

pub(crate) fn write_header(file: &mut File, manifest: &Manifest) -> Result<(), CoreError> {
    validate(manifest)?;
    let bytes = serde_json::to_vec(manifest).map_err(|_| CoreError::BackupInvalid)?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err(CoreError::BackupInvalid);
    }
    file.write_all(MAGIC)
        .and_then(|_| file.write_all(&(bytes.len() as u64).to_le_bytes()))
        .and_then(|_| file.write_all(&bytes))
        .map_err(file_error)
}

/// Copy exactly the advertised payload, streaming and comparing its digest.
pub(crate) fn transfer(
    input: &mut impl Read,
    output: &mut impl Write,
    entry: &Entry,
) -> Result<(), CoreError> {
    let mut digest = Sha256::new();
    let mut remaining = entry.size_bytes;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let limit = remaining.min(buffer.len() as u64) as usize;
        input
            .read_exact(&mut buffer[..limit])
            .map_err(|_| CoreError::BackupInvalid)?;
        digest.update(&buffer[..limit]);
        output.write_all(&buffer[..limit]).map_err(file_error)?;
        remaining -= limit as u64;
    }
    if format!("{:x}", digest.finalize()) != entry.sha256 {
        return Err(CoreError::BackupInvalid);
    }
    Ok(())
}

pub(crate) fn verify(file: &mut File) -> Result<Verified, CoreError> {
    file.rewind().map_err(file_error)?;
    let mut magic = [0u8; 16];
    let mut length = [0u8; 8];
    file.read_exact(&mut magic)
        .and_then(|_| file.read_exact(&mut length))
        .map_err(|_| CoreError::BackupInvalid)?;
    let length = u64::from_le_bytes(length);
    if &magic != MAGIC || length > MAX_MANIFEST {
        return Err(CoreError::BackupInvalid);
    }
    let mut bytes = vec![0u8; length as usize];
    file.read_exact(&mut bytes)
        .map_err(|_| CoreError::BackupInvalid)?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| CoreError::BackupInvalid)?;
    let total_bytes = validate(&manifest)?;
    let payload_offset = 24 + length;
    if payload_offset.checked_add(total_bytes) != Some(file.metadata().map_err(file_error)?.len()) {
        return Err(CoreError::BackupInvalid);
    }
    for entry in &manifest.entries {
        if !entry.directory {
            transfer(file, &mut std::io::sink(), entry)?;
        }
    }
    let (_, sha256) = database_backup::hash(file)?;
    let preview = Preview {
        version: manifest.version,
        warehouse_id: manifest.warehouse_id,
        schema_version: manifest.schema_version,
        created_at_utc: manifest.created_at_utc.clone(),
        includes_recycle_bin: manifest.includes_recycle_bin,
        file_count: manifest
            .entries
            .iter()
            .filter(|entry| !entry.directory)
            .count(),
        total_bytes,
        sha256,
    };
    file.seek(SeekFrom::Start(payload_offset))
        .map_err(file_error)?;
    Ok(Verified {
        manifest,
        preview,
        payload_offset,
    })
}
