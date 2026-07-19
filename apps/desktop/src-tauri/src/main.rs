#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod close_flow;
mod commands;

use app::{AppInit, AppState};
use tauri::{Emitter, Manager};

fn main() {
    let env_override = std::env::var("ELECTRONICS_INVENTORY_DATA_DIR").ok();
    let appdata = std::env::var("APPDATA").ok();
    let layout = app::prepare_layout(env_override.as_deref(), appdata.as_deref())
        .expect("failed to prepare application data directory");

    let _log_guard =
        inventory_core::logging::init(&layout.logs).expect("failed to initialize logging");
    tracing::info!("application starting");

    let init = match AppInit::open(layout) {
        Ok(init) => init,
        Err(e) => {
            tracing::error!("database startup failed: {e}");
            panic!("failed to open inventory database: {e}");
        }
    };

    match init.db.validate_invariants() {
        Ok(report) if report.is_clean() => {
            tracing::info!(parts = report.parts_checked, "inventory invariants clean");
        }
        Ok(report) => {
            for d in &report.discrepancies {
                tracing::error!(
                    part = d.part_id.as_str(),
                    field = %d.field,
                    stored = d.stored,
                    recomputed = d.recomputed,
                    "inventory invariant violation detected"
                );
            }
        }
        Err(e) => tracing::error!("invariant validation failed to run: {e}"),
    }

    let builder = commands::builder();

    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default(),
            "../src/bindings.gen.ts",
        )
        .expect("failed to export typescript bindings");

    tauri::Builder::default()
        .manage(AppState {
            layout: init.layout,
            db: std::sync::Mutex::new(init.db),
        })
        .invoke_handler(builder.invoke_handler())
        // Close-time publish flow (Phase 6 Task 6): intercept the window
        // close, hand control to the frontend dialog exactly once, and let
        // its `finalize_close` command (`AppHandle::exit(0)`, which never
        // re-raises CloseRequested) perform the actual exit. See
        // `close_flow` for the guard's contract.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if close_flow::begin_close_flow(&close_flow::CLOSE_FLOW_STARTED) {
                    if let Err(e) = window.emit("close-publish-requested", ()) {
                        // Fail-safe: a webview that can't receive the event
                        // can't run the dialog that would call
                        // `finalize_close` — never trap the user in an app
                        // that cannot close.
                        tracing::error!("failed to emit close-publish-requested: {e}");
                        window.app_handle().exit(0);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
