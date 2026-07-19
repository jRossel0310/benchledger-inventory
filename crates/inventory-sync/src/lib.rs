//! Snapshot export, GitHub publish/backup, restore. Implemented in Phases 6-7.

pub mod github;
pub mod publish;
pub mod snapshot;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Db(#[from] inventory_db::DbError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Publishing has not been configured (no `publish_owner`/`publish_repo`
    /// in `settings`). Not a retryable failure — nothing was attempted, so
    /// `publish::publish_snapshot` sets no pending marker for it.
    #[error("publishing is not configured")]
    NotConfigured,
    /// No GitHub token is stored in the OS credential store. Raised by the
    /// command layer (which constructs the live client from the token)
    /// rather than by `publish_snapshot` itself, which only ever sees an
    /// already-constructed `GitHubApi`.
    #[error("no GitHub token is stored")]
    TokenMissing,
    /// A GitHub API call failed. `GitHubError`'s Display strings are fixed
    /// classifications (never a response body or the token), so this stays
    /// safe to log or surface verbatim.
    #[error(transparent)]
    GitHub(#[from] github::GitHubError),
}
