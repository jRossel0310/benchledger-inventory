//! Application startup: resolve data dir, ensure layout, init logging, open DB.

use std::sync::Mutex;

use inventory_core::paths::{ensure_layout, resolve_data_dir, DataLayout, PathsError};
use inventory_db::{Database, DbError};

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error(transparent)]
    Paths(#[from] PathsError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct AppInit {
    pub layout: DataLayout,
    pub db: Database,
}

impl AppInit {
    /// Resolve directories and open the database. Pure inputs for testability;
    /// `main` passes real env values.
    pub fn initialize(env_override: Option<&str>, appdata: Option<&str>) -> Result<Self, InitError> {
        let root = resolve_data_dir(env_override, appdata)?;
        let layout = ensure_layout(&root)?;
        let db = Database::open_and_migrate(&layout.root.join("inventory.sqlite"), &layout.local_backups)?;
        Ok(AppInit { layout, db })
    }
}

/// Shared Tauri state.
pub struct AppState {
    pub layout: DataLayout,
    pub db: Mutex<Database>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub app_version: String,
    pub schema_version: u32,
    pub data_dir: String,
}

pub fn status_of(state: &AppState, app_version: &str) -> Result<AppStatus, DbError> {
    let db = state.db.lock().expect("db mutex poisoned");
    Ok(AppStatus {
        app_version: app_version.to_string(),
        schema_version: db.schema_version()?,
        data_dir: state.layout.root.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_creates_layout_and_database() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        assert!(root.join("inventory.sqlite").exists());
        assert!(root.join("logs").is_dir());
        assert_eq!(init.db.schema_version().unwrap(), inventory_db::SUPPORTED_SCHEMA_VERSION);
    }

    #[test]
    fn status_reports_version_and_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState { layout: init.layout, db: Mutex::new(init.db) };
        let status = status_of(&state, "0.1.0").unwrap();
        assert_eq!(status.app_version, "0.1.0");
        assert_eq!(status.schema_version, 1);
        assert!(status.data_dir.ends_with("data"));
    }
}
