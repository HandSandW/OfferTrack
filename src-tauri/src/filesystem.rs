use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, SecondsFormat, Utc};

use crate::error::{CoreError, file_error};

const MAX_NAME_PART_CHARS: usize = 40;

#[derive(Debug)]
pub struct ScannedFile {
    pub relative_path: String,
    pub display_name: String,
    pub media_type: Option<String>,
    pub size_bytes: i64,
    pub modified_at_utc: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedDirectory {
    pub relative_path: String,
    pub empty: bool,
}

pub fn normalized_application_folder_name(
    current_name: &str,
    company_name: &str,
    position_name: &str,
    short_id: &str,
) -> String {
    let prefix = current_name.split("__").next().filter(|value| {
        value.len() == 19
            && value
                .chars()
                .enumerate()
                .all(|(index, character)| match index {
                    4 | 7 => character == '-',
                    10 => character == '_',
                    13 | 16 => character == '-',
                    _ => character.is_ascii_digit(),
                })
    });
    format!(
        "{}__{}__{}__{}",
        prefix.unwrap_or("unknown-time"),
        sanitize_name_part(company_name),
        sanitize_name_part(position_name),
        short_id
    )
}

pub fn sanitize_name_part(value: &str) -> String {
    let mut cleaned: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .take(MAX_NAME_PART_CHARS)
        .collect();
    cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() {
        cleaned = "未命名".to_owned();
    }
    if is_windows_reserved_name(&cleaned) {
        cleaned.insert(0, '_');
    }
    cleaned
}

pub(crate) fn is_windows_reserved_name(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .trim_end()
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix)
            .is_some_and(|suffix| matches!(suffix, "¹" | "²" | "³"))
    }) || (stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.as_bytes()[3].is_ascii_digit()
        && stem.as_bytes()[3] != b'0')
}

pub fn application_folder(warehouse_root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    if Path::new(relative).components().count() != 2 {
        return Err(CoreError::UnsafePath);
    }
    safe_warehouse_path(warehouse_root, relative, "applications")
}

pub fn trash_folder(warehouse_root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    safe_warehouse_path(warehouse_root, relative, "recycle-bin")
}

fn safe_warehouse_path(
    warehouse_root: &Path,
    relative: &str,
    expected_top_level: &str,
) -> Result<PathBuf, CoreError> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || relative_path.components().next()
            != Some(Component::Normal(OsStr::new(expected_top_level)))
    {
        return Err(CoreError::UnsafePath);
    }
    let path = warehouse_root.join(relative_path);
    validate_no_reparse(warehouse_root, &path)?;
    Ok(path)
}

/// Inspect every existing ancestor before resolving a warehouse path. Missing
/// leaves are allowed for creation, but links (including dangling ones) are not.
pub fn validate_no_reparse(root: &Path, path: &Path) -> Result<(), CoreError> {
    let relative = path.strip_prefix(root).map_err(|_| CoreError::UnsafePath)?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CoreError::UnsafePath);
    }
    let mut current = root.to_path_buf();
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(component) = component {
            current.push(component);
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || is_reparse_point(&metadata) => {
                return Err(CoreError::UnsafePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(file_error(error)),
        }
    }
    Ok(())
}

pub fn scan_application_files(
    warehouse_root: &Path,
    folder_relative_path: &str,
) -> Result<Vec<ScannedFile>, CoreError> {
    let root = application_folder(warehouse_root, folder_relative_path)?;
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(file_error(error)),
        Ok(metadata) if !metadata.is_dir() => return Err(CoreError::FileTypeMismatch),
        Ok(_) => {}
    }
    let mut files = Vec::new();
    scan_directory(&root, &root, &mut files, &mut Vec::new())?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

pub fn scan_application_directories(
    warehouse_root: &Path,
    folder_relative_path: &str,
) -> Result<Vec<ScannedDirectory>, CoreError> {
    let root = application_folder(warehouse_root, folder_relative_path)?;
    let metadata = fs::symlink_metadata(&root).map_err(file_error)?;
    if !metadata.is_dir() {
        return Err(CoreError::FileTypeMismatch);
    }
    let mut directories = Vec::new();
    scan_directory(&root, &root, &mut Vec::new(), &mut directories)?;
    directories.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(directories)
}

fn scan_directory(
    root: &Path,
    directory: &Path,
    output: &mut Vec<ScannedFile>,
    directories: &mut Vec<ScannedDirectory>,
) -> Result<(), CoreError> {
    // Protect recursive traversal from pathological external directory trees.
    if directory
        .strip_prefix(root)
        .map_err(|_| CoreError::UnsafePath)?
        .components()
        .count()
        > 64
    {
        return Err(CoreError::FileOperation);
    }
    let mut empty = true;
    for entry in fs::read_dir(directory).map_err(file_error)? {
        empty = false;
        let entry = entry.map_err(file_error)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(file_error)?;
        if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return Err(CoreError::UnsafePath);
        }
        if metadata.is_dir() {
            scan_directory(root, &path, output, directories)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| CoreError::UnsafePath)?
                .to_string_lossy()
                .replace('\\', "/");
            output.push(ScannedFile {
                display_name: entry.file_name().to_string_lossy().into_owned(),
                media_type: media_type_for_path(&path),
                relative_path: relative,
                size_bytes: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
                modified_at_utc: metadata.modified().ok().map(system_time_to_utc),
            });
        }
    }
    if directory != root {
        if directories.len() >= 10000 {
            return Err(CoreError::FileOperation);
        }
        directories.push(ScannedDirectory {
            relative_path: directory
                .strip_prefix(root)
                .map_err(|_| CoreError::UnsafePath)?
                .to_string_lossy()
                .replace('\\', "/"),
            empty,
        });
    }
    Ok(())
}

fn system_time_to_utc(value: SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn media_type_for_path(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    Some(
        match extension.as_str() {
            "pdf" => "application/pdf",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "txt" | "md" => "text/plain",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            _ => "application/octet-stream",
        }
        .to_owned(),
    )
}

pub fn list_candidate_directories(
    warehouse_root: &Path,
    include_hidden: bool,
) -> Result<Vec<(String, bool)>, CoreError> {
    let applications = warehouse_root.join("applications");
    validate_no_reparse(warehouse_root, &applications)?;
    let mut result = Vec::new();
    for entry in fs::read_dir(applications).map_err(|_| CoreError::FileOperation)? {
        let entry = entry.map_err(|_| CoreError::FileOperation)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| CoreError::FileOperation)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let hidden = name.starts_with('.') || is_hidden(&metadata);
        if !hidden || include_hidden {
            result.push((name, hidden));
        }
    }
    result.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(result)
}

pub fn directory_size(path: &Path) -> Result<u64, CoreError> {
    let metadata = fs::symlink_metadata(path).map_err(file_error)?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut total = 0_u64;
    for entry in fs::read_dir(path).map_err(|_| CoreError::FileOperation)? {
        total = total.saturating_add(directory_size(
            &entry.map_err(|_| CoreError::FileOperation)?.path(),
        )?);
    }
    Ok(total)
}

#[cfg(windows)]
pub fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
pub fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn is_hidden(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x2 != 0
}

#[cfg(not(windows))]
fn is_hidden(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_windows_names_and_reserved_devices() {
        assert_eq!(sanitize_name_part("产品/研发:*?"), "产品_研发___");
        assert_eq!(sanitize_name_part("CON"), "_CON");
        assert_eq!(sanitize_name_part("name. "), "name");
    }

    #[test]
    fn generated_folder_name_keeps_stable_short_id() {
        let name = normalized_application_folder_name(
            "2026-09-03_12-34-56",
            "示例公司",
            "开发工程师",
            "A7K3M9",
        );
        assert!(name.ends_with("__示例公司__开发工程师__A7K3M9"));
        assert_eq!(name.split("__").next().expect("timestamp").len(), 19);
    }

    #[test]
    fn safe_path_rejects_parent_and_wrong_root() {
        let root = Path::new("warehouse");
        assert!(application_folder(root, "applications/record").is_ok());
        assert!(matches!(
            application_folder(root, "applications/../outside"),
            Err(CoreError::UnsafePath)
        ));
        assert!(matches!(
            application_folder(root, "recycle-bin/record"),
            Err(CoreError::UnsafePath)
        ));
    }
}
