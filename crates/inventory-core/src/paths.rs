//! Application data directory resolution and layout. Pure functions: callers
//! pass in environment values so this stays unit-testable.

use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "ElectronicsInventory";
pub const ENV_OVERRIDE: &str = "ELECTRONICS_INVENTORY_DATA_DIR";

#[derive(Debug, thiserror::Error)]
pub enum PathsError {
    #[error("no data directory available: set {ENV_OVERRIDE} or ensure %APPDATA% exists")]
    NoDataDir,
}

/// Resolve the data directory: explicit override beats %APPDATA% default.
pub fn resolve_data_dir(
    env_override: Option<&str>,
    appdata: Option<&str>,
) -> Result<PathBuf, PathsError> {
    if let Some(over) = env_override.filter(|s| !s.trim().is_empty()) {
        return Ok(PathBuf::from(over));
    }
    if let Some(base) = appdata.filter(|s| !s.trim().is_empty()) {
        return Ok(Path::new(base).join(APP_DIR_NAME));
    }
    Err(PathsError::NoDataDir)
}

#[derive(Debug, Clone)]
pub struct DataLayout {
    pub root: PathBuf,
    pub attachments: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub pending_sync: PathBuf,
    pub local_backups: PathBuf,
}

/// Create the standard subdirectory layout beneath `root` (idempotent).
pub fn ensure_layout(root: &Path) -> std::io::Result<DataLayout> {
    let layout = DataLayout {
        root: root.to_path_buf(),
        attachments: root.join("attachments"),
        cache: root.join("cache"),
        logs: root.join("logs"),
        pending_sync: root.join("pending-sync"),
        local_backups: root.join("local-backups"),
    };
    for dir in [
        &layout.root,
        &layout.attachments,
        &layout.cache,
        &layout.logs,
        &layout.pending_sync,
        &layout.local_backups,
    ] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_wins() {
        let dir = resolve_data_dir(
            Some("C:\\custom\\data"),
            Some("C:\\Users\\x\\AppData\\Roaming"),
        )
        .unwrap();
        assert_eq!(dir, std::path::PathBuf::from("C:\\custom\\data"));
    }

    #[test]
    fn defaults_to_appdata_subfolder() {
        let dir = resolve_data_dir(None, Some("C:\\Users\\x\\AppData\\Roaming")).unwrap();
        assert_eq!(
            dir,
            std::path::PathBuf::from("C:\\Users\\x\\AppData\\Roaming\\ElectronicsInventory")
        );
    }

    #[test]
    fn missing_both_is_an_error() {
        assert!(resolve_data_dir(None, None).is_err());
    }

    #[test]
    fn ensure_layout_creates_all_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let layout = ensure_layout(dir.path()).unwrap();
        for p in [
            &layout.attachments,
            &layout.cache,
            &layout.logs,
            &layout.pending_sync,
            &layout.local_backups,
        ] {
            assert!(p.is_dir(), "{p:?} should exist");
        }
        assert_eq!(layout.root, dir.path());
    }
}
