mod applications;
mod auxiliary_states;
mod copying;
mod domain;
mod error;
mod file_health;
mod filesystem;
mod migrations;
mod platform;
mod preferences;
mod recycle_bin;
mod session_access;
mod storage;
mod views;
mod warehouse;
mod workflows;

#[cfg(test)]
mod metadata_tests;

#[cfg(test)]
mod mvp_acceptance_tests;

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
    preferences::remember_warehouse(config_dir, session.root().to_path_buf())?;
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

fn write_session<T>(
    state: &AppState,
    operation: impl FnOnce(&mut WarehouseSession) -> Result<T, CoreError>,
) -> Result<T, AppErrorPayload> {
    let mut active = session_access::try_lock(&state.warehouse)?;
    operation(active.as_mut().ok_or(CoreError::WarehouseNotOpen)?).map_err(Into::into)
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
    let help_usage = MenuItem::with_id(app, "help", "使用帮助", true, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", "关于 OfferTrack", true, None::<&str>)?;

    let file = Submenu::with_items(
        app,
        "文件",
        true,
        &[&new_warehouse, &open_warehouse, &close_warehouse, &quit],
    )?;
    let view = Submenu::with_items(app, "视图", true, &[&overview, &settings])?;
    let help = Submenu::with_items(app, "帮助", true, &[&help_usage, &about])?;
    Menu::with_items(app, &[&file, &view, &help])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
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
            } else {
                let _ = app.emit("menu-action", id);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_startup_state,
            create_warehouse,
            open_warehouse,
            close_warehouse,
            list_applications,
            get_application,
            create_application,
            update_application,
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
            reveal_document,
            get_document_path,
            open_web_url
        ])
        .run(tauri::generate_context!())
        .expect("OfferTrack failed to start");
}
