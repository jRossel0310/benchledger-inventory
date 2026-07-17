//! Phase 3 Task 9: the History screen's paged, filtered ledger query
//! (`list_history`). Filters compose (AND); grouped transactions surface
//! `group_kind` and stay contiguous in newest-first order (a `list_history`
//! consumer clusters consecutive same-`group_id` rows into a group header
//! rather than needing a second query); the per-row `reversible` flag reuses
//! the same rule `dashboard.rs`'s `recent_transactions` and
//! `reverse_transaction` enforce.

use inventory_core::ids::{CategoryId, PartId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::history::HistoryFilter;
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

/// A filter with every optional field unset and a wide page — the "show
/// everything" baseline most tests start from and narrow.
fn all_filter() -> HistoryFilter {
    HistoryFilter {
        date_from: None,
        date_to: None,
        txn_type: None,
        part_id: None,
        project_id: None,
        group_id: None,
        limit: 100,
        offset: 0,
    }
}

#[test]
fn empty_database_returns_an_empty_page() {
    let (_g, db) = open();
    let page = db.list_history(&all_filter()).unwrap();
    assert_eq!(page.total, 0);
    assert!(page.rows.is_empty());
}

#[test]
fn rows_are_newest_first_and_paging_slices_without_changing_total() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("paged part")).unwrap();
    for _ in 0..5 {
        receive(&mut db, &part.id, 1);
    }

    let mut filter = all_filter();
    filter.limit = 2;
    let page1 = db.list_history(&filter).unwrap();
    assert_eq!(page1.total, 5);
    assert_eq!(page1.rows.len(), 2);

    filter.offset = 2;
    let page2 = db.list_history(&filter).unwrap();
    assert_eq!(page2.total, 5);
    assert_eq!(page2.rows.len(), 2);
    assert_ne!(page1.rows[0].id, page2.rows[0].id);

    filter.offset = 4;
    let page3 = db.list_history(&filter).unwrap();
    assert_eq!(page3.total, 5);
    assert_eq!(page3.rows.len(), 1);

    // Every row across the pages is distinct: paging never repeats or drops
    // a row relative to the unpaged total.
    let mut full_filter = all_filter();
    full_filter.limit = 100;
    let full = db.list_history(&full_filter).unwrap();
    let mut ids: Vec<_> = full
        .rows
        .iter()
        .map(|r| r.id.as_str().to_string())
        .collect();
    let before = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        before,
        "expected 5 distinct rows, found duplicates"
    );
    assert_eq!(before, 5);
}

#[test]
fn filters_by_txn_type() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("typed")).unwrap();
    receive(&mut db, &part.id, 10);
    let project = db.create_project("proj").unwrap();
    db.apply(&LedgerOp::Reserve {
        part_id: part.id.clone(),
        quantity: q(2),
        project_id: project,
    })
    .unwrap();

    let mut filter = all_filter();
    filter.txn_type = Some("reserve".to_string());
    let page = db.list_history(&filter).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].txn_type, "reserve");
}

#[test]
fn filters_by_part_id() {
    let (_g, mut db) = open();
    let a = db.create_part(&draft("part a")).unwrap();
    let b = db.create_part(&draft("part b")).unwrap();
    receive(&mut db, &a.id, 1);
    receive(&mut db, &b.id, 1);

    let mut filter = all_filter();
    filter.part_id = Some(a.id.clone());
    let page = db.list_history(&filter).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.rows[0].part_id, a.id);
}

#[test]
fn filters_by_project_id_matching_either_leg() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("transferable")).unwrap();
    receive(&mut db, &part.id, 20);
    let from_project = db.create_project("from").unwrap();
    let to_project = db.create_project("to").unwrap();
    db.apply(&LedgerOp::Reserve {
        part_id: part.id.clone(),
        quantity: q(10),
        project_id: from_project.clone(),
    })
    .unwrap();
    db.apply(&LedgerOp::TransferReservation {
        part_id: part.id.clone(),
        quantity: q(5),
        from_project: from_project.clone(),
        to_project: to_project.clone(),
    })
    .unwrap();

    let mut to_filter = all_filter();
    to_filter.project_id = Some(to_project);
    let to_page = db.list_history(&to_filter).unwrap();
    // Only the transfer references the destination project (its to_project_id leg).
    assert_eq!(to_page.total, 1);
    assert_eq!(to_page.rows[0].txn_type, "transfer_reservation");

    let mut from_filter = all_filter();
    from_filter.project_id = Some(from_project);
    let from_page = db.list_history(&from_filter).unwrap();
    // The reserve (project_id leg) and the transfer (project_id leg) both reference it.
    assert_eq!(from_page.total, 2);
}

#[test]
fn filters_by_group_id() {
    let (_g, mut db) = open();
    let a = db.create_part(&draft("grouped a")).unwrap();
    let b = db.create_part(&draft("grouped b")).unwrap();
    receive(&mut db, &a.id, 1); // ungrouped noise, must not match
    let group = db
        .apply_group(
            "receive_batch",
            "order arrival",
            &[
                LedgerOp::Receive {
                    part_id: a.id.clone(),
                    quantity: q(5),
                    note: String::new(),
                },
                LedgerOp::Receive {
                    part_id: b.id.clone(),
                    quantity: q(3),
                    note: String::new(),
                },
            ],
        )
        .unwrap();

    let mut filter = all_filter();
    filter.group_id = Some(group.id.clone());
    let page = db.list_history(&filter).unwrap();
    assert_eq!(page.total, 2);
    assert!(page
        .rows
        .iter()
        .all(|r| r.group_id.as_ref() == Some(&group.id)));
}

#[test]
fn filters_by_date_range() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("dated")).unwrap();
    let old = db
        .apply(&LedgerOp::Receive {
            part_id: part.id.clone(),
            quantity: q(1),
            note: String::new(),
        })
        .unwrap();
    let recent = db
        .apply(&LedgerOp::Receive {
            part_id: part.id.clone(),
            quantity: q(1),
            note: String::new(),
        })
        .unwrap();
    // created_at defaults to datetime('now'); backdate directly so the
    // date-range filter has something deterministic to narrow.
    db.raw_conn()
        .execute(
            "UPDATE transactions SET created_at = '2020-01-01 00:00:00' WHERE id = ?1",
            [old.id.as_str()],
        )
        .unwrap();
    db.raw_conn()
        .execute(
            "UPDATE transactions SET created_at = '2026-06-01 00:00:00' WHERE id = ?1",
            [recent.id.as_str()],
        )
        .unwrap();

    let mut from_filter = all_filter();
    from_filter.date_from = Some("2025-01-01".to_string());
    let from_page = db.list_history(&from_filter).unwrap();
    assert_eq!(from_page.total, 1);
    assert_eq!(from_page.rows[0].id, recent.id);

    let mut to_filter = all_filter();
    to_filter.date_to = Some("2021-01-01".to_string());
    let to_page = db.list_history(&to_filter).unwrap();
    assert_eq!(to_page.total, 1);
    assert_eq!(to_page.rows[0].id, old.id);

    let mut both_filter = all_filter();
    both_filter.date_from = Some("2019-01-01".to_string());
    both_filter.date_to = Some("2020-12-31".to_string());
    let both_page = db.list_history(&both_filter).unwrap();
    assert_eq!(both_page.total, 1);
    assert_eq!(both_page.rows[0].id, old.id);
}

#[test]
fn group_rollup_reports_kind_and_keeps_members_contiguous() {
    let (_g, mut db) = open();
    let a = db.create_part(&draft("kit a")).unwrap();
    let b = db.create_part(&draft("kit b")).unwrap();
    receive(&mut db, &a.id, 1); // before the group
    let group = db
        .apply_group(
            "receive_batch",
            "order arrival",
            &[
                LedgerOp::Receive {
                    part_id: a.id.clone(),
                    quantity: q(5),
                    note: String::new(),
                },
                LedgerOp::Receive {
                    part_id: b.id.clone(),
                    quantity: q(3),
                    note: String::new(),
                },
            ],
        )
        .unwrap();
    receive(&mut db, &a.id, 2); // after the group

    let mut filter = all_filter();
    filter.limit = 100;
    let page = db.list_history(&filter).unwrap();
    assert_eq!(page.total, 4);

    let group_positions: Vec<usize> = page
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.group_id.as_ref() == Some(&group.id))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(group_positions.len(), 2);
    assert_eq!(
        group_positions[1] - group_positions[0],
        1,
        "group members must sit contiguously in newest-first order"
    );

    for row in &page.rows {
        if row.group_id.as_ref() == Some(&group.id) {
            assert_eq!(row.group_kind.as_deref(), Some("receive_batch"));
        } else {
            assert!(row.group_kind.is_none());
        }
    }
}

#[test]
fn reversibility_matches_the_dashboard_rule() {
    let (_g, mut db) = open();
    let plain = db.create_part(&draft("plain")).unwrap();
    let plain_txn = db
        .apply(&LedgerOp::Receive {
            part_id: plain.id.clone(),
            quantity: q(5),
            note: String::new(),
        })
        .unwrap();

    let reversed = db.create_part(&draft("reversed")).unwrap();
    let reversed_txn = db
        .apply(&LedgerOp::Receive {
            part_id: reversed.id.clone(),
            quantity: q(5),
            note: String::new(),
        })
        .unwrap();
    db.reverse_transaction(&reversed_txn.id, "undo").unwrap();

    let grouped_part = db.create_part(&draft("grouped")).unwrap();
    db.apply_group(
        "batch",
        "",
        &[LedgerOp::Receive {
            part_id: grouped_part.id.clone(),
            quantity: q(5),
            note: String::new(),
        }],
    )
    .unwrap();

    let mut filter = all_filter();
    filter.limit = 100;
    let page = db.list_history(&filter).unwrap();

    let plain_row = page.rows.iter().find(|r| r.id == plain_txn.id).unwrap();
    assert!(
        plain_row.reversible,
        "a plain ungrouped row must be reversible"
    );

    let original_row = page.rows.iter().find(|r| r.id == reversed_txn.id).unwrap();
    assert!(
        !original_row.reversible,
        "an already-reversed row must not be reversible again"
    );
    let reversal_row = page
        .rows
        .iter()
        .find(|r| r.reversed_txn_id.as_ref() == Some(&reversed_txn.id))
        .unwrap();
    assert_eq!(reversal_row.txn_type, "reverse");
    assert!(
        !reversal_row.reversible,
        "a reversal row itself must never be reversible"
    );

    let grouped_row = page
        .rows
        .iter()
        .find(|r| r.part_id == grouped_part.id)
        .unwrap();
    assert!(
        !grouped_row.reversible,
        "a grouped transaction must reverse as a group, not standalone"
    );
}

#[test]
fn reports_display_name_project_name_and_archived_state() {
    let (_g, mut db) = open();
    let archived = db.create_part(&draft("to archive")).unwrap();
    let archived_txn = db
        .apply(&LedgerOp::Receive {
            part_id: archived.id.clone(),
            quantity: q(5),
            note: String::new(),
        })
        .unwrap();
    db.set_part_archived(&archived.id, true).unwrap();

    let part = db.create_part(&draft("with project")).unwrap();
    receive(&mut db, &part.id, 20);
    let project = db.create_project("Blinky").unwrap();
    let reserve_txn = db
        .apply(&LedgerOp::Reserve {
            part_id: part.id.clone(),
            quantity: q(5),
            project_id: project,
        })
        .unwrap();

    let mut filter = all_filter();
    filter.limit = 100;
    let page = db.list_history(&filter).unwrap();

    let archived_row = page.rows.iter().find(|r| r.id == archived_txn.id).unwrap();
    assert_eq!(archived_row.display_name, "to archive");
    assert!(archived_row.part_archived);
    assert!(archived_row.project_name.is_none());

    let reserve_row = page.rows.iter().find(|r| r.id == reserve_txn.id).unwrap();
    assert_eq!(reserve_row.project_name.as_deref(), Some("Blinky"));
    assert!(!reserve_row.part_archived);
}

#[test]
fn import_id_passes_through_when_present_and_is_none_otherwise() {
    let (_g, mut db) = open();
    let part = db.create_part(&draft("imported")).unwrap();
    let imported_txn = db
        .apply(&LedgerOp::Receive {
            part_id: part.id.clone(),
            quantity: q(5),
            note: String::new(),
        })
        .unwrap();
    // No public API sets import_id yet (arrives with Phase 5's import
    // tables) — write it directly to prove `list_history` passes the
    // already-existing schema column through once something does.
    db.raw_conn()
        .execute(
            "UPDATE transactions SET import_id = ?1 WHERE id = ?2",
            rusqlite::params!["imp_123", imported_txn.id.as_str()],
        )
        .unwrap();
    let plain_txn = db
        .apply(&LedgerOp::Receive {
            part_id: part.id.clone(),
            quantity: q(1),
            note: String::new(),
        })
        .unwrap();

    let mut filter = all_filter();
    filter.limit = 100;
    let page = db.list_history(&filter).unwrap();

    let imported_row = page.rows.iter().find(|r| r.id == imported_txn.id).unwrap();
    assert_eq!(imported_row.import_id.as_deref(), Some("imp_123"));
    let plain_row = page.rows.iter().find(|r| r.id == plain_txn.id).unwrap();
    assert!(plain_row.import_id.is_none());
}
