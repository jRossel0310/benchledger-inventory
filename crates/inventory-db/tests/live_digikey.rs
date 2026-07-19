//! LIVE DigiKey Product Information V4 verification (Phase 5c Task 6).
//!
//! `#[ignore]` by default so `cargo test` (and `scripts/verify.ps1`, which
//! never passes `--include-ignored`) stays fully hermetic — this test does
//! real network I/O against the DigiKey API and needs real credentials
//! already stored in the OS credential store (Task 1's dev bin,
//! `set_digikey_credentials`; see `inventory_core::secrets`). The
//! orchestrator runs it explicitly, after credentials are in place, via:
//!
//! ```text
//! cargo test -p inventory-db --test live_digikey -- --ignored --nocapture
//! ```
//!
//! Optionally set `DIGIKEY_LIVE_ENV=production` first to run the lookup
//! against the real production API instead of the default sandbox — same
//! credentials work against both (only the base URL differs; see
//! `DigiKeyEnv::base_url`).
//!
//! # What this proves
//!
//! End-to-end: `Database::enrich_part_preview` (Task 5) driving the real
//! `DigiKeyClient` (Task 4) against the live API for a well-known part
//! (Texas Instruments NE555P, the same MPN the offline unit-test fixture
//! `digikey_v4_ne555p.json` is modeled on) — not just the pure JSON-mapping
//! logic those hermetic tests already cover.
//!
//! # No secret ever printed
//!
//! The only credential this test touches is loaded internally by
//! `DigiKeyClient` via `inventory_core::secrets::load_digikey_credentials`
//! and never returned to this test at all — nothing here has a client
//! id/secret/token in scope to print in the first place. The `eprintln!`
//! summary below prints only non-secret enrichment output (field keys,
//! their sources, and the notes/provider-summary strings), which is exactly
//! what a UI diff screen would render.
//!
//! # Failure is loud, not silently skipped
//!
//! If credentials are missing (or the API rejects them), this test FAILS
//! with a clear message rather than passing/no-op'ing — being `#[ignore]`d
//! already keeps it out of the hermetic gate, so there's no reason for a
//! second, silent "skip if not configured" layer on top that could mask a
//! real regression when the orchestrator does run it.

use inventory_core::ids::CategoryId;
use inventory_core::quantity::QuantityUnit;
use inventory_db::parts::{PartDraft, VariantDraft};
use inventory_db::{Database, DbError};

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

fn category_id(db: &Database, name: &str) -> CategoryId {
    let raw: String = db
        .raw_conn()
        .query_row("SELECT id FROM categories WHERE name = ?1", [name], |r| {
            r.get(0)
        })
        .unwrap();
    CategoryId::from_string(raw).unwrap()
}

#[test]
#[ignore = "hits the live DigiKey API; needs real credentials in the OS credential store — see this file's module doc for the exact orchestrator command"]
fn live_digikey_enrichment_preview() {
    let (dir, mut db) = open();
    let cache_dir = dir.path().join("cache");

    // A fresh, dedicated temp cache dir (not the app's real `%APPDATA%`
    // cache) so this run always hits the live API rather than replaying a
    // previously cached response — see `DigiKeyClient`'s
    // cache-before-credentials doc comment for why a stale cache entry
    // would otherwise mask a real credential/API problem.
    let environment = std::env::var("DIGIKEY_LIVE_ENV").unwrap_or_else(|_| "sandbox".to_string());
    db.set_setting("digikey_environment", &environment).unwrap();

    // Seed a minimal part with a well-known manufacturer/MPN + category
    // hint (mirrors the offline fixture `digikey_v4_ne555p.json` this
    // crate's hermetic unit tests already exercise the JSON-mapping logic
    // against — this test is what proves the live API actually returns
    // that same shape).
    let draft = PartDraft {
        display_name: "NE555P timer IC".into(),
        category_id: category_id(&db, "Timing IC"),
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    let part = db.create_part(&draft).unwrap();
    let variant = db
        .add_variant(
            &part.id,
            &VariantDraft {
                manufacturer: "Texas Instruments".into(),
                mpn: "NE555P".into(),
                description: String::new(),
                package: None,
                datasheet_url: None,
                product_url: None,
                lifecycle: None,
                notes: String::new(),
            },
        )
        .unwrap();
    db.set_preferred_variant(&part.id, &variant.id).unwrap();

    let diff = match db.enrich_part_preview(&part.id, &cache_dir) {
        Ok(diff) => diff,
        Err(DbError::Sqlite(e)) => panic!(
            "enrich_part_preview failed at the database layer (not a DigiKey/credential \
             problem): {e}"
        ),
        Err(e) => panic!("enrich_part_preview failed: {e}"),
    };

    eprintln!(
        "live DigiKey ({environment}) providers run: {:?}",
        diff.provider_summary
    );
    if !diff.notes.is_empty() {
        eprintln!("live DigiKey ({environment}) notes: {:?}", diff.notes);
    }
    for d in &diff.diffs {
        eprintln!(
            "  {key} <- {proposed:?} (source={source}, requires_review={review})",
            key = d.key,
            proposed = d.proposed,
            source = d.source,
            review = d.requires_review,
        );
    }

    assert!(
        diff.provider_summary.contains(&"digikey".to_string()),
        "the digikey provider must have run at all — if it silently contributed nothing, \
         credentials are likely missing from the OS credential store (see this file's module \
         doc for how to store them via Task 1's dev bin); provider_summary was: {:?}",
        diff.provider_summary
    );

    let digikey_diffs: Vec<_> = diff
        .diffs
        .iter()
        .filter(|d| d.source == "digikey")
        .collect();
    assert!(
        !digikey_diffs.is_empty(),
        "expected at least one digikey-sourced candidate for a well-known part (NE555P) — \
         got none; full diff: {diff:?}"
    );

    // At least one of the fields spec §11 calls out by name (datasheet,
    // description, package, lifecycle) must have come back — a plausible
    // real-world enrichment, not just some obscure attr.* parameter.
    let has_a_headline_field = digikey_diffs.iter().any(|d| {
        matches!(
            d.key.as_str(),
            "variant.datasheet_url" | "description" | "variant.package" | "variant.lifecycle"
        )
    });
    assert!(
        has_a_headline_field,
        "expected at least one of datasheet_url/description/package/lifecycle among the \
         digikey-sourced candidates; got keys: {:?}",
        digikey_diffs.iter().map(|d| &d.key).collect::<Vec<_>>()
    );
}
