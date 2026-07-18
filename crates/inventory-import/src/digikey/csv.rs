//! DigiKey CSV order/invoice parser.
//!
//! DigiKey's CSV line-item export varies by export tool and account
//! settings: column order and exact header text differ, some exports have a
//! preamble (title/address block) above the header row, and some rows carry
//! no part identifier at all (shipping/tax/tariff fee lines). This parser
//! locates the header by NAME (via [`super::columns::ColumnMap`], shared
//! with the XLSX parser) rather than assuming a fixed layout, and never
//! drops a row — unrecognizable rows are captured as [`LineKind::Unknown`]
//! with reduced confidence and a warning instead.
//!
//! **Currency assumption:** DigiKey CSV line exports do not carry a
//! per-file currency cell, so this parser defaults `order.currency` and
//! every parsed [`Money`] to `"USD"`. **Order-metadata assumption:** the
//! line-item CSV export (unlike the PDF PO acknowledgement) generally does
//! not include order number, invoice number, order date, or totals, so
//! `ParsedOrderMeta` is left mostly `None` here rather than fabricated —
//! richer metadata comes from the PDF/XLSX parsers when available.

use crate::digikey::columns::{ColumnMap, Field};
use crate::model::{LineKind, Money, ParsedInvoice, ParsedLine, ParsedOrderMeta, SourceFormat};
use crate::parser::{ImportError, InvoiceParser};
use inventory_core::quantity::Quantity;

const CURRENCY: &str = "USD";
const FEE_KEYWORDS: [&str; 4] = ["SHIPPING", "TARIFF", "TAX", "FREIGHT"];

pub struct DigiKeyCsvParser;

impl InvoiceParser for DigiKeyCsvParser {
    fn supplier(&self) -> &str {
        "DigiKey"
    }

    fn source_format(&self) -> SourceFormat {
        SourceFormat::Csv
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedInvoice, ImportError> {
        if bytes.is_empty() {
            return Err(ImportError::Empty);
        }

        let text = decode_utf8_bom_tolerant(bytes)?;
        if text.trim().is_empty() {
            return Err(ImportError::Empty);
        }

        let records = read_records(&text)?;
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

        for row in &records[header_idx + 1..] {
            if row.iter().all(|cell| cell.trim().is_empty()) {
                continue;
            }
            if let Some(line) = parse_row(header, row, &map, &mut warnings) {
                lines.push(line);
            }
        }

        Ok(ParsedInvoice {
            supplier: "DigiKey".to_string(),
            source_format: SourceFormat::Csv,
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

/// Strip a leading UTF-8 BOM (if present) and decode the remaining bytes as
/// UTF-8 text.
fn decode_utf8_bom_tolerant(bytes: &[u8]) -> Result<String, ImportError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .map_err(|e| ImportError::Encoding(e.to_string()))
}

/// Read every row of the CSV as plain string vectors, with no assumption
/// about which row (if any) is the header — that is located separately so a
/// preamble above the header doesn't get mistaken for data.
fn read_records(text: &str) -> Result<Vec<Vec<String>>, ImportError> {
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());

    let mut records = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| ImportError::Malformed(e.to_string()))?;
        records.push(record.iter().map(|c| c.to_string()).collect());
    }
    Ok(records)
}

/// Build the `raw` JSON object of original header -> cell pairs for one row,
/// verbatim (untrimmed), so nothing the source contained is lost even when
/// the typed fields below couldn't capture it.
fn raw_json(header: &[String], row: &[String]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (h, c) in header.iter().zip(row.iter()) {
        map.insert(h.clone(), serde_json::Value::String(c.clone()));
    }
    serde_json::Value::Object(map)
}

/// Trimmed, empty-string-as-None lookup of a field's cell by name.
fn cell(row: &[String], map: &ColumnMap, field: Field) -> Option<String> {
    map.index_of(field)
        .and_then(|idx| row.get(idx))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Whole-number DigiKey quantity ("3", "1,000") -> a `Quantity` of `each`.
fn parse_quantity(text: &str) -> Option<Quantity> {
    let cleaned = text.replace(',', "");
    let n: i64 = cleaned.trim().parse().ok()?;
    Quantity::from_whole(n).ok()
}

fn is_zero(money: &Option<Money>) -> bool {
    matches!(money, Some(m) if m.micros == 0)
}

fn parse_row(
    header: &[String],
    row: &[String],
    map: &ColumnMap,
    warnings: &mut Vec<String>,
) -> Option<ParsedLine> {
    let line_number = cell(row, map, Field::LineNumber).and_then(|s| s.parse::<u32>().ok());
    let supplier_sku = cell(row, map, Field::SupplierSku);
    let mpn = cell(row, map, Field::Mpn);
    let manufacturer = cell(row, map, Field::Manufacturer);
    let description = cell(row, map, Field::Description);
    let packaging = cell(row, map, Field::Packaging);
    let customer_reference = cell(row, map, Field::CustomerReference);

    let shipped = cell(row, map, Field::Shipped).and_then(|s| parse_quantity(&s));
    let ordered = cell(row, map, Field::Ordered).and_then(|s| parse_quantity(&s));
    let backordered = cell(row, map, Field::Backordered).and_then(|s| parse_quantity(&s));

    let unit_price = cell(row, map, Field::UnitPrice).and_then(|s| Money::parse(&s, CURRENCY));
    let extended_price =
        cell(row, map, Field::ExtendedPrice).and_then(|s| Money::parse(&s, CURRENCY));

    let has_part_id = supplier_sku.is_some() || mpn.is_some();
    let fee_keyword = description
        .as_deref()
        .map(|d| d.to_ascii_uppercase())
        .and_then(|d| FEE_KEYWORDS.iter().find(|kw| d.contains(*kw)).copied());

    let (kind, confidence) = if has_part_id {
        if is_zero(&unit_price) || is_zero(&extended_price) {
            (LineKind::NoCharge, 1.0)
        } else if mpn.is_none() {
            // Missing MPN on an otherwise-valid part row: keep it as Part,
            // just with reduced confidence — captured, not dropped.
            (LineKind::Part, 0.8)
        } else {
            (LineKind::Part, 1.0)
        }
    } else if let Some(kw) = fee_keyword {
        if kw == "TARIFF" {
            (LineKind::Tariff, 1.0)
        } else {
            (LineKind::Fee, 1.0)
        }
    } else {
        // Quantity/price present but no SKU, no MPN, and no recognizable
        // fee keyword — never drop it, just flag it as unrecognized.
        warnings.push(format!(
            "row without a supplier SKU/MPN or fee keyword classified as Unknown: {:?}",
            row
        ));
        (LineKind::Unknown, 0.3)
    };

    Some(ParsedLine {
        line_number,
        supplier_sku,
        mpn,
        manufacturer,
        description,
        ordered,
        shipped,
        backordered,
        unit_price,
        extended_price,
        packaging,
        customer_reference,
        kind,
        confidence,
        raw: raw_json(header, row),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn supplier_and_format_are_digikey_csv() {
        let parser = DigiKeyCsvParser;
        assert_eq!(parser.supplier(), "DigiKey");
        assert_eq!(parser.source_format(), SourceFormat::Csv);
    }

    #[test]
    fn empty_bytes_return_empty_error() {
        let parser = DigiKeyCsvParser;
        let err = parser.parse(b"").unwrap_err();
        assert!(matches!(err, ImportError::Empty));
    }

    #[test]
    fn blank_text_returns_empty_error() {
        let parser = DigiKeyCsvParser;
        let err = parser.parse(b"   \n\n  ").unwrap_err();
        assert!(matches!(err, ImportError::Empty));
    }

    #[test]
    fn csv_without_recognizable_header_is_malformed() {
        let parser = DigiKeyCsvParser;
        let err = parser.parse(b"foo,bar,baz\n1,2,3\n4,5,6\n").unwrap_err();
        assert!(matches!(err, ImportError::Malformed(_)));
    }

    #[test]
    fn parses_fixture_part_rows_with_exact_quantity_and_price() {
        let parser = DigiKeyCsvParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.csv")).unwrap();
        assert_eq!(invoice.supplier, "DigiKey");
        assert_eq!(invoice.source_format, SourceFormat::Csv);
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
    fn raw_json_preserves_original_header_and_cells_verbatim() {
        let parser = DigiKeyCsvParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.csv")).unwrap();
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
        assert_eq!(
            raw.get("Unit Price").and_then(|v| v.as_str()),
            Some("1.82000")
        );
    }

    #[test]
    fn header_reordered_variant_parses_identically() {
        let parser = DigiKeyCsvParser;
        let original = parser.parse(&fixture_bytes("digikey_order.csv")).unwrap();
        let reordered = parser
            .parse(&fixture_bytes("digikey_order_reordered.csv"))
            .unwrap();

        assert_eq!(original.lines.len(), reordered.lines.len());
        for sku in [
            "475-BPW34-ND",
            "296-MCP6002-I-P-ND",
            "296-NE555P-ND",
            "160-1366-5-ND",
            "296-MCP1702-5002E-TO-ND",
            "296-LM393NNS-NOPB-ND",
        ] {
            let a = find_line(&original, sku);
            let b = find_line(&reordered, sku);
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
            assert_eq!(a.unit_price, b.unit_price, "unit_price mismatch for {sku}");
            assert_eq!(
                a.extended_price, b.extended_price,
                "extended_price mismatch for {sku}"
            );
            assert_eq!(a.kind, b.kind, "kind mismatch for {sku}");
        }
    }

    #[test]
    fn fee_row_is_classified_as_fee_not_part() {
        let parser = DigiKeyCsvParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.csv")).unwrap();
        let fee_line = invoice
            .lines
            .iter()
            .find(|l| l.kind == LineKind::Fee)
            .expect("expected a Fee line in the fixture");
        assert!(fee_line.supplier_sku.is_none());
        assert!(fee_line.mpn.is_none());
        assert_eq!(fee_line.extended_price.as_ref().unwrap().micros, 4_990_000);
        assert_ne!(fee_line.kind, LineKind::Part);

        // A helper that only counts LineKind::Part must not see the fee row.
        let parts_only: Vec<_> = invoice
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Part)
            .collect();
        assert!(!parts_only
            .iter()
            .any(|l| l.extended_price.as_ref().map(|m| m.micros) == Some(4_990_000)));
    }

    #[test]
    fn tariff_row_is_classified_as_tariff() {
        let parser = DigiKeyCsvParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.csv")).unwrap();
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
        let parser = DigiKeyCsvParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.csv")).unwrap();
        let line = find_line(&invoice, "160-1366-5-ND");
        assert_eq!(line.mpn, None);
        assert_eq!(line.kind, LineKind::Part);
        assert!(line.confidence < 1.0, "confidence should be reduced");
        assert_eq!(line.manufacturer.as_deref(), Some("LITEON"));
    }

    #[test]
    fn no_charge_promo_row_is_classified_as_no_charge() {
        let parser = DigiKeyCsvParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.csv")).unwrap();
        let promo = find_line(&invoice, "490-PROMO-ND");
        assert_eq!(promo.kind, LineKind::NoCharge);
        assert_eq!(promo.unit_price.as_ref().unwrap().micros, 0);
    }

    #[test]
    fn row_with_quantity_but_no_sku_or_mpn_is_unknown_and_warned() {
        let parser = DigiKeyCsvParser;
        let invoice = parser.parse(&fixture_bytes("digikey_order.csv")).unwrap();
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

    #[test]
    fn preamble_line_above_header_still_finds_header() {
        let parser = DigiKeyCsvParser;
        let base = fixture_bytes("digikey_order.csv");
        let mut with_preamble = b"DigiKey Order Export,,,,,,,,,,\n".to_vec();
        with_preamble.extend_from_slice(&base);

        let invoice = parser.parse(&with_preamble).unwrap();
        let baseline = parser.parse(&base).unwrap();
        assert_eq!(invoice.lines.len(), baseline.lines.len());
        let line1 = find_line(&invoice, "475-BPW34-ND");
        assert_eq!(line1.mpn.as_deref(), Some("BPW34"));
    }

    #[test]
    fn bom_prefixed_csv_parses() {
        let parser = DigiKeyCsvParser;
        let base = fixture_bytes("digikey_order.csv");
        let mut with_bom = b"\xEF\xBB\xBF".to_vec();
        with_bom.extend_from_slice(&base);

        let invoice = parser.parse(&with_bom).unwrap();
        let baseline = parser.parse(&base).unwrap();
        assert_eq!(invoice.lines.len(), baseline.lines.len());
        let line1 = find_line(&invoice, "475-BPW34-ND");
        assert_eq!(line1.unit_price.as_ref().unwrap().micros, 1_820_000);
    }
}
