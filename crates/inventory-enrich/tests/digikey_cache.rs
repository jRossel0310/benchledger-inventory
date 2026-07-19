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
    let digikey_cache_dir = cache_root.path().join("digikey");
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
    let digikey_cache_dir = cache_root.path().join("digikey");
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
    let digikey_cache_dir = cache_root.path().join("digikey");
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
