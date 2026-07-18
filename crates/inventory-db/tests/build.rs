//! Reserve-BOM and atomic build-from-BOM (Phase 4 Task 4) — the core of
//! Phase 4. `reserve_bom`/`release_bom_reservations`/`build_from_bom` all
//! reuse the Phase 2a `apply_group` atomic-transaction-group machinery
//! (crates/inventory-db/src/ledger.rs); no parallel transaction path.

use inventory_core::ids::{CategoryId, PartId, ProjectId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};
use inventory_db::bom::BomItemDraft;
use inventory_db::parts::PartDraft;
use inventory_db::projects::{ProjectDraft, ProjectStatus};
use inventory_db::{Database, DbError, MISC_CATEGORY_ID};

fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("b");
    std::fs::create_dir_all(&backups).unwrap();
    let db = Database::open_and_migrate(&dir.path().join("t.sqlite"), &backups).unwrap();
    (dir, db)
}

fn make_part_with_behavior(db: &mut Database, name: &str, usage_behavior: &str) -> PartId {
    let draft = PartDraft {
        display_name: name.to_string(),
        category_id: CategoryId::from_string(MISC_CATEGORY_ID.into()).unwrap(),
        description: String::new(),
        bin_label: None,
        usage_behavior: usage_behavior.into(),
        quantity_unit: QuantityUnit::Each,
        low_stock_threshold: None,
        public_notes: String::new(),
        private_notes: String::new(),
    };
    db.create_part(&draft).unwrap().id
}

fn make_part(db: &mut Database, name: &str) -> PartId {
    make_part_with_behavior(db, name, "usually_consumed")
}

fn make_reusable_part(db: &mut Database, name: &str) -> PartId {
    make_part_with_behavior(db, name, "usually_checked_out")
}

fn make_project(db: &mut Database, name: &str, build_quantity: i64) -> ProjectId {
    let draft = ProjectDraft {
        name: name.to_string(),
        description: String::new(),
        build_quantity,
        repo_link: None,
        notes: String::new(),
    };
    db.create_project_full(&draft).unwrap().id
}

fn q(n: i64) -> Quantity {
    Quantity::from_whole(n).unwrap()
}

fn item_draft(part_id: &PartId, qty_per_build: i64) -> BomItemDraft {
    BomItemDraft {
        part_id: part_id.clone(),
        quantity_per_build: q(qty_per_build),
        reference_designators: "R1".into(),
        required: true,
        notes: String::new(),
    }
}

fn receive(db: &mut Database, part: &PartId, n: i64) {
    db.apply(&LedgerOp::Receive {
        part_id: part.clone(),
        quantity: q(n),
        note: String::new(),
    })
    .unwrap();
}

// --- reserve_bom ------------------------------------------------------

#[test]
fn reserve_bom_reserves_min_of_needed_and_available_per_line() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reserve Test", 1);
    let part_a = make_part(&mut db, "part a"); // total_required 5, available 10 -> fully covered
    let part_b = make_part(&mut db, "part b"); // total_required 10, available 4 -> partial
    db.add_bom_item(&project, &item_draft(&part_a, 5)).unwrap();
    db.add_bom_item(&project, &item_draft(&part_b, 10)).unwrap();
    receive(&mut db, &part_a, 10);
    receive(&mut db, &part_b, 4);

    let group = db.reserve_bom(&project).unwrap();
    assert_eq!(group.kind, "reserve_bom");
    assert_eq!(group.transactions.len(), 2);
    assert!(group
        .transactions
        .iter()
        .all(|t| t.txn_type == "reserve" && t.project_id == Some(project.clone())));

    let list = db.list_bom(&project).unwrap();
    let a = list.iter().find(|i| i.part_id == part_a).unwrap();
    let b = list.iter().find(|i| i.part_id == part_b).unwrap();
    assert_eq!(a.reserved, q(5), "fully reserved, capped at total_required");
    assert_eq!(a.available, q(5), "10 received - 5 reserved");
    assert_eq!(
        b.reserved,
        q(4),
        "partial: only 4 of 10 needed were available"
    );
    assert_eq!(b.available, Quantity::ZERO);
}

#[test]
fn reserve_bom_reserves_nothing_more_once_a_line_is_fully_reserved() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Already Reserved", 1);
    let part = make_part(&mut db, "fully reserved part");
    db.add_bom_item(&project, &item_draft(&part, 5)).unwrap();
    receive(&mut db, &part, 10);
    db.reserve_bom(&project).unwrap(); // reserves 5, leaves 0 needed

    let err = db.reserve_bom(&project).unwrap_err();
    assert!(
        matches!(err, DbError::EmptyGroup),
        "nothing left to reserve; got {err:?}"
    );
}

#[test]
fn reserve_bom_ignores_optional_lines() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Optional Skip", 1);
    let part = make_part(&mut db, "optional part");
    let mut draft = item_draft(&part, 5);
    draft.required = false;
    db.add_bom_item(&project, &draft).unwrap();
    receive(&mut db, &part, 5);

    let err = db.reserve_bom(&project).unwrap_err();
    assert!(matches!(err, DbError::EmptyGroup));
    assert_eq!(db.list_bom(&project).unwrap()[0].reserved, Quantity::ZERO);
}

#[test]
fn reserve_bom_skips_reusable_parts() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reusable Skip", 1);
    let tool = make_reusable_part(&mut db, "oscilloscope");
    db.add_bom_item(&project, &item_draft(&tool, 1)).unwrap();
    receive(&mut db, &tool, 2);

    let err = db.reserve_bom(&project).unwrap_err();
    assert!(
        matches!(err, DbError::EmptyGroup),
        "reusable parts are checked out at build time, not pre-reserved"
    );
}

#[test]
fn reserve_bom_with_no_bom_items_is_empty_group() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "No Lines", 1);
    let err = db.reserve_bom(&project).unwrap_err();
    assert!(matches!(err, DbError::EmptyGroup));
}

#[test]
fn reserve_bom_rejects_missing_project() {
    let (_g, mut db) = open();
    let err = db.reserve_bom(&ProjectId::new()).unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}

// --- release_bom_reservations -------------------------------------------

#[test]
fn release_bom_reservations_releases_everything_reserved_for_the_project() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Release Test", 1);
    let part = make_part(&mut db, "release part");
    db.add_bom_item(&project, &item_draft(&part, 5)).unwrap();
    receive(&mut db, &part, 5);
    db.reserve_bom(&project).unwrap();
    assert_eq!(db.list_bom(&project).unwrap()[0].reserved, q(5));

    let group = db.release_bom_reservations(&project).unwrap();
    assert_eq!(group.kind, "release_bom");
    assert_eq!(group.transactions.len(), 1);
    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].reserved, Quantity::ZERO);
    assert_eq!(list[0].available, q(5));

    // reserving again after release works (needed is recomputed fresh)
    db.reserve_bom(&project).unwrap();
    assert_eq!(db.list_bom(&project).unwrap()[0].reserved, q(5));
}

#[test]
fn release_bom_reservations_with_nothing_reserved_is_empty_group() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Nothing Reserved", 1);
    let part = make_part(&mut db, "unreserved part");
    db.add_bom_item(&project, &item_draft(&part, 5)).unwrap();
    let err = db.release_bom_reservations(&project).unwrap_err();
    assert!(matches!(err, DbError::EmptyGroup));
}

// --- plan_build (dry run) -------------------------------------------------

#[test]
fn plan_build_computes_reserved_available_needed_and_missing_without_mutating() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Plan Test", 1);
    let part = make_part(&mut db, "plan part");
    let added = db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 6);
    db.reserve_bom(&project).unwrap(); // reserves min(10, 6) = 6
    receive(&mut db, &part, 3); // available now 3; remaining need after reserved is 4

    let stock_before = db.get_stock(&part).unwrap();

    let plan = db.plan_build(&project).unwrap();
    assert_eq!(plan.lines.len(), 1);
    let line = &plan.lines[0];
    assert_eq!(line.bom_item_id, added.id);
    assert_eq!(line.part_id, part);
    assert!(line.required);
    assert!(!line.is_reusable);
    assert_eq!(line.reserved_qty, q(6));
    assert_eq!(line.available_needed, q(4), "10 required - 6 reserved");
    assert_eq!(line.checkout_qty, Quantity::ZERO);
    assert_eq!(
        line.missing,
        q(1),
        "4 needed from available but only 3 available"
    );

    let stock_after = db.get_stock(&part).unwrap();
    assert_eq!(
        stock_before.available, stock_after.available,
        "plan_build must not mutate stock"
    );
    assert_eq!(stock_before.reserved, stock_after.reserved);
}

#[test]
fn plan_build_reusable_line_reports_checkout_qty() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Plan Reusable", 1);
    let tool = make_reusable_part(&mut db, "hot air station");
    let added = db.add_bom_item(&project, &item_draft(&tool, 1)).unwrap();
    receive(&mut db, &tool, 2);

    let plan = db.plan_build(&project).unwrap();
    let line = &plan.lines[0];
    assert_eq!(line.bom_item_id, added.id);
    assert!(line.is_reusable);
    assert_eq!(line.reserved_qty, Quantity::ZERO);
    assert_eq!(line.checkout_qty, q(1));
    assert_eq!(line.missing, Quantity::ZERO);
}

// --- build_from_bom happy path -------------------------------------------

#[test]
fn build_from_bom_happy_path_consumes_reserved_in_one_group() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Happy Build", 1);
    let part_a = make_part(&mut db, "resistor");
    let part_b = make_part(&mut db, "capacitor");
    db.add_bom_item(&project, &item_draft(&part_a, 5)).unwrap();
    db.add_bom_item(&project, &item_draft(&part_b, 3)).unwrap();
    receive(&mut db, &part_a, 5);
    receive(&mut db, &part_b, 3);
    db.reserve_bom(&project).unwrap(); // fully reserves both

    let group = db.build_from_bom(&project, &[]).unwrap();
    assert_eq!(group.kind, "build_from_bom");
    assert_eq!(group.transactions.len(), 2);
    assert!(group
        .transactions
        .iter()
        .all(|t| t.txn_type == "consume_reserved"));

    let stock_a = db.get_stock(&part_a).unwrap();
    assert_eq!(stock_a.reserved, Quantity::ZERO);
    assert_eq!(stock_a.lifetime_consumed, q(5));
    let stock_b = db.get_stock(&part_b).unwrap();
    assert_eq!(stock_b.reserved, Quantity::ZERO);
    assert_eq!(stock_b.lifetime_consumed, q(3));

    let list = db.list_bom(&project).unwrap();
    for item in &list {
        assert_eq!(item.reserved, Quantity::ZERO);
    }
    let a_line = list.iter().find(|i| i.part_id == part_a).unwrap();
    assert_eq!(a_line.consumed, q(5));

    // the build is retrievable as a group in the ledger
    let fetched = db.get_group(&group.id).unwrap().unwrap();
    assert_eq!(fetched.kind, "build_from_bom");
    assert_eq!(fetched.transactions.len(), 2);
}

#[test]
fn build_from_bom_with_no_bom_items_is_empty_group() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "No Lines Build", 1);
    let err = db.build_from_bom(&project, &[]).unwrap_err();
    assert!(matches!(err, DbError::EmptyGroup));
}

// --- build_from_bom with approved available-draw -------------------------

#[test]
fn build_from_bom_with_approved_available_line_consumes_reserved_plus_available() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Approved Draw", 1);
    let part = make_part(&mut db, "short part");
    let added = db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 6);
    db.reserve_bom(&project).unwrap(); // reserves 6
    receive(&mut db, &part, 4); // exactly covers the remaining 4

    let group = db.build_from_bom(&project, &[added.id.clone()]).unwrap();
    assert_eq!(
        group.transactions.len(),
        2,
        "consume_reserved + consume_available"
    );
    let types: Vec<&str> = group
        .transactions
        .iter()
        .map(|t| t.txn_type.as_str())
        .collect();
    assert!(types.contains(&"consume_reserved"));
    assert!(types.contains(&"consume_available"));

    let stock = db.get_stock(&part).unwrap();
    assert_eq!(stock.reserved, Quantity::ZERO);
    assert_eq!(stock.available, Quantity::ZERO);
    assert_eq!(stock.lifetime_consumed, q(10));

    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].consumed, q(10));
    // `missing` (Task 3, crates/inventory-db/src/bom.rs) is
    // `total_required - available - reserved`: it reflects currently-held
    // stock, not build history, so a fully-built line (everything consumed,
    // nothing held) reads as fully "missing" again -- accurate to that
    // already-tested derivation, not a build.rs regression.
    assert_eq!(list[0].missing, q(10));
}

#[test]
fn build_from_bom_does_not_draw_from_available_unless_approved() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Unapproved Draw", 1);
    let part = make_part(&mut db, "unapproved part");
    db.add_bom_item(&project, &item_draft(&part, 10)).unwrap();
    receive(&mut db, &part, 6);
    db.reserve_bom(&project).unwrap();
    receive(&mut db, &part, 4);

    let group = db.build_from_bom(&project, &[]).unwrap();
    assert_eq!(group.transactions.len(), 1);
    assert_eq!(group.transactions[0].txn_type, "consume_reserved");

    let stock = db.get_stock(&part).unwrap();
    assert_eq!(
        stock.available,
        q(4),
        "unapproved available draw must be left alone"
    );
}

// --- ATOMICITY: the key test ----------------------------------------------

#[test]
fn build_from_bom_with_insufficient_stock_on_an_approved_line_rolls_back_the_whole_group() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Atomic Build", 1);
    let part_ok = make_part(&mut db, "will succeed");
    let part_short = make_part(&mut db, "will fail");
    db.add_bom_item(&project, &item_draft(&part_ok, 5)).unwrap();
    let short_item = db
        .add_bom_item(&project, &item_draft(&part_short, 10))
        .unwrap();
    receive(&mut db, &part_ok, 5);
    receive(&mut db, &part_short, 3);
    db.apply(&LedgerOp::Reserve {
        part_id: part_short.clone(),
        quantity: q(3),
        project_id: project.clone(),
    })
    .unwrap();
    db.apply(&LedgerOp::Reserve {
        part_id: part_ok.clone(),
        quantity: q(5),
        project_id: project.clone(),
    })
    .unwrap();
    // part_short: total_required=10, reserved=3, real available=0 (3 received,
    // all 3 reserved) -> plan's available_needed is the *full* remaining 7
    // (not clamped to real available -- the CHECK constraint is the
    // enforcement mechanism here, exactly like a bare apply_group). Approving
    // this line makes build_from_bom attempt to consume 7 available when only
    // 0 remains.

    let err = db
        .build_from_bom(&project, &[short_item.id.clone()])
        .unwrap_err();
    assert!(matches!(err, DbError::InsufficientStock(_)), "got {err:?}");

    // nothing committed: part_ok's already-successful-looking op must have
    // been rolled back along with the failing one.
    let stock_ok = db.get_stock(&part_ok).unwrap();
    assert_eq!(
        stock_ok.reserved,
        q(5),
        "the whole group must roll back, not just the failing op"
    );
    assert_eq!(stock_ok.lifetime_consumed, Quantity::ZERO);

    let stock_short = db.get_stock(&part_short).unwrap();
    assert_eq!(stock_short.reserved, q(3));
    assert_eq!(stock_short.available, Quantity::ZERO);
    assert_eq!(stock_short.lifetime_consumed, Quantity::ZERO);

    // no group row was created for the failed build
    let group_count: i64 = db
        .raw_conn()
        .query_row(
            "SELECT COUNT(*) FROM transaction_groups WHERE kind = 'build_from_bom'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(group_count, 0);

    // a failed build must not auto-activate the project either
    let project_after = db.get_project(&project).unwrap().unwrap();
    assert_eq!(project_after.status, ProjectStatus::Planned);
}

// --- reusable checkout -----------------------------------------------------

#[test]
fn build_from_bom_checks_out_reusable_parts_instead_of_consuming() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reusable Build", 1);
    let tool = make_reusable_part(&mut db, "oscilloscope");
    let added = db.add_bom_item(&project, &item_draft(&tool, 1)).unwrap();
    receive(&mut db, &tool, 2);

    let plan = db.plan_build(&project).unwrap();
    assert_eq!(plan.lines[0].bom_item_id, added.id);

    let group = db.build_from_bom(&project, &[]).unwrap();
    assert_eq!(group.transactions.len(), 1);
    assert_eq!(group.transactions[0].txn_type, "check_out");

    let stock = db.get_stock(&tool).unwrap();
    assert_eq!(stock.checked_out, q(1));
    assert_eq!(stock.available, q(1), "2 received - 1 checked out");
    assert_eq!(
        stock.lifetime_consumed,
        Quantity::ZERO,
        "reusable items are checked out, not consumed"
    );

    let list = db.list_bom(&project).unwrap();
    assert_eq!(
        list[0].consumed,
        Quantity::ZERO,
        "checkout is not a consume-type op"
    );
}

// --- reversal ---------------------------------------------------------

#[test]
fn reversing_the_build_group_restores_all_stock() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Reverse Build", 1);
    let part = make_part(&mut db, "reversible part");
    db.add_bom_item(&project, &item_draft(&part, 5)).unwrap();
    receive(&mut db, &part, 5);
    db.reserve_bom(&project).unwrap();
    let group = db.build_from_bom(&project, &[]).unwrap();

    let stock_after_build = db.get_stock(&part).unwrap();
    assert_eq!(stock_after_build.lifetime_consumed, q(5));

    let reversal = db.reverse_group(&group.id, "undo build").unwrap();
    assert_eq!(reversal.transactions.len(), group.transactions.len());

    let stock_after_reverse = db.get_stock(&part).unwrap();
    assert_eq!(
        stock_after_reverse.reserved,
        q(5),
        "reversing consume_reserved restores the reservation"
    );
    assert_eq!(stock_after_reverse.available, Quantity::ZERO);
    assert_eq!(stock_after_reverse.lifetime_consumed, Quantity::ZERO);
}

// --- auto-activation --------------------------------------------------

#[test]
fn build_from_bom_auto_activates_a_planned_project() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Auto Activate", 1);
    let part = make_part(&mut db, "activator part");
    db.add_bom_item(&project, &item_draft(&part, 2)).unwrap();
    receive(&mut db, &part, 2);
    db.reserve_bom(&project).unwrap();

    let before = db.get_project(&project).unwrap().unwrap();
    assert_eq!(before.status, ProjectStatus::Planned);

    db.build_from_bom(&project, &[]).unwrap();

    let after = db.get_project(&project).unwrap().unwrap();
    assert_eq!(after.status, ProjectStatus::Active);
}

#[test]
fn build_from_bom_does_not_touch_status_of_a_non_planned_project() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Already Completed", 1);
    db.set_project_status(&project, ProjectStatus::Completed)
        .unwrap();
    let part = make_part(&mut db, "completed project part");
    db.add_bom_item(&project, &item_draft(&part, 2)).unwrap();
    receive(&mut db, &part, 2);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(2),
        project_id: project.clone(),
    })
    .unwrap();

    db.build_from_bom(&project, &[]).unwrap();
    let after = db.get_project(&project).unwrap().unwrap();
    assert_eq!(
        after.status,
        ProjectStatus::Completed,
        "build must not override a non-planned status"
    );
}

// --- optional lines: informational only, not a gate on ops ---------------

#[test]
fn build_from_bom_still_consumes_reserved_stock_on_an_optional_line() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Optional Line", 1);
    let part = make_part(&mut db, "optional part");
    let mut draft = item_draft(&part, 5);
    draft.required = false;
    let added = db.add_bom_item(&project, &draft).unwrap();
    assert!(!added.required);
    receive(&mut db, &part, 2);
    db.apply(&LedgerOp::Reserve {
        part_id: part.clone(),
        quantity: q(2),
        project_id: project.clone(),
    })
    .unwrap();

    // Do NOT approve this line's available-draw: it has none available
    // anyway (all 2 received were reserved), so approving it would attempt
    // to consume the full unclamped shortfall (3) against 0 real available
    // and fail the group -- exactly the insufficient-stock case the
    // atomicity test exercises. Here we're only checking that an optional
    // line's already-reserved portion still gets consumed.
    let group = db.build_from_bom(&project, &[]).unwrap();
    assert_eq!(
        group.transactions.len(),
        1,
        "consume_reserved for the 2 that were reserved"
    );
    let list = db.list_bom(&project).unwrap();
    assert_eq!(list[0].consumed, q(2));
    // `missing` (Task 3) reflects currently-held stock only -- see the note
    // in the sibling "approved available line" test above.
    assert_eq!(list[0].missing, q(5));
}

// --- associate_checkout ----------------------------------------------------

#[test]
fn associate_checkout_checks_out_a_part_to_a_project() {
    let (_g, mut db) = open();
    let project = make_project(&mut db, "Ad Hoc Checkout", 1);
    let tool = make_reusable_part(&mut db, "multimeter");
    receive(&mut db, &tool, 3);

    let txn = db.associate_checkout(&project, &tool, q(1)).unwrap();
    assert_eq!(txn.txn_type, "check_out");
    assert_eq!(txn.project_id, Some(project.clone()));

    let stock = db.get_stock(&tool).unwrap();
    assert_eq!(stock.checked_out, q(1));
    assert_eq!(stock.available, q(2));
}

#[test]
fn associate_checkout_rejects_missing_project() {
    let (_g, mut db) = open();
    let tool = make_reusable_part(&mut db, "lonely tool");
    receive(&mut db, &tool, 1);
    let err = db
        .associate_checkout(&ProjectId::new(), &tool, q(1))
        .unwrap_err();
    assert!(matches!(err, DbError::ProjectNotFound));
}
