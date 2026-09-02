mod error;
mod migrations;
mod preferences;
mod storage;
mod warehouse;

use std::{path::PathBuf, sync::Mutex};

use serde::Serialize;
use tauri::{
    Emitter, Manager, Runtime, State,
    menu::{Menu, MenuItem, Submenu},
};

use crate::{
    error::{AppErrorPayload, CoreError},
    warehouse::{WarehouseAccessMode, WarehouseSession, WarehouseSummary},
};

#[derive(Default)]
struct AppState {
    warehouse: Mutex<Option<WarehouseSession>>,
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
    let active_warehouse = state
        .warehouse
        .lock()
        .map_err(|_| CoreError::StateUnavailable)?
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
    install_session(&app, &state, warehouse::create(&PathBuf::from(path))?)
}

#[tauri::command]
fn open_warehouse(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    access_mode: WarehouseAccessMode,
) -> Result<WarehouseSummary, AppErrorPayload> {
    install_session(
        &app,
        &state,
        warehouse::open(&PathBuf::from(path), access_mode)?,
    )
}

fn install_session(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    session: WarehouseSession,
) -> Result<WarehouseSummary, AppErrorPayload> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|_| CoreError::Storage)?;
    preferences::remember_warehouse(config_dir, session.root().to_path_buf())?;
    let summary = session.summary();
    let mut active = state
        .warehouse
        .lock()
        .map_err(|_| CoreError::StateUnavailable)?;
    *active = Some(session);
    Ok(summary)
}

#[tauri::command]
fn close_warehouse(state: State<'_, AppState>) -> Result<(), AppErrorPayload> {
    let mut active = state
        .warehouse
        .lock()
        .map_err(|_| CoreError::StateUnavailable)?;
    *active = None;
    Ok(())
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
                app.exit(0);
            } else {
                let _ = app.emit("menu-action", id);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_startup_state,
            create_warehouse,
            open_warehouse,
            close_warehouse
        ])
        .run(tauri::generate_context!())
        .expect("OfferTrack failed to start");
}
