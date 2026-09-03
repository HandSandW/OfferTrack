use std::{
    path::{Component, Path, PathBuf},
    process::Command,
};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    error::{CoreError, file_error},
    filesystem,
};

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileOpenMode {
    Default,
    ChooseOther,
    Edge,
    Chrome,
    Firefox,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserChoice {
    Default,
    Edge,
    Chrome,
    Firefox,
}

pub fn open_application_folder(
    connection: &Connection,
    warehouse_root: &Path,
    application_id: &str,
) -> Result<(), CoreError> {
    let folder = application_folder_for_id(connection, warehouse_root, application_id)?;
    let metadata = std::fs::symlink_metadata(&folder).map_err(file_error)?;
    if !metadata.is_dir() {
        return Err(CoreError::FileTypeMismatch);
    }
    open::that_detached(folder).map_err(|_| CoreError::FileOperation)
}

pub fn open_document(
    connection: &Connection,
    warehouse_root: &Path,
    application_id: &str,
    document_id: &str,
    mode: FileOpenMode,
) -> Result<(), CoreError> {
    let path = document_path(connection, warehouse_root, application_id, document_id)?;
    match mode {
        FileOpenMode::Default => open::that_detached(path).map_err(|_| CoreError::FileOperation),
        FileOpenMode::ChooseOther => choose_other_application(&path),
        browser => {
            let uri = pdf_uri(&path)?;
            let choice = match browser {
                FileOpenMode::Edge => BrowserChoice::Edge,
                FileOpenMode::Chrome => BrowserChoice::Chrome,
                FileOpenMode::Firefox => BrowserChoice::Firefox,
                _ => return Err(CoreError::Validation),
            };
            launch_browser(choice, &uri)
        }
    }
}

pub fn reveal_document(
    connection: &Connection,
    warehouse_root: &Path,
    application_id: &str,
    document_id: &str,
) -> Result<(), CoreError> {
    let path = document_path(connection, warehouse_root, application_id, document_id)?;
    reveal_in_folder(&path)
}

pub fn open_url(value: &str, browser: BrowserChoice) -> Result<(), CoreError> {
    let parsed = url::Url::parse(value).map_err(|_| CoreError::Validation)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CoreError::Validation);
    }
    match browser {
        BrowserChoice::Default => {
            open::that_detached(parsed.as_str()).map_err(|_| CoreError::FileOperation)
        }
        choice => launch_browser(choice, parsed.as_str()),
    }
}

pub fn available_browsers() -> Vec<BrowserChoice> {
    [
        BrowserChoice::Edge,
        BrowserChoice::Chrome,
        BrowserChoice::Firefox,
    ]
    .into_iter()
    .filter(|choice| find_browser(*choice).is_some())
    .collect()
}

fn launch_browser(choice: BrowserChoice, value: &str) -> Result<(), CoreError> {
    let executable = find_browser(choice).ok_or(CoreError::NotFound)?;
    Command::new(executable)
        .arg(value)
        .spawn()
        .map(|_| ())
        .map_err(|_| CoreError::FileOperation)
}

fn pdf_uri(path: &Path) -> Result<String, CoreError> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    {
        return Err(CoreError::Validation);
    }
    // A single encoded file URL cannot be interpreted as browser switches.
    url::Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| CoreError::UnsafePath)
}

fn application_folder_for_id(
    connection: &Connection,
    warehouse_root: &Path,
    application_id: &str,
) -> Result<PathBuf, CoreError> {
    let relative = connection
        .query_row(
            "SELECT folder_relative_path FROM applications WHERE id = ?1",
            [application_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    filesystem::application_folder(warehouse_root, &relative)
}

pub fn document_path(
    connection: &Connection,
    warehouse_root: &Path,
    application_id: &str,
    document_id: &str,
) -> Result<PathBuf, CoreError> {
    let folder = application_folder_for_id(connection, warehouse_root, application_id)?;
    let relative = connection
        .query_row(
            "SELECT relative_path FROM documents
             WHERE id = ?1 AND application_id = ?2",
            params![document_id, application_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    let relative_path = Path::new(&relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CoreError::UnsafePath);
    }
    let path = folder.join(relative_path);
    filesystem::validate_no_reparse(warehouse_root, &path)?;
    let canonical_folder = fs_canonicalize(&folder)?;
    let canonical_path = fs_canonicalize(&path)?;
    if !canonical_path.starts_with(&canonical_folder) {
        return Err(CoreError::UnsafePath);
    }
    let metadata = std::fs::symlink_metadata(&canonical_path).map_err(file_error)?;
    if metadata.file_type().is_symlink() || filesystem::is_reparse_point(&metadata) {
        return Err(CoreError::UnsafePath);
    }
    if !metadata.is_file() {
        return Err(CoreError::FileTypeMismatch);
    }
    Ok(canonical_path)
}

fn fs_canonicalize(path: &Path) -> Result<PathBuf, CoreError> {
    std::fs::canonicalize(path).map_err(file_error)
}

#[cfg(windows)]
fn choose_other_application(path: &Path) -> Result<(), CoreError> {
    Command::new("rundll32.exe")
        .arg("shell32.dll,OpenAs_RunDLL")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| CoreError::FileOperation)
}

#[cfg(not(windows))]
fn choose_other_application(_path: &Path) -> Result<(), CoreError> {
    Err(CoreError::FileOperation)
}

#[cfg(windows)]
fn reveal_in_folder(path: &Path) -> Result<(), CoreError> {
    Command::new("explorer.exe")
        .arg("/select,")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| CoreError::FileOperation)
}

#[cfg(not(windows))]
fn reveal_in_folder(path: &Path) -> Result<(), CoreError> {
    let parent = path.parent().ok_or(CoreError::UnsafePath)?;
    open::that_detached(parent).map_err(|_| CoreError::FileOperation)
}

fn find_browser(choice: BrowserChoice) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
        let program_files_x86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let candidates: Vec<PathBuf> = match choice {
            BrowserChoice::Edge => [program_files_x86, program_files]
                .into_iter()
                .flatten()
                .map(|root| root.join("Microsoft/Edge/Application/msedge.exe"))
                .collect(),
            BrowserChoice::Chrome => [program_files, program_files_x86, local_app_data]
                .into_iter()
                .flatten()
                .map(|root| root.join("Google/Chrome/Application/chrome.exe"))
                .collect(),
            BrowserChoice::Firefox => [program_files, program_files_x86]
                .into_iter()
                .flatten()
                .map(|root| root.join("Mozilla Firefox/firefox.exe"))
                .collect(),
            BrowserChoice::Default => Vec::new(),
        };
        candidates.into_iter().find(|path| path.is_file())
    }
    #[cfg(not(windows))]
    {
        let _ = choice;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_pdf_argument_is_encoded_and_other_types_are_rejected() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("简历 #1 & --flag.PDF");
        let uri = pdf_uri(&path).unwrap();
        let parsed = url::Url::parse(&uri).unwrap();
        assert_eq!(parsed.scheme(), "file");
        assert_eq!(parsed.to_file_path().unwrap(), path);
        assert!(!uri.contains('#'));
        assert!(matches!(
            pdf_uri(&fixture.path().join("resume.docx")),
            Err(CoreError::Validation)
        ));
        for invalid in [
            "javascript:alert(1)",
            "file:///resume.pdf",
            "data:text/html,test",
            "--new-window",
        ] {
            assert!(matches!(
                open_url(invalid, BrowserChoice::Default),
                Err(CoreError::Validation)
            ));
        }
    }
}
