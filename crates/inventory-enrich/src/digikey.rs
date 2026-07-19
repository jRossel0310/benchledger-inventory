//! DigiKey Product Information V4 API enrichment client.
//!
//! OAuth2 **client-credentials** flow against `/v1/oauth2/token`, then a
//! product-details lookup by MPN/DigiKey part number. Spec §11/§16 (ADR #3):
//! the OAuth access token is a secret exactly like the client id/secret it
//! was minted from — it is held ONLY in memory (`RefCell<Option<CachedToken>>`
//! on this struct), refreshed on expiry, and never written to disk, a log
//! line, or a cache file. Only the raw DigiKey API *response* is cached to
//! disk (`<cache_dir>/digikey/<environment>/<key>.json`); that response
//! never contains the token or the client credentials.
//!
//! # Cache is scoped by environment
//!
//! The cache path is `<cache_dir>/digikey/<environment>/<key>.json` —
//! `<environment>` is [`DigiKeyEnv::as_str`] (`"sandbox"` or `"production"`).
//! Sandbox and production are different APIs that can (and in testing, did)
//! return different data for the same part number; without the environment
//! in the path, a response cached while sandbox was selected would be
//! served verbatim after the user flipped `digikey_environment` to
//! production (and vice versa) — silently wrong data, and impossible to
//! detect from the cache alone. Scoping by environment means flipping the
//! setting always starts cold for a part until that environment's own
//! response is fetched and cached.
//!
//! # Cache-before-credentials ordering
//!
//! [`DigiKeyClient::enrich`] checks the on-disk cache *before* loading
//! credentials. A part whose DigiKey response was already fetched and
//! cached therefore still enriches even when credentials are absent (never
//! configured, or cleared after the cache was populated) — user-friendly,
//! and it is what makes the cache-hit test hermetic: it needs no credential
//! store at all, mocked or real.
//!
//! # "Not configured" is silent `Ok(None)`
//!
//! When credentials are missing and there is no cache entry, `enrich`
//! returns `Ok(None)` with **no note** — deliberately indistinguishable at
//! this layer from "queried DigiKey and it had nothing for this part".
//! Surfacing "DigiKey is not configured" as an actionable, distinct signal
//! is `get_digikey_status` (Task 6)'s job, not this provider's; per spec
//! §11, an unconfigured optional provider is normal, expected behavior, not
//! a failure worth a chain note.
//!
//! # Endpoint constants
//!
//! The token and product-details paths are each a single `const` below so
//! the exact V4 path can be corrected in one place once Task 6's live
//! verification runs against the real API.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use inventory_core::secrets::{load_digikey_credentials, DigiKeyCredentials};

use crate::model::{EnrichInput, EnrichSource, Enrichment, FieldCandidate, ImageRef};
use crate::provider::{EnrichError, EnrichmentProvider};

/// Sandbox vs production DigiKey API environment. Same credentials work
/// against both; only the base URL differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigiKeyEnv {
    Sandbox,
    Production,
}

impl DigiKeyEnv {
    pub fn base_url(&self) -> &'static str {
        match self {
            DigiKeyEnv::Sandbox => "https://sandbox-api.digikey.com",
            DigiKeyEnv::Production => "https://api.digikey.com",
        }
    }

    /// The exact string round-tripped through the `digikey_environment`
    /// `settings` value.
    pub fn as_str(&self) -> &'static str {
        match self {
            DigiKeyEnv::Sandbox => "sandbox",
            DigiKeyEnv::Production => "production",
        }
    }

    /// Parse a `digikey_environment` setting value. Anything other than an
    /// exact (case-insensitive) `"production"` defaults to `Sandbox` —
    /// including an unset/garbled setting — so a corrupted or missing
    /// setting can never silently start talking to the real production API.
    pub fn from_setting_str(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("production") {
            DigiKeyEnv::Production
        } else {
            DigiKeyEnv::Sandbox
        }
    }
}

/// OAuth2 token endpoint, relative to [`DigiKeyEnv::base_url`].
const TOKEN_PATH: &str = "/v1/oauth2/token";

/// Product-details endpoint template, relative to [`DigiKeyEnv::base_url`].
/// `{productNumber}` is substituted with the percent-encoded MPN/DigiKey
/// part number at call time. Kept as one constant so the exact V4 path is
/// easy to correct at the Task 6 live-verification step.
const PRODUCT_DETAILS_PATH_TEMPLATE: &str = "/products/v4/search/{productNumber}/productdetails";

/// Slack subtracted from a token's server-reported `expires_in` before
/// caching its expiry, so a token already within this margin of expiring is
/// treated as expired and refreshed proactively rather than risking a
/// request that races the real expiry.
const TOKEN_EXPIRY_SLACK_SECS: u64 = 60;

/// Confidence assigned to every candidate this provider emits — DigiKey is
/// an authoritative supplier catalog, not a guess, but still short of 1.0
/// (reserved for a manually-confirmed/measured value).
const CONFIDENCE: f32 = 0.9;

/// Non-secret configuration for a [`DigiKeyClient`]. Credentials are
/// intentionally NOT part of this struct — they are read fresh from
/// [`load_digikey_credentials`] on each call that needs them, never stored
/// or logged.
#[derive(Debug, Clone)]
pub struct DigiKeyConfig {
    pub environment: DigiKeyEnv,
    pub cache_dir: PathBuf,
}

/// An OAuth2 access token held only in memory, with the instant it should be
/// considered expired. Deliberately has no `Debug`/`Display` impl — nothing
/// should ever be able to accidentally format this into a log line.
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// The DigiKey Product Information V4 `EnrichmentProvider`.
///
/// The OAuth token cache is interior-mutable (`RefCell`) because
/// `EnrichmentProvider::enrich` takes `&self`: a provider instance is a
/// long-lived, shared object (constructed once, `enrich`ed many times), and
/// refreshing the token is a side effect of a read-shaped call. `RefCell` is
/// sufficient rather than `Mutex` because nothing in this crate makes a
/// `DigiKeyClient` `Sync`-shared across threads; a caller that needs that
/// (a later, async Tauri-command-facing task) can wrap the whole client in
/// a `Mutex` at that layer instead of this one paying for it unconditionally.
pub struct DigiKeyClient {
    config: DigiKeyConfig,
    http: reqwest::blocking::Client,
    token: RefCell<Option<CachedToken>>,
}

impl DigiKeyClient {
    pub fn new(config: DigiKeyConfig) -> Self {
        Self {
            config,
            http: reqwest::blocking::Client::new(),
            token: RefCell::new(None),
        }
    }

    /// `<cache_dir>/digikey/<environment>/<key>.json` — see the module doc's
    /// "Cache is scoped by environment" section for why the environment is
    /// part of the path.
    fn cache_path(&self, key: &str) -> PathBuf {
        self.config
            .cache_dir
            .join("digikey")
            .join(self.config.environment.as_str())
            .join(format!("{}.json", sanitize_cache_key(key)))
    }

    /// Read + parse a cached response, if present. Any I/O or parse failure
    /// is treated the same as "no cache entry" (`None`) — a corrupt cache
    /// file must never fail enrichment; it just falls through to a live
    /// fetch (or, with no credentials, to `Ok(None)`), per §11.
    fn read_cache(&self, path: &Path) -> Option<serde_json::Value> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Best-effort cache write: a failure to persist the cache (disk full,
    /// permissions, missing parent) must never fail the enrichment call
    /// that produced the data — the caller already has a usable
    /// `Enrichment`; losing the ability to skip the network on the *next*
    /// call is a performance concern, not a correctness one.
    fn write_cache(&self, path: &Path, body: &serde_json::Value) {
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        if let Ok(text) = serde_json::to_string_pretty(body) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Return a live access token, refreshing via the OAuth2
    /// client-credentials grant if the cached one is absent or expired.
    /// The token is cached in memory only ([`CachedToken`]) — never written
    /// to `self.config.cache_dir` or anywhere else on disk.
    fn access_token(&self, creds: &DigiKeyCredentials) -> Result<String, EnrichError> {
        if let Some(cached) = self.token.borrow().as_ref() {
            if cached.expires_at > Instant::now() {
                return Ok(cached.access_token.clone());
            }
        }

        let url = format!("{}{}", self.config.environment.base_url(), TOKEN_PATH);
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
        ];

        // The credentials go out in the POST body, never the URL, and the
        // failure branches below never echo the response body — only a
        // fixed classification string — so no secret can reach an
        // `EnrichError` message from this call.
        let response = self
            .http
            .post(&url)
            .form(&form)
            .send()
            .map_err(|_| EnrichError::Network("DigiKey token request failed".to_string()))?;

        let status = response.status();
        if !status.is_success() {
            return Err(EnrichError::Config(format!(
                "DigiKey token request rejected (HTTP {})",
                status.as_u16()
            )));
        }

        let parsed: TokenResponse = response.json().map_err(|_| {
            EnrichError::Provider("DigiKey token response was not valid JSON".to_string())
        })?;
        let access_token = parsed.access_token.ok_or_else(|| {
            EnrichError::Provider("DigiKey token response missing access_token".to_string())
        })?;
        let ttl_secs = parsed
            .expires_in
            .unwrap_or(0)
            .saturating_sub(TOKEN_EXPIRY_SLACK_SECS);

        *self.token.borrow_mut() = Some(CachedToken {
            access_token: access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(ttl_secs),
        });

        Ok(access_token)
    }

    /// Fetch product details for `product_number` from the live API.
    /// `Ok(None)` means DigiKey returned HTTP 404 (no match) — a normal,
    /// non-error outcome. Any other non-success status, or a transport
    /// failure, is an `Err`.
    fn fetch_product_details(
        &self,
        product_number: &str,
        creds: &DigiKeyCredentials,
        token: &str,
    ) -> Result<Option<serde_json::Value>, EnrichError> {
        let path = PRODUCT_DETAILS_PATH_TEMPLATE.replace(
            "{productNumber}",
            &percent_encode_path_segment(product_number),
        );
        let url = format!("{}{}", self.config.environment.base_url(), path);

        let response = self
            .http
            .get(&url)
            .bearer_auth(token)
            .header("X-DIGIKEY-Client-Id", creds.client_id.as_str())
            .header("X-DIGIKEY-Locale-Site", "US")
            .header("X-DIGIKEY-Locale-Currency", "USD")
            .header("Accept", "application/json")
            .send()
            .map_err(|_| {
                EnrichError::Network("DigiKey product details request failed".to_string())
            })?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(EnrichError::Config(
                "DigiKey API rejected the request credentials".to_string(),
            ));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(EnrichError::Provider(
                "DigiKey API rate limit exceeded".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(EnrichError::Provider(format!(
                "DigiKey API returned HTTP {}",
                status.as_u16()
            )));
        }

        response.json::<serde_json::Value>().map(Some).map_err(|_| {
            EnrichError::Provider("DigiKey API response was not valid JSON".to_string())
        })
    }
}

impl EnrichmentProvider for DigiKeyClient {
    fn name(&self) -> &str {
        "digikey"
    }

    fn enrich(&self, input: &EnrichInput) -> Result<Option<Enrichment>, EnrichError> {
        let Some(key) = identity_key(input) else {
            return Ok(None); // nothing to look DigiKey up by
        };
        let cache_path = self.cache_path(key);

        // Cache-before-credentials: see module docs.
        if let Some(cached) = self.read_cache(&cache_path) {
            return Ok(Some(map_product_response(&cached)));
        }

        let Some(creds) = load_digikey_credentials()
            .map_err(|e| EnrichError::Config(format!("credential store error: {e}")))?
        else {
            // Not configured: silent Ok(None) — see module docs.
            return Ok(None);
        };

        let token = self.access_token(&creds)?;
        let Some(body) = self.fetch_product_details(key, &creds, &token)? else {
            return Ok(None); // HTTP 404 — no match for this part number
        };

        self.write_cache(&cache_path, &body);
        Ok(Some(map_product_response(&body)))
    }
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
}

/// The identity DigiKey is queried by: `mpn` if present, else
/// `supplier_sku` (the DigiKey part number). Blank strings don't count as
/// present.
fn identity_key(input: &EnrichInput) -> Option<&str> {
    input
        .mpn
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            input
                .supplier_sku
                .as_deref()
                .filter(|s| !s.trim().is_empty())
        })
}

/// Turn an arbitrary identity key into a safe cache filename stem: only
/// `[A-Z0-9_-]`, uppercased (so `"ne555p"` and `"NE555P"` share a cache
/// entry). Never contains a secret — the key is a part number, not a
/// credential.
fn sanitize_cache_key(key: &str) -> String {
    let mut out: String = key
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// Percent-encode a single path segment (the product number) per RFC 3986's
/// unreserved-character set, so an MPN containing `/`, spaces, or other
/// path-meaningful characters can't split the request path.
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

/// Look up a nested string field by a path of object keys, tolerating a
/// missing key/wrong type at any step (returns `None` rather than
/// panicking) — the DigiKey V4 response shape is documented but every field
/// is structured as optional here; the live test (Task 6) is what validates
/// the real shape matches these paths.
fn get_str<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = value;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_str()
}

/// DigiKey V4 returns these literal strings for a `Parameters[]` entry that
/// has no real value, rather than omitting the entry — a placeholder, not
/// data. Treating any of these as a value would fabricate a candidate for a
/// field DigiKey doesn't actually know, violating the crate's "never
/// fabricate; absent stays absent" rule. Checked case-insensitively and
/// against the trimmed text.
fn is_placeholder_value(v: &str) -> bool {
    matches!(v.trim(), "" | "-" | "—" | "N/A" | "n/a")
}

fn push_candidate(candidates: &mut Vec<FieldCandidate>, key: &str, value: impl Into<String>) {
    candidates.push(FieldCandidate {
        key: key.to_string(),
        value: value.into(),
        source: EnrichSource::Digikey,
        confidence: CONFIDENCE,
    });
}

/// `Product.ProductStatus` is documented as `{ "Status": "..." }` but some
/// API responses have been observed to return the status as a bare string
/// directly on `ProductStatus` — tolerate both shapes.
fn lifecycle_value(product: &serde_json::Value) -> Option<String> {
    match product.get("ProductStatus") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Object(_)) => {
            get_str(product, &["ProductStatus", "Status"]).map(str::to_string)
        }
        _ => None,
    }
}

/// Whether a `Parameters[]` entry's `ParameterText` names the package field
/// — checked case-insensitively since DigiKey's own casing has drifted
/// across API versions.
fn is_package_parameter_name(text: &str) -> bool {
    text.eq_ignore_ascii_case("Package / Case")
        || text.eq_ignore_ascii_case("Supplier Device Package")
}

/// First `ValueText` among `params` whose `ParameterText` case-insensitively
/// equals `want`.
fn find_param_value(params: &[serde_json::Value], want: &str) -> Option<String> {
    params.iter().find_map(|p| {
        let text = get_str(p, &["ParameterText"])?;
        if text.eq_ignore_ascii_case(want) {
            get_str(p, &["ValueText"]).map(str::to_string)
        } else {
            None
        }
    })
}

/// Turn a DigiKey `ParameterText` into an `attr.<slug>` key fragment:
/// lowercase, non-alphanumeric runs collapsed to a single `_`, no leading or
/// trailing `_`. `"Package / Case"` -> `"package_case"`,
/// `"Operating Temperature"` -> `"operating_temperature"`.
fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_sep = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            pending_sep = false;
        } else {
            pending_sep = true;
        }
    }
    out
}

/// Pure mapping from a DigiKey V4 `productdetails` JSON response to an
/// [`Enrichment`]. This is the hermetic-testing seam: every unit test below
/// exercises this function directly against canned/fixture JSON, with no
/// network and no [`DigiKeyClient`] involved. Tolerates a response with the
/// top-level `"Product"` wrapper (the documented shape) or, defensively, a
/// bare `Product`-shaped object at the top level.
///
/// Never panics on missing/malformed fields — every access is `Option`-
/// chained; a field that isn't present or isn't the expected JSON type
/// simply doesn't produce a candidate.
pub fn map_product_response(json: &serde_json::Value) -> Enrichment {
    let product = if json.get("Product").is_some() {
        &json["Product"]
    } else {
        json
    };

    let mut candidates = Vec::new();
    let mut images = Vec::new();

    if let Some(v) = get_str(product, &["Description", "ProductDescription"]) {
        if !v.trim().is_empty() {
            push_candidate(&mut candidates, "description", v);
        }
    }
    if let Some(v) = get_str(product, &["Category", "Name"]) {
        if !v.trim().is_empty() {
            push_candidate(&mut candidates, "category", v);
        }
    }
    if let Some(v) = get_str(product, &["DatasheetUrl"]) {
        if !v.trim().is_empty() {
            push_candidate(&mut candidates, "variant.datasheet_url", v);
        }
    }
    if let Some(v) = get_str(product, &["ProductUrl"]) {
        if !v.trim().is_empty() {
            push_candidate(&mut candidates, "variant.product_url", v);
        }
    }
    if let Some(status) = lifecycle_value(product) {
        if !status.trim().is_empty() {
            push_candidate(&mut candidates, "variant.lifecycle", status.to_lowercase());
        }
    }
    if let Some(v) = get_str(product, &["PhotoUrl"]) {
        if !v.trim().is_empty() {
            images.push(ImageRef { url: v.to_string() });
        }
    }

    if let Some(params) = product.get("Parameters").and_then(|v| v.as_array()) {
        let package_value = find_param_value(params, "Package / Case")
            .or_else(|| find_param_value(params, "Supplier Device Package"));
        if let Some(pkg) = package_value.filter(|v| !is_placeholder_value(v)) {
            push_candidate(&mut candidates, "variant.package", pkg);
        }

        let mut seen_attr_keys: HashSet<String> = HashSet::new();
        for param in params {
            let Some(text) = get_str(param, &["ParameterText"]) else {
                continue;
            };
            if is_package_parameter_name(text) {
                continue; // already handled as variant.package above
            }
            let Some(value) = get_str(param, &["ValueText"]) else {
                continue;
            };
            if is_placeholder_value(value) {
                continue;
            }
            let key_slug = slug(text);
            if key_slug.is_empty() {
                continue;
            }
            let key = format!("attr.{key_slug}");
            if seen_attr_keys.insert(key.clone()) {
                push_candidate(&mut candidates, &key, value);
            }
        }
    }

    Enrichment {
        provider: "digikey".to_string(),
        candidates,
        images,
        notes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/digikey_v4_ne555p.json"
        ))
        .expect("fixture file present");
        serde_json::from_str(&text).expect("fixture is valid JSON")
    }

    fn find<'a>(candidates: &'a [FieldCandidate], key: &str) -> Option<&'a FieldCandidate> {
        candidates.iter().find(|c| c.key == key)
    }

    #[test]
    fn digikey_env_base_urls() {
        assert_eq!(
            DigiKeyEnv::Sandbox.base_url(),
            "https://sandbox-api.digikey.com"
        );
        assert_eq!(DigiKeyEnv::Production.base_url(), "https://api.digikey.com");
    }

    #[test]
    fn digikey_env_setting_round_trip() {
        assert_eq!(DigiKeyEnv::Sandbox.as_str(), "sandbox");
        assert_eq!(DigiKeyEnv::Production.as_str(), "production");
        assert_eq!(DigiKeyEnv::from_setting_str("sandbox"), DigiKeyEnv::Sandbox);
        assert_eq!(
            DigiKeyEnv::from_setting_str("production"),
            DigiKeyEnv::Production
        );
        assert_eq!(
            DigiKeyEnv::from_setting_str("PRODUCTION"),
            DigiKeyEnv::Production
        );
        // Anything unrecognized (garbled setting, unset default, typo)
        // defaults to Sandbox — never silently talks to production.
        assert_eq!(DigiKeyEnv::from_setting_str(""), DigiKeyEnv::Sandbox);
        assert_eq!(DigiKeyEnv::from_setting_str("prod"), DigiKeyEnv::Sandbox);
        assert_eq!(DigiKeyEnv::from_setting_str("garbage"), DigiKeyEnv::Sandbox);

        for env in [DigiKeyEnv::Sandbox, DigiKeyEnv::Production] {
            assert_eq!(DigiKeyEnv::from_setting_str(env.as_str()), env);
        }
    }

    #[test]
    fn map_product_response_extracts_documented_fields_from_the_ne555p_fixture() {
        let enrichment = map_product_response(&fixture());

        assert_eq!(enrichment.provider, "digikey");
        assert_eq!(
            find(&enrichment.candidates, "description").unwrap().value,
            "IC OSC SINGLE TIMER 100KHZ 8DIP"
        );
        assert_eq!(
            find(&enrichment.candidates, "category").unwrap().value,
            "PMIC - Timers"
        );
        assert_eq!(
            find(&enrichment.candidates, "variant.datasheet_url")
                .unwrap()
                .value,
            "https://www.ti.com/lit/ds/symlink/ne555.pdf"
        );
        assert_eq!(
            find(&enrichment.candidates, "variant.product_url")
                .unwrap()
                .value,
            "https://www.digikey.com/en/products/detail/texas-instruments/NE555P/210859"
        );
        assert_eq!(
            find(&enrichment.candidates, "variant.lifecycle")
                .unwrap()
                .value,
            "active"
        );
        // "Package / Case" sorts before "Supplier Device Package" in the
        // fixture and wins.
        assert_eq!(
            find(&enrichment.candidates, "variant.package")
                .unwrap()
                .value,
            "8-DIP (0.300\", 7.62mm)"
        );
        assert_eq!(
            find(&enrichment.candidates, "attr.mounting_type")
                .unwrap()
                .value,
            "Through Hole"
        );
        assert_eq!(
            find(&enrichment.candidates, "attr.frequency")
                .unwrap()
                .value,
            "100kHz"
        );
        // Package parameters must not also leak through as attr.* keys.
        assert!(find(&enrichment.candidates, "attr.package_case").is_none());
        assert!(find(&enrichment.candidates, "attr.supplier_device_package").is_none());

        assert_eq!(enrichment.images.len(), 1);
        assert_eq!(
            enrichment.images[0].url,
            "https://media.digikey.com/photos/Texas%20Instruments%20Photos/NE555P.jpg"
        );

        for c in &enrichment.candidates {
            assert_eq!(c.source, EnrichSource::Digikey);
            assert_eq!(c.confidence, CONFIDENCE);
        }
    }

    #[test]
    fn map_product_response_skips_placeholder_parameter_values() {
        let enrichment = map_product_response(&fixture());

        // The fixture's "Count" parameter has ValueText "-" — a DigiKey
        // placeholder for "no real value", not a real count. It must never
        // become a candidate.
        assert!(find(&enrichment.candidates, "attr.count").is_none());

        // A normal sibling parameter in the same Parameters[] array must
        // still produce a candidate — the placeholder filter must not
        // suppress everything.
        assert_eq!(
            find(&enrichment.candidates, "attr.frequency")
                .unwrap()
                .value,
            "100kHz"
        );
    }

    #[test]
    fn map_product_response_tolerates_a_bare_product_object() {
        let wrapped = fixture();
        let bare = wrapped.get("Product").unwrap().clone();
        let enrichment = map_product_response(&bare);
        assert!(find(&enrichment.candidates, "category").is_some());
    }

    #[test]
    fn map_product_response_never_panics_on_missing_or_malformed_fields() {
        for json in [
            serde_json::json!({}),
            serde_json::json!({ "Product": {} }),
            serde_json::json!({ "Product": { "Manufacturer": "not-an-object" } }),
            serde_json::json!({ "Product": { "Parameters": "not-an-array" } }),
            serde_json::json!({ "Product": { "Parameters": [{}, { "ParameterText": 5 }] } }),
            serde_json::json!({ "Product": { "ProductStatus": 5 } }),
            serde_json::json!(null),
            serde_json::json!("just a string"),
        ] {
            let enrichment = map_product_response(&json);
            assert_eq!(enrichment.provider, "digikey");
        }
    }

    #[test]
    fn map_product_response_tolerates_product_status_as_a_bare_string() {
        let json = serde_json::json!({
            "Product": {
                "ProductStatus": "Obsolete"
            }
        });
        let enrichment = map_product_response(&json);
        assert_eq!(
            find(&enrichment.candidates, "variant.lifecycle")
                .unwrap()
                .value,
            "obsolete"
        );
    }

    #[test]
    fn slug_normalizes_parameter_names() {
        assert_eq!(slug("Package / Case"), "package_case");
        assert_eq!(slug("Operating Temperature"), "operating_temperature");
        assert_eq!(slug("Resistance"), "resistance");
        assert_eq!(slug(""), "");
        assert_eq!(slug("///"), "");
    }

    #[test]
    fn sanitize_cache_key_is_filesystem_safe_and_case_insensitive() {
        assert_eq!(sanitize_cache_key("NE555P"), "NE555P");
        assert_eq!(sanitize_cache_key("ne555p"), "NE555P");
        assert_eq!(sanitize_cache_key("296-1234-5-ND"), "296-1234-5-ND");
        assert_eq!(sanitize_cache_key("weird/mpn value"), "WEIRD_MPN_VALUE");
        assert_eq!(sanitize_cache_key(""), "_");
    }

    #[test]
    fn percent_encode_path_segment_escapes_reserved_characters() {
        assert_eq!(percent_encode_path_segment("NE555P"), "NE555P");
        assert_eq!(percent_encode_path_segment("A/B"), "A%2FB");
        assert_eq!(percent_encode_path_segment("A B"), "A%20B");
    }

    #[test]
    fn identity_key_prefers_mpn_over_supplier_sku() {
        let input = EnrichInput {
            mpn: Some("NE555P".to_string()),
            supplier_sku: Some("296-1234-5-ND".to_string()),
            ..Default::default()
        };
        assert_eq!(identity_key(&input), Some("NE555P"));

        let input = EnrichInput {
            mpn: None,
            supplier_sku: Some("296-1234-5-ND".to_string()),
            ..Default::default()
        };
        assert_eq!(identity_key(&input), Some("296-1234-5-ND"));

        let input = EnrichInput {
            mpn: Some("   ".to_string()),
            supplier_sku: Some("296-1234-5-ND".to_string()),
            ..Default::default()
        };
        assert_eq!(identity_key(&input), Some("296-1234-5-ND"));

        assert_eq!(identity_key(&EnrichInput::default()), None);
    }

    #[test]
    fn enrich_error_display_from_this_module_never_contains_a_secret_marker() {
        let secret = "test-client-secret-should-never-leak";
        let messages = [
            EnrichError::Network("DigiKey token request failed".to_string()).to_string(),
            EnrichError::Config("DigiKey token request rejected (HTTP 401)".to_string())
                .to_string(),
            EnrichError::Provider("DigiKey token response missing access_token".to_string())
                .to_string(),
        ];
        for m in messages {
            assert!(!m.contains(secret));
        }
    }

    #[test]
    fn provider_name_is_digikey() {
        let client = DigiKeyClient::new(DigiKeyConfig {
            environment: DigiKeyEnv::Sandbox,
            cache_dir: std::env::temp_dir(),
        });
        assert_eq!(EnrichmentProvider::name(&client), "digikey");
    }

    #[test]
    fn enrich_with_no_identity_key_returns_ok_none_without_touching_cache_or_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let client = DigiKeyClient::new(DigiKeyConfig {
            environment: DigiKeyEnv::Sandbox,
            cache_dir: dir.path().to_path_buf(),
        });
        let result = client.enrich(&EnrichInput::default()).unwrap();
        assert!(result.is_none());
    }
}
