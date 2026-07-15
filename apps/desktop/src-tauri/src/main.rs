#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use app::{status_of, AppInit, AppState, AppStatus};

#[tauri::command]
fn app_status(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<AppStatus, String> {
    let version = app.package_info().version.to_string();
    status_of(&state, &version).map_err(|e| e.to_string())
}

fn main() {
    let env_override = std::env::var("ELECTRONICS_INVENTORY_DATA_DIR").ok();
    let appdata = std::env::var("APPDATA").ok();
    let init = AppInit::initialize(env_override.as_deref(), appdata.as_deref())
        .expect("failed to initialize application data directory and database");

    let _log_guard =
        inventory_core::logging::init(&init.layout.logs).expect("failed to initialize logging");
    tracing::info!("application starting");

    tauri::Builder::default()
        .manage(AppState {
            layout: init.layout,
            db: std::sync::Mutex::new(init.db),
        })
        .invoke_handler(tauri::generate_handler![app_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
