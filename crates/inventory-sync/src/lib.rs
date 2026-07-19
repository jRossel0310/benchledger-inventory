//! Snapshot export, GitHub publish/backup, restore. Implemented in Phases 6-7.

pub mod github;
pub mod snapshot;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Db(#[from] inventory_db::DbError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
