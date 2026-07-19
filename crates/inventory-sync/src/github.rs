//! GitHub Contents API client for publishing the public snapshot.
//!
//! The publish path (Task 4) talks to GitHub only through the [`GitHubApi`]
//! trait, so every gate test is hermetic (an in-memory mock in
//! `tests/github.rs`); [`ReqwestGitHub`] is the production implementation.
//! Its live behavior is verified at the Task 9 handshake, mirroring the
//! DigiKey probe pattern — no network test runs in the gate.
//!
//! # Secrets discipline
//!
//! The token is held in [`TokenHolder`], a private struct with **no**
//! `Debug`/`Display`/`Clone` impls ([`ReqwestGitHub`] likewise derives
//! nothing), so no formatting path can print it. It is written into exactly
//! one place per request: the `Authorization: Bearer` header. [`GitHubError`]
//! carries only fixed classification strings and status codes — never a
//! response body, a `reqwest` error's `Display` (which can embed URLs), or
//! any caller-supplied value — so every error from this module is safe to
//! log or propagate without a redaction pass.
//!
//! # Status-code mapping (documented choices)
//!
//! | Status | `get_file` | `put_file` |
//! |--------|------------|------------|
//! | 200/201 | parse body | parse body |
//! | 401 | `Auth` | `Auth` |
//! | 403 | `RateLimited` if `x-ratelimit-remaining: 0`, else `Auth` | same |
//! | 404 | `Ok(None)` — file simply absent (first publish) | `NotFound` — repo/branch missing or token lacks access |
//! | 409 | `Api(409)` | `Conflict` |
//! | 422 | `Api(422)` | `Conflict` — GitHub reports a stale/missing `sha` for an existing file as 422; other 422 causes (malformed request) are indistinguishable without parsing the body, and the conservative reading ("remote changed, re-fetch and retry") is the safe recovery for both |
//! | 429 | `RateLimited` | `RateLimited` |
//! | other non-2xx | `Api(status)` | `Api(status)` |
//!
//! The 403 rate-limit distinction is trivially detectable from the
//! `x-ratelimit-remaining` header GitHub sends on every response; when the
//! header is absent or nonzero, a 403 means the token lacks permission →
//! `Auth`.
//!
//! # HTTP hygiene
//!
//! Same client shape as the DigiKey enrichment client: blocking, 15s total
//! request timeout, 5s connect timeout, with the identical fallback if the
//! builder fails. GitHub additionally requires a `User-Agent` header on
//! every request (requests without one are rejected with 403) — a fixed
//! `"BenchLedger"`. `Accept: application/vnd.github+json` selects the
//! stable REST media type.

use std::time::Duration;

use base64::Engine as _;

/// Which repository + branch to read/write. Not secret — this comes from
/// the `settings` table, never the credential store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub repo: String,
    pub branch: String,
}

/// A file fetched from the repo: its blob `sha` (needed as `prev_sha` for
/// the next update) and its decoded content bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub sha: String,
    pub content: Vec<u8>,
}

/// Result of a successful `put_file`: the new blob sha GitHub assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutOutcome {
    pub new_sha: String,
}

/// The publish path's only view of GitHub. `get_file` returning `Ok(None)`
/// means the file does not exist yet (a normal first-publish state), not an
/// error.
pub trait GitHubApi {
    fn get_file(&self, cfg: &RepoRef, path: &str) -> Result<Option<RemoteFile>, GitHubError>;
    fn put_file(
        &self,
        cfg: &RepoRef,
        path: &str,
        content: &[u8],
        message: &str,
        prev_sha: Option<&str>,
    ) -> Result<PutOutcome, GitHubError>;
}

/// Failure talking to GitHub. Every `Display` string is fixed (or a fixed
/// template over a status code) — see the module doc's secrets-discipline
/// section. `Network`'s payload is a fixed classification chosen at the
/// construction site, never a transport error's own message.
#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    /// 401, or 403 without an exhausted rate limit: the token is missing,
    /// expired, revoked, or lacks Contents permission on this repo.
    #[error("GitHub rejected the token")]
    Auth,
    /// Repo, branch, or (for `put_file`) parent path doesn't exist — or the
    /// token can't see it (GitHub deliberately 404s private repos the token
    /// can't access rather than 403ing).
    #[error("GitHub repository, branch, or path not found")]
    NotFound,
    /// The remote file changed since it was last fetched (stale `prev_sha`).
    /// Recovery: re-fetch, then retry the put with the fresh sha.
    #[error("GitHub file changed remotely (sha conflict)")]
    Conflict,
    /// 429, or 403 with `x-ratelimit-remaining: 0`.
    #[error("GitHub API rate limit exceeded")]
    RateLimited,
    /// Transport-level failure. The payload is a fixed classification
    /// string (e.g. [`NETWORK_CLASSIFICATION`]), never a response body or a
    /// `reqwest` error message.
    #[error("{0}")]
    Network(String),
    /// Any other non-success HTTP status — only the code, never the body.
    #[error("GitHub API returned HTTP {0}")]
    Api(u16),
}

/// The single fixed string every transport failure maps to.
pub const NETWORK_CLASSIFICATION: &str = "network error or timeout";

const API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = "BenchLedger";
const ACCEPT: &str = "application/vnd.github+json";

/// Same budgets as the DigiKey client: every call runs synchronously inside
/// a command handler (or the close flow), so an unbounded timeout would
/// hang the UI on a slow endpoint.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The token's in-memory home. Deliberately has no `Debug`, `Display`,
/// `Clone`, or serde impls — nothing can format or copy it out; the only
/// read is the private field access that writes the `Authorization` header.
struct TokenHolder {
    token: String,
}

/// Production [`GitHubApi`] over the REST Contents API. No `Debug` derive —
/// the struct holds the token (see [`TokenHolder`]).
pub struct ReqwestGitHub {
    token: TokenHolder,
    http: reqwest::blocking::Client,
}

impl ReqwestGitHub {
    /// Build a client holding `token` in memory only. The builder-failure
    /// fallback mirrors the DigiKey client: `.build()` only fails on a
    /// malformed client configuration (e.g. a broken TLS backend), never on
    /// the plain-duration timeouts, and such a failure would recur on the
    /// fallback's own construction — so `new` stays infallible.
    pub fn new(token: String) -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self {
            token: TokenHolder { token },
            http,
        }
    }
}

impl GitHubApi for ReqwestGitHub {
    fn get_file(&self, cfg: &RepoRef, path: &str) -> Result<Option<RemoteFile>, GitHubError> {
        let url = format!(
            "{}?ref={}",
            contents_url(cfg, path),
            percent_encode_path_segment(&cfg.branch)
        );
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.token.token)
            .header("Accept", ACCEPT)
            .header("User-Agent", USER_AGENT)
            .send()
            .map_err(|_| GitHubError::Network(NETWORK_CLASSIFICATION.to_string()))?;

        let status = response.status().as_u16();
        let rate_limit_exhausted = rate_limit_exhausted(&response);
        match status {
            200 => {}
            404 => return Ok(None),
            _ => return Err(classify_error_status(status, rate_limit_exhausted, false)),
        }

        let body: ContentsGetResponse = response
            .json()
            .map_err(|_| GitHubError::Network(NETWORK_CLASSIFICATION.to_string()))?;
        let content = decode_base64_content(&body.content)
            // A 200 whose content doesn't decode is a malformed/unexpected
            // response shape (e.g. the path is a directory) — classify by
            // the transport layer's fixed vocabulary, never echo the body.
            .ok_or_else(|| GitHubError::Network(NETWORK_CLASSIFICATION.to_string()))?;
        Ok(Some(RemoteFile {
            sha: body.sha,
            content,
        }))
    }

    fn put_file(
        &self,
        cfg: &RepoRef,
        path: &str,
        content: &[u8],
        message: &str,
        prev_sha: Option<&str>,
    ) -> Result<PutOutcome, GitHubError> {
        let body = ContentsPutRequest {
            message,
            content: encode_base64_content(content),
            branch: &cfg.branch,
            sha: prev_sha,
        };
        let response = self
            .http
            .put(contents_url(cfg, path))
            .bearer_auth(&self.token.token)
            .header("Accept", ACCEPT)
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .send()
            .map_err(|_| GitHubError::Network(NETWORK_CLASSIFICATION.to_string()))?;

        let status = response.status().as_u16();
        let rate_limit_exhausted = rate_limit_exhausted(&response);
        if !(status == 200 || status == 201) {
            return Err(classify_error_status(status, rate_limit_exhausted, true));
        }

        let body: ContentsPutResponse = response
            .json()
            .map_err(|_| GitHubError::Network(NETWORK_CLASSIFICATION.to_string()))?;
        Ok(PutOutcome {
            new_sha: body.content.sha,
        })
    }
}

/// `https://api.github.com/repos/{owner}/{repo}/contents/{path}` with every
/// segment percent-encoded. `path` may contain `/` separators — each
/// segment between them is encoded individually so the separators survive
/// but nothing inside a segment can restructure the URL.
fn contents_url(cfg: &RepoRef, path: &str) -> String {
    format!(
        "{}/repos/{}/{}/contents/{}",
        API_BASE,
        percent_encode_path_segment(&cfg.owner),
        percent_encode_path_segment(&cfg.repo),
        encode_repo_path(path)
    )
}

/// Percent-encode each `/`-separated segment of a repo-relative file path,
/// preserving the separators.
fn encode_repo_path(path: &str) -> String {
    path.split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Percent-encode a single path segment per RFC 3986's unreserved set
/// (same helper shape as the DigiKey client's).
fn percent_encode_path_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Map a non-success status to a [`GitHubError`] per the module doc's
/// table. `is_put` selects the write-path meanings of 404/409/422.
fn classify_error_status(status: u16, rate_limit_exhausted: bool, is_put: bool) -> GitHubError {
    match status {
        401 => GitHubError::Auth,
        403 if rate_limit_exhausted => GitHubError::RateLimited,
        403 => GitHubError::Auth,
        404 if is_put => GitHubError::NotFound,
        409 | 422 if is_put => GitHubError::Conflict,
        429 => GitHubError::RateLimited,
        _ => GitHubError::Api(status),
    }
}

/// Whether the response says the rate limit is spent — GitHub sends
/// `x-ratelimit-remaining` on every API response; `0` alongside a 403 is
/// the documented primary-rate-limit signal.
fn rate_limit_exhausted(response: &reqwest::blocking::Response) -> bool {
    response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim() == "0")
        .unwrap_or(false)
}

/// Standard base64 for the PUT body. GitHub accepts unwrapped (no-newline)
/// input.
fn encode_base64_content(content: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(content)
}

/// Decode the `content` field of a Contents GET response. GitHub wraps the
/// base64 at 60 columns with `\n`s, so all ASCII whitespace is stripped
/// before decoding. `None` on any invalid base64.
fn decode_base64_content(content: &str) -> Option<Vec<u8>> {
    let compact: String = content
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(compact)
        .ok()
}

#[derive(serde::Deserialize)]
struct ContentsGetResponse {
    sha: String,
    #[serde(default)]
    content: String,
}

#[derive(serde::Serialize)]
struct ContentsPutRequest<'a> {
    message: &'a str,
    content: String,
    branch: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha: Option<&'a str>,
}

#[derive(serde::Deserialize)]
struct ContentsPutResponse {
    content: ContentsPutResponseContent,
}

#[derive(serde::Deserialize)]
struct ContentsPutResponseContent {
    sha: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_holds_token_without_exposing_it() {
        // Compile-time property made explicit: ReqwestGitHub and
        // TokenHolder have no Debug/Display, so there is nothing to
        // assert-format here — constructing one must simply work.
        let _client = ReqwestGitHub::new("fake-token-abc".to_string());
    }

    #[test]
    fn base64_round_trips_bytes() {
        let payload = b"{\"snapshot\": true}\n";
        let encoded = encode_base64_content(payload);
        assert!(!encoded.contains('\n'));
        assert_eq!(decode_base64_content(&encoded).unwrap(), payload);
    }

    #[test]
    fn base64_decode_strips_githubs_newline_wrapping() {
        // GitHub returns the blob base64 wrapped at 60 columns.
        let encoded = "aGVs\nbG8g\nd29y\nbGQ=\n";
        assert_eq!(decode_base64_content(encoded).unwrap(), b"hello world");
    }

    #[test]
    fn base64_decode_rejects_invalid_input() {
        assert!(decode_base64_content("not base64 !!!").is_none());
    }

    #[test]
    fn base64_empty_content_round_trips() {
        assert_eq!(encode_base64_content(b""), "");
        assert_eq!(decode_base64_content("").unwrap(), b"");
    }

    #[test]
    fn contents_url_encodes_segments_but_preserves_path_separators() {
        let cfg = RepoRef {
            owner: "jacob".to_string(),
            repo: "bench-ledger".to_string(),
            branch: "main".to_string(),
        };
        assert_eq!(
            contents_url(&cfg, "apps/web/public/inventory.snapshot.json"),
            "https://api.github.com/repos/jacob/bench-ledger/contents/apps/web/public/inventory.snapshot.json"
        );
        // A segment containing URL-meaningful characters can't restructure
        // the request.
        assert_eq!(
            contents_url(&cfg, "dir/we ird?.json"),
            "https://api.github.com/repos/jacob/bench-ledger/contents/dir/we%20ird%3F.json"
        );
    }

    #[test]
    fn classify_error_status_maps_get_side_statuses() {
        assert!(matches!(
            classify_error_status(401, false, false),
            GitHubError::Auth
        ));
        assert!(matches!(
            classify_error_status(403, false, false),
            GitHubError::Auth
        ));
        assert!(matches!(
            classify_error_status(403, true, false),
            GitHubError::RateLimited
        ));
        assert!(matches!(
            classify_error_status(429, false, false),
            GitHubError::RateLimited
        ));
        // GET-side 409/422 have no sha-conflict meaning — surface the code.
        assert!(matches!(
            classify_error_status(409, false, false),
            GitHubError::Api(409)
        ));
        assert!(matches!(
            classify_error_status(422, false, false),
            GitHubError::Api(422)
        ));
        assert!(matches!(
            classify_error_status(500, false, false),
            GitHubError::Api(500)
        ));
    }

    #[test]
    fn classify_error_status_maps_put_side_statuses() {
        assert!(matches!(
            classify_error_status(404, false, true),
            GitHubError::NotFound
        ));
        assert!(matches!(
            classify_error_status(409, false, true),
            GitHubError::Conflict
        ));
        assert!(matches!(
            classify_error_status(422, false, true),
            GitHubError::Conflict
        ));
        assert!(matches!(
            classify_error_status(403, true, true),
            GitHubError::RateLimited
        ));
    }

    #[test]
    fn put_request_body_omits_sha_when_absent() {
        let without = serde_json::to_value(ContentsPutRequest {
            message: "Publish snapshot",
            content: encode_base64_content(b"x"),
            branch: "main",
            sha: None,
        })
        .unwrap();
        assert!(without.get("sha").is_none());

        let with = serde_json::to_value(ContentsPutRequest {
            message: "Publish snapshot",
            content: encode_base64_content(b"x"),
            branch: "main",
            sha: Some("abc123"),
        })
        .unwrap();
        assert_eq!(with["sha"], "abc123");
    }

    #[test]
    fn error_display_strings_are_fixed_and_token_free() {
        let token = "fake-token-abc";
        let errors = [
            GitHubError::Auth,
            GitHubError::NotFound,
            GitHubError::Conflict,
            GitHubError::RateLimited,
            GitHubError::Network(NETWORK_CLASSIFICATION.to_string()),
            GitHubError::Api(500),
        ];
        for err in &errors {
            let display = err.to_string();
            let debug = format!("{err:?}");
            assert!(!display.contains(token), "Display leaked: {display}");
            assert!(!debug.contains(token), "Debug leaked: {debug}");
        }
        assert_eq!(GitHubError::Auth.to_string(), "GitHub rejected the token");
        assert_eq!(
            GitHubError::Network(NETWORK_CLASSIFICATION.to_string()).to_string(),
            "network error or timeout"
        );
        assert_eq!(
            GitHubError::Api(502).to_string(),
            "GitHub API returned HTTP 502"
        );
    }
}
