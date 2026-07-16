//! Phase 3 Task 3: dashboard aggregate queries (`dashboard_summary`,
//! `recent_transactions`).

use inventory_core::ids::{CategoryId, PartId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::parts::PartDraft;
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

fn draft(name: &str) -> PartDraft {
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

fn q(n: i64) -> Quantity {
    Quantity::from_whole(n).unwrap()
}

fn receive(db: &mut Database, part: &PartId, n: i64) {
    db.apply(&LedgerOp::Receive {
        part_id: part.clone(),
        quantity: q(n),
        note: String::new(),
    })
    .unwrap();
}

#[test]
fn summary_is_all_zero_on_an_empty_database() {
    let (_g, db) = open();
    let summary = db.dashboard_summary().unwrap();
    assert_eq!(summary.part_count, 0);
    assert_eq!(summary.available_units, 0);
    assert_eq!(summary.reserved_units, 0);
    assert_eq!(summary.checked_out_units, 0);
    assert_eq!(summary.low_stock_count, 0);
    assert_eq!(summary.active_project_count, 0);
    assert_eq!(summary.metadata_incomplete_count, 0);
    assert_eq!(summary.unbinned_count, 0);
}

#[test]
fn summary_sums_stock_across_parts_and_counts_flagged_parts() {
    let (_g, mut db) = open();

    // Binned, plenty of stock, no threshold.
    let mut a = db.create_part(&draft("plenty resistor")).unwrap();
    a.bin_label = Some("A1".to_string());
    db.update_part(&a).unwrap();
    receive(&mut db, &a.id, 100);

    // Binned, under its low-stock threshold.
    let mut b = draft("low resistor");
    b.bin_label = Some("A2".to_string());
    b.low_stock_threshold = Some(q(50));
    let b = db.create_part(&b).unwrap();
    receive(&mut db, &b.id, 10);

    // No bin at all -> counts toward unbinned.
    let c = db.create_part(&draft("unbinned capacitor")).unwrap();
    receive(&mut db, &c.id, 5);

    let summary = db.dashboard_summary().unwrap();
    assert_eq!(summary.part_count, 3);
    assert_eq!(summary.available_units, q(100 + 10 + 5).as_milli());
    assert_eq!(summary.low_stock_count, 1);
    assert_eq!(summary.unbinned_count, 1);
    // metadata_complete defaults to false for every freshly created part.
    assert_eq!(summary.metadata_incomplete_count, 3);
}

#[test]
fn summary_excludes_archived_parts_from_every_count() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("to be archived")).unwrap();
    receive(&mut db, &part.id, 20);
    db.set_part_archived(&part.id, true).unwrap();

    let summary = db.dashboard_summary().unwrap();
    assert_eq!(summary.part_count, 0);
    assert_eq!(summary.available_units, 0);
    assert_eq!(summary.unbinned_count, 0);
    assert_eq!(summary.metadata_incomplete_count, 0);
}

#[test]
fn summary_splits_reserved_and_checked_out_units() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("split stock")).unwrap();
    receive(&mut db, &part.id, 100);
    let project = db.create_project("Test project").unwrap();
    db.apply(&LedgerOp::Reserve {
        part_id: part.id.clone(),
        quantity: q(30),
        project_id: project.clone(),
    })
    .unwrap();
    db.apply(&LedgerOp::CheckOut {
        part_id: part.id.clone(),
        quantity: q(20),
        project_id: project.clone(),
    })
    .unwrap();

    let summary = db.dashboard_summary().unwrap();
    assert_eq!(summary.available_units, q(50).as_milli());
    assert_eq!(summary.reserved_units, q(30).as_milli());
    assert_eq!(summary.checked_out_units, q(20).as_milli());
    assert_eq!(summary.active_project_count, 1);
}

#[test]
fn summary_counts_every_project_regardless_of_activity() {
    let (_g, mut db) = open();
    db.create_project("Alpha").unwrap();
    db.create_project("Beta").unwrap();
    let summary = db.dashboard_summary().unwrap();
    assert_eq!(summary.active_project_count, 2);
}

#[test]
fn recent_transactions_are_newest_first_and_respect_the_limit() {
    let (_g, mut db) = open();
    let first = db.create_part(&draft("first part")).unwrap();
    receive(&mut db, &first.id, 1);
    receive(&mut db, &first.id, 1);
    let second = db.create_part(&draft("second part")).unwrap();
    receive(&mut db, &second.id, 1);
    receive(&mut db, &second.id, 1);
    receive(&mut db, &second.id, 1);

    // Limit narrower than the full history: only the 3 most-recently
    // inserted rows come back, all from the part received into last.
    let limited = db.recent_transactions(3).unwrap();
    assert_eq!(limited.len(), 3);
    assert!(limited.iter().all(|r| r.part_id == second.id));

    // A wide-enough limit returns every row, second part's rows first
    // (newest via rowid DESC), then first part's, each internally
    // newest-first too.
    let all = db.recent_transactions(10).unwrap();
    assert_eq!(all.len(), 5);
    assert!(all[0..3].iter().all(|r| r.part_id == second.id));
    assert!(all[3..5].iter().all(|r| r.part_id == first.id));
}

#[test]
fn recent_transactions_report_display_name_type_quantity_and_unit() {
    let (_g, mut db) = open();
    let mut draft_part = draft("wire spool");
    draft_part.quantity_unit = QuantityUnit::Meter;
    let part = db.create_part(&draft_part).unwrap();
    db.apply(&LedgerOp::Receive {
        part_id: part.id.clone(),
        quantity: Quantity::from_milli(1500, QuantityUnit::Meter).unwrap(),
        note: "spool".to_string(),
    })
    .unwrap();

    let recent = db.recent_transactions(10).unwrap();
    assert_eq!(recent.len(), 1);
    let row = &recent[0];
    assert_eq!(row.part_id, part.id);
    assert_eq!(row.display_name, "wire spool");
    assert_eq!(row.txn_type, "receive");
    assert_eq!(row.quantity.as_milli(), 1500);
    assert_eq!(row.quantity_unit, QuantityUnit::Meter);
    assert!(row.group_id.is_none());
}

#[test]
fn recent_transactions_flags_a_plain_ungrouped_unreversed_row_as_reversible() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("reversible part")).unwrap();
    receive(&mut db, &part.id, 10);

    let recent = db.recent_transactions(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert!(recent[0].reversible);
}

#[test]
fn recent_transactions_flags_an_already_reversed_row_as_not_reversible() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("reversed already")).unwrap();
    let txn = db
        .apply(&LedgerOp::Receive {
            part_id: part.id.clone(),
            quantity: q(10),
            note: String::new(),
        })
        .unwrap();
    db.reverse_transaction(&txn.id, "undo").unwrap();

    let recent = db.recent_transactions(10).unwrap();
    // Two rows: the reversal itself, and the original it reversed.
    assert_eq!(recent.len(), 2);
    let original = recent.iter().find(|r| r.id == txn.id).unwrap();
    assert!(
        !original.reversible,
        "an already-reversed row must not be reversible again"
    );
    let reversal = recent.iter().find(|r| r.id != txn.id).unwrap();
    assert_eq!(reversal.txn_type, "reverse");
    assert!(
        !reversal.reversible,
        "a reversal row itself must never be reversible"
    );
}

#[test]
fn recent_transactions_flags_a_grouped_row_as_not_reversible() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("grouped part")).unwrap();
    db.apply_group(
        "receive_batch",
        "batch note",
        &[LedgerOp::Receive {
            part_id: part.id.clone(),
            quantity: q(5),
            note: String::new(),
        }],
    )
    .unwrap();

    let recent = db.recent_transactions(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert!(recent[0].group_id.is_some());
    assert!(
        !recent[0].reversible,
        "a grouped transaction must reverse as a group, not standalone"
    );
}
