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

/// Resolve the data directory and create its layout. Safe to call before logging.
pub fn prepare_layout(
    env_override: Option<&str>,
    appdata: Option<&str>,
) -> Result<DataLayout, InitError> {
    let root = resolve_data_dir(env_override, appdata)?;
    Ok(ensure_layout(&root)?)
}

impl AppInit {
    /// Open + migrate the database for an already-prepared layout.
    pub fn open(layout: DataLayout) -> Result<Self, InitError> {
        let db = Database::open_and_migrate(
            &layout.root.join("inventory.sqlite"),
            &layout.local_backups,
        )?;
        Ok(AppInit { layout, db })
    }

    /// Convenience: prepare_layout + open in one call (tests, future recovery mode).
    /// Not called from `main` (which needs logging initialized between the two
    /// steps), so this binary crate sees it as unused outside `#[cfg(test)]`.
    #[allow(dead_code)]
    pub fn initialize(
        env_override: Option<&str>,
        appdata: Option<&str>,
    ) -> Result<Self, InitError> {
        Self::open(prepare_layout(env_override, appdata)?)
    }
}

/// Shared Tauri state.
pub struct AppState {
    pub layout: DataLayout,
    pub db: Mutex<Database>,
}

#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub app_version: String,
    pub schema_version: u32,
    pub data_dir: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_layout_succeeds_without_database() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let layout = prepare_layout(Some(root.to_str().unwrap()), None).unwrap();
        assert!(layout.logs.is_dir());
        assert!(!root.join("inventory.sqlite").exists());
    }

    #[test]
    fn initialize_creates_layout_and_database() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        assert!(root.join("inventory.sqlite").exists());
        assert!(root.join("logs").is_dir());
        assert_eq!(
            init.db.schema_version().unwrap(),
            inventory_db::SUPPORTED_SCHEMA_VERSION
        );
    }
}
