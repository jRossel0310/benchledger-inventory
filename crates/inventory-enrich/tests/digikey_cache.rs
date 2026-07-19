//! Hermetic cache-hit test for the DigiKey client: pre-seed a cache file and
//! assert `enrich` returns from it with NO network call and NO credentials
//! set up (mocked or real). This is only possible because `DigiKeyClient`
//! checks the on-disk cache *before* loading credentials (see
//! `crates/inventory-enrich/src/digikey.rs`'s module docs) — otherwise this
//! test would need to touch the OS credential store just to prove a fact
//! that has nothing to do with credentials.
//!
//! No network client is ever constructed here beyond `DigiKeyClient::new`
//! (which only builds an idle `reqwest::blocking::Client`, no I/O) — if this
//! test made an HTTP request, it would hang or fail in this sandboxed,
//! network-denied test environment, so a passing run is itself evidence no
//! request was made.
//!
//! Cache files below live under `<cache_root>/digikey/sandbox/<key>.json` —
//! the cache is scoped by environment (see the module doc's "Cache is
//! scoped by environment" section), so every test here uses a
//! `DigiKeyEnv::Sandbox` client and writes under the matching `sandbox/`
//! subdirectory.

use inventory_enrich::{DigiKeyClient, DigiKeyConfig, DigiKeyEnv, EnrichInput, EnrichmentProvider};

fn fixture_json() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/digikey_v4_ne555p.json"
    ))
    .expect("fixture file present")
}

#[test]
fn enrich_returns_the_cached_response_without_network_or_credentials() {
    let cache_root = tempfile::tempdir().expect("temp dir");
    let digikey_cache_dir = cache_root.path().join("digikey").join("sandbox");
    std::fs::create_dir_all(&digikey_cache_dir).unwrap();
    // Cache key is the sanitized, uppercased MPN — see `sanitize_cache_key`.
    std::fs::write(digikey_cache_dir.join("NE555P.json"), fixture_json()).unwrap();

    let client = DigiKeyClient::new(DigiKeyConfig {
        environment: DigiKeyEnv::Sandbox,
        cache_dir: cache_root.path().to_path_buf(),
    });

    let input = EnrichInput {
        mpn: Some("NE555P".to_string()),
        ..Default::default()
    };

    let result = client.enrich(&input).expect("cache hit must not error");
    let enrichment = result.expect("a cached response must yield Some(Enrichment)");

    assert_eq!(enrichment.provider, "digikey");
    assert!(enrichment
        .candidates
        .iter()
        .any(|c| c.key == "variant.datasheet_url"));
    assert!(enrichment
        .candidates
        .iter()
        .any(|c| c.key == "variant.lifecycle" && c.value == "active"));
    assert!(enrichment.candidates.iter().any(|c| c.key == "category"));
    assert!(!enrichment.images.is_empty());
}

#[test]
fn enrich_cache_lookup_is_case_insensitive_on_the_identity_key() {
    let cache_root = tempfile::tempdir().expect("temp dir");
    let digikey_cache_dir = cache_root.path().join("digikey").join("sandbox");
    std::fs::create_dir_all(&digikey_cache_dir).unwrap();
    std::fs::write(digikey_cache_dir.join("NE555P.json"), fixture_json()).unwrap();

    let client = DigiKeyClient::new(DigiKeyConfig {
        environment: DigiKeyEnv::Sandbox,
        cache_dir: cache_root.path().to_path_buf(),
    });

    // Lowercase input MPN; the cache file was written under the sanitized
    // (uppercase) key, exercising the same normalization `enrich` applies.
    let input = EnrichInput {
        mpn: Some("ne555p".to_string()),
        ..Default::default()
    };

    let result = client.enrich(&input).expect("cache hit must not error");
    assert!(result.is_some());
}

#[test]
fn enrich_falls_back_to_supplier_sku_cache_key_when_mpn_is_absent() {
    let cache_root = tempfile::tempdir().expect("temp dir");
    let digikey_cache_dir = cache_root.path().join("digikey").join("sandbox");
    std::fs::create_dir_all(&digikey_cache_dir).unwrap();
    std::fs::write(digikey_cache_dir.join("296-1234-5-ND.json"), fixture_json()).unwrap();

    let client = DigiKeyClient::new(DigiKeyConfig {
        environment: DigiKeyEnv::Sandbox,
        cache_dir: cache_root.path().to_path_buf(),
    });

    let input = EnrichInput {
        mpn: None,
        supplier_sku: Some("296-1234-5-ND".to_string()),
        ..Default::default()
    };

    let result = client.enrich(&input).expect("cache hit must not error");
    assert!(result.is_some());
}

/// FINDING 1 (Phase 5c final review): the cache must be scoped by
/// environment. Seeds TWO distinguishable cached responses for the SAME
/// part number — one under the sandbox cache path, a different one
/// (`ProductDescription` overwritten with a sentinel) under the production
/// cache path — then asserts a production-configured client returns the
/// production-scoped response, never the sandbox one.
///
/// Deliberately does not rely on "no credentials configured" to force a
/// miss: this machine may have real DigiKey credentials already stored in
/// the OS credential store from earlier live verification (see
/// `docs/enrichment.md`'s "Live finding" note), which would make a
/// no-cache-entry version of this test perform a real network call —
/// non-hermetic, and a merge-gate test must never depend on ambient
/// developer-machine credential state or network access. Seeding a cache
/// hit for BOTH environments keeps this test hitting the cache-before-
/// credentials path (see the module doc) for both clients, so neither ever
/// reaches `load_digikey_credentials` at all.
///
/// Before the fix, `cache_path` ignored the environment entirely, so both
/// clients would read/write the same `digikey/<key>.json` file and this
/// test would observe the production client returning the sandbox
/// (first-written) fixture's `description` instead of its own.
#[test]
fn enrich_does_not_serve_a_cache_entry_written_under_a_different_environment() {
    let cache_root = tempfile::tempdir().expect("temp dir");
    let sandbox_cache_dir = cache_root.path().join("digikey").join("sandbox");
    let production_cache_dir = cache_root.path().join("digikey").join("production");
    std::fs::create_dir_all(&sandbox_cache_dir).unwrap();
    std::fs::create_dir_all(&production_cache_dir).unwrap();

    std::fs::write(sandbox_cache_dir.join("NE555P.json"), fixture_json()).unwrap();

    let mut production_fixture: serde_json::Value = serde_json::from_str(&fixture_json()).unwrap();
    production_fixture["Product"]["Description"]["ProductDescription"] =
        serde_json::Value::String("PRODUCTION-ONLY-SENTINEL".to_string());
    std::fs::write(
        production_cache_dir.join("NE555P.json"),
        serde_json::to_string_pretty(&production_fixture).unwrap(),
    )
    .unwrap();

    let production_client = DigiKeyClient::new(DigiKeyConfig {
        environment: DigiKeyEnv::Production,
        cache_dir: cache_root.path().to_path_buf(),
    });

    let input = EnrichInput {
        mpn: Some("NE555P".to_string()),
        ..Default::default()
    };

    let result = production_client
        .enrich(&input)
        .expect("cache hit must not error")
        .expect("a cached response must yield Some(Enrichment)");

    let description = result
        .candidates
        .iter()
        .find(|c| c.key == "description")
        .expect("description candidate present")
        .value
        .clone();

    assert_eq!(
        description, "PRODUCTION-ONLY-SENTINEL",
        "a production client must read the production-scoped cache entry"
    );
    assert_ne!(
        description, "IC OSC SINGLE TIMER 100KHZ 8DIP",
        "a production client must never be served the response cached under sandbox"
    );
}
