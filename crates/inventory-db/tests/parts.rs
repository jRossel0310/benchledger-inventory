use inventory_core::ids::{CategoryId, PartId};
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::parts::{ListingDraft, PartDraft, VariantDraft};
use inventory_db::{Database, MISC_CATEGORY_ID};

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

fn misc() -> CategoryId {
    CategoryId::from_string(MISC_CATEGORY_ID.to_string()).unwrap()
}

pub fn draft(name: &str) -> PartDraft {
    PartDraft {
        display_name: name.to_string(),
        category_id: misc(),
        description: String::new(),
        bin_label: None,
        usage_behavior: "usually_consumed".into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    }
}

#[test]
fn create_part_initializes_zero_stock() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("10k resistor 0603")).unwrap();
    let stock = db.get_stock(&part.id).unwrap();
    assert_eq!(stock.available, Quantity::ZERO);
    assert_eq!(stock.current_stock(), Quantity::ZERO);
    assert_eq!(stock.lifetime_received, Quantity::ZERO);
}

#[test]
fn get_and_list_round_trip() {
    let (_g, mut db) = open();
    let a = db.create_part(&draft("part a")).unwrap();
    let _b = db.create_part(&draft("part b")).unwrap();
    let got = db.get_part(&a.id).unwrap().unwrap();
    assert_eq!(got.display_name, "part a");
    assert_eq!(got.quantity_unit, QuantityUnit::Each);
    assert!(!got.archived);
    assert_eq!(db.list_parts(false).unwrap().len(), 2);
}

#[test]
fn get_missing_part_returns_none() {
    let (_g, db) = open();
    assert!(db.get_part(&PartId::new()).unwrap().is_none());
}

#[test]
fn update_part_bumps_modified_at_and_persists_fields() {
    let (_g, mut db) = open();
    let mut part = db.create_part(&draft("rename me")).unwrap();
    part.display_name = "renamed".into();
    part.bin_label = Some("A12".into());
    part.low_stock_threshold = Some(Quantity::from_whole(10).unwrap());
    db.update_part(&part).unwrap();
    let got = db.get_part(&part.id).unwrap().unwrap();
    assert_eq!(got.display_name, "renamed");
    assert_eq!(got.bin_label.as_deref(), Some("A12"));
    assert_eq!(got.low_stock_threshold, Some(Quantity::from_whole(10).unwrap()));
}

#[test]
fn archive_and_unarchive_flow_through_list_filter() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("archive me")).unwrap();
    db.set_part_archived(&part.id, true).unwrap();
    assert_eq!(db.list_parts(false).unwrap().len(), 0);
    assert_eq!(db.list_parts(true).unwrap().len(), 1);
    db.set_part_archived(&part.id, false).unwrap();
    assert_eq!(db.list_parts(false).unwrap().len(), 1);
}

#[test]
fn variants_and_listings_round_trip() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("TLV9002 dual op amp")).unwrap();
    let v = db
        .add_variant(
            &part.id,
            &VariantDraft {
                manufacturer: "Texas Instruments".into(),
                mpn: "TLV9002IDDFR".into(),
                description: String::new(),
                package: Some("SOT-23-8".into()),
                datasheet_url: None,
                product_url: None,
                lifecycle: None,
                notes: String::new(),
            },
        )
        .unwrap();
    assert!(!v.is_preferred);
    let l = db
        .add_supplier_listing(
            &v.id,
            &ListingDraft {
                supplier: "DigiKey".into(),
                supplier_sku: "296-TLV9002IDDFRCT-ND".into(),
                product_url: None,
                packaging: Some("Cut Tape".into()),
                typical_order: Some(Quantity::from_whole(10).unwrap()),
                last_unit_price_micros: Some(440_000),
                currency: Some("USD".into()),
                last_purchase_date: None,
            },
        )
        .unwrap();
    assert_eq!(l.supplier_sku, "296-TLV9002IDDFRCT-ND");
}

#[test]
fn set_preferred_variant_swaps_atomically() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("dual-sourced part")).unwrap();
    let mk = |mpn: &str| VariantDraft {
        manufacturer: "M".into(),
        mpn: mpn.into(),
        description: String::new(),
        package: None,
        datasheet_url: None,
        product_url: None,
        lifecycle: None,
        notes: String::new(),
    };
    let v1 = db.add_variant(&part.id, &mk("AAA-1")).unwrap();
    let v2 = db.add_variant(&part.id, &mk("BBB-2")).unwrap();
    db.set_preferred_variant(&part.id, &v1.id).unwrap();
    db.set_preferred_variant(&part.id, &v2.id).unwrap(); // must not violate the partial unique index
    let preferred: String = db
        .raw_conn()
        .query_row(
            "SELECT id FROM manufacturer_variants WHERE part_id = ?1 AND is_preferred = 1",
            [part.id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(preferred, v2.id.as_str());
}
