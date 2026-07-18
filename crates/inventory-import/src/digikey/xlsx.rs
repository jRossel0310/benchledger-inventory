//! DigiKey XLSX order/invoice parser.
//!
//! Structurally mirrors [`super::csv::DigiKeyCsvParser`]: locate the header
//! row by NAME (via [`super::columns::ColumnMap`], shared with CSV) rather
//! than assuming a fixed layout — DigiKey XLSX exports commonly carry a
//! title/address preamble above the header row — then classify every data
//! row through the same [`super::row::parse_row`] helper CSV uses, so the
//! two formats cannot drift apart on fee/tariff/no-charge/missing-MPN
//! rules.
//!
//! **Numeric cells:** calamine reads XLSX numeric cells as `Data::Int`/
//! `Data::Float`, not text. A whole-valued quantity cell must not turn into
//! `"3.0"` before it reaches [`row::parse_row`]'s integer-quantity parsing
//! (`"3.0".parse::<i64>()` fails, silently dropping the quantity) — so
//! [`cell_to_string`] normalizes an integral float to its plain integer
//! text first. Money cells may be numeric or text either way; both are
//! stringified and handed to `Money::parse` exactly like a CSV cell.
//!
//! **Currency/order-metadata assumptions** are identical to the CSV parser
//! — see its module docs: neither line-item export format carries a
//! per-file currency cell or order metadata, so those are defaulted/`None`
//! here too.

use calamine::{Data, Reader, Xlsx};
use std::io::Cursor;

use crate::digikey::columns::ColumnMap;
use crate::digikey::row::{self, CURRENCY};
use crate::model::{ParsedInvoice, ParsedOrderMeta, SourceFormat};
use crate::parser::{ImportError, InvoiceParser};

pub struct DigiKeyXlsxParser;

impl InvoiceParser for DigiKeyXlsxParser {
    fn supplier(&self) -> &str {
        "DigiKey"
    }

    fn source_format(&self) -> SourceFormat {
        SourceFormat::Xlsx
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedInvoice, ImportError> {
        if bytes.is_empty() {
            return Err(ImportError::Empty);
        }

        let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(bytes))
            .map_err(|e| ImportError::Malformed(format!("failed to open workbook: {e}")))?;

        let sheet_name = workbook
            .sheet_names()
            .into_iter()
            .next()
            .ok_or(ImportError::Empty)?;

        let range = workbook
            .worksheet_range(&sheet_name)
            .map_err(|e| ImportError::Malformed(format!("failed to read worksheet: {e}")))?;

        let records: Vec<Vec<String>> = range
            .rows()
            .map(|row| row.iter().map(cell_to_string).collect())
            .collect();

        if records.is_empty()
            || records
                .iter()
                .all(|row| row.iter().all(|cell| cell.trim().is_empty()))
        {
            return Err(ImportError::Empty);
        }

        let header_idx = records
            .iter()
            .position(|row| ColumnMap::looks_like_header(row))
            .ok_or_else(|| {
                ImportError::Malformed("no recognizable DigiKey header row found".to_string())
            })?;

        let header = &records[header_idx];
        let map = ColumnMap::from_header(header);

        let mut lines = Vec::new();
        let mut warnings = Vec::new();

        for record in &records[header_idx + 1..] {
            if record.iter().all(|cell| cell.trim().is_empty()) {
                continue;
            }
            if let Some(line) = row::parse_row(header, record, &map, &mut warnings) {
                lines.push(line);
            }
        }

        Ok(ParsedInvoice {
            supplier: "DigiKey".to_string(),
            source_format: SourceFormat::Xlsx,
            order: ParsedOrderMeta {
                order_number: None,
                invoice_number: None,
                shipment_number: None,
                order_date: None,
                currency: CURRENCY.to_string(),
                subtotal: None,
                shipping: None,
                tax: None,
                tariff: None,
                total: None,
                web_order_id: None,
            },
            lines,
            warnings,
        })
    }
}

/// Stringify one calamine cell the way [`row::parse_row`]'s quantity/money
/// parsing expects: empty text for an empty cell, the literal text for a
/// string cell, and — critically — a whole-valued numeric cell renders as
/// `"3"`, never `"3.0"`, so an XLSX quantity column round-trips to a whole
/// `Quantity` exactly like a CSV text cell does. Non-integral numbers keep
/// their full decimal text (e.g. a numeric `1.82` price cell -> `"1.82"`,
/// which `Money::parse` scales to micros identically to CSV's `"1.82000"`).
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::Float(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 => {
            (*f as i64).to_string()
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LineKind, ParsedLine};
    use inventory_core::quantity::Quantity;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    fn fixture_bytes(name: &str) -> Vec<u8> {
        std::fs::read(fixture_path(name))
            .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
    }

    fn find_line<'a>(invoice: &'a ParsedInvoice, sku: &str) -> &'a ParsedLine {
        invoice
            .lines
            .iter()
            .find(|l| l.supplier_sku.as_deref() == Some(sku))
            .unwrap_or_else(|| panic!("no line with supplier_sku {sku}"))
    }

    #[test]
    fn cell_to_string_normalizes_whole_valued_floats_to_integer_text() {
        // The bug this guards: calamine reads a plain "3" quantity cell as
        // Data::Float(3.0); naively formatting the float must not yield
        // "3.0" (which would fail `"3.0".parse::<i64>()` downstream and
        // silently drop the quantity).
        assert_eq!(cell_to_string(&Data::Float(3.0)), "3");
        assert_eq!(cell_to_string(&Data::Float(1000.0)), "1000");
        assert_eq!(cell_to_string(&Data::Float(0.0)), "0");
        // Genuinely fractional numeric cells keep their decimal text.
        assert_eq!(cell_to_string(&Data::Float(1.82)), "1.82");
        assert_eq!(cell_to_string(&Data::Int(3)), "3");
        assert_eq!(
            cell_to_string(&Data::String("Cut Tape".to_string())),
            "Cut Tape"
        );
        assert_eq!(cell_to_string(&Data::Empty), "");
    }

    #[test]
    fn supplier_and_format_are_digikey_xlsx() {
        let parser = DigiKeyXlsxParser;
        assert_eq!(parser.supplier(), "DigiKey");
        assert_eq!(parser.source_format(), SourceFormat::Xlsx);
    }

    #[test]
    fn empty_bytes_return_empty_error() {
        let parser = DigiKeyXlsxParser;
        let err = parser.parse(b"").unwrap_err();
        assert!(matches!(err, ImportError::Empty));
    }

    #[test]
    fn garbage_bytes_are_malformed_not_a_panic() {
        let parser = DigiKeyXlsxParser;
        let err = parser.parse(b"not a real xlsx file").unwrap_err();
        assert!(matches!(err, ImportError::Malformed(_)));
    }

    #[test]
    fn parses_fixture_part_rows_with_exact_quantity_and_price() {
        let parser = DigiKeyXlsxParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.xlsx")).unwrap();
        assert_eq!(invoice.supplier, "DigiKey");
        assert_eq!(invoice.source_format, SourceFormat::Xlsx);
        assert_eq!(invoice.order.currency, "USD");

        let part_count = invoice
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Part)
            .count();
        assert_eq!(part_count, 6, "expected 6 part lines in the fixture");

        let line1 = find_line(&invoice, "475-BPW34-ND");
        assert_eq!(line1.mpn.as_deref(), Some("BPW34"));
        assert_eq!(line1.manufacturer.as_deref(), Some("AMS-OSRAM USA INC."));
        assert_eq!(
            line1.description.as_deref(),
            Some("SENSOR PHOTODIODE 850NM 2DIP")
        );
        // The fixture's row-1 Quantity cell is a numeric (float) "3", not
        // text — proves the "3.0" normalization actually runs end to end.
        assert_eq!(line1.shipped, Some(Quantity::from_whole(3).unwrap()));
        assert_eq!(line1.ordered, Some(Quantity::from_whole(3).unwrap()));
        assert_eq!(line1.backordered, Some(Quantity::from_whole(0).unwrap()));
        assert_eq!(line1.unit_price.as_ref().unwrap().micros, 1_820_000);
        assert_eq!(line1.extended_price.as_ref().unwrap().micros, 5_460_000);
        assert_eq!(line1.kind, LineKind::Part);
        assert_eq!(line1.confidence, 1.0);
        assert_eq!(line1.packaging.as_deref(), Some("Cut Tape"));

        // Backorder line: ordered 3 / shipped 1 / backordered 2.
        let backorder_line = find_line(&invoice, "296-MCP1702-5002E-TO-ND");
        assert_eq!(
            backorder_line.shipped,
            Some(Quantity::from_whole(1).unwrap())
        );
        assert_eq!(
            backorder_line.ordered,
            Some(Quantity::from_whole(3).unwrap())
        );
        assert_eq!(
            backorder_line.backordered,
            Some(Quantity::from_whole(2).unwrap())
        );
    }

    #[test]
    fn raw_json_preserves_row_cells_keyed_by_header() {
        let parser = DigiKeyXlsxParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.xlsx")).unwrap();
        let line1 = find_line(&invoice, "475-BPW34-ND");
        let raw = line1.raw.as_object().expect("raw must be a JSON object");
        assert_eq!(
            raw.get("DigiKey Part #").and_then(|v| v.as_str()),
            Some("475-BPW34-ND")
        );
        assert_eq!(
            raw.get("Manufacturer Part Number").and_then(|v| v.as_str()),
            Some("BPW34")
        );
    }

    #[test]
    fn header_is_located_below_title_and_address_preamble() {
        let parser = DigiKeyXlsxParser;
        // The fixture itself carries a 3-row title/address block above the
        // header row — this asserts the header-discovery path is actually
        // exercised, not merely present.
        let invoice = parser.parse(&fixture_bytes("digikey_order.xlsx")).unwrap();
        assert!(!invoice.lines.is_empty());
        let line1 = find_line(&invoice, "475-BPW34-ND");
        assert_eq!(line1.mpn.as_deref(), Some("BPW34"));
    }

    #[test]
    fn fee_row_is_classified_as_fee_not_part() {
        let parser = DigiKeyXlsxParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.xlsx")).unwrap();
        let fee_line = invoice
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Fee)
            .expect("expected a Fee line in the fixture");
        assert!(fee_line.supplier_sku.is_none());
        assert!(fee_line.mpn.is_none());
        assert_eq!(fee_line.extended_price.as_ref().unwrap().micros, 4_990_000);
    }

    #[test]
    fn tariff_row_is_classified_as_tariff() {
        let parser = DigiKeyXlsxParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.xlsx")).unwrap();
        let tariff_line = invoice
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Tariff)
            .expect("expected a Tariff line in the fixture");
        assert!(tariff_line.supplier_sku.is_none());
        assert_eq!(
            tariff_line.extended_price.as_ref().unwrap().micros,
            2_870_000
        );
    }

    #[test]
    fn missing_mpn_row_is_captured_as_part_with_reduced_confidence() {
        let parser = DigiKeyXlsxParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.xlsx")).unwrap();
        let line = find_line(&invoice, "160-1366-5-ND");
        assert_eq!(line.mpn, None);
        assert_eq!(line.kind, LineKind::Part);
        assert!(line.confidence < 1.0, "confidence should be reduced");
        assert_eq!(line.manufacturer.as_deref(), Some("LITEON"));
    }

    #[test]
    fn no_charge_promo_row_is_classified_as_no_charge() {
        let parser = DigiKeyXlsxParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.xlsx")).unwrap();
        let promo = find_line(&invoice, "490-PROMO-ND");
        assert_eq!(promo.kind, LineKind::NoCharge);
        assert_eq!(promo.unit_price.as_ref().unwrap().micros, 0);
    }

    #[test]
    fn row_with_quantity_but_no_sku_or_mpn_is_unknown_and_warned() {
        let parser = DigiKeyXlsxParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.xlsx")).unwrap();
        let unknown = invoice
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Unknown)
            .expect("expected an Unknown line in the fixture");
        assert!(unknown.supplier_sku.is_none());
        assert!(unknown.mpn.is_none());
        assert!(unknown.confidence < 1.0);
        assert!(
            !invoice.warnings.is_empty(),
            "an Unknown row must add a warning"
        );
    }

    /// Proves the shared `row::parse_row` helper is actually shared: the
    /// CSV fixture and the XLSX fixture encode the same logical invoice
    /// (same SKUs, quantities, prices, classifications) even though one
    /// arrives as CSV text cells and the other as calamine `Data` cells
    /// (a mix of strings, ints, and floats) — every typed field must match
    /// across both once each row reaches the shared classifier.
    #[test]
    fn csv_and_xlsx_fixtures_classify_identically_through_the_shared_helper() {
        let csv_parser = crate::digikey::DigiKeyCsvParser;
        let xlsx_parser = DigiKeyXlsxParser;
        let csv_invoice = csv_parser
            .parse(&fixture_bytes("digikey_order.csv"))
            .unwrap();
        let xlsx_invoice = xlsx_parser
            .parse(&fixture_bytes("digikey_order.xlsx"))
            .unwrap();

        assert_eq!(csv_invoice.lines.len(), xlsx_invoice.lines.len());
        for sku in [
            "475-BPW34-ND",
            "296-MCP6002-I-P-ND",
            "296-NE555P-ND",
            "160-1366-5-ND",
            "296-MCP1702-5002E-TO-ND",
            "296-LM393NNS-NOPB-ND",
            "490-PROMO-ND",
        ] {
            let a = find_line(&csv_invoice, sku);
            let b = find_line(&xlsx_invoice, sku);
            assert_eq!(a.mpn, b.mpn, "mpn mismatch for {sku}");
            assert_eq!(
                a.manufacturer, b.manufacturer,
                "manufacturer mismatch for {sku}"
            );
            assert_eq!(
                a.description, b.description,
                "description mismatch for {sku}"
            );
            assert_eq!(a.shipped, b.shipped, "shipped mismatch for {sku}");
            assert_eq!(a.ordered, b.ordered, "ordered mismatch for {sku}");
            assert_eq!(
                a.backordered, b.backordered,
                "backordered mismatch for {sku}"
            );
            assert_eq!(a.unit_price, b.unit_price, "unit_price mismatch for {sku}");
            assert_eq!(
                a.extended_price, b.extended_price,
                "extended_price mismatch for {sku}"
            );
            assert_eq!(a.kind, b.kind, "kind mismatch for {sku}");
        }
    }
}
