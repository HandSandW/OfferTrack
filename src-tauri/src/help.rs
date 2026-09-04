use std::{path::Path, sync::Mutex};

use serde::Serialize;
use tauri::{Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::{
    error::{AppErrorPayload, CoreError, file_error},
    filesystem,
};

#[derive(Clone, Serialize)]
pub(crate) struct Location {
    topic: String,
    revision: u32,
}

pub(crate) struct HelpState(Mutex<Location>);
impl Default for HelpState {
    fn default() -> Self {
        Self(Mutex::new(Location {
            topic: "manual".into(),
            revision: 0,
        }))
    }
}
impl HelpState {
    pub(crate) fn location(&self) -> Result<Location, CoreError> {
        self.0
            .lock()
            .map(|value| value.clone())
            .map_err(|_| CoreError::StateUnavailable)
    }
    fn select(&self, topic: &str) -> Result<Location, CoreError> {
        if !valid_topic(topic) {
            return Err(CoreError::Validation);
        }
        let mut value = self.0.lock().map_err(|_| CoreError::StateUnavailable)?;
        value.revision = value
            .revision
            .checked_add(1)
            .ok_or(CoreError::StateUnavailable)?;
        value.topic = topic.into();
        Ok(value.clone())
    }
}

fn valid_topic(topic: &str) -> bool {
    matches!(
        topic,
        "manual"
            | "quick-start"
            | "shortcuts"
            | "data"
            | "faq"
            | "diagnostics"
            | "about"
            | "overview"
            | "applications"
            | "tasks"
            | "templates"
            | "archive"
            | "recycle"
            | "settings"
            | "files"
            | "agent"
            | "backup"
    )
}

// Custom Tauri commands are otherwise available to every local webview. Keep
// this central gate so future business commands cannot leak into the help view.
pub(crate) fn command_allowed(label: &str, command: &str) -> bool {
    label == "main"
        || (label == "help"
            && matches!(
                command,
                "get_help_location" | "get_help_diagnostics" | "open_help_logs"
            ))
        || (label == "application-detail"
            && matches!(
                command,
                "get_application_detail_target"
                    | "notify_application_detail_changed"
                    | "get_startup_state"
                    | "get_application"
                    | "list_field_definitions"
                    | "update_application"
                    | "change_application_stage"
                    | "save_workflow_stage"
                    | "delete_workflow_stage"
                    | "reorder_application_workflow"
                    | "update_application_states"
                    | "save_workflow_as_template"
                    | "save_interview_round"
                    | "delete_interview_round"
                    | "preview_application_duplicate"
                    | "duplicate_application"
                    | "set_application_archived"
                    | "move_application_to_trash"
                    | "scan_application_documents"
                    | "retry_folder_normalization"
                    | "rename_document"
                    | "trash_document"
                    | "list_application_directories"
                    | "inspect_application_files"
                    | "open_application_folder"
                    | "open_document"
                    | "available_browsers"
                    | "reveal_document"
                    | "get_document_path"
                    | "open_web_url"
            ))
}

pub(crate) fn show<R: Runtime>(
    app: &tauri::AppHandle<R>,
    topic: &str,
) -> Result<(), AppErrorPayload> {
    let location = app.state::<HelpState>().select(topic)?;
    if let Some(window) = app.get_webview_window("help") {
        window
            .emit_to("help", "help-location", &location)
            .map_err(|_| CoreError::StateUnavailable)?;
        window
            .unminimize()
            .map_err(|_| CoreError::StateUnavailable)?;
        window.show().map_err(|_| CoreError::StateUnavailable)?;
        window
            .set_focus()
            .map_err(|_| CoreError::StateUnavailable)?;
    } else {
        let dev_url = app.config().build.dev_url.clone();
        WebviewWindowBuilder::new(app, "help", WebviewUrl::App("help.html".into()))
            .title("OfferTrack 使用帮助")
            .inner_size(1040.0, 760.0)
            .min_inner_size(620.0, 480.0)
            .center()
            .menu(tauri::menu::Menu::new(app).map_err(|_| CoreError::StateUnavailable)?)
            .on_navigation(move |url| navigation_allowed(url, dev_url.as_ref(), cfg!(dev)))
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .build()
            .map_err(|_| CoreError::StateUnavailable)?;
    }
    Ok(())
}

fn navigation_allowed(url: &url::Url, dev: Option<&url::Url>, development: bool) -> bool {
    if url.path() != "/help.html"
        || url.query().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let bundled = url.port().is_none()
        && ((url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (matches!(url.scheme(), "http" | "https")
                && url.host_str() == Some("tauri.localhost")));
    bundled || (development && dev.is_some_and(|base| url.origin() == base.origin()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Diagnostics {
    version: u8,
    application: &'static str,
    application_version: &'static str,
    platform: &'static str,
    architecture: &'static str,
    build: &'static str,
    sqlite_version: &'static str,
    supported_schema: i64,
    warehouse_access: Access,
    persistent_application_log: bool,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Access {
    Closed,
    Write,
    ReadOnly,
    Busy,
}

// By construction accepts no free-form strings, paths, DB rows or errors.
pub(crate) fn diagnostics(access: Access) -> Diagnostics {
    Diagnostics {
        version: 1,
        application: "OfferTrack",
        application_version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        build: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        sqlite_version: rusqlite::version(),
        supported_schema: crate::migrations::CURRENT_SCHEMA_VERSION,
        warehouse_access: access,
        persistent_application_log: false,
    }
}

pub(crate) fn existing_log_directory(path: &Path) -> Result<bool, CoreError> {
    // Path is resolved by AppHandle, never supplied by the calling webview.
    for ancestor in path.ancestors() {
        filesystem::validate_no_reparse(ancestor, ancestor)?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(CoreError::FileTypeMismatch),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(file_error(error)),
    }
}

pub(crate) fn open_logs<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, CoreError> {
    let path = app.path().app_log_dir().map_err(|_| CoreError::Storage)?;
    if !existing_log_directory(&path)? {
        return Ok(false);
    }
    open::that_detached(path).map_err(|_| CoreError::FileOperation)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_cannot_invoke_business_or_future_commands() {
        for command in [
            "get_startup_state",
            "create_warehouse",
            "get_application",
            "set_agent_permission",
            "restore_full_backup",
            "empty_recycle_bin",
            "open_web_url",
            "open_help",
            "future_business_command",
        ] {
            assert!(command_allowed("main", command));
            assert!(!command_allowed("help", command));
            assert!(!command_allowed("unknown", command));
        }
        for command in [
            "get_help_location",
            "get_help_diagnostics",
            "open_help_logs",
        ] {
            assert!(command_allowed("help", command));
            assert!(!command_allowed("unknown", command));
        }
        for command in [
            "get_application_detail_target",
            "get_application",
            "update_application",
            "list_application_directories",
            "inspect_application_files",
            "open_document",
        ] {
            assert!(command_allowed("application-detail", command));
        }
        for command in [
            "create_warehouse",
            "empty_recycle_bin",
            "set_agent_permission",
            "restore_full_backup",
        ] {
            assert!(!command_allowed("application-detail", command));
        }
        let capabilities: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/help.json")).unwrap();
        assert_eq!(capabilities["windows"], serde_json::json!(["help"]));
        assert_eq!(
            capabilities["permissions"],
            serde_json::json!(["core:event:allow-listen", "core:event:allow-unlisten"])
        );
        let detail: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/detail.json")).unwrap();
        assert_eq!(detail["windows"], serde_json::json!(["application-detail"]));
    }

    #[test]
    fn topics_are_fixed_and_latest_request_is_retained() {
        let state = HelpState::default();
        assert_eq!(state.location().unwrap().topic, "manual");
        let first = state.select("backup").unwrap();
        let second = state.select("about").unwrap();
        assert!(second.revision > first.revision);
        for topic in [
            "../index.html",
            "https://example.invalid",
            "",
            "manual#script",
            "BACKUP",
        ] {
            assert!(state.select(topic).is_err());
        }
        assert_eq!(state.location().unwrap().topic, "about");
    }

    #[test]
    fn help_navigation_rejects_external_main_and_queries() {
        let dev = url::Url::parse("http://127.0.0.1:1420").unwrap();
        for value in [
            "http://tauri.localhost/help.html",
            "tauri://localhost/help.html#chapter",
            "https://tauri.localhost/help.html",
        ] {
            assert!(navigation_allowed(
                &url::Url::parse(value).unwrap(),
                None,
                false
            ));
        }
        let local = url::Url::parse("http://127.0.0.1:1420/help.html").unwrap();
        assert!(navigation_allowed(&local, Some(&dev), true));
        assert!(!navigation_allowed(&local, Some(&dev), false));
        for value in [
            "https://example.invalid/help.html",
            "http://tauri.localhost/index.html",
            "http://tauri.localhost/help.html?file=private",
            "http://tauri.localhost:123/help.html",
            "http://user@tauri.localhost/help.html",
            "file:///help.html",
            "http://127.0.0.1:1421/help.html",
        ] {
            assert!(!navigation_allowed(
                &url::Url::parse(value).unwrap(),
                Some(&dev),
                true
            ));
        }
    }

    #[test]
    fn diagnostic_schema_has_no_private_or_free_form_fields() {
        for access in [
            Access::Closed,
            Access::Write,
            Access::ReadOnly,
            Access::Busy,
        ] {
            let value = serde_json::to_value(diagnostics(access)).unwrap();
            let keys: std::collections::BTreeSet<_> = value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                [
                    "version",
                    "application",
                    "applicationVersion",
                    "platform",
                    "architecture",
                    "build",
                    "sqliteVersion",
                    "supportedSchema",
                    "warehouseAccess",
                    "persistentApplicationLog"
                ]
                .into_iter()
                .collect()
            );
            assert_eq!(value["persistentApplicationLog"], false);
            assert_eq!(
                value["supportedSchema"],
                crate::migrations::CURRENT_SCHEMA_VERSION
            );
        }
    }

    #[test]
    fn log_inspection_never_creates_or_deletes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("missing");
        assert!(!existing_log_directory(&path).unwrap());
        assert!(!path.exists());
        std::fs::write(&path, "sentinel").unwrap();
        assert!(existing_log_directory(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "sentinel");
        assert!(existing_log_directory(root.path()).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn log_directory_rejects_junction_in_its_ancestors() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("logs")).unwrap();
        std::fs::write(outside.path().join("logs/sentinel"), "unchanged").unwrap();
        let link = root.path().join("redirect");
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
            .env("OFFERTRACK_TEST_LINK", &link).env("OFFERTRACK_TEST_TARGET", outside.path()).output().unwrap();
        assert!(output.status.success());
        assert!(matches!(
            existing_log_directory(&link.join("logs")),
            Err(CoreError::UnsafePath)
        ));
        assert_eq!(
            std::fs::read_to_string(outside.path().join("logs/sentinel")).unwrap(),
            "unchanged"
        );
    }
}
