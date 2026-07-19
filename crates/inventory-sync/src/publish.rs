//! Publish orchestration (Phase 6 Task 4): ties the deterministic snapshot
//! builder (`snapshot.rs`) to the GitHub Contents client (`github.rs`)
//! through the digest/pending state in `inventory_db`'s `app_state` table.
//!
//! ## Flow (`publish_snapshot_at`)
//!
//! 1. Load [`PublishConfig`] from the `settings` table — `None` (owner or
//!    repo missing) is [`SyncError::NotConfigured`], with NO pending marker
//!    set: nothing was attempted, so there is nothing to retry.
//! 2. `build_snapshot` → `content_digest`. If the digest equals
//!    `app_state.last_published_digest`, return
//!    [`PublishOutcome::Unchanged`] with ZERO GitHub calls — this is what
//!    keeps the committed file's bytes (and the Vercel deploy history) from
//!    churning when the inventory hasn't changed. A stale `pending_publish`
//!    marker is cleared on this path too: the digest matching means the
//!    remote already holds exactly this content (the marker can only be
//!    stale here — a failed FIRST publish can't reach this branch, since
//!    `last_published_digest` is only ever written after a successful put).
//! 3. Otherwise render the publish form with the caller's `published_at`,
//!    `get_file` the configured path to learn the current blob sha (absent
//!    file → first publish, no sha), then `put_file` threading that sha —
//!    always get-before-put, because the Contents API (and the test mock)
//!    rejects an update of an existing file without its current sha.
//! 4. On success: record `last_published_digest` + `last_published_at`,
//!    clear `pending_publish`, return [`PublishOutcome::Published`]. On ANY
//!    failure after the Unchanged check: set `pending_publish` and return
//!    the typed error — the close flow / startup retry (Task 6) drives
//!    re-attempts off that marker.
//!
//! ## Clock seam
//!
//! [`publish_snapshot`] is the production entry point (reads the real UTC
//! clock); [`publish_snapshot_at`] takes `published_at` as a parameter so
//! every test controls the timestamp and can prove digest stability across
//! differing publish times. The formatter is a dependency-free
//! days-from-civil inversion (Howard Hinnant's `civil_from_days`) — the
//! workspace deliberately has no chrono/time dependency.

use inventory_db::Database;

use crate::github::{GitHubApi, RepoRef};
use crate::snapshot::{build_snapshot, content_digest, to_canonical_json, Snapshot};
use crate::SyncError;

/// `settings` keys holding the publish configuration (spec §13: the repo
/// coordinates are not secret — only the token is, and it lives in the OS
/// credential store via `inventory_core::secrets`).
pub const OWNER_SETTING: &str = "publish_owner";
pub const REPO_SETTING: &str = "publish_repo";
pub const BRANCH_SETTING: &str = "publish_branch";
pub const PATH_SETTING: &str = "publish_path";
pub const VERCEL_URL_SETTING: &str = "publish_vercel_url";

/// Defaults applied when the branch/path settings are absent or empty.
pub const DEFAULT_BRANCH: &str = "main";
pub const DEFAULT_PATH: &str = "apps/web/public/inventory.snapshot.json";

/// `app_state` keys (migration 0010) tracking publish state.
pub const LAST_PUBLISHED_DIGEST_KEY: &str = "last_published_digest";
pub const LAST_PUBLISHED_AT_KEY: &str = "last_published_at";
/// Present (value `"1"`) iff the last publish attempt failed after the
/// Unchanged check; absence means nothing is pending.
pub const PENDING_PUBLISH_KEY: &str = "pending_publish";

/// The fixed commit message every snapshot publish uses.
pub const PUBLISH_COMMIT_MESSAGE: &str = "Publish inventory snapshot";

/// Where (and whether) to publish: owner/repo/branch/path plus the
/// display-only Vercel URL. Loaded from the `settings` table; `None` from
/// [`PublishConfig::load`] means publishing has not been configured yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishConfig {
    pub owner: String,
    pub repo: String,
    pub branch: String,
    pub path: String,
    /// Display-only (the Settings/Dashboard UI links to it); never used to
    /// talk to any API.
    pub vercel_url: Option<String>,
}

impl PublishConfig {
    /// Read the publish configuration from the `settings` table. Returns
    /// `Ok(None)` when owner or repo is missing/blank (publishing not
    /// configured); branch and path fall back to [`DEFAULT_BRANCH`] /
    /// [`DEFAULT_PATH`] when absent or blank, and a blank Vercel URL reads
    /// as `None` (the set-config command stores `""` to mean "cleared").
    pub fn load(db: &Database) -> Result<Option<PublishConfig>, SyncError> {
        let owner = non_blank(db.get_setting(OWNER_SETTING)?);
        let repo = non_blank(db.get_setting(REPO_SETTING)?);
        let (Some(owner), Some(repo)) = (owner, repo) else {
            return Ok(None);
        };
        let branch =
            non_blank(db.get_setting(BRANCH_SETTING)?).unwrap_or_else(|| DEFAULT_BRANCH.into());
        let path = non_blank(db.get_setting(PATH_SETTING)?).unwrap_or_else(|| DEFAULT_PATH.into());
        let vercel_url = non_blank(db.get_setting(VERCEL_URL_SETTING)?);
        Ok(Some(PublishConfig {
            owner,
            repo,
            branch,
            path,
            vercel_url,
        }))
    }

    /// The [`RepoRef`] the GitHub client addresses files by.
    pub fn repo_ref(&self) -> RepoRef {
        RepoRef {
            owner: self.owner.clone(),
            repo: self.repo.clone(),
            branch: self.branch.clone(),
        }
    }
}

fn non_blank(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// What a publish attempt did. `Unchanged` means the content digest matched
/// `last_published_digest`, so no GitHub call was made and the remote file
/// is already byte-current (modulo its embedded `published_at`, which is
/// deliberately not part of the digest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    Published { digest: String },
    Unchanged,
}

/// Publish the current snapshot with a real UTC wall-clock `published_at`.
/// Production entry point; everything testable lives in
/// [`publish_snapshot_at`] (see the module doc's clock-seam section).
pub fn publish_snapshot(
    db: &mut Database,
    api: &dyn GitHubApi,
) -> Result<PublishOutcome, SyncError> {
    publish_snapshot_at(db, api, &utc_now_rfc3339())
}

/// Publish the current snapshot stamping `published_at` into the uploaded
/// form. See the module doc for the full flow and failure semantics.
pub fn publish_snapshot_at(
    db: &mut Database,
    api: &dyn GitHubApi,
    published_at: &str,
) -> Result<PublishOutcome, SyncError> {
    let Some(config) = PublishConfig::load(db)? else {
        return Err(SyncError::NotConfigured);
    };

    let snapshot = build_snapshot(db)?;
    let digest = content_digest(&snapshot);
    if db.get_app_state(LAST_PUBLISHED_DIGEST_KEY)?.as_deref() == Some(digest.as_str()) {
        // The remote already holds this exact content; a leftover pending
        // marker (failed publish whose changes were since reverted) is
        // stale — clear it so the UI stops offering a retry of a no-op.
        db.clear_app_state(PENDING_PUBLISH_KEY)?;
        return Ok(PublishOutcome::Unchanged);
    }

    match upload(api, &config, &snapshot, published_at) {
        Ok(()) => {
            db.set_app_state(LAST_PUBLISHED_DIGEST_KEY, &digest)?;
            db.set_app_state(LAST_PUBLISHED_AT_KEY, published_at)?;
            db.clear_app_state(PENDING_PUBLISH_KEY)?;
            Ok(PublishOutcome::Published { digest })
        }
        Err(e) => {
            db.set_app_state(PENDING_PUBLISH_KEY, "1")?;
            Err(e)
        }
    }
}

/// get-then-put with the sha threaded through: an existing remote file can
/// only be updated by presenting its current blob sha (the mock and the
/// real Contents API both reject a sha-less overwrite), and a missing file
/// (first publish) must be created without one.
fn upload(
    api: &dyn GitHubApi,
    config: &PublishConfig,
    snapshot: &Snapshot,
    published_at: &str,
) -> Result<(), SyncError> {
    let repo = config.repo_ref();
    let body = to_canonical_json(snapshot, Some(published_at));
    let prev_sha = api.get_file(&repo, &config.path)?.map(|f| f.sha);
    api.put_file(
        &repo,
        &config.path,
        body.as_bytes(),
        PUBLISH_COMMIT_MESSAGE,
        prev_sha.as_deref(),
    )?;
    Ok(())
}

/// Current UTC time as RFC 3339 at whole-second resolution
/// (`YYYY-MM-DDTHH:MM:SSZ`) — the only volatile value a published snapshot
/// carries. Sub-second precision is deliberately dropped: nothing compares
/// publish times closer than a second, and the shorter form reads better in
/// the web banner.
fn utc_now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_epoch_seconds(secs)
}

/// Format seconds-since-Unix-epoch as `YYYY-MM-DDTHH:MM:SSZ`.
fn format_epoch_seconds(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Days-since-1970-01-01 → (year, month, day) in the proleptic Gregorian
/// calendar. Howard Hinnant's `civil_from_days` algorithm, exact for the
/// full `i64` day range this can ever see.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_formats_as_1970() {
        assert_eq!(format_epoch_seconds(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_timestamps_format_correctly() {
        // 2026-07-19T00:00:00Z: 20,653 days after the epoch.
        assert_eq!(format_epoch_seconds(1_784_419_200), "2026-07-19T00:00:00Z");
        // A leap day: 2024-02-29T12:34:56Z.
        assert_eq!(
            format_epoch_seconds(1_709_164_800 + 12 * 3600 + 34 * 60 + 56),
            "2024-02-29T12:34:56Z"
        );
        // End-of-year boundary: 2023-12-31T23:59:59Z is one second before
        // 2024-01-01T00:00:00Z (1_704_067_200).
        assert_eq!(format_epoch_seconds(1_704_067_199), "2023-12-31T23:59:59Z");
        assert_eq!(format_epoch_seconds(1_704_067_200), "2024-01-01T00:00:00Z");
    }

    #[test]
    fn utc_now_rfc3339_has_the_fixed_shape() {
        let now = utc_now_rfc3339();
        assert_eq!(now.len(), 20, "unexpected timestamp shape: {now}");
        let bytes = now.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            match i {
                4 | 7 => assert_eq!(*b, b'-', "bad separator in {now}"),
                10 => assert_eq!(*b, b'T', "bad separator in {now}"),
                13 | 16 => assert_eq!(*b, b':', "bad separator in {now}"),
                19 => assert_eq!(*b, b'Z', "bad suffix in {now}"),
                _ => assert!(b.is_ascii_digit(), "non-digit in {now}"),
            }
        }
        // Sanity: the year is in the era this code can actually run in.
        assert!(&now[..4] >= "2026", "implausible year in {now}");
    }
}
