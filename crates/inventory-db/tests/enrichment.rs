//! Integration tests for Phase 5c Task 5's provenance-aware enrichment
//! compare-and-apply, driven entirely through `Database`'s public API
//! (`apply_enrichment`, `enrich_part_preview`, plus the ordinary part/
//! variant/attribute methods) — the `build_diff` seam itself is
//! `pub(crate)` and gets its own hermetic unit tests inside
//! `src/enrichment.rs`. Everything here is hermetic too: no network, no
//! real DigiKey credentials, no keyring touched (every seeded part in the
//! `enrich_part_preview` test has no mpn/supplier_sku, so the DigiKey
//! provider's `identity_key` check returns `None` before it would ever
//! consult the credential store).

use inventory_core::ids::{CategoryId, PartId};
use inventory_core::quantity::QuantityUnit;
use inventory_db::enrichment::AppliedField;
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

/// Seed a part in `category` with one preferred variant (manufacturer/mpn
/// set, `datasheet_url` blank).
fn seed_part(db: &mut Database, category: &str) -> PartId {
    let draft = PartDraft {
        display_name: "10k resistor".into(),
        category_id: category_id(db, category),
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
                manufacturer: "Yageo".into(),
                mpn: "CFR-25JB-52-10K".into(),
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
    part.id
}

fn insert_provenance(db: &Database, part_id: &PartId, field_key: &str, source: &str) {
    db.raw_conn()
        .execute(
            "INSERT INTO field_provenance (id, part_id, field_key, source, confidence)
             VALUES (?1, ?2, ?3, ?4, NULL)",
            rusqlite::params![
                inventory_core::id::new_id(),
                part_id.as_str(),
                field_key,
                source
            ],
        )
        .unwrap();
}

fn provenance_source(db: &Database, part_id: &PartId, field_key: &str) -> Option<String> {
    db.raw_conn()
        .query_row(
            "SELECT source FROM field_provenance WHERE part_id = ?1 AND field_key = ?2",
            rusqlite::params![part_id.as_str(), field_key],
            |r| r.get(0),
        )
        .ok()
}

fn provenance_row_count(db: &Database, part_id: &PartId, field_key: &str) -> i64 {
    db.raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM field_provenance WHERE part_id = ?1 AND field_key = ?2",
            rusqlite::params![part_id.as_str(), field_key],
            |r| r.get(0),
        )
        .unwrap()
}

fn attr_value(db: &Database, part_id: &PartId, key: &str) -> Option<String> {
    db.get_attributes(part_id)
        .unwrap()
        .into_iter()
        .find(|(k, _, _)| k == key)
        .map(|(_, text, _)| text)
}

fn applied(key: &str, value: &str, source: &str) -> AppliedField {
    AppliedField {
        key: key.to_string(),
        value: value.to_string(),
        source: source.to_string(),
    }
}

#[test]
fn apply_datasheet_only_writes_it_and_leaves_manual_attribute_untouched() {
    let (_g, mut db) = open();
    let part_id = seed_part(&mut db, "Resistor");
    db.set_attribute(&part_id, "resistance", "10k").unwrap();
    insert_provenance(&db, &part_id, "attr.resistance", "manual");

    db.apply_enrichment(
        &part_id,
        &[applied(
            "variant.datasheet_url",
            "https://example.com/ds.pdf",
            "digikey",
        )],
    )
    .unwrap();

    let variant = db
        .list_variants(&part_id)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        variant.datasheet_url.as_deref(),
        Some("https://example.com/ds.pdf")
    );
    assert_eq!(
        provenance_source(&db, &part_id, "variant.datasheet_url").as_deref(),
        Some("digikey")
    );

    // The manually-sourced attribute was never in `applied` — untouched.
    assert_eq!(
        attr_value(&db, &part_id, "resistance").as_deref(),
        Some("10k")
    );
    assert_eq!(
        provenance_source(&db, &part_id, "attr.resistance").as_deref(),
        Some("manual")
    );
}

#[test]
fn apply_can_explicitly_overwrite_a_manual_attribute_and_upserts_provenance_not_duplicating() {
    let (_g, mut db) = open();
    let part_id = seed_part(&mut db, "Resistor");
    db.set_attribute(&part_id, "resistance", "10k").unwrap();
    insert_provenance(&db, &part_id, "attr.resistance", "manual");

    db.apply_enrichment(&part_id, &[applied("attr.resistance", "10.2k", "digikey")])
        .unwrap();

    assert_eq!(
        attr_value(&db, &part_id, "resistance").as_deref(),
        Some("10.2k")
    );
    assert_eq!(
        provenance_source(&db, &part_id, "attr.resistance").as_deref(),
        Some("digikey")
    );
    assert_eq!(provenance_row_count(&db, &part_id, "attr.resistance"), 1);

    // Apply the same key again — the UNIQUE(part_id, field_key) row is
    // updated in place, not duplicated.
    db.apply_enrichment(
        &part_id,
        &[applied("attr.resistance", "11k", "manufacturer")],
    )
    .unwrap();

    assert_eq!(
        attr_value(&db, &part_id, "resistance").as_deref(),
        Some("11k")
    );
    assert_eq!(
        provenance_source(&db, &part_id, "attr.resistance").as_deref(),
        Some("manufacturer")
    );
    assert_eq!(
        provenance_row_count(&db, &part_id, "attr.resistance"),
        1,
        "a second apply of the same key must UPSERT, not insert a duplicate row"
    );
}

#[test]
fn apply_category_resolves_a_real_name_and_skips_an_unresolvable_one_without_aborting() {
    let (_g, mut db) = open();
    let part_id = seed_part(&mut db, "Resistor");

    db.apply_enrichment(&part_id, &[applied("category", "Capacitor", "digikey")])
        .unwrap();
    let part = db.get_part(&part_id).unwrap().unwrap();
    assert_eq!(part.category_id, category_id(&db, "Capacitor"));
    assert_eq!(
        provenance_source(&db, &part_id, "category").as_deref(),
        Some("digikey")
    );

    // An unresolvable category name, bundled with a real field write in the
    // SAME apply call, must be skipped — not abort the whole transaction.
    db.apply_enrichment(
        &part_id,
        &[
            applied("category", "NoSuchCategory", "digikey"),
            applied(
                "variant.datasheet_url",
                "https://example.com/ds.pdf",
                "digikey",
            ),
        ],
    )
    .unwrap();

    let part = db.get_part(&part_id).unwrap().unwrap();
    assert_eq!(
        part.category_id,
        category_id(&db, "Capacitor"),
        "category_id must be unchanged — the bad name was skipped, not applied"
    );
    assert_eq!(
        provenance_source(&db, &part_id, "category").as_deref(),
        Some("digikey"),
        "the skipped bad-name attempt must not have touched the earlier provenance row"
    );
    let variant = db
        .list_variants(&part_id)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        variant.datasheet_url.as_deref(),
        Some("https://example.com/ds.pdf"),
        "the other field in the same apply call must still have been written"
    );
}

#[test]
fn apply_rolls_back_every_field_when_a_later_field_fails() {
    let (_g, mut db) = open();
    let part_id = seed_part(&mut db, "Resistor");

    let result = db.apply_enrichment(
        &part_id,
        &[
            applied(
                "variant.datasheet_url",
                "https://example.com/ds.pdf",
                "digikey",
            ),
            // Not a parseable resistance value — `set_attribute_in_tx`
            // rejects it, which must abort (and roll back) the whole
            // transaction, including the datasheet_url write above.
            applied("attr.resistance", "not-a-resistance-value", "digikey"),
        ],
    );
    assert!(
        matches!(result, Err(DbError::InvalidAttributeValue { .. })),
        "expected InvalidAttributeValue, got {result:?}"
    );

    let variant = db
        .list_variants(&part_id)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        variant.datasheet_url, None,
        "the earlier field's write must have been rolled back"
    );
    assert!(provenance_source(&db, &part_id, "variant.datasheet_url").is_none());
    assert!(attr_value(&db, &part_id, "resistance").is_none());
}

#[test]
fn apply_rejects_an_invalid_source_string_before_writing_anything() {
    let (_g, mut db) = open();
    let part_id = seed_part(&mut db, "Resistor");

    let result = db.apply_enrichment(
        &part_id,
        &[applied(
            "variant.datasheet_url",
            "https://example.com/ds.pdf",
            "not_a_real_source",
        )],
    );
    assert!(
        matches!(result, Err(DbError::InvalidEnrichmentSource(_))),
        "expected InvalidEnrichmentSource, got {result:?}"
    );

    let variant = db
        .list_variants(&part_id)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(variant.datasheet_url, None);
    assert!(provenance_source(&db, &part_id, "variant.datasheet_url").is_none());
}

#[test]
fn apply_sets_metadata_complete_once_manufacturer_mpn_and_datasheet_are_present() {
    let (_g, mut db) = open();
    let part_id = seed_part(&mut db, "Resistor");
    assert!(
        !db.get_part(&part_id).unwrap().unwrap().metadata_complete,
        "a fresh part with a blank datasheet_url must not start out complete"
    );

    db.apply_enrichment(
        &part_id,
        &[applied(
            "variant.datasheet_url",
            "https://example.com/ds.pdf",
            "digikey",
        )],
    )
    .unwrap();

    assert!(
        db.get_part(&part_id).unwrap().unwrap().metadata_complete,
        "manufacturer + mpn + datasheet_url all present now — metadata_complete must flip on"
    );
}

#[test]
fn enrich_part_preview_offline_yields_description_derived_candidates_without_network() {
    let (dir, mut db) = open();
    let cache_dir = dir.path().join("cache");

    // No variant at all: the DigiKey provider's `identity_key` check finds
    // no mpn/supplier_sku and returns `Ok(None)` before ever touching the
    // credential store — this test is hermetic regardless of what (if
    // anything) is in the machine's real OS keyring.
    let draft = PartDraft {
        display_name: "loose resistor".into(),
        category_id: category_id(&db, "Resistor"),
        description: "RES 10K OHM 1% 1/4W 0603".into(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    let part = db.create_part(&draft).unwrap();

    let diff = db.enrich_part_preview(&part.id, &cache_dir).unwrap();

    assert_eq!(diff.part_id, part.id);
    assert!(
        diff.provider_summary.contains(&"digikey".to_string())
            && diff.provider_summary.contains(&"description".to_string()),
        "both providers should have run: {:?}",
        diff.provider_summary
    );

    let find = |key: &str| diff.diffs.iter().find(|d| d.key == key);
    assert_eq!(find("attr.resistance").unwrap().proposed, "10K");
    assert_eq!(find("attr.tolerance").unwrap().proposed, "1%");
    assert_eq!(find("attr.power_rating").unwrap().proposed, "1/4W");
    assert_eq!(find("variant.package").unwrap().proposed, "0603");
    for key in [
        "attr.resistance",
        "attr.tolerance",
        "attr.power_rating",
        "variant.package",
    ] {
        let d = find(key).unwrap();
        assert_eq!(d.source, "inferred");
        assert!(
            !d.requires_review,
            "{key} has no current value yet — must not require review"
        );
    }
}
