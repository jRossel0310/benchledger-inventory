//! Phase 6 Task 4 gate tests for the publish orchestration: hermetic, driven
//! entirely through a `MockGitHub` implementing the `GitHubApi` trait — the
//! same in-memory mock pattern `tests/github.rs` establishes (including its
//! real-API conflict rule: updating an existing file without its current
//! sha is a `Conflict`), so a publish path that forgot the get-before-put
//! sha threading fails here exactly like it would against GitHub.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use inventory_core::ids::CategoryId;
use inventory_core::quantity::QuantityUnit;
use inventory_db::parts::PartDraft;
use inventory_db::{dev_seed, Database, MISC_CATEGORY_ID};
use inventory_sync::github::{GitHubApi, GitHubError, PutOutcome, RemoteFile, RepoRef};
use inventory_sync::publish::{
    publish_snapshot_at, PublishOutcome, BRANCH_SETTING, DEFAULT_BRANCH, DEFAULT_PATH,
    LAST_PUBLISHED_AT_KEY, LAST_PUBLISHED_DIGEST_KEY, OWNER_SETTING, PATH_SETTING,
    PENDING_PUBLISH_KEY, REPO_SETTING,
};
use inventory_sync::snapshot::{build_snapshot, content_digest, to_canonical_json};
use inventory_sync::SyncError;

/// (owner, repo, branch, path) — the full addressing tuple for one file.
type FileKey = (String, String, String, String);

/// In-memory GitHub Contents stand-in — same shape as `tests/github.rs`'s
/// mock: files keyed by the full addressing tuple, monotonic `"sha-<n>"`
/// shas, a `calls` counter the unchanged-skip test asserts on, and a
/// `fail_with_auth` switch modelling a revoked token. Crucially it enforces
/// the real API's conflict semantics (existing file + missing/stale sha →
/// `Conflict`), which is what makes these tests prove the publish path
/// threads the sha from `get_file` into `put_file`.
#[derive(Default)]
struct MockGitHub {
    files: RefCell<HashMap<FileKey, (String, Vec<u8>)>>,
    calls: Cell<u32>,
    next_sha: Cell<u64>,
    fail_with_auth: Cell<bool>,
}

impl MockGitHub {
    fn key(cfg: &RepoRef, path: &str) -> FileKey {
        (
            cfg.owner.clone(),
            cfg.repo.clone(),
            cfg.branch.clone(),
            path.to_string(),
        )
    }

    fn mint_sha(&self) -> String {
        let n = self.next_sha.get() + 1;
        self.next_sha.set(n);
        format!("sha-{n}")
    }

    fn stored(&self, cfg: &RepoRef, path: &str) -> Option<(String, Vec<u8>)> {
        self.files.borrow().get(&Self::key(cfg, path)).cloned()
    }
}

impl GitHubApi for MockGitHub {
    fn get_file(&self, cfg: &RepoRef, path: &str) -> Result<Option<RemoteFile>, GitHubError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail_with_auth.get() {
            return Err(GitHubError::Auth);
        }
        Ok(self
            .files
            .borrow()
            .get(&Self::key(cfg, path))
            .map(|(sha, content)| RemoteFile {
                sha: sha.clone(),
                content: content.clone(),
            }))
    }

    fn repo_exists(&self, _cfg: &RepoRef) -> Result<bool, GitHubError> {
        // The publish path never probes repo existence (only the Settings
        // connection test does); every repo simply exists here.
        Ok(true)
    }

    fn put_file(
        &self,
        cfg: &RepoRef,
        path: &str,
        content: &[u8],
        _message: &str,
        prev_sha: Option<&str>,
    ) -> Result<PutOutcome, GitHubError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail_with_auth.get() {
            return Err(GitHubError::Auth);
        }
        let key = Self::key(cfg, path);
        let mut files = self.files.borrow_mut();
        match (files.get(&key), prev_sha) {
            (Some((current, _)), Some(prev)) if current == prev => {}
            (Some(_), _) => return Err(GitHubError::Conflict),
            (None, Some(_)) => return Err(GitHubError::Conflict),
            (None, None) => {}
        }
        let new_sha = self.mint_sha();
        files.insert(key, (new_sha.clone(), content.to_vec()));
        Ok(PutOutcome { new_sha })
    }
}

fn open_seeded_db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let mut db =
        Database::open_and_migrate(&dir.path().join("t.sqlite"), &dir.path().join("backups"))
            .unwrap();
    dev_seed::run(&mut db).unwrap();
    (dir, db)
}

/// Point the publish config at the mock's coordinates. Branch and path are
/// left unset so the defaults apply — `repo_ref()` below must agree.
fn configure_publish(db: &mut Database) {
    db.set_setting(OWNER_SETTING, "jacob").unwrap();
    db.set_setting(REPO_SETTING, "bench-ledger-public").unwrap();
}

fn repo_ref() -> RepoRef {
    RepoRef {
        owner: "jacob".to_string(),
        repo: "bench-ledger-public".to_string(),
        branch: DEFAULT_BRANCH.to_string(),
    }
}

/// Add one more part so the snapshot's content (and therefore its digest)
/// changes between two publishes.
fn mutate_inventory(db: &mut Database, name: &str) {
    let misc = CategoryId::from_string(MISC_CATEGORY_ID.to_string()).unwrap();
    db.create_part(&PartDraft {
        display_name: name.to_string(),
        category_id: misc,
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".to_string(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    })
    .unwrap();
}

#[test]
fn missing_config_is_not_configured_with_zero_api_calls_and_no_pending_marker() {
    let (_dir, mut db) = open_seeded_db();
    let gh = MockGitHub::default();

    let err = publish_snapshot_at(&mut db, &gh, "2026-07-19T10:00:00Z")
        .expect_err("unconfigured publish must fail");
    assert!(matches!(err, SyncError::NotConfigured));
    assert_eq!(gh.calls.get(), 0);
    // Nothing was attempted, so nothing is pending to retry.
    assert_eq!(db.get_app_state(PENDING_PUBLISH_KEY).unwrap(), None);
}

#[test]
fn first_publish_creates_the_file_without_a_sha_and_records_app_state() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);
    let gh = MockGitHub::default();

    let outcome = publish_snapshot_at(&mut db, &gh, "2026-07-19T10:00:00Z").unwrap();
    let PublishOutcome::Published { digest } = outcome else {
        panic!("first publish must be Published, got {outcome:?}");
    };

    // Exactly one get (no file yet) + one put (created without a sha —
    // the mock would have returned Conflict had a sha been supplied).
    assert_eq!(gh.calls.get(), 2);

    // The uploaded bytes are the canonical publish form with the caller's
    // published_at stamped in, at the DEFAULT path on the DEFAULT branch.
    let (_sha, content) = gh.stored(&repo_ref(), DEFAULT_PATH).expect("file created");
    let snapshot = build_snapshot(&db).unwrap();
    assert_eq!(
        String::from_utf8(content).unwrap(),
        to_canonical_json(&snapshot, Some("2026-07-19T10:00:00Z"))
    );

    // app_state records the digest + timestamp; nothing pending.
    assert_eq!(digest, content_digest(&snapshot));
    assert_eq!(
        db.get_app_state(LAST_PUBLISHED_DIGEST_KEY).unwrap(),
        Some(digest)
    );
    assert_eq!(
        db.get_app_state(LAST_PUBLISHED_AT_KEY).unwrap(),
        Some("2026-07-19T10:00:00Z".to_string())
    );
    assert_eq!(db.get_app_state(PENDING_PUBLISH_KEY).unwrap(), None);
}

#[test]
fn unchanged_content_short_circuits_with_zero_api_calls_even_with_a_new_published_at() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);
    let gh = MockGitHub::default();

    publish_snapshot_at(&mut db, &gh, "2026-07-19T10:00:00Z").unwrap();
    let calls_after_first = gh.calls.get();

    // Same content, different wall-clock time: the digest is computed over
    // the published_at-stripped form, so this must be Unchanged — and must
    // not touch the API at all.
    let outcome = publish_snapshot_at(&mut db, &gh, "2026-07-20T08:30:00Z").unwrap();
    assert_eq!(outcome, PublishOutcome::Unchanged);
    assert_eq!(gh.calls.get(), calls_after_first);

    // The recorded publish time still belongs to the publish that actually
    // uploaded bytes.
    assert_eq!(
        db.get_app_state(LAST_PUBLISHED_AT_KEY).unwrap(),
        Some("2026-07-19T10:00:00Z".to_string())
    );
}

#[test]
fn changed_content_republishes_by_threading_the_remote_sha_through_get_then_put() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);
    let gh = MockGitHub::default();

    publish_snapshot_at(&mut db, &gh, "2026-07-19T10:00:00Z").unwrap();
    let (first_sha, _) = gh.stored(&repo_ref(), DEFAULT_PATH).unwrap();
    let first_digest = db
        .get_app_state(LAST_PUBLISHED_DIGEST_KEY)
        .unwrap()
        .unwrap();

    mutate_inventory(&mut db, "Part added between publishes");

    // The mock rejects any sha-less overwrite of an existing file, so a
    // successful second publish proves get_file's sha was threaded into
    // put_file.
    let outcome = publish_snapshot_at(&mut db, &gh, "2026-07-20T09:00:00Z").unwrap();
    let PublishOutcome::Published { digest } = outcome else {
        panic!("changed content must republish, got {outcome:?}");
    };
    assert_ne!(digest, first_digest);
    assert_eq!(
        db.get_app_state(LAST_PUBLISHED_DIGEST_KEY).unwrap(),
        Some(digest)
    );

    let (second_sha, content) = gh.stored(&repo_ref(), DEFAULT_PATH).unwrap();
    assert_ne!(second_sha, first_sha);
    let snapshot = build_snapshot(&db).unwrap();
    assert_eq!(
        String::from_utf8(content).unwrap(),
        to_canonical_json(&snapshot, Some("2026-07-20T09:00:00Z"))
    );
}

#[test]
fn custom_branch_and_path_settings_are_respected() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);
    db.set_setting(BRANCH_SETTING, "preview").unwrap();
    db.set_setting(PATH_SETTING, "data/inventory.json").unwrap();
    let gh = MockGitHub::default();

    publish_snapshot_at(&mut db, &gh, "2026-07-19T10:00:00Z").unwrap();

    let custom = RepoRef {
        branch: "preview".to_string(),
        ..repo_ref()
    };
    assert!(gh.stored(&custom, "data/inventory.json").is_some());
    assert!(gh.stored(&repo_ref(), DEFAULT_PATH).is_none());
}

#[test]
fn failure_after_the_unchanged_check_sets_pending_and_returns_the_typed_github_error() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);
    let gh = MockGitHub::default();
    gh.fail_with_auth.set(true);

    let err = publish_snapshot_at(&mut db, &gh, "2026-07-19T10:00:00Z")
        .expect_err("auth failure must propagate");
    assert!(matches!(err, SyncError::GitHub(GitHubError::Auth)));

    // The pending marker is set so the close flow / startup retry knows
    // there is unpublished content; no publish state was falsely recorded.
    assert_eq!(
        db.get_app_state(PENDING_PUBLISH_KEY).unwrap(),
        Some("1".to_string())
    );
    assert_eq!(db.get_app_state(LAST_PUBLISHED_DIGEST_KEY).unwrap(), None);
    assert_eq!(db.get_app_state(LAST_PUBLISHED_AT_KEY).unwrap(), None);
}

/// A `GitHubApi` whose very first network call panics — the deterministic
/// stand-in for "the process was killed while the upload was in flight"
/// (close-dialog timeout + "Close anyway" exits the process before the
/// request settles). A panic, like a kill, reaches NO completion path in
/// `publish_snapshot_at`: neither the success writes nor the failure
/// handler run, so the only way the pending marker can survive is if it
/// was written BEFORE the upload began.
struct KilledMidFlightGitHub;

impl GitHubApi for KilledMidFlightGitHub {
    fn get_file(&self, _cfg: &RepoRef, _path: &str) -> Result<Option<RemoteFile>, GitHubError> {
        panic!("simulated process kill during the in-flight upload");
    }

    fn repo_exists(&self, _cfg: &RepoRef) -> Result<bool, GitHubError> {
        unreachable!("the publish path never probes repo existence");
    }

    fn put_file(
        &self,
        _cfg: &RepoRef,
        _path: &str,
        _content: &[u8],
        _message: &str,
        _prev_sha: Option<&str>,
    ) -> Result<PutOutcome, GitHubError> {
        unreachable!("get_file already killed the attempt");
    }
}

#[test]
fn a_kill_mid_upload_still_leaves_the_pending_marker_for_the_next_launch() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);

    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        publish_snapshot_at(&mut db, &KilledMidFlightGitHub, "2026-07-19T10:00:00Z")
    }));
    assert!(attempt.is_err(), "the mock must have panicked mid-upload");

    // The marker must have been set before the upload started: the panic
    // skipped every completion write, exactly like a killed process would.
    assert_eq!(
        db.get_app_state(PENDING_PUBLISH_KEY).unwrap(),
        Some("1".to_string())
    );
}

#[test]
fn retry_after_a_failure_publishes_and_clears_the_pending_marker() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);
    let gh = MockGitHub::default();

    gh.fail_with_auth.set(true);
    publish_snapshot_at(&mut db, &gh, "2026-07-19T10:00:00Z").unwrap_err();
    assert_eq!(
        db.get_app_state(PENDING_PUBLISH_KEY).unwrap(),
        Some("1".to_string())
    );

    // Token fixed (auth restored): the retry succeeds and clears pending.
    gh.fail_with_auth.set(false);
    let outcome = publish_snapshot_at(&mut db, &gh, "2026-07-19T11:00:00Z").unwrap();
    assert!(matches!(outcome, PublishOutcome::Published { .. }));
    assert_eq!(db.get_app_state(PENDING_PUBLISH_KEY).unwrap(), None);
    assert_eq!(
        db.get_app_state(LAST_PUBLISHED_AT_KEY).unwrap(),
        Some("2026-07-19T11:00:00Z".to_string())
    );
}

#[test]
fn unchanged_outcome_clears_a_stale_pending_marker_without_api_calls() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);
    let gh = MockGitHub::default();

    publish_snapshot_at(&mut db, &gh, "2026-07-19T10:00:00Z").unwrap();
    let calls_after_first = gh.calls.get();

    // Simulate: a later publish failed (marker set) but its content change
    // was reverted, so the current digest again matches what's published.
    db.set_app_state(PENDING_PUBLISH_KEY, "1").unwrap();

    let outcome = publish_snapshot_at(&mut db, &gh, "2026-07-19T12:00:00Z").unwrap();
    assert_eq!(outcome, PublishOutcome::Unchanged);
    assert_eq!(gh.calls.get(), calls_after_first);
    // The remote already holds this exact content, so nothing is pending.
    assert_eq!(db.get_app_state(PENDING_PUBLISH_KEY).unwrap(), None);
}

#[test]
fn digest_is_independent_of_published_at_but_tracks_content() {
    let (_dir, mut db) = open_seeded_db();
    configure_publish(&mut db);

    let snapshot = build_snapshot(&db).unwrap();
    let digest = content_digest(&snapshot);
    // Two publish forms with different timestamps hash to the same digest
    // (the digest form strips published_at)...
    assert_ne!(
        to_canonical_json(&snapshot, Some("2026-07-19T10:00:00Z")),
        to_canonical_json(&snapshot, Some("2026-07-20T10:00:00Z"))
    );
    assert_eq!(digest, content_digest(&snapshot));

    // ...while a content change produces a different digest.
    mutate_inventory(&mut db, "Digest-changing part");
    let changed = build_snapshot(&db).unwrap();
    assert_ne!(content_digest(&changed), digest);
}
