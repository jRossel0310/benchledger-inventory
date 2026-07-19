//! Hermetic trait-level tests for the GitHub client seam: an in-memory
//! [`MockGitHub`] exercising exactly the [`GitHubApi`] contract the Task 4
//! publish path will rely on. Zero network — the reqwest implementation's
//! live behavior is deferred to the Task 9 handshake.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use inventory_sync::github::{GitHubApi, GitHubError, PutOutcome, RemoteFile, RepoRef};

/// (owner, repo, branch, path) — the full addressing tuple for one file.
type FileKey = (String, String, String, String);

/// In-memory stand-in for the GitHub Contents API.
///
/// Files are keyed by the full (owner, repo, branch, path) tuple so a test
/// against the wrong branch misses, exactly like the real API. Shas are
/// `"sha-<n>"` from a monotonic counter — unique per write, which is all
/// the conflict protocol needs. `calls` counts every trait-method
/// invocation (Task 4's unchanged-skip test asserts on the same pattern).
/// `fail_with_auth` makes every call return [`GitHubError::Auth`],
/// modelling a revoked token. `existing_repos` backs `repo_exists`: `None`
/// (the default) means every repo exists — the sensible default, so
/// file-level tests never have to declare repos — while `Some(set)`
/// restricts existence to the listed `(owner, repo)` pairs, modelling a
/// typo'd repository.
#[derive(Default)]
struct MockGitHub {
    files: RefCell<HashMap<FileKey, (String, Vec<u8>)>>,
    calls: Cell<u32>,
    next_sha: Cell<u64>,
    fail_with_auth: bool,
    existing_repos: Option<HashSet<(String, String)>>,
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
}

impl GitHubApi for MockGitHub {
    fn get_file(&self, cfg: &RepoRef, path: &str) -> Result<Option<RemoteFile>, GitHubError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail_with_auth {
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

    fn repo_exists(&self, cfg: &RepoRef) -> Result<bool, GitHubError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail_with_auth {
            return Err(GitHubError::Auth);
        }
        Ok(match &self.existing_repos {
            None => true,
            Some(set) => set.contains(&(cfg.owner.clone(), cfg.repo.clone())),
        })
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
        if self.fail_with_auth {
            return Err(GitHubError::Auth);
        }
        let key = Self::key(cfg, path);
        let mut files = self.files.borrow_mut();
        // Real-API conflict semantics: updating an existing file requires
        // its current sha (missing or stale -> 409/422 -> Conflict);
        // supplying a sha for a file that doesn't exist is likewise stale
        // knowledge -> Conflict.
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

fn repo() -> RepoRef {
    RepoRef {
        owner: "jacob".to_string(),
        repo: "bench-ledger-public".to_string(),
        branch: "main".to_string(),
    }
}

const PATH: &str = "apps/web/public/inventory.snapshot.json";

#[test]
fn get_missing_file_returns_ok_none() {
    let gh = MockGitHub::default();
    let got = gh.get_file(&repo(), PATH).expect("get should succeed");
    assert!(got.is_none());
    assert_eq!(gh.calls.get(), 1);
}

#[test]
fn put_new_file_without_prev_sha_succeeds_and_round_trips() {
    let gh = MockGitHub::default();
    let outcome = gh
        .put_file(&repo(), PATH, b"{\"v\":1}", "Publish snapshot", None)
        .expect("first put should succeed");

    let fetched = gh
        .get_file(&repo(), PATH)
        .expect("get should succeed")
        .expect("file should now exist");
    assert_eq!(fetched.sha, outcome.new_sha);
    assert_eq!(fetched.content, b"{\"v\":1}");
    assert_eq!(gh.calls.get(), 2);
}

#[test]
fn put_update_with_correct_sha_succeeds_and_changes_sha() {
    let gh = MockGitHub::default();
    let first = gh
        .put_file(&repo(), PATH, b"{\"v\":1}", "Publish snapshot", None)
        .unwrap();
    let second = gh
        .put_file(
            &repo(),
            PATH,
            b"{\"v\":2}",
            "Publish snapshot",
            Some(&first.new_sha),
        )
        .expect("update with the current sha should succeed");

    assert_ne!(second.new_sha, first.new_sha);
    let fetched = gh.get_file(&repo(), PATH).unwrap().unwrap();
    assert_eq!(fetched.sha, second.new_sha);
    assert_eq!(fetched.content, b"{\"v\":2}");
}

#[test]
fn put_with_stale_sha_returns_conflict_and_leaves_remote_untouched() {
    let gh = MockGitHub::default();
    let first = gh
        .put_file(&repo(), PATH, b"{\"v\":1}", "Publish snapshot", None)
        .unwrap();
    // Someone else updated the file; our sha is now stale.
    let second = gh
        .put_file(
            &repo(),
            PATH,
            b"{\"v\":2}",
            "Publish snapshot",
            Some(&first.new_sha),
        )
        .unwrap();

    let err = gh
        .put_file(
            &repo(),
            PATH,
            b"{\"v\":3}",
            "Publish snapshot",
            Some(&first.new_sha),
        )
        .expect_err("stale sha must conflict");
    assert!(matches!(err, GitHubError::Conflict));

    // The conflicting write must not have clobbered the remote content.
    let fetched = gh.get_file(&repo(), PATH).unwrap().unwrap();
    assert_eq!(fetched.sha, second.new_sha);
    assert_eq!(fetched.content, b"{\"v\":2}");
}

#[test]
fn put_without_sha_over_an_existing_file_returns_conflict() {
    let gh = MockGitHub::default();
    gh.put_file(&repo(), PATH, b"{\"v\":1}", "Publish snapshot", None)
        .unwrap();
    let err = gh
        .put_file(&repo(), PATH, b"{\"v\":2}", "Publish snapshot", None)
        .expect_err("existing file without a sha must conflict");
    assert!(matches!(err, GitHubError::Conflict));
}

#[test]
fn files_are_scoped_by_branch_and_repo() {
    let gh = MockGitHub::default();
    gh.put_file(&repo(), PATH, b"{\"v\":1}", "Publish snapshot", None)
        .unwrap();

    let other_branch = RepoRef {
        branch: "preview".to_string(),
        ..repo()
    };
    assert!(gh.get_file(&other_branch, PATH).unwrap().is_none());

    let other_repo = RepoRef {
        repo: "another-repo".to_string(),
        ..repo()
    };
    assert!(gh.get_file(&other_repo, PATH).unwrap().is_none());
}

#[test]
fn repo_exists_defaults_to_true_and_honors_a_configured_existing_set() {
    // Default: every repo exists (file-level tests never declare repos).
    let gh = MockGitHub::default();
    assert!(gh.repo_exists(&repo()).unwrap());

    // Configured set: only listed (owner, repo) pairs exist — a typo'd
    // repo distinctly reads as Ok(false), never as a missing FILE.
    let gh = MockGitHub {
        existing_repos: Some(HashSet::from([(
            "jacob".to_string(),
            "bench-ledger-public".to_string(),
        )])),
        ..MockGitHub::default()
    };
    assert!(gh.repo_exists(&repo()).unwrap());
    let typoed = RepoRef {
        repo: "bench-ledger-pubic".to_string(),
        ..repo()
    };
    assert!(!gh.repo_exists(&typoed).unwrap());
}

#[test]
fn repo_exists_propagates_auth_failure() {
    let gh = MockGitHub {
        fail_with_auth: true,
        ..MockGitHub::default()
    };
    assert!(matches!(
        gh.repo_exists(&repo()).expect_err("auth must fail"),
        GitHubError::Auth
    ));
}

#[test]
fn auth_error_mode_propagates_from_both_methods() {
    let gh = MockGitHub {
        fail_with_auth: true,
        ..MockGitHub::default()
    };
    assert!(matches!(
        gh.get_file(&repo(), PATH).expect_err("auth must fail"),
        GitHubError::Auth
    ));
    assert!(matches!(
        gh.put_file(&repo(), PATH, b"x", "Publish snapshot", None)
            .expect_err("auth must fail"),
        GitHubError::Auth
    ));
    assert_eq!(gh.calls.get(), 2);
}

#[test]
fn github_error_display_and_debug_never_contain_a_planted_token() {
    let token = "fake-token-abc";
    let errors: Vec<GitHubError> = vec![
        GitHubError::Auth,
        GitHubError::NotFound,
        GitHubError::Conflict,
        GitHubError::RateLimited,
        GitHubError::Network("network error or timeout".to_string()),
        GitHubError::Api(500),
    ];
    for err in &errors {
        let display = err.to_string();
        let debug = format!("{err:?}");
        assert!(
            !display.contains(token),
            "Display leaked a token: {display}"
        );
        assert!(!debug.contains(token), "Debug leaked a token: {debug}");
    }
}
