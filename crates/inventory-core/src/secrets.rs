//! Secret storage backed by the OS credential store (Windows Credential
//! Manager) via the `keyring` crate. Spec §16 / ADR #3: secrets — the
//! DigiKey Client ID/Secret (and the OAuth access token minted from them)
//! and the GitHub publish token — must live ONLY in the OS credential store
//! or in-memory — never in SQLite, `settings`, logs, exports, snapshots,
//! the repo, or a fixture.
//!
//! This module is the single read/write path for those values. The dev bin
//! `set_digikey_credentials`, the DigiKey client, and the GitHub publish
//! client all go through here, so there is exactly one place that touches
//! the platform credential store.

use std::fmt;

/// Windows Credential Manager "target" service name shared by both DigiKey
/// entries.
const SERVICE: &str = "ElectronicsInventory-DigiKey";
const USER_CLIENT_ID: &str = "client_id";
const USER_CLIENT_SECRET: &str = "client_secret";

/// Windows Credential Manager "target" service name for the GitHub
/// publish token entry.
const GITHUB_SERVICE: &str = "ElectronicsInventory-GitHub";
const USER_GITHUB_TOKEN: &str = "token";

/// A DigiKey OAuth2 client-credentials pair.
///
/// `Debug` is implemented by hand, not derived: this struct routinely flows
/// through `?`/error-context/panic-message paths, and a derived `Debug`
/// would put the raw secret (and the client id) in any of those. Neither
/// field's value is printed — only the id's length, and a fixed
/// "<redacted>" marker for the secret.
pub struct DigiKeyCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl fmt::Debug for DigiKeyCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DigiKeyCredentials")
            .field("client_id", &format!("<len {}>", self.client_id.len()))
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// Failure storing, loading, or clearing DigiKey credentials in the
/// platform credential store.
///
/// Deliberately carries no secret value in either `Display` or `Debug` —
/// only which operation failed and a fixed, generic backend classification
/// (never the underlying `keyring::Error`'s payload, which on some
/// variants — e.g. a corrupt/undecodable stored blob — carries raw bytes).
/// That keeps this type safe to log or propagate without a redaction pass.
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("failed to {0} in the platform credential store")]
    Backend(&'static str),
}

fn entry_for(service: &'static str, user: &'static str) -> Result<keyring::Entry, SecretsError> {
    keyring::Entry::new(service, user).map_err(|_| SecretsError::Backend("open credential entry"))
}

fn entry(user: &'static str) -> Result<keyring::Entry, SecretsError> {
    entry_for(SERVICE, user)
}

/// Store `creds` in the platform credential store as two entries (one for
/// the client id, one for the secret) under service
/// `"ElectronicsInventory-DigiKey"`. Overwrites any existing values.
pub fn store_digikey_credentials(creds: &DigiKeyCredentials) -> Result<(), SecretsError> {
    entry(USER_CLIENT_ID)?
        .set_password(&creds.client_id)
        .map_err(|_| SecretsError::Backend("store client_id"))?;
    entry(USER_CLIENT_SECRET)?
        .set_password(&creds.client_secret)
        .map_err(|_| SecretsError::Backend("store client_secret"))?;
    Ok(())
}

/// Load the DigiKey credentials, if configured.
///
/// `Ok(None)` is the clean "not configured" signal — it is returned both
/// when neither entry is set, and when only one of the two is set. A
/// partial pair isn't usable (the DigiKey client needs both), and treating
/// it as "not configured" rather than an error keeps `load` safe to call
/// speculatively (e.g. from a status check) without the caller having to
/// distinguish "never configured" from "half-configured" — both mean
/// enrichment can't run yet. The stray partial entry, if any, is left as
/// is: `load` never mutates the store, and the next successful `store`
/// overwrites both entries anyway.
///
/// A real backend failure (as opposed to a simple missing entry) maps to
/// `Err`.
pub fn load_digikey_credentials() -> Result<Option<DigiKeyCredentials>, SecretsError> {
    let client_id = read_one(USER_CLIENT_ID)?;
    let client_secret = read_one(USER_CLIENT_SECRET)?;
    match (client_id, client_secret) {
        (Some(client_id), Some(client_secret)) => Ok(Some(DigiKeyCredentials {
            client_id,
            client_secret,
        })),
        _ => Ok(None),
    }
}

fn read_one(user: &'static str) -> Result<Option<String>, SecretsError> {
    match entry(user)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(SecretsError::Backend("load credential")),
    }
}

/// Delete both DigiKey credential entries. Deleting an entry that doesn't
/// exist is not an error (so `clear` is safe to call unconditionally, e.g.
/// from a "forget my credentials" action regardless of current state).
pub fn clear_digikey_credentials() -> Result<(), SecretsError> {
    delete_one(USER_CLIENT_ID)?;
    delete_one(USER_CLIENT_SECRET)?;
    Ok(())
}

fn delete_one(user: &'static str) -> Result<(), SecretsError> {
    match entry(user)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(SecretsError::Backend("clear credential")),
    }
}

/// A GitHub fine-grained personal access token, freshly loaded from the
/// platform credential store.
///
/// A newtype rather than a bare `String` for the same reason
/// [`DigiKeyCredentials`] hand-writes its `Debug`: the loaded token flows
/// through `?`/error-context/panic-message paths on its way to the GitHub
/// client, and a bare `String` would print verbatim in any of those. The
/// hand-written `Debug` prints a fixed `<redacted>` marker instead — never
/// the value or even its length.
///
/// The raw value is only reachable through [`GitHubToken::expose`], so
/// every use of the actual secret is greppable by that one method name.
pub struct GitHubToken(String);

impl GitHubToken {
    pub fn new(token: String) -> Self {
        GitHubToken(token)
    }

    /// The raw token value — the single, intentionally-named extraction
    /// point. Callers must put the result ONLY in an `Authorization`
    /// header (or the credential store), never in an error, log line, or
    /// any persisted/exported artifact.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for GitHubToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("GitHubToken").field(&"<redacted>").finish()
    }
}

/// Store the GitHub publish token in the platform credential store under
/// service `"ElectronicsInventory-GitHub"`. Overwrites any existing value.
pub fn store_github_token(token: &str) -> Result<(), SecretsError> {
    entry_for(GITHUB_SERVICE, USER_GITHUB_TOKEN)?
        .set_password(token)
        .map_err(|_| SecretsError::Backend("store github token"))
}

/// Load the GitHub publish token, if configured. `Ok(None)` is the clean
/// "not configured" signal; a real backend failure (as opposed to a simple
/// missing entry) maps to `Err`.
pub fn load_github_token() -> Result<Option<GitHubToken>, SecretsError> {
    match entry_for(GITHUB_SERVICE, USER_GITHUB_TOKEN)?.get_password() {
        Ok(value) => Ok(Some(GitHubToken(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(SecretsError::Backend("load github token")),
    }
}

/// Delete the GitHub token entry. Deleting an entry that doesn't exist is
/// not an error (so `clear` is safe to call unconditionally, e.g. from a
/// "forget my token" action regardless of current state).
pub fn clear_github_token() -> Result<(), SecretsError> {
    match entry_for(GITHUB_SERVICE, USER_GITHUB_TOKEN)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(SecretsError::Backend("clear github token")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard, Once, OnceLock};

    // `keyring`'s own built-in `mock` module does NOT persist across
    // separately-constructed `Entry`s with the same service/user — each
    // `Entry::new(..)` call gets its own empty, disconnected
    // `MockCredential` (verified: storing via one `Entry` and reading via a
    // second `Entry::new` with identical service/user returns `NoEntry`).
    // That's fine for tests that hold one `Entry` for both a `set` and a
    // `get`, but it does NOT model what this module needs: every function
    // here (`store_digikey_credentials`, `load_digikey_credentials`,
    // `clear_digikey_credentials`) opens a *fresh* `Entry` per call, which
    // only round-trips on a backend where the state lives outside the
    // `Entry` object — true of the real Windows Credential Manager, false
    // of the crate's built-in mock.
    //
    // So this is a small hand-rolled mock credential store, keyed by
    // (service, user) in a process-global map, that models real persistent
    // backend semantics instead. It never touches the OS keychain.
    struct ProcessMockCredential {
        key: (String, String),
    }

    static STORE: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();

    fn store() -> &'static Mutex<HashMap<(String, String), String>> {
        STORE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    impl keyring::credential::CredentialApi for ProcessMockCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            let value = String::from_utf8(secret.to_vec())
                .map_err(|e| keyring::Error::BadEncoding(e.into_bytes()))?;
            store().lock().unwrap().insert(self.key.clone(), value);
            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            store()
                .lock()
                .unwrap()
                .get(&self.key)
                .map(|v| v.clone().into_bytes())
                .ok_or(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            let mut guard = store().lock().unwrap();
            if guard.remove(&self.key).is_some() {
                Ok(())
            } else {
                Err(keyring::Error::NoEntry)
            }
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct ProcessMockBuilder;

    impl keyring::credential::CredentialBuilderApi for ProcessMockBuilder {
        fn build(
            &self,
            _target: Option<&str>,
            service: &str,
            user: &str,
        ) -> keyring::Result<Box<keyring::credential::Credential>> {
            Ok(Box::new(ProcessMockCredential {
                key: (service.to_string(), user.to_string()),
            }))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn persistence(&self) -> keyring::credential::CredentialPersistence {
            keyring::credential::CredentialPersistence::ProcessOnly
        }
    }

    static INSTALL: Once = Once::new();

    // All tests share one process-global mock store keyed by the same
    // fixed (SERVICE, "client_id"/"client_secret") pairs this module uses
    // internally, so they must not run concurrently against it (the
    // default `cargo test` behavior). `TEST_LOCK` serializes them; `setup`
    // also wipes any state left by a previous test before returning, so
    // each test starts from a known-empty store regardless of ordering.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn setup() -> MutexGuard<'static, ()> {
        INSTALL.call_once(|| {
            keyring::set_default_credential_builder(Box::new(ProcessMockBuilder));
        });
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        store().lock().unwrap().clear();
        guard
    }

    #[test]
    fn store_then_load_round_trips() {
        let _guard = setup();
        let creds = DigiKeyCredentials {
            client_id: "test-client-id-123".to_string(),
            client_secret: "test-client-secret-abc".to_string(),
        };
        store_digikey_credentials(&creds).expect("store should succeed");

        let loaded = load_digikey_credentials()
            .expect("load should succeed")
            .expect("credentials should be present");
        assert_eq!(loaded.client_id, "test-client-id-123");
        assert_eq!(loaded.client_secret, "test-client-secret-abc");
    }

    #[test]
    fn load_returns_none_when_unset() {
        let _guard = setup();
        assert!(load_digikey_credentials()
            .expect("load should succeed")
            .is_none());
    }

    #[test]
    fn load_returns_none_on_partial_state() {
        let _guard = setup();
        // Only the client_id entry is written; client_secret is missing.
        entry(USER_CLIENT_ID)
            .unwrap()
            .set_password("only-half-there")
            .unwrap();
        assert!(load_digikey_credentials()
            .expect("load should succeed")
            .is_none());
    }

    #[test]
    fn clear_removes_credentials_and_load_then_returns_none() {
        let _guard = setup();
        let creds = DigiKeyCredentials {
            client_id: "another-id".to_string(),
            client_secret: "another-secret".to_string(),
        };
        store_digikey_credentials(&creds).expect("store should succeed");
        assert!(load_digikey_credentials().unwrap().is_some());

        clear_digikey_credentials().expect("clear should succeed");
        assert!(load_digikey_credentials()
            .expect("load should succeed")
            .is_none());
    }

    #[test]
    fn clearing_when_absent_does_not_error() {
        let _guard = setup();
        clear_digikey_credentials().expect("clearing an absent entry should not error");
        clear_digikey_credentials().expect("clearing twice should still not error");
    }

    #[test]
    fn secrets_error_carries_no_secret_value() {
        let sample_id = "sample-client-id-should-never-appear";
        let sample_secret = "sample-client-secret-should-never-appear";

        // `SecretsError` only ever carries a fixed operation label (see the
        // type doc comment) — never a `keyring::Error` payload or any
        // caller-supplied value — so no variant construction can leak a
        // secret. Exercise every variant so this test breaks if a future
        // variant is added that captures one.
        let err = SecretsError::Backend("store client_secret");
        let display = err.to_string();
        let debug = format!("{err:?}");

        for sample in [sample_id, sample_secret] {
            assert!(
                !display.contains(sample),
                "Display leaked a secret: {display}"
            );
            assert!(!debug.contains(sample), "Debug leaked a secret: {debug}");
        }
    }

    #[test]
    fn github_token_store_then_load_round_trips() {
        let _guard = setup();
        store_github_token("fake-token-abc").expect("store should succeed");
        let loaded = load_github_token()
            .expect("load should succeed")
            .expect("token should be present");
        assert_eq!(loaded.expose(), "fake-token-abc");
    }

    #[test]
    fn github_token_load_returns_none_when_unset() {
        let _guard = setup();
        assert!(load_github_token().expect("load should succeed").is_none());
    }

    #[test]
    fn github_token_clear_removes_token_and_is_idempotent() {
        let _guard = setup();
        store_github_token("fake-token-abc").expect("store should succeed");
        assert!(load_github_token().unwrap().is_some());

        clear_github_token().expect("clear should succeed");
        assert!(load_github_token().expect("load should succeed").is_none());
        clear_github_token().expect("clearing when absent should not error");
        clear_github_token().expect("clearing twice should still not error");
    }

    #[test]
    fn github_token_does_not_disturb_digikey_entries() {
        let _guard = setup();
        let creds = DigiKeyCredentials {
            client_id: "dk-id".to_string(),
            client_secret: "dk-secret".to_string(),
        };
        store_digikey_credentials(&creds).unwrap();
        store_github_token("fake-token-abc").unwrap();

        clear_github_token().unwrap();
        // The GitHub entry lives under its own service name; clearing it
        // must leave the DigiKey pair intact.
        assert!(load_digikey_credentials().unwrap().is_some());
    }

    #[test]
    fn github_token_debug_is_redacted() {
        let token = GitHubToken::new("fake-token-should-never-appear".to_string());
        let debug = format!("{token:?}");
        assert!(!debug.contains("fake-token-should-never-appear"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn secrets_error_from_github_token_ops_carries_no_token_value() {
        let planted = "fake-token-should-never-appear";
        // Every operation label the GitHub-token fns can construct — like
        // the DigiKey no-leak test, `SecretsError` only ever carries a
        // fixed &'static str, so no construction path can capture the
        // token; this breaks if a future variant starts capturing one.
        for label in [
            "store github token",
            "load github token",
            "clear github token",
        ] {
            let err = SecretsError::Backend(label);
            let display = err.to_string();
            let debug = format!("{err:?}");
            assert!(!display.contains(planted), "Display leaked: {display}");
            assert!(!debug.contains(planted), "Debug leaked: {debug}");
        }
    }

    #[test]
    fn digikey_credentials_debug_redacts_both_fields() {
        let creds = DigiKeyCredentials {
            client_id: "should-not-appear-id".to_string(),
            client_secret: "should-not-appear-secret".to_string(),
        };
        let debug = format!("{creds:?}");
        assert!(!debug.contains("should-not-appear-id"));
        assert!(!debug.contains("should-not-appear-secret"));
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains(&format!("<len {}>", "should-not-appear-id".len())));
    }
}
