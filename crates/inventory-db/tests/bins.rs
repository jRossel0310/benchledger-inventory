//! Phase 3 Task 8: bin browser aggregate (`list_bins`) and bulk `rename_bin`.

use inventory_core::ids::CategoryId;
use inventory_core::quantity::QuantityUnit;
use inventory_db::parts::PartDraft;
use inventory_db::{Database, DbError, MISC_CATEGORY_ID};

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

fn binned(name: &str, bin: &str) -> PartDraft {
    let mut d = draft(name);
    d.bin_label = Some(bin.to_string());
    d
}

#[test]
fn list_bins_is_empty_on_an_empty_database() {
    let (_g, db) = open();
    assert_eq!(db.list_bins().unwrap(), vec![]);
}

#[test]
fn list_bins_groups_parts_by_bin_label() {
    let (_g, mut db) = open();
    db.create_part(&binned("part in A1", "A1")).unwrap();
    db.create_part(&binned("another part in A1", "A1")).unwrap();
    db.create_part(&binned("part in B2", "B2")).unwrap();

    let bins = db.list_bins().unwrap();
    assert_eq!(bins.len(), 2);
    let a1 = bins
        .iter()
        .find(|b| b.bin_label.as_deref() == Some("A1"))
        .unwrap();
    assert_eq!(a1.part_count, 2);
    let b2 = bins
        .iter()
        .find(|b| b.bin_label.as_deref() == Some("B2"))
        .unwrap();
    assert_eq!(b2.part_count, 1);
}

#[test]
fn list_bins_groups_bin_labels_that_differ_only_by_case_into_one_row() {
    let (_g, mut db) = open();
    db.create_part(&binned("part in A1", "A1")).unwrap();
    db.create_part(&binned("part in a1", "a1")).unwrap();

    let bins = db.list_bins().unwrap();
    let matching: Vec<_> = bins
        .iter()
        .filter(|b| b.bin_label.as_deref().map(|l| l.eq_ignore_ascii_case("a1")) == Some(true))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected one case-insensitively merged bin row, got {matching:?}"
    );
    assert_eq!(matching[0].part_count, 2);
}

#[test]
fn list_bins_includes_a_distinct_unassigned_bucket_for_null_bin_label() {
    let (_g, mut db) = open();
    db.create_part(&draft("no bin part")).unwrap();
    db.create_part(&binned("binned part", "A1")).unwrap();

    let bins = db.list_bins().unwrap();
    let unassigned = bins.iter().find(|b| b.bin_label.is_none()).unwrap();
    assert_eq!(unassigned.part_count, 1);
}

#[test]
fn list_bins_omits_the_unassigned_bucket_when_every_part_has_a_bin() {
    let (_g, mut db) = open();
    db.create_part(&binned("binned part", "A1")).unwrap();

    let bins = db.list_bins().unwrap();
    assert!(!bins.iter().any(|b| b.bin_label.is_none()));
}

#[test]
fn list_bins_sorts_named_bins_alphabetically_case_insensitively_with_unassigned_last() {
    let (_g, mut db) = open();
    for label in ["C3", "a1", "B2"] {
        db.create_part(&binned(&format!("part in {label}"), label))
            .unwrap();
    }
    db.create_part(&draft("no bin part")).unwrap();

    let bins = db.list_bins().unwrap();
    let labels: Vec<Option<String>> = bins.iter().map(|b| b.bin_label.clone()).collect();
    assert_eq!(
        labels,
        vec![
            Some("a1".to_string()),
            Some("B2".to_string()),
            Some("C3".to_string()),
            None,
        ]
    );
}

#[test]
fn list_bins_excludes_archived_parts_from_every_count() {
    let (_g, mut db) = open();
    let part = db.create_part(&binned("archived part", "A1")).unwrap();
    db.set_part_archived(&part.id, true).unwrap();

    assert_eq!(db.list_bins().unwrap(), vec![]);
}

#[test]
fn list_bins_still_counts_other_non_archived_parts_in_a_bin_with_an_archived_member() {
    let (_g, mut db) = open();
    let archived = db.create_part(&binned("archived part", "A1")).unwrap();
    db.set_part_archived(&archived.id, true).unwrap();
    db.create_part(&binned("active part", "A1")).unwrap();

    let bins = db.list_bins().unwrap();
    let a1 = bins
        .iter()
        .find(|b| b.bin_label.as_deref() == Some("A1"))
        .unwrap();
    assert_eq!(a1.part_count, 1);
}

#[test]
fn rename_bin_moves_every_part_in_the_old_bin_to_the_new_label() {
    let (_g, mut db) = open();
    db.create_part(&binned("r1", "OLD")).unwrap();
    db.create_part(&binned("r2", "OLD")).unwrap();
    let unrelated = db.create_part(&binned("unrelated", "OTHER")).unwrap();

    let moved = db.rename_bin("OLD", "NEW").unwrap();
    assert_eq!(moved, 2);

    let bins = db.list_bins().unwrap();
    assert!(bins.iter().all(|b| b.bin_label.as_deref() != Some("OLD")));
    let new_summary = bins
        .iter()
        .find(|b| b.bin_label.as_deref() == Some("NEW"))
        .unwrap();
    assert_eq!(new_summary.part_count, 2);

    // The unrelated bin is untouched.
    let unrelated_part = db.get_part(&unrelated.id).unwrap().unwrap();
    assert_eq!(unrelated_part.bin_label.as_deref(), Some("OTHER"));
}

#[test]
fn rename_bin_matches_the_old_label_case_insensitively() {
    let (_g, mut db) = open();
    db.create_part(&binned("case test", "A1")).unwrap();

    let moved = db.rename_bin("a1", "A2").unwrap();
    assert_eq!(moved, 1);
}

#[test]
fn rename_bin_trims_the_new_label() {
    let (_g, mut db) = open();
    let part = db.create_part(&binned("part", "OLD")).unwrap();

    db.rename_bin("OLD", "  NEW  ").unwrap();

    let stored = db.get_part(&part.id).unwrap().unwrap();
    assert_eq!(stored.bin_label.as_deref(), Some("NEW"));
}

#[test]
fn rename_bin_merges_into_an_existing_occupied_label() {
    let (_g, mut db) = open();
    db.create_part(&binned("moving part", "OLD")).unwrap();
    db.create_part(&binned("already there", "NEW")).unwrap();

    let moved = db.rename_bin("OLD", "NEW").unwrap();
    assert_eq!(moved, 1);

    let bins = db.list_bins().unwrap();
    let new_summary = bins
        .iter()
        .find(|b| b.bin_label.as_deref() == Some("NEW"))
        .unwrap();
    assert_eq!(new_summary.part_count, 2);
}

#[test]
fn rename_bin_rejects_an_empty_new_label_and_changes_nothing() {
    let (_g, mut db) = open();
    db.create_part(&binned("part", "A1")).unwrap();

    let err = db.rename_bin("A1", "   ").unwrap_err();
    assert!(matches!(err, DbError::InvalidBinLabel(_)));

    let bins = db.list_bins().unwrap();
    assert!(bins.iter().any(|b| b.bin_label.as_deref() == Some("A1")));
}

#[test]
fn rename_bin_is_a_no_op_for_a_label_with_no_current_parts() {
    let (_g, mut db) = open();
    let moved = db.rename_bin("NOBODY-HERE", "NEW").unwrap();
    assert_eq!(moved, 0);
}

#[test]
fn rename_bin_never_touches_archived_parts() {
    let (_g, mut db) = open();
    let part = db.create_part(&binned("archived in bin", "OLD")).unwrap();
    db.set_part_archived(&part.id, true).unwrap();

    let moved = db.rename_bin("OLD", "NEW").unwrap();
    assert_eq!(moved, 0);
    let stored = db.get_part(&part.id).unwrap().unwrap();
    assert_eq!(stored.bin_label.as_deref(), Some("OLD"));
}

#[test]
fn rename_bin_keeps_the_part_searchable_by_its_new_bin_label() {
    let (_g, mut db) = open();
    db.create_part(&binned("searchable part", "OLD")).unwrap();
    db.rename_bin("OLD", "NEW").unwrap();

    let hits = db.search("bin:NEW").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].display_name, "searchable part");

    let stale = db.search("bin:OLD").unwrap();
    assert!(stale.is_empty());
}
