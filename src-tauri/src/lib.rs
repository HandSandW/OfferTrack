mod agent_access;
pub mod agent_cli;
mod agent_mcp;
mod agent_write;
mod applications;
mod auxiliary_states;
mod backup_archive;
mod batch;
mod copying;
mod database_backup;
mod document_files;
mod document_trash;
mod domain;
mod error;
mod export;
mod file_health;
mod filesystem;
mod full_backup;
mod help;
mod migrations;
mod overview;
mod platform;
mod preferences;
mod recruitment;
mod recycle_bin;
mod schedule;
mod session_access;
mod storage;
mod tasks;
mod views;
mod warehouse;
mod workflows;

#[cfg(test)]
mod metadata_tests;

#[cfg(test)]
mod mvp_acceptance_tests;

#[cfg(test)]
mod performance_tests;

use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{
    Emitter, Manager, Runtime, State,
    menu::{Menu, MenuItem, Submenu},
};

use crate::{
    domain::{
        ApplicationDetail, ApplicationListItem, ApplicationScope, ChangeStageRequest,
        ClaimFolderRequest, CreateApplicationRequest, DuplicateMode, DuplicatePreview,
        EmptyTrashChallenge, EmptyTrashResult, FieldDefinition, FieldDefinitionRequest,
        InterviewRoundRequest, ReorderWorkflowRequest, SavedView, SavedViewChange,
        SavedViewRequest, TrashEntry, UnlinkedFolder, UpdateApplicationRequest,
        UpdateAuxiliaryStatesRequest, UpdateWorkflowTemplateRequest, ViewMetadataRequest,
        WorkflowStageRequest, WorkflowTemplate, WorkflowTemplateDetail,
    },
    error::{AppErrorPayload, CoreError},
    platform::{BrowserChoice, FileOpenMode},
    warehouse::{WarehouseAccessMode, WarehouseSession, WarehouseSummary},
};

#[derive(Default)]
struct AppState {
    warehouse: Mutex<Option<WarehouseSession>>,
    trash_confirmation: Mutex<Option<TrashConfirmation>>,
    backup_trash_confirmation: Mutex<Option<recycle_bin::backups::Confirmation>>,
    document_trash_confirmation: Mutex<Option<document_trash::cleanup::Confirmation>>,
    file_watcher: Mutex<Option<RecommendedWatcher>>,
}

struct TrashConfirmation {
    warehouse_id: String,
    token: String,
    expires_at: Instant,
    item_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupState {
    remembered_warehouse_path: Option<String>,
    active_warehouse: Option<WarehouseSummary>,
}

#[tauri::command]
fn get_startup_state(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<StartupState, AppErrorPayload> {
    let active_warehouse = session_access::try_lock(&state.warehouse)?
        .as_ref()
        .map(WarehouseSession::summary);
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|_| CoreError::Storage)?;
    let remembered_warehouse_path = preferences::load_remembered_warehouse(config_dir)
        .map(|path| path.to_string_lossy().into_owned());

    Ok(StartupState {
        remembered_warehouse_path,
        active_warehouse,
    })
}

#[tauri::command]
fn create_warehouse(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<WarehouseSummary, AppErrorPayload> {
    let mut active = session_access::try_lock(&state.warehouse)?;
    install_session(
        &app,
        &state,
        &mut active,
        warehouse::create(&PathBuf::from(path))?,
    )
}

#[tauri::command]
fn open_warehouse(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    access_mode: WarehouseAccessMode,
) -> Result<WarehouseSummary, AppErrorPayload> {
    let mut active = session_access::try_lock(&state.warehouse)?;
    install_session(
        &app,
        &state,
        &mut active,
        warehouse::open(&PathBuf::from(path), access_mode)?,
    )
}

fn install_session(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    active: &mut Option<WarehouseSession>,
    mut session: WarehouseSession,
) -> Result<WarehouseSummary, AppErrorPayload> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|_| CoreError::Storage)?;
    applications::scan_all_documents(&mut session)?;
    let watch_root = session.root().join("applications");
    let watcher_app = app.clone();
    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| !event.kind.is_access()) {
                let _ = watcher_app.emit("filesystem-changed", ());
            }
        },
        Config::default(),
    )
    .map_err(|_| CoreError::FileOperation)?;
    watcher
        .watch(&watch_root, RecursiveMode::Recursive)
        .map_err(|_| CoreError::FileOperation)?;
    let summary = session.summary();
    // Do not remember a failed restore/switch target before validation and watcher setup succeed.
    preferences::remember_warehouse(config_dir, session.root().to_path_buf())?;
    *active = Some(session);
    *state
        .trash_confirmation
        .lock()
        .map_err(|_| CoreError::StateUnavailable)? = None;
    *state
        .file_watcher
        .lock()
        .map_err(|_| CoreError::StateUnavailable)? = Some(watcher);
    Ok(summary)
}

#[tauri::command]
fn close_warehouse(state: State<'_, AppState>) -> Result<(), AppErrorPayload> {
    let mut active = session_access::try_lock(&state.warehouse)?;
    *active = None;
    *state
        .file_watcher
        .lock()
        .map_err(|_| CoreError::StateUnavailable)? = None;
    *state
        .trash_confirmation
        .lock()
        .map_err(|_| CoreError::StateUnavailable)? = None;
    Ok(())
}

fn read_session<T>(
    state: &AppState,
    operation: impl FnOnce(&WarehouseSession) -> Result<T, CoreError>,
) -> Result<T, AppErrorPayload> {
    let active = session_access::try_lock(&state.warehouse)?;
    operation(active.as_ref().ok_or(CoreError::WarehouseNotOpen)?).map_err(Into::into)
}

#[tauri::command]
fn get_export_catalog(state: State<'_, AppState>) -> Result<export::Catalog, AppErrorPayload> {
    read_session(&state, export::catalog)
}

#[tauri::command]
fn get_agent_connection(
    state: State<'_, AppState>,
) -> Result<agent_mcp::config::Connection, AppErrorPayload> {
    read_session(&state, |s| {
        let executable = std::env::current_exe().map_err(|_| CoreError::FileMissing)?;
        agent_mcp::config::connection(&executable, s.root())
    })
}

#[tauri::command]
fn get_agent_permission(
    state: State<'_, AppState>,
) -> Result<agent_write::settings::Permission, AppErrorPayload> {
    read_session(&state, |s| agent_write::settings::get(s.connection()))
}

#[tauri::command]
fn set_agent_permission(
    state: State<'_, AppState>,
    enabled: bool,
    revision: i64,
) -> Result<agent_write::settings::Permission, AppErrorPayload> {
    write_session(&state, |s| agent_write::settings::set(s, enabled, revision))
}

#[tauri::command]
fn list_agent_audit(
    state: State<'_, AppState>,
) -> Result<Vec<agent_write::AuditItem>, AppErrorPayload> {
    read_session(&state, agent_write::audit_list)
}

#[tauri::command]
fn get_agent_audit(
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, AppErrorPayload> {
    read_session(&state, |s| agent_write::audit_detail(s, &id))
}

#[tauri::command]
async fn check_agent_snapshot(
    app: tauri::AppHandle,
    warehouse_id: String,
    warehouse_path: String,
) -> Result<agent_access::freshness::Report, AppErrorPayload> {
    tauri::async_runtime::spawn_blocking(move || {
        read_session(&app.state::<AppState>(), |s| {
            // These strings are identity assertions, NEVER filesystem operation targets.
            if s.summary().warehouse_id.to_string() != warehouse_id
                || s.summary().display_path != warehouse_path
            {
                return Err(CoreError::AgentWarehouseChanged);
            }
            Ok(agent_access::freshness::check(s, s.is_writable()))
        })
    })
    .await
    .map_err(|_| AppErrorPayload::from(CoreError::StateUnavailable))?
}

#[tauri::command]
async fn create_agent_snapshot(
    app: tauri::AppHandle,
) -> Result<agent_access::Created, AppErrorPayload> {
    let expected = read_session(&app.state::<AppState>(), |s| {
        Ok((s.summary().warehouse_id, s.root().to_path_buf()))
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        // A derived generation only records its checkpoint; no daily backup or business mutation.
        read_session(&app.state::<AppState>(), |s| {
            if s.summary().warehouse_id != expected.0 || s.root() != expected.1 {
                return Err(CoreError::RevisionConflict);
            }
            agent_access::create(s)
        })
    })
    .await
    .map_err(|_| AppErrorPayload::from(CoreError::StateUnavailable))?
}

#[tauri::command]
async fn export_applications(
    app: tauri::AppHandle,
    parent_directory: String,
    request: export::Request,
) -> Result<export::Created, AppErrorPayload> {
    let expected = read_session(&app.state::<AppState>(), |s| {
        Ok((s.summary().warehouse_id, s.root().to_path_buf()))
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        // Deliberately use a read session: exporting must not trigger daily backups or writes.
        read_session(&app.state::<AppState>(), |s| {
            if s.summary().warehouse_id != expected.0 || s.root() != expected.1 {
                return Err(CoreError::RevisionConflict);
            }
            export::create(s, &PathBuf::from(parent_directory), &request)
        })
    })
    .await
    .map_err(|_| AppErrorPayload::from(CoreError::StateUnavailable))?
}

fn write_session<T>(
    state: &AppState,
    operation: impl FnOnce(&mut WarehouseSession) -> Result<T, CoreError>,
) -> Result<T, AppErrorPayload> {
    let mut active = session_access::try_lock(&state.warehouse)?;
    let session = active.as_mut().ok_or(CoreError::WarehouseNotOpen)?;
    document_files::recover(session)?;
    document_trash::recover(session)?;
    database_backup::ensure_daily(session)?;
    operation(session).map_err(Into::into)
}

#[tauri::command]
fn list_database_backups(
    state: State<'_, AppState>,
) -> Result<database_backup::BackupCatalog, AppErrorPayload> {
    read_session(&state, database_backup::catalog)
}

#[tauri::command]
fn get_overview(state: State<'_, AppState>) -> Result<overview::Overview, AppErrorPayload> {
    read_session(&state, |s| {
        overview::get(s, chrono::Local::now().fixed_offset())
    })
}

#[tauri::command]
fn list_tasks(state: State<'_, AppState>) -> Result<Vec<tasks::Task>, AppErrorPayload> {
    read_session(&state, |s| tasks::list(s.connection()))
}

#[tauri::command]
fn list_recruitment_events(
    state: State<'_, AppState>,
) -> Result<Vec<recruitment::Event>, AppErrorPayload> {
    read_session(&state, |s| recruitment::list(s.connection()))
}

#[tauri::command]
fn save_recruitment_event(
    state: State<'_, AppState>,
    request: recruitment::SaveEvent,
) -> Result<recruitment::Event, AppErrorPayload> {
    write_session(&state, |s| recruitment::save(s, &request))
}

#[tauri::command]
fn complete_recruitment_event(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
    completed: bool,
) -> Result<recruitment::Event, AppErrorPayload> {
    write_session(&state, |s| {
        recruitment::complete(s, &id, revision, completed)
    })
}

#[tauri::command]
fn save_task(
    state: State<'_, AppState>,
    request: tasks::SaveTask,
) -> Result<tasks::Task, AppErrorPayload> {
    write_session(&state, |s| tasks::save(s, &request))
}

#[tauri::command]
fn complete_task(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
    completed: bool,
) -> Result<tasks::Task, AppErrorPayload> {
    write_session(&state, |s| tasks::complete(s, &id, revision, completed))
}

#[tauri::command]
fn list_reminder_rules(
    state: State<'_, AppState>,
) -> Result<Vec<tasks::ReminderRule>, AppErrorPayload> {
    read_session(&state, |s| tasks::rules(s.connection()))
}

#[tauri::command]
fn save_reminder_rules(
    state: State<'_, AppState>,
    rules: Vec<tasks::ReminderRule>,
) -> Result<Vec<tasks::ReminderRule>, AppErrorPayload> {
    write_session(&state, |s| tasks::save_rules(s, &rules))
}

#[tauri::command]
fn respond_to_reminder(
    state: State<'_, AppState>,
    key: String,
    fingerprint: String,
    snooze: bool,
) -> Result<(), AppErrorPayload> {
    write_session(&state, |s| overview::respond(s, &key, &fingerprint, snooze))
}

#[tauri::command]
async fn create_full_backup(
    app: tauri::AppHandle,
    parent_directory: String,
    include_recycle_bin: bool,
) -> Result<full_backup::Created, AppErrorPayload> {
    background_session_operation(app, move |session| {
        full_backup::create(
            session,
            &PathBuf::from(parent_directory),
            include_recycle_bin,
        )
    })
    .await
}

#[tauri::command]
async fn preview_full_backup(
    app: tauri::AppHandle,
    archive_path: String,
) -> Result<backup_archive::Preview, AppErrorPayload> {
    background_optional_session_operation(app, move |_| {
        full_backup::preview(&PathBuf::from(archive_path))
    })
    .await
}

#[tauri::command]
async fn restore_full_backup(
    app: tauri::AppHandle,
    archive_path: String,
    parent_directory: String,
    expected_sha256: String,
) -> Result<full_backup::Restored, AppErrorPayload> {
    background_optional_session_operation(app, move |session| {
        full_backup::restore(
            &PathBuf::from(archive_path),
            &PathBuf::from(parent_directory),
            &expected_sha256,
            session.map(|s| s.root()),
        )
    })
    .await
}

#[tauri::command]
async fn migrate_warehouse(
    app: tauri::AppHandle,
    parent_directory: String,
) -> Result<full_backup::Restored, AppErrorPayload> {
    background_session_operation(app, move |session| {
        full_backup::migrate(session, &PathBuf::from(parent_directory))
    })
    .await
}

async fn background_optional_session_operation<T: Send + 'static>(
    app: tauri::AppHandle,
    operation: impl FnOnce(Option<&WarehouseSession>) -> Result<T, CoreError> + Send + 'static,
) -> Result<T, AppErrorPayload> {
    let identity =
        |session: &WarehouseSession| (session.summary().warehouse_id, session.root().to_path_buf());
    let expected = session_access::try_lock(&app.state::<AppState>().warehouse)?
        .as_ref()
        .map(identity);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let active = session_access::try_lock(&state.warehouse)?;
        if active.as_ref().map(identity) != expected {
            return Err(CoreError::RevisionConflict.into());
        }
        operation(active.as_ref()).map_err(AppErrorPayload::from)
    })
    .await
    .map_err(|_| AppErrorPayload::from(CoreError::StateUnavailable))?
}

#[tauri::command]
async fn preview_application_batch(
    app: tauri::AppHandle,
    request: batch::Request,
) -> Result<batch::Preview, AppErrorPayload> {
    background_session_operation(app, move |session| batch::preview(session, &request)).await
}

#[tauri::command]
async fn preview_external_database_backup(
    app: tauri::AppHandle,
    directory: String,
) -> Result<database_backup::ExternalPreview, AppErrorPayload> {
    background_optional_session_operation(app, move |_| {
        database_backup::preview_external(&PathBuf::from(directory))
    })
    .await
}

#[tauri::command]
async fn restore_external_database_backup(
    app: tauri::AppHandle,
    directory: String,
    parent_directory: String,
    expected_fingerprint: String,
) -> Result<database_backup::DatabaseRestore, AppErrorPayload> {
    background_optional_session_operation(app, move |session| {
        database_backup::restore_external(
            &PathBuf::from(directory),
            &PathBuf::from(parent_directory),
            &expected_fingerprint,
            session.map(|s| s.root()),
        )
    })
    .await
}

#[tauri::command]
fn prepare_backup_recycle_bin(
    state: State<'_, AppState>,
) -> Result<recycle_bin::backups::Challenge, AppErrorPayload> {
    let (confirmation, challenge) = read_session(&state, recycle_bin::backups::prepare)?;
    *session_access::try_lock(&state.backup_trash_confirmation)? = Some(confirmation);
    Ok(challenge)
}

#[tauri::command]
async fn empty_backup_recycle_bin(
    app: tauri::AppHandle,
    confirmation_token: String,
) -> Result<recycle_bin::backups::Purged, AppErrorPayload> {
    let confirmation =
        session_access::try_lock(&app.state::<AppState>().backup_trash_confirmation)?
            .take()
            .ok_or(CoreError::InvalidConfirmation)?;
    background_session_operation(app, move |session| {
        recycle_bin::backups::purge(session, confirmation, &confirmation_token)
    })
    .await
}

#[tauri::command]
async fn apply_application_batch(
    app: tauri::AppHandle,
    request: batch::Request,
    expected_fingerprint: String,
) -> Result<batch::Applied, AppErrorPayload> {
    background_session_operation(app, move |session| {
        batch::apply(session, &request, &expected_fingerprint)
    })
    .await
}

#[tauri::command]
async fn create_database_backup(
    app: tauri::AppHandle,
) -> Result<database_backup::BackupCreated, AppErrorPayload> {
    background_session_operation(app, |session| database_backup::create(session)).await
}

#[tauri::command]
async fn preview_database_backup(
    app: tauri::AppHandle,
    backup_id: String,
    recycled: bool,
) -> Result<database_backup::BackupPreview, AppErrorPayload> {
    background_session_operation(app, move |session| {
        database_backup::preview(session, &backup_id, recycled)
    })
    .await
}

#[tauri::command]
async fn restore_database_backup(
    app: tauri::AppHandle,
    backup_id: String,
    recycled: bool,
    expected_sha256: String,
    parent_directory: String,
) -> Result<database_backup::DatabaseRestore, AppErrorPayload> {
    background_session_operation(app, move |session| {
        database_backup::restore(
            session,
            &backup_id,
            recycled,
            &expected_sha256,
            &PathBuf::from(parent_directory),
        )
    })
    .await
}

#[tauri::command]
fn list_applications(
    state: State<'_, AppState>,
    scope: ApplicationScope,
) -> Result<Vec<ApplicationListItem>, AppErrorPayload> {
    read_session(&state, |session| applications::list(session, scope))
}

#[tauri::command]
fn get_application(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApplicationDetail, AppErrorPayload> {
    read_session(&state, |session| applications::get(session, &id))
}

#[tauri::command]
fn create_application(
    state: State<'_, AppState>,
    request: CreateApplicationRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| applications::create(session, request))
}

#[tauri::command]
fn update_application(
    state: State<'_, AppState>,
    request: UpdateApplicationRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| applications::update(session, request))
}

#[tauri::command]
fn edit_application_cell(
    state: State<'_, AppState>,
    request: applications::cell_edit::Request,
) -> Result<applications::cell_edit::Applied, AppErrorPayload> {
    write_session(&state, |session| {
        applications::cell_edit::apply(session, request)
    })
}

#[tauri::command]
fn change_application_stage(
    state: State<'_, AppState>,
    request: ChangeStageRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        applications::change_stage(session, request)
    })
}

#[tauri::command]
fn save_workflow_stage(
    state: State<'_, AppState>,
    request: WorkflowStageRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        applications::save_workflow_stage(session, request)
    })
}

#[tauri::command]
fn delete_workflow_stage(
    state: State<'_, AppState>,
    application_id: String,
    stage_id: String,
    revision: i64,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        applications::delete_workflow_stage(session, &application_id, &stage_id, revision)
    })
}

#[tauri::command]
fn get_workflow_template(
    state: State<'_, AppState>,
    id: String,
) -> Result<WorkflowTemplateDetail, AppErrorPayload> {
    read_session(&state, |session| workflows::get_template(session, &id))
}

#[tauri::command]
fn update_workflow_template(
    state: State<'_, AppState>,
    request: UpdateWorkflowTemplateRequest,
) -> Result<WorkflowTemplateDetail, AppErrorPayload> {
    write_session(&state, |session| {
        workflows::update_template(session, request)
    })
}

#[tauri::command]
fn duplicate_workflow_template(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
    name: String,
) -> Result<WorkflowTemplateDetail, AppErrorPayload> {
    write_session(&state, |session| {
        workflows::duplicate_template(session, &id, revision, &name)
    })
}

#[tauri::command]
fn set_default_workflow_template(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
) -> Result<WorkflowTemplateDetail, AppErrorPayload> {
    write_session(&state, |session| {
        workflows::set_default(session, &id, revision)
    })
}

#[tauri::command]
fn reorder_application_workflow(
    state: State<'_, AppState>,
    request: ReorderWorkflowRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        workflows::reorder_record(session, request)
    })
}

#[tauri::command]
fn update_application_states(
    state: State<'_, AppState>,
    request: UpdateAuxiliaryStatesRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        auxiliary_states::update_record(session, request)
    })
}

#[tauri::command]
fn update_template_states(
    state: State<'_, AppState>,
    request: UpdateAuxiliaryStatesRequest,
) -> Result<WorkflowTemplateDetail, AppErrorPayload> {
    write_session(&state, |session| {
        auxiliary_states::update_template(session, request)
    })
}

#[tauri::command]
fn list_workflow_templates(
    state: State<'_, AppState>,
) -> Result<Vec<WorkflowTemplate>, AppErrorPayload> {
    read_session(&state, applications::list_workflow_templates)
}

#[tauri::command]
fn save_workflow_as_template(
    state: State<'_, AppState>,
    application_id: String,
    name: String,
    set_default: bool,
) -> Result<Vec<WorkflowTemplate>, AppErrorPayload> {
    write_session(&state, |session| {
        applications::save_workflow_as_template(session, &application_id, &name, set_default)
    })
}

#[tauri::command]
fn save_interview_round(
    state: State<'_, AppState>,
    request: InterviewRoundRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        applications::save_interview_round(session, request)
    })
}

#[tauri::command]
fn delete_interview_round(
    state: State<'_, AppState>,
    application_id: String,
    round_id: String,
    revision: i64,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        applications::delete_interview_round(session, &application_id, &round_id, revision)
    })
}

#[tauri::command]
fn list_field_definitions(
    state: State<'_, AppState>,
) -> Result<Vec<FieldDefinition>, AppErrorPayload> {
    read_session(&state, applications::list_field_definitions)
}

#[tauri::command]
fn save_field_definition(
    state: State<'_, AppState>,
    request: FieldDefinitionRequest,
) -> Result<Vec<FieldDefinition>, AppErrorPayload> {
    write_session(&state, |session| {
        applications::save_field_definition(session, request)
    })
}

#[tauri::command]
fn list_application_views(state: State<'_, AppState>) -> Result<Vec<SavedView>, AppErrorPayload> {
    read_session(&state, applications::list_views)
}

#[tauri::command]
fn save_application_view(
    state: State<'_, AppState>,
    request: SavedViewRequest,
) -> Result<SavedViewChange, AppErrorPayload> {
    write_session(&state, |session| applications::save_view(session, request))
}

#[tauri::command]
fn delete_application_view(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
) -> Result<Vec<SavedView>, AppErrorPayload> {
    write_session(&state, |session| views::delete(session, &id, revision))
}

#[tauri::command]
fn update_view_metadata(
    state: State<'_, AppState>,
    request: ViewMetadataRequest,
) -> Result<SavedViewChange, AppErrorPayload> {
    write_session(&state, |session| views::metadata(session, request))
}

#[tauri::command]
fn duplicate_application_view(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
    name: String,
) -> Result<SavedViewChange, AppErrorPayload> {
    write_session(&state, |session| {
        views::duplicate(session, &id, revision, &name)
    })
}

#[tauri::command]
fn get_application_page_size(state: State<'_, AppState>) -> Result<i64, AppErrorPayload> {
    read_session(&state, applications::page_size)
}

#[tauri::command]
fn set_application_page_size(
    state: State<'_, AppState>,
    value: i64,
) -> Result<i64, AppErrorPayload> {
    write_session(&state, |session| {
        applications::set_page_size(session, value)
    })
}

#[tauri::command]
fn set_application_archived(
    state: State<'_, AppState>,
    application_id: String,
    archived: bool,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        applications::set_archived(session, &application_id, archived)
    })
}

#[tauri::command]
fn scan_application_documents(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<Vec<crate::domain::DocumentEntry>, AppErrorPayload> {
    write_session(&state, |session| {
        applications::scan_documents(session, &application_id)
    })
}

#[tauri::command]
fn rename_document(
    state: State<'_, AppState>,
    request: document_files::RenameDocumentRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| document_files::rename(session, request))
}

#[tauri::command]
fn trash_document(
    state: State<'_, AppState>,
    request: document_trash::TrashRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| document_trash::trash(session, request))
}

#[tauri::command]
fn list_document_trash(
    state: State<'_, AppState>,
) -> Result<Vec<document_trash::Entry>, AppErrorPayload> {
    read_session(&state, document_trash::list)
}

#[tauri::command]
fn restore_document(
    state: State<'_, AppState>,
    id: String,
) -> Result<document_trash::Restored, AppErrorPayload> {
    write_session(&state, |session| document_trash::restore(session, &id))
}

#[tauri::command]
fn prepare_document_trash_cleanup(
    state: State<'_, AppState>,
) -> Result<document_trash::cleanup::Challenge, AppErrorPayload> {
    let (confirmation, challenge) = read_session(&state, document_trash::cleanup::prepare)?;
    *session_access::try_lock(&state.document_trash_confirmation)? = Some(confirmation);
    Ok(challenge)
}

#[tauri::command]
fn empty_document_trash(
    state: State<'_, AppState>,
    confirmation_token: String,
) -> Result<document_trash::cleanup::Purged, AppErrorPayload> {
    let confirmation = session_access::try_lock(&state.document_trash_confirmation)?
        .take()
        .ok_or(CoreError::InvalidConfirmation)?;
    write_session(&state, |session| {
        document_trash::cleanup::purge(session, confirmation, &confirmation_token)
    })
}

#[tauri::command]
fn list_application_directories(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<document_files::ApplicationDirectories, AppErrorPayload> {
    read_session(&state, |session| {
        document_files::list_directories(session, &application_id)
    })
}

#[tauri::command]
fn refresh_file_index(state: State<'_, AppState>) -> Result<(), AppErrorPayload> {
    write_session(&state, applications::scan_all_documents)
}

#[tauri::command]
fn list_unlinked_folders(
    state: State<'_, AppState>,
    include_hidden: bool,
) -> Result<Vec<UnlinkedFolder>, AppErrorPayload> {
    read_session(&state, |session| {
        applications::list_unlinked_folders(session, include_hidden)
    })
}

#[tauri::command]
fn claim_application_folder(
    state: State<'_, AppState>,
    request: ClaimFolderRequest,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        applications::claim_folder(session, request)
    })
}

#[tauri::command]
fn retry_folder_normalization(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<ApplicationDetail, AppErrorPayload> {
    write_session(&state, |session| {
        applications::retry_folder_normalization(session, &application_id)
    })
}

#[tauri::command]
async fn preview_application_duplicate(
    app: tauri::AppHandle,
    application_id: String,
    mode: DuplicateMode,
) -> Result<DuplicatePreview, AppErrorPayload> {
    background_session_operation(app, move |session| {
        applications::duplicate_preview(session, &application_id, mode)
    })
    .await
}

#[tauri::command]
async fn duplicate_application(
    app: tauri::AppHandle,
    application_id: String,
    mode: DuplicateMode,
) -> Result<ApplicationDetail, AppErrorPayload> {
    background_session_operation(app, move |session| {
        applications::duplicate(session, &application_id, mode)
    })
    .await
}

// Commands remain thin: expensive copying and hashing run off the event thread.
// Capture warehouse identity before scheduling; never operate on a newly switched warehouse.
async fn background_session_operation<T: Send + 'static>(
    app: tauri::AppHandle,
    operation: impl FnOnce(&mut WarehouseSession) -> Result<T, CoreError> + Send + 'static,
) -> Result<T, AppErrorPayload> {
    let expected = read_session(&app.state::<AppState>(), |session| {
        Ok((session.summary().warehouse_id, session.root().to_path_buf()))
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        write_session(&app.state::<AppState>(), |session| {
            if session.summary().warehouse_id != expected.0 || session.root() != expected.1 {
                return Err(CoreError::RevisionConflict);
            }
            operation(session)
        })
    })
    .await
    .map_err(|_| AppErrorPayload::from(CoreError::StateUnavailable))?
}

#[tauri::command]
fn inspect_application_files(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<file_health::PathObservation, AppErrorPayload> {
    read_session(&state, |session| {
        file_health::inspect_application(session, &application_id)
    })
}

#[tauri::command]
fn get_recovery_diagnostics(
    state: State<'_, AppState>,
) -> Result<file_health::RecoveryDiagnostics, AppErrorPayload> {
    read_session(&state, file_health::recovery_diagnostics)
}

#[tauri::command]
fn move_application_to_trash(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<(), AppErrorPayload> {
    write_session(&state, |session| {
        recycle_bin::move_application_to_trash(session, &application_id)
    })
}

#[tauri::command]
fn list_trash(state: State<'_, AppState>) -> Result<Vec<TrashEntry>, AppErrorPayload> {
    read_session(&state, recycle_bin::list)
}

#[tauri::command]
fn restore_application(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<recycle_bin::RestoreResult, AppErrorPayload> {
    write_session(&state, |session| {
        recycle_bin::restore_application(session, &application_id)
    })
}

#[tauri::command]
fn prepare_empty_recycle_bin(
    state: State<'_, AppState>,
) -> Result<EmptyTrashChallenge, AppErrorPayload> {
    let (warehouse_id, item_count, item_ids) = read_session(&state, |session| {
        if !session.is_writable() {
            return Err(CoreError::ReadOnlyWarehouse);
        }
        Ok((
            session.summary().warehouse_id.to_string(),
            recycle_bin::active_item_count(session)?,
            recycle_bin::active_item_ids(session)?,
        ))
    })?;
    let confirmation_token = uuid::Uuid::new_v4().to_string();
    *state
        .trash_confirmation
        .lock()
        .map_err(|_| CoreError::StateUnavailable)? = Some(TrashConfirmation {
        warehouse_id: warehouse_id.clone(),
        token: confirmation_token.clone(),
        expires_at: Instant::now() + Duration::from_secs(60),
        item_ids,
    });
    Ok(EmptyTrashChallenge {
        warehouse_id,
        confirmation_token,
        item_count,
    })
}

#[tauri::command]
fn empty_recycle_bin(
    state: State<'_, AppState>,
    warehouse_id: String,
    confirmation_token: String,
) -> Result<EmptyTrashResult, AppErrorPayload> {
    let challenge = state
        .trash_confirmation
        .lock()
        .map_err(|_| CoreError::StateUnavailable)?
        .take()
        .ok_or(CoreError::InvalidConfirmation)?;
    if challenge.warehouse_id != warehouse_id
        || challenge.token != confirmation_token
        || challenge.expires_at < Instant::now()
    {
        return Err(CoreError::InvalidConfirmation.into());
    }
    write_session(&state, |session| {
        if session.summary().warehouse_id.to_string() != warehouse_id {
            return Err(CoreError::InvalidConfirmation);
        }
        if recycle_bin::active_item_ids(session)? != challenge.item_ids {
            return Err(CoreError::InvalidConfirmation);
        }
        recycle_bin::empty(session)
    })
}

#[tauri::command]
fn open_application_folder(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<(), AppErrorPayload> {
    read_session(&state, |session| {
        platform::open_application_folder(session.connection(), session.root(), &application_id)
    })
}

#[tauri::command]
fn open_document(
    state: State<'_, AppState>,
    application_id: String,
    document_id: String,
    mode: FileOpenMode,
) -> Result<(), AppErrorPayload> {
    read_session(&state, |session| {
        platform::open_document(
            session.connection(),
            session.root(),
            &application_id,
            &document_id,
            mode,
        )
    })
}

#[tauri::command]
fn reveal_document(
    state: State<'_, AppState>,
    application_id: String,
    document_id: String,
) -> Result<(), AppErrorPayload> {
    read_session(&state, |session| {
        platform::reveal_document(
            session.connection(),
            session.root(),
            &application_id,
            &document_id,
        )
    })
}

#[tauri::command]
fn get_document_path(
    state: State<'_, AppState>,
    application_id: String,
    document_id: String,
) -> Result<String, AppErrorPayload> {
    read_session(&state, |session| {
        platform::document_path(
            session.connection(),
            session.root(),
            &application_id,
            &document_id,
        )
        .map(|path| path.to_string_lossy().into_owned())
    })
}

#[tauri::command]
fn open_web_url(url: String, browser: BrowserChoice) -> Result<(), AppErrorPayload> {
    platform::open_url(&url, browser).map_err(Into::into)
}

#[tauri::command]
fn available_browsers() -> Vec<BrowserChoice> {
    platform::available_browsers()
}

#[tauri::command]
async fn open_help(app: tauri::AppHandle, topic: String) -> Result<(), AppErrorPayload> {
    help::show(&app, &topic)
}

#[tauri::command]
fn get_help_location(state: State<'_, help::HelpState>) -> Result<help::Location, AppErrorPayload> {
    state.location().map_err(Into::into)
}

#[tauri::command]
fn get_help_diagnostics(state: State<'_, AppState>) -> help::Diagnostics {
    let access = match state.warehouse.try_lock() {
        Ok(guard) => match guard.as_ref().map(|session| session.summary().access_mode) {
            None => help::Access::Closed,
            Some(WarehouseAccessMode::Write) => help::Access::Write,
            Some(WarehouseAccessMode::ReadOnly) => help::Access::ReadOnly,
        },
        Err(_) => help::Access::Busy,
    };
    help::diagnostics(access)
}

#[tauri::command]
fn open_help_logs(app: tauri::AppHandle) -> Result<bool, AppErrorPayload> {
    help::open_logs(&app).map_err(Into::into)
}

fn build_menu<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<Menu<R>> {
    let new_warehouse =
        MenuItem::with_id(app, "new-warehouse", "新建数据仓库…", true, None::<&str>)?;
    let open_warehouse =
        MenuItem::with_id(app, "open-warehouse", "打开数据仓库…", true, None::<&str>)?;
    let close_warehouse =
        MenuItem::with_id(app, "close-warehouse", "关闭数据仓库", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 OfferTrack", true, None::<&str>)?;
    let overview = MenuItem::with_id(app, "overview", "概览", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let help_items = [
        ("manual", "完整使用手册"),
        ("quick-start", "快速开始"),
        ("shortcuts", "快捷键"),
        ("data", "数据与文件说明"),
        ("faq", "常见问题"),
        ("logs", "打开日志目录"),
        ("diagnostics", "复制诊断信息…"),
        ("about", "关于 OfferTrack"),
    ]
    .into_iter()
    .map(|(topic, label)| {
        MenuItem::with_id(app, format!("help:{topic}"), label, true, None::<&str>)
    })
    .collect::<tauri::Result<Vec<_>>>()?;

    let file = Submenu::with_items(
        app,
        "文件",
        true,
        &[&new_warehouse, &open_warehouse, &close_warehouse, &quit],
    )?;
    let view = Submenu::with_items(app, "视图", true, &[&overview, &settings])?;
    let help = Submenu::new(app, "帮助", true)?;
    for item in &help_items {
        help.append(item)?;
    }
    Menu::with_items(app, &[&file, &view, &help])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(help::HelpState::default())
        .setup(|app| {
            app.set_menu(build_menu(app.handle())?)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if id == "quit" {
                // Use the same close-request path as the title-bar button so
                // unsaved drafts and in-flight operations can prevent closing.
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.close();
                }
            } else if let Some(topic) = id.strip_prefix("help:") {
                let app = app.clone();
                let topic = topic.to_owned();
                // WebView2 window creation must not run synchronously inside
                // a native event handler (runtime-documented deadlock).
                tauri::async_runtime::spawn_blocking(move || {
                    if topic == "logs" {
                        match help::open_logs(&app) {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(_) => {
                                let _ = app.emit_to("main", "help-logs-failed", ());
                            }
                        }
                    }
                    let topic = if topic == "logs" {
                        "diagnostics"
                    } else {
                        &topic
                    };
                    if help::show(&app, topic).is_err() {
                        let _ = app.emit_to("main", "help-open-failed", ());
                    }
                });
            } else {
                let _ = app.emit_to("main", "menu-action", id);
            }
        })
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                // Only after the main draft guard allowed actual destruction.
                // An auxiliary window must not keep the process/write lock alive.
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(|invoke| {
            if !help::command_allowed(
                invoke.message.webview_ref().label(),
                invoke.message.command(),
            ) {
                invoke.resolver.reject(AppErrorPayload {
                    code: "WINDOW_ACCESS_DENIED",
                    message: "此窗口无权调用该应用接口。",
                    retryable: false,
                });
                return true;
            }
            let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
                open_help,
                get_help_location,
                get_help_diagnostics,
                open_help_logs,
                get_startup_state,
                create_warehouse,
                open_warehouse,
                close_warehouse,
                list_applications,
                get_application,
                create_application,
                update_application,
                edit_application_cell,
                change_application_stage,
                save_workflow_stage,
                delete_workflow_stage,
                list_workflow_templates,
                get_workflow_template,
                update_workflow_template,
                duplicate_workflow_template,
                set_default_workflow_template,
                reorder_application_workflow,
                update_application_states,
                update_template_states,
                save_workflow_as_template,
                save_interview_round,
                delete_interview_round,
                list_field_definitions,
                save_field_definition,
                list_application_views,
                save_application_view,
                delete_application_view,
                update_view_metadata,
                duplicate_application_view,
                get_application_page_size,
                set_application_page_size,
                set_application_archived,
                scan_application_documents,
                rename_document,
                trash_document,
                list_document_trash,
                restore_document,
                prepare_document_trash_cleanup,
                empty_document_trash,
                list_application_directories,
                refresh_file_index,
                list_unlinked_folders,
                claim_application_folder,
                retry_folder_normalization,
                preview_application_duplicate,
                duplicate_application,
                inspect_application_files,
                get_recovery_diagnostics,
                move_application_to_trash,
                list_trash,
                restore_application,
                prepare_empty_recycle_bin,
                empty_recycle_bin,
                open_application_folder,
                open_document,
                available_browsers,
                list_database_backups,
                create_full_backup,
                get_export_catalog,
                export_applications,
                create_agent_snapshot,
                check_agent_snapshot,
                get_agent_connection,
                get_agent_permission,
                set_agent_permission,
                list_agent_audit,
                get_agent_audit,
                preview_full_backup,
                restore_full_backup,
                migrate_warehouse,
                create_database_backup,
                preview_application_batch,
                preview_external_database_backup,
                get_overview,
                list_recruitment_events,
                save_recruitment_event,
                complete_recruitment_event,
                list_tasks,
                save_task,
                complete_task,
                list_reminder_rules,
                save_reminder_rules,
                respond_to_reminder,
                restore_external_database_backup,
                prepare_backup_recycle_bin,
                empty_backup_recycle_bin,
                apply_application_batch,
                preview_database_backup,
                restore_database_backup,
                reveal_document,
                get_document_path,
                open_web_url
            ];
            handler(invoke)
        })
        .run(tauri::generate_context!())
        .expect("OfferTrack failed to start");
}
