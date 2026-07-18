//! DigiKey PDF PO Acknowledgement reconstruction tests — driven entirely by
//! the committed positioned-token fixture
//! (`digikey_po_100353602.tokens.json`), never a live PDF/pdfium. See
//! `crates/inventory-import/src/pdf/mod.rs` for why: a live `PdfiumTextSource`
//! extraction and this fixture normalize to the same coordinate convention,
//! so exercising `reconstruct` against the fixture is a faithful test of the
//! real reconstruction logic without any native dependency.

use inventory_core::quantity::Quantity;
use inventory_import::digikey::pdf::{reconstruct, DigiKeyPdfParser};
use inventory_import::{
    load_token_fixture, ImportError, InvoiceParser, LineKind, ParsedInvoice, ParsedLine,
    PdfTextSource, PositionedToken, SourceFormat,
};

fn invoice() -> ParsedInvoice {
    let tokens = load_token_fixture("digikey_po_100353602.tokens.json");
    reconstruct(&tokens)
}

fn find_line<'a>(lines: &'a [ParsedLine], sku: &str) -> &'a ParsedLine {
    lines
        .iter()
        .find(|l| l.supplier_sku.as_deref() == Some(sku))
        .unwrap_or_else(|| panic!("no line with supplier_sku {sku}"))
}

fn part_lines(invoice: &ParsedInvoice) -> Vec<&ParsedLine> {
    invoice
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Part)
        .collect()
}

#[test]
fn order_metadata_parses_exactly() {
    let invoice = invoice();
    assert_eq!(invoice.supplier, "DigiKey");
    assert_eq!(invoice.source_format, SourceFormat::Pdf);
    assert_eq!(invoice.order.order_number.as_deref(), Some("100353602"));
    assert_eq!(invoice.order.currency, "USD");
    assert_eq!(invoice.order.order_date.as_deref(), Some("13-JUL-2026"));
    assert_eq!(invoice.order.web_order_id.as_deref(), Some("373838988"));
}

#[test]
fn six_part_lines_are_extracted_in_order_no_more_no_less() {
    let invoice = invoice();
    let lines = part_lines(&invoice);
    let skus: Vec<&str> = lines
        .iter()
        .map(|l| l.supplier_sku.as_deref().unwrap())
        .collect();
    assert_eq!(
        skus,
        vec![
            "475-BPW34-ND",
            "MCP6002-I/P-ND",
            "296-NE555P-ND",
            "160-1366-5-ND",
            "MCP1702-5002E/TO-ND",
            "LM393NNS/NOPB-ND",
        ]
    );
    let line_numbers: Vec<Option<u32>> = lines.iter().map(|l| l.line_number).collect();
    assert_eq!(
        line_numbers,
        vec![Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)]
    );
    // Noise (ECCN/HTSUS/ROHS3/Mercury/TARIFF/footer) and the MFG rows must
    // never surface as their own lines.
    assert_eq!(invoice.lines.len(), 6, "no extra lines besides the 6 parts");
}

#[test]
fn line_1_bpw34_parses_exactly() {
    let invoice = invoice();
    let line = find_line(&invoice.lines, "475-BPW34-ND");
    assert_eq!(line.mpn.as_deref(), Some("BPW34"));
    assert_eq!(line.manufacturer.as_deref(), Some("AMS-OSRAM USA INC."));
    assert_eq!(
        line.description.as_deref(),
        Some("SENSOR PHOTODIODE 850NM 2DIP")
    );
    assert_eq!(line.ordered, Some(Quantity::from_whole(3).unwrap()));
    assert_eq!(line.shipped, Some(Quantity::from_whole(3).unwrap()));
    assert_eq!(line.backordered, Some(Quantity::from_whole(0).unwrap()));
    assert_eq!(line.unit_price.as_ref().unwrap().micros, 1_820_000);
    assert_eq!(line.extended_price.as_ref().unwrap().micros, 5_460_000);
    assert_eq!(line.kind, LineKind::Part);
    assert_eq!(line.confidence, 1.0);
}

#[test]
fn line_2_mcp6002_parses_exactly() {
    let invoice = invoice();
    let line = find_line(&invoice.lines, "MCP6002-I/P-ND");
    assert_eq!(line.mpn.as_deref(), Some("MCP6002-I/P"));
    assert_eq!(line.manufacturer.as_deref(), Some("MICROCHIP TECHNOLOGY"));
    assert_eq!(
        line.description.as_deref(),
        Some("IC OPAMP GP 2 CIRCUIT 8DIP")
    );
    assert_eq!(line.shipped, Some(Quantity::from_whole(3).unwrap()));
    assert_eq!(line.unit_price.as_ref().unwrap().micros, 440_000);
    assert_eq!(line.extended_price.as_ref().unwrap().micros, 1_320_000);
}

#[test]
fn line_3_ne555p_parses_exactly() {
    let invoice = invoice();
    let line = find_line(&invoice.lines, "296-NE555P-ND");
    assert_eq!(line.mpn.as_deref(), Some("NE555P"));
    assert_eq!(line.manufacturer.as_deref(), Some("TEXAS INSTRUMENTS"));
    assert_eq!(
        line.description.as_deref(),
        Some("IC OSC SINGLE TIMER 100KHZ 8-DIP")
    );
    assert_eq!(line.shipped, Some(Quantity::from_whole(3).unwrap()));
    assert_eq!(line.unit_price.as_ref().unwrap().micros, 560_000);
    assert_eq!(line.extended_price.as_ref().unwrap().micros, 1_680_000);
}

#[test]
fn line_4_ltv817_parses_exactly() {
    let invoice = invoice();
    let line = find_line(&invoice.lines, "160-1366-5-ND");
    assert_eq!(line.mpn.as_deref(), Some("LTV-817"));
    assert_eq!(line.manufacturer.as_deref(), Some("LITEON"));
    assert_eq!(
        line.description.as_deref(),
        Some("OPTOISOLATOR 5KV 1CH TRANS 4-DIP")
    );
    assert_eq!(line.shipped, Some(Quantity::from_whole(3).unwrap()));
    assert_eq!(line.unit_price.as_ref().unwrap().micros, 370_000);
    assert_eq!(line.extended_price.as_ref().unwrap().micros, 1_110_000);
}

#[test]
fn line_5_mcp1702_parses_exactly_across_the_page_break() {
    let invoice = invoice();
    let line = find_line(&invoice.lines, "MCP1702-5002E/TO-ND");
    assert_eq!(line.mpn.as_deref(), Some("MCP1702-5002E/TO"));
    assert_eq!(line.manufacturer.as_deref(), Some("MICROCHIP TECHNOLOGY"));
    assert_eq!(
        line.description.as_deref(),
        Some("IC REG LINEAR 5V 250MA TO92-3")
    );
    assert_eq!(line.shipped, Some(Quantity::from_whole(3).unwrap()));
    assert_eq!(line.unit_price.as_ref().unwrap().micros, 650_000);
    assert_eq!(line.extended_price.as_ref().unwrap().micros, 1_950_000);
}

#[test]
fn line_6_lm393n_parses_exactly() {
    let invoice = invoice();
    let line = find_line(&invoice.lines, "LM393NNS/NOPB-ND");
    assert_eq!(line.mpn.as_deref(), Some("LM393N/NOPB"));
    assert_eq!(line.manufacturer.as_deref(), Some("TEXAS INSTRUMENTS"));
    assert_eq!(
        line.description.as_deref(),
        Some("IC COMPARATOR 2CH 8-PDIP")
    );
    assert_eq!(line.shipped, Some(Quantity::from_whole(3).unwrap()));
    assert_eq!(line.unit_price.as_ref().unwrap().micros, 920_000);
    assert_eq!(line.extended_price.as_ref().unwrap().micros, 2_760_000);
}

#[test]
fn unit_price_times_shipped_matches_extended_price_for_every_line() {
    let invoice = invoice();
    for line in part_lines(&invoice) {
        let unit = line.unit_price.as_ref().unwrap();
        let shipped = line.shipped.unwrap();
        let extended = line.extended_price.as_ref().unwrap();
        let expected = unit.micros * shipped.as_milli() / 1000;
        assert_eq!(
            expected, extended.micros,
            "sku {:?}: unit_price x shipped should equal extended_price",
            line.supplier_sku
        );
    }
    assert!(
        invoice.warnings.is_empty(),
        "no consistency warnings expected for the real fixture, got: {:?}",
        invoice.warnings
    );
}

#[test]
fn totals_parse_exactly_and_reconcile() {
    let invoice = invoice();
    let order = &invoice.order;
    assert_eq!(order.subtotal.as_ref().unwrap().micros, 14_280_000);
    assert_eq!(order.tariff.as_ref().unwrap().micros, 2_870_000);
    assert_eq!(order.shipping.as_ref().unwrap().micros, 4_990_000);
    assert_eq!(order.tax.as_ref().unwrap().micros, 1_240_000);
    assert_eq!(order.total.as_ref().unwrap().micros, 23_380_000);

    let sum = order.subtotal.as_ref().unwrap().micros
        + order.tariff.as_ref().unwrap().micros
        + order.shipping.as_ref().unwrap().micros
        + order.tax.as_ref().unwrap().micros;
    assert_eq!(
        sum,
        order.total.as_ref().unwrap().micros,
        "subtotal + tariff + shipping + tax should equal total"
    );
}

#[test]
fn raw_json_preserves_extracted_text_per_line() {
    let invoice = invoice();
    let line = find_line(&invoice.lines, "475-BPW34-ND");
    let raw = line.raw.as_object().expect("raw must be a JSON object");
    assert_eq!(
        raw.get("supplier_sku").and_then(|v| v.as_str()),
        Some("475-BPW34-ND")
    );
    assert_eq!(
        raw.get("unit_price").and_then(|v| v.as_str()),
        Some("1.82000")
    );
    assert_eq!(
        raw.get("extended_price").and_then(|v| v.as_str()),
        Some("5.46")
    );
}

/// A canned [`PdfTextSource`] proving `DigiKeyPdfParser::parse` wires
/// `source.extract` through to [`reconstruct`] — no pdfium involved, just
/// the same committed fixture returned directly.
struct FixtureTextSource;

impl PdfTextSource for FixtureTextSource {
    fn extract(&self, _bytes: &[u8]) -> Result<Vec<PositionedToken>, ImportError> {
        Ok(load_token_fixture("digikey_po_100353602.tokens.json"))
    }
}

#[test]
fn parser_wires_source_extraction_through_to_reconstruct() {
    let parser = DigiKeyPdfParser::new(FixtureTextSource);
    assert_eq!(parser.supplier(), "DigiKey");
    assert_eq!(parser.source_format(), SourceFormat::Pdf);

    let invoice = parser.parse(b"pretend pdf bytes").unwrap();
    assert_eq!(part_lines(&invoice).len(), 6);
    assert_eq!(invoice.order.order_number.as_deref(), Some("100353602"));
}

#[test]
fn parser_reports_empty_bytes_without_calling_the_source() {
    let parser = DigiKeyPdfParser::new(FixtureTextSource);
    let err = parser.parse(b"").unwrap_err();
    assert!(matches!(err, ImportError::Empty));
}
