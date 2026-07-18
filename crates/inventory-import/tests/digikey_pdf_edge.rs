//! Task 9 robustness tests for `crate::digikey::pdf::reconstruct`: explicit
//! noise-row skipping, TARIFF sub-rows becoming their own `Tariff` lines,
//! wrapped descriptions, missing-MPN confidence, fee rows, and unrecognized
//! ("Unknown") rows. Driven entirely by committed positioned-token fixtures
//! — see `crates/inventory-import/tests/digikey_pdf.rs` for the base
//! (T8) fixture's own regression tests, which still pass unchanged aside
//! from the one assertion Task 9 intentionally updates (tariff lines now
//! count towards `invoice.lines.len()`).

use inventory_core::quantity::Quantity;
use inventory_import::digikey::pdf::reconstruct;
use inventory_import::{load_token_fixture, LineKind, ParsedInvoice};

fn invoice(fixture: &str) -> ParsedInvoice {
    let tokens = load_token_fixture(fixture);
    reconstruct(&tokens)
}

fn base_invoice() -> ParsedInvoice {
    invoice("digikey_po_100353602.tokens.json")
}

// ---------------------------------------------------------------------
// A.1 — explicit noise-row skipping (base fixture)
// ---------------------------------------------------------------------

#[test]
fn base_fixture_yields_exactly_six_part_lines() {
    let invoice = base_invoice();
    assert_eq!(
        invoice.part_lines().count(),
        6,
        "exactly the 6 real PART lines"
    );
}

#[test]
fn base_fixture_lines_never_carry_noise_text() {
    let invoice = base_invoice();
    let noise_markers = ["ECCN:", "HTSUS:", "ROHS3", "Mercury:", "All transactions"];
    for line in &invoice.lines {
        let haystack = format!(
            "{} {} {}",
            line.description.as_deref().unwrap_or(""),
            line.supplier_sku.as_deref().unwrap_or(""),
            line.manufacturer.as_deref().unwrap_or(""),
        );
        for marker in noise_markers {
            assert!(
                !haystack.contains(marker),
                "line {:?} unexpectedly contains noise marker {marker:?}: {haystack:?}",
                line.line_number
            );
        }
    }
}

#[test]
fn base_fixture_still_yields_zero_warnings() {
    // Regression guard: address-block rows, the totals block, the WEB
    // ORDER ID row, and every noise row must all be recognized so the
    // Task 9 Unknown scan never fires on the real fixture.
    let invoice = base_invoice();
    assert!(
        invoice.warnings.is_empty(),
        "base fixture should still produce zero warnings, got: {:?}",
        invoice.warnings
    );
}

// ---------------------------------------------------------------------
// A.2 — TARIFF sub-rows become their own Tariff lines (base fixture)
// ---------------------------------------------------------------------

#[test]
fn tariff_rows_are_classified_as_tariff_not_part_and_attached_by_line_number() {
    let invoice = base_invoice();
    let tariff_lines: Vec<_> = invoice
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Tariff)
        .collect();
    assert_eq!(tariff_lines.len(), 6, "one Tariff line per part line");

    // Expected per-line tariff amounts (micros), in line-number order —
    // read directly off the sanitized `.txt` dump's TARIFF sub-rows.
    let expected_micros: [(u32, i64); 6] = [
        (1, 2_180_000),
        (2, 110_000),
        (3, 130_000),
        (4, 70_000),
        (5, 160_000),
        (6, 220_000),
    ];
    for (line_number, micros) in expected_micros {
        let tariff = tariff_lines
            .iter()
            .find(|l| l.line_number == Some(line_number))
            .unwrap_or_else(|| panic!("no Tariff line associated with line_number {line_number}"));
        assert_eq!(
            tariff.extended_price.as_ref().unwrap().micros,
            micros,
            "tariff amount for line {line_number}"
        );
        assert_eq!(tariff.kind, LineKind::Tariff);
        assert!(tariff.supplier_sku.is_none());
    }

    // Sum of per-line tariffs reconciles with the order-level tariff total.
    let sum: i64 = tariff_lines
        .iter()
        .map(|l| l.extended_price.as_ref().unwrap().micros)
        .sum();
    assert_eq!(sum, invoice.order.tariff.as_ref().unwrap().micros);
}

// ---------------------------------------------------------------------
// B — crafted edge-case fixtures
// ---------------------------------------------------------------------

#[test]
fn backorder_fixture_captures_ordered_shipped_backordered_exactly() {
    let invoice = invoice("digikey_backorder.tokens.json");
    let part_lines: Vec<_> = invoice.part_lines().collect();
    assert_eq!(part_lines.len(), 1);
    let line = part_lines[0];
    assert_eq!(line.supplier_sku.as_deref(), Some("TEST-BACKORDER-ND"));
    assert_eq!(line.ordered, Some(Quantity::from_whole(10).unwrap()));
    assert_eq!(line.shipped, Some(Quantity::from_whole(8).unwrap()));
    assert_eq!(line.backordered, Some(Quantity::from_whole(2).unwrap()));
    assert_eq!(line.kind, LineKind::Part);
}

#[test]
fn wrapped_description_fixture_stitches_full_description_onto_one_line() {
    let invoice = invoice("digikey_wrapped_desc.tokens.json");
    let part_lines: Vec<_> = invoice.part_lines().collect();
    assert_eq!(
        part_lines.len(),
        1,
        "the wrapped description row must not become its own line"
    );
    let line = part_lines[0];
    assert_eq!(line.supplier_sku.as_deref(), Some("TEST-WRAP-ND"));
    assert_eq!(
        line.description.as_deref(),
        Some("WRAPPED DESCRIPTION CONTINUES HERE"),
        "description should be stitched across both rows"
    );
    assert_eq!(line.mpn.as_deref(), Some("TW-MPN-1"));
}

#[test]
fn missing_mpn_fixture_keeps_part_kind_with_reduced_confidence() {
    let invoice = invoice("digikey_missing_mpn.tokens.json");
    let part_lines: Vec<_> = invoice.part_lines().collect();
    assert_eq!(part_lines.len(), 1);
    let line = part_lines[0];
    assert_eq!(line.supplier_sku.as_deref(), Some("TEST-NOMPN-ND"));
    assert_eq!(line.mpn, None);
    assert_eq!(line.kind, LineKind::Part);
    assert!(
        line.confidence < 1.0,
        "confidence should be reduced when MPN is missing, got {}",
        line.confidence
    );
}

#[test]
fn fee_row_fixture_is_classified_as_fee_never_part() {
    let invoice = invoice("digikey_fee_row.tokens.json");
    assert!(
        invoice.lines.iter().all(|l| l.kind != LineKind::Part),
        "a fee row with no PART: must never become a Part line"
    );
    let fee_lines: Vec<_> = invoice
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Fee)
        .collect();
    assert_eq!(fee_lines.len(), 1);
    let line = fee_lines[0];
    assert_eq!(line.extended_price.as_ref().unwrap().micros, 4_990_000);
    assert!(line.supplier_sku.is_none());
}

#[test]
fn unknown_row_fixture_surfaces_as_unknown_line_with_warning() {
    let invoice = invoice("digikey_unknown_row.tokens.json");
    assert!(
        invoice.lines.iter().all(|l| l.kind != LineKind::Part),
        "an unrecognized row must never become a Part line"
    );
    let unknown_lines: Vec<_> = invoice
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Unknown)
        .collect();
    assert_eq!(
        unknown_lines.len(),
        1,
        "the row must be captured, not dropped"
    );
    let line = unknown_lines[0];
    assert!(line.confidence < 1.0);
    assert!(
        line.description
            .as_deref()
            .unwrap_or("")
            .contains("MISCELLANEOUS"),
        "raw row text should be preserved on the Unknown line: {:?}",
        line.description
    );
    assert!(
        !invoice.warnings.is_empty(),
        "an unrecognized row must push a warnings entry, never drop silently"
    );
    assert!(
        invoice.warnings.iter().any(|w| w.contains("MISCELLANEOUS")),
        "warning should reference the unrecognized row's content: {:?}",
        invoice.warnings
    );
}
