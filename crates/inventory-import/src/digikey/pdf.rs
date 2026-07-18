//! DigiKey PDF PO Acknowledgement table reconstruction.
//!
//! A PDF carries no rows/columns, only positioned words. This module turns
//! [`PositionedToken`]s (already normalized to top-left/`y`-down PDF points
//! — see `crate::pdf`) back into the invoice's metadata + line-item table by
//! (1) clustering tokens into visual rows by `y`, (2) deriving column
//! x-bands from the header row's own token positions, then (3) reading each
//! `PART:`/`DESC:`/`MFG :` triple and the totals block off those rows/bands.
//! [`reconstruct`] is the pure core — driven directly by
//! `crate::pdf::load_token_fixture` in tests, no pdfium involved.
//!
//! **Row tolerance:** inspecting `tests/fixtures/digikey_po_100353602.tokens.json`
//! directly (not the `pdftotext -layout` `.txt` sibling — see below) shows a
//! PART row's own two text baselines (the `PART:`/`DESC:` label line and the
//! qty/price/amount number line just under it) sit only ~1.0-1.1pt apart,
//! while distinct visual rows (PART row -> its `MFG :` row, or one label row
//! in the totals block -> the next) are always >= 8.6pt apart. [`ROW_Y_TOLERANCE`]
//! (4.5pt) sits between those two magnitudes, so it merges same-row text
//! while keeping real rows separate.
//!
//! **The "wrapping" amount, resolved:** the plan's design note warns that
//! DigiKey's extended-amount column can land away from the `PART:` row. The
//! sanitized `.txt` (`pdftotext -layout`) fixture *looks* like exactly that
//! for line 1: `5.46` appears to visually drift toward the `TARIFF` sub-row
//! a few lines down. Checking the real token coordinates shows this is a
//! `pdftotext -layout` rendering artifact, not the underlying geometry: the
//! `Amount`-band token for every one of the 6 real lines has a `y` within
//! ~1.1pt of that same line's `PART:` token (i.e. the same row-cluster), and
//! the next `Amount`-band token after it (the `TARIFF` sub-row's own
//! duty amount) is always >= 34pt further down. [`extract_lines`] still
//! implements the general algorithm the plan describes — scan the whole
//! block from a `PART:` row down to just before the next one, and take the
//! *first* `Amount`-band token found — rather than hard-assuming same-row
//! placement, so a real invoice where the amount genuinely wraps further
//! down would still resolve correctly; it just happens to resolve on the
//! first row for this fixture.
//!
//! **Scope (Task 8 vs Task 9):** noise rows (`ECCN`, `HTSUS`, `ROHS3...`,
//! `Mercury:`, footer/page-number rows) are never turned into lines because
//! line creation is keyed *only* on the literal `PART:` token — they are
//! simply never visited by [`extract_lines`], not explicitly classified.
//! `TARIFF` sub-rows are read only far enough to be correctly *excluded*
//! from a line's own extended-price lookup (see above); giving them their
//! own [`crate::model::LineKind::Tariff`] line, wrapped descriptions,
//! missing-MPN confidence, and an explicit `Unknown`/warning path for
//! unrecognized rows are Task 9.

use std::collections::BTreeSet;

use crate::digikey::row::CURRENCY;
use crate::model::{LineKind, Money, ParsedInvoice, ParsedLine, ParsedOrderMeta, SourceFormat};
use crate::parser::{ImportError, InvoiceParser};
use crate::pdf::{PdfTextSource, PositionedToken};
use inventory_core::quantity::Quantity;

/// See the module doc's "Row tolerance" section for how this was derived
/// from the fixture's actual token `y` values.
const ROW_Y_TOLERANCE: f32 = 4.5;

/// [`InvoiceParser`] for DigiKey's PDF "PO Acknowledgement" document.
/// Generic over [`PdfTextSource`] so unit tests can inject a fixture-backed
/// source (no pdfium) while the app uses `pdf::PdfiumTextSource` at runtime.
pub struct DigiKeyPdfParser<S: PdfTextSource> {
    pub source: S,
}

impl<S: PdfTextSource> DigiKeyPdfParser<S> {
    pub fn new(source: S) -> Self {
        DigiKeyPdfParser { source }
    }
}

impl<S: PdfTextSource> InvoiceParser for DigiKeyPdfParser<S> {
    fn supplier(&self) -> &str {
        "DigiKey"
    }

    fn source_format(&self) -> SourceFormat {
        SourceFormat::Pdf
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedInvoice, ImportError> {
        if bytes.is_empty() {
            return Err(ImportError::Empty);
        }
        let tokens = self.source.extract(bytes)?;
        Ok(reconstruct(&tokens))
    }
}

/// One visual row: tokens whose `y` clustered together, ordered left to
/// right (`x` ascending) once the row is finalized.
type Row<'a> = Vec<&'a PositionedToken>;

/// Cluster `tokens` (already filtered to one page) into rows by `y`
/// proximity ([`ROW_Y_TOLERANCE`]), then order rows top-to-bottom and each
/// row's tokens left-to-right.
fn group_rows<'a>(tokens: &[&'a PositionedToken]) -> Vec<Row<'a>> {
    let mut sorted = tokens.to_vec();
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y).then_with(|| a.x.total_cmp(&b.x)));

    let mut rows: Vec<Row> = Vec::new();
    for tok in sorted {
        match rows.last_mut() {
            // Compare against the row's first (smallest-y) token, not a
            // running average, so tolerance doesn't drift across a long
            // chain of near-duplicate y values.
            Some(row) if (tok.y - row[0].y).abs() <= ROW_Y_TOLERANCE => row.push(tok),
            _ => rows.push(vec![tok]),
        }
    }
    for row in &mut rows {
        row.sort_by(|a, b| a.x.total_cmp(&b.x));
    }
    rows
}

/// Column x-band boundaries derived from one page's header row tokens
/// (`Line`, `Ordered`, `Available`, `Backordered`, `Unit`, `Amount`). The
/// `Item Number/Description` column deliberately has no boundary of its
/// own here: its header label is positioned oddly relative to where
/// `PART:`/`DESC:` data actually starts (see [`derive_bands`]), so
/// line-item content is instead located by the literal `PART:`/`DESC:`
/// tokens and just bounded on the right by `unit_price_start`.
struct Bands {
    line_number_end: f32,
    ordered_end: f32,
    available_end: f32,
    unit_price_start: f32,
    amount_start: f32,
}

/// Locate the header tokens on one page and derive [`Bands`] from their `x`
/// positions. Returns `None` if a page doesn't carry a recognizable DigiKey
/// header (so the caller can skip line-item extraction on that page rather
/// than guessing).
fn derive_bands(tokens: &[&PositionedToken]) -> Option<Bands> {
    let find = |text: &str| tokens.iter().find(|t| t.text == text).map(|t| t.x);
    let line_x = find("Line")?;
    let ordered_x = find("Ordered")?;
    let available_x = find("Available")?;
    let backordered_x = find("Backordered")?;
    let unit_price_x = find("Unit")?;
    let amount_x = find("Amount")?;

    Some(Bands {
        line_number_end: (line_x + ordered_x) / 2.0,
        ordered_end: (ordered_x + available_x) / 2.0,
        available_end: (available_x + backordered_x) / 2.0,
        unit_price_start: unit_price_x,
        amount_start: amount_x,
    })
}

enum QtyColumn {
    LineNumber,
    Ordered,
    Available,
    Backordered,
    Other,
}

fn classify_qty_x(x: f32, bands: &Bands) -> QtyColumn {
    if x < bands.line_number_end {
        QtyColumn::LineNumber
    } else if x < bands.ordered_end {
        QtyColumn::Ordered
    } else if x < bands.available_end {
        QtyColumn::Available
    } else if x < bands.unit_price_start {
        QtyColumn::Backordered
    } else {
        QtyColumn::Other
    }
}

/// The literal text of the first token found within `block` (scanned in
/// row order, i.e. top-to-bottom then left-to-right) whose `x` falls in
/// `[start, end)`. Used for both a line's own Unit Price/Amount lookup
/// (`block` = one line's rows) and totals extraction (`block` = the whole
/// page, `start`/`end` = the Amount band).
fn find_band_text(block: &[Row<'_>], start: f32, end: f32) -> Option<String> {
    for row in block {
        for tok in row {
            if tok.x >= start && tok.x < end {
                return Some(tok.text.clone());
            }
        }
    }
    None
}

/// Find a literal token sequence (e.g. `["Order", "Date:"]`) anywhere
/// within any row, and return the text of the token immediately following
/// it in that row. Position-based (not row-prefix-based) so an unrelated
/// token merged into the same row cluster ahead of the label (e.g. `"General -"`
/// ahead of `"WEB ORDER ID:"`, both close enough in `y` to share a row)
/// doesn't break the match.
fn value_after_sequence(rows: &[Row<'_>], sequence: &[&str]) -> Option<String> {
    for row in rows {
        for start in 0..row.len() {
            if start + sequence.len() > row.len() {
                break;
            }
            let is_match = sequence
                .iter()
                .enumerate()
                .all(|(i, expected)| row[start + i].text == *expected);
            if is_match {
                if let Some(value) = row.get(start + sequence.len()) {
                    return Some(value.text.clone());
                }
            }
        }
    }
    None
}

fn extract_order_number(rows: &[Row<'_>], order: &mut ParsedOrderMeta) {
    if order.order_number.is_none() {
        order.order_number = value_after_sequence(rows, &["Acknowledgement"]);
    }
}

fn extract_order_date(rows: &[Row<'_>], order: &mut ParsedOrderMeta) {
    if order.order_date.is_none() {
        order.order_date = value_after_sequence(rows, &["Order", "Date:"]);
    }
}

fn extract_web_order_id(rows: &[Row<'_>], order: &mut ParsedOrderMeta) {
    if order.web_order_id.is_none() {
        order.web_order_id = value_after_sequence(rows, &["WEB", "ORDER", "ID:"]);
    }
}

/// The PO Acknowledgement states its currency as a literal `USD` token
/// (repeated in several places: header, column sub-labels, totals). Any
/// occurrence confirms the same code, so the first one found wins;
/// `order.currency` is already defaulted to [`CURRENCY`] before this runs.
fn extract_currency(tokens: &[&PositionedToken], order: &mut ParsedOrderMeta) {
    if let Some(t) = tokens.iter().find(|t| t.text == "USD") {
        order.currency = t.text.clone();
    }
}

/// Map the 5 known totals labels to `order`'s fields. A row is split at
/// `bands.amount_start`: tokens left of it are the label, the first token
/// at or past it is the value — which also naturally excludes rows with no
/// Amount-band token (nothing to pair) and rows that are pure header/label
/// noise (nothing on the label side). Matching is on the *exact* joined
/// label text, which is what keeps `"Sales Amount"` from also matching the
/// distractor row `"Total Sales and Estimated Tariff Amount"` (that row's
/// own value, 17.15, is simply never assigned to anything).
fn extract_totals(rows: &[Row<'_>], bands: &Bands, order: &mut ParsedOrderMeta) {
    for row in rows {
        let mut label_parts: Vec<&str> = Vec::new();
        let mut value_text: Option<&str> = None;
        for tok in row {
            if tok.x < bands.amount_start {
                label_parts.push(tok.text.as_str());
            } else if value_text.is_none() {
                value_text = Some(tok.text.as_str());
            }
        }
        if label_parts.is_empty() {
            continue;
        }
        let Some(value_text) = value_text else {
            continue;
        };
        let Some(money) = Money::parse(value_text, CURRENCY) else {
            continue;
        };

        match label_parts.join(" ").as_str() {
            "Sales Amount" => order.subtotal = Some(money),
            "Estimated Tariff Amount" => order.tariff = Some(money),
            "Shipping charges applied" => order.shipping = Some(money),
            "Sales Tax" => order.tax = Some(money),
            "Total" => order.total = Some(money),
            _ => {}
        }
    }
}

/// Build a line's identity/description/quantity fields from its `PART:`
/// row. Unit price and extended price are deliberately left `None` here —
/// [`extract_lines`] fills them from the wider block scan (see the module
/// doc's "wrapping" section) — and manufacturer/mpn are filled from the
/// following `MFG :` row.
fn parse_part_row(row: &Row<'_>, bands: &Bands, fallback_line_number: u32) -> ParsedLine {
    let part_idx = row.iter().position(|t| t.text == "PART:");
    let supplier_sku = part_idx
        .and_then(|i| row.get(i + 1))
        .map(|t| t.text.clone());

    let desc_idx = row.iter().position(|t| t.text == "DESC:");
    let description = desc_idx.and_then(|i| {
        let words: Vec<&str> = row[i + 1..]
            .iter()
            .take_while(|t| t.x < bands.unit_price_start)
            .map(|t| t.text.as_str())
            .collect();
        (!words.is_empty()).then(|| words.join(" "))
    });

    let mut line_number: Option<u32> = None;
    let mut ordered_raw: Option<i64> = None;
    let mut available_raw: Option<i64> = None;
    let mut backordered_raw: Option<i64> = None;

    for tok in row.iter() {
        if tok.text.is_empty() || !tok.text.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        match classify_qty_x(tok.x, bands) {
            QtyColumn::LineNumber => line_number = tok.text.parse().ok(),
            QtyColumn::Ordered => ordered_raw = tok.text.parse().ok(),
            QtyColumn::Available => available_raw = tok.text.parse().ok(),
            QtyColumn::Backordered => backordered_raw = tok.text.parse().ok(),
            QtyColumn::Other => {}
        }
    }

    ParsedLine {
        line_number: line_number.or(Some(fallback_line_number)),
        supplier_sku,
        mpn: None,
        manufacturer: None,
        description,
        ordered: ordered_raw.and_then(|v| Quantity::from_whole(v).ok()),
        shipped: available_raw.and_then(|v| Quantity::from_whole(v).ok()),
        backordered: backordered_raw.and_then(|v| Quantity::from_whole(v).ok()),
        unit_price: None,
        extended_price: None,
        packaging: None,
        customer_reference: None,
        kind: LineKind::Part,
        confidence: 1.0,
        raw: serde_json::Value::Null,
    }
}

/// Fill `manufacturer`/`mpn` from a `MFG : <manufacturer> / <mpn>` row.
/// Manufacturer may be several tokens ("AMS-OSRAM USA INC."); mpn is
/// everything after the `/` token (itself possibly a single hyphenated
/// token like "MCP6002-I/P" that happens to contain its own `/`).
fn fill_manufacturer_mpn(mfg_row: &Row<'_>, line: &mut ParsedLine) {
    let colon_idx = mfg_row.iter().position(|t| t.text == ":");
    let slash_idx = mfg_row.iter().position(|t| t.text == "/");
    let (Some(colon_idx), Some(slash_idx)) = (colon_idx, slash_idx) else {
        return;
    };
    if slash_idx <= colon_idx {
        return;
    }

    let manufacturer = mfg_row[colon_idx + 1..slash_idx]
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mpn = mfg_row[slash_idx + 1..]
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    if !manufacturer.is_empty() {
        line.manufacturer = Some(manufacturer);
    }
    if !mpn.is_empty() {
        line.mpn = Some(mpn);
    }
}

/// Self-check only — never "corrects" a parsed value, just flags a
/// disagreement so a caller/reviewer can look closer. Uses integer micros
/// throughout (no `f64`), matching every other money computation in this
/// crate.
fn check_price_consistency(line: &ParsedLine, warnings: &mut Vec<String>) {
    if let (Some(unit), Some(shipped), Some(extended)) =
        (&line.unit_price, line.shipped, &line.extended_price)
    {
        let expected_micros = unit.micros * shipped.as_milli() / 1000;
        if expected_micros != extended.micros {
            warnings.push(format!(
                "line {:?} ({}): unit_price x shipped = {expected_micros} micros, but extended_price is {} micros — keeping the parsed extended_price as-is",
                line.line_number,
                line.supplier_sku.as_deref().unwrap_or("unknown"),
                extended.micros
            ));
        }
    }
}

/// Snapshot of what the parser actually read for this line, independent of
/// how well the typed fields above captured it — the PDF equivalent of the
/// CSV/XLSX parsers' original-header-and-cell `raw` (there is no natural
/// "header/cell" in a PDF, so this is the extracted text per logical
/// field instead).
fn build_raw(
    line: &ParsedLine,
    unit_price_text: Option<&str>,
    extended_price_text: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "line_number": line.line_number,
        "supplier_sku": line.supplier_sku,
        "description": line.description,
        "ordered": line.ordered.map(|q| q.as_milli() / 1000),
        "shipped": line.shipped.map(|q| q.as_milli() / 1000),
        "backordered": line.backordered.map(|q| q.as_milli() / 1000),
        "unit_price": unit_price_text,
        "extended_price": extended_price_text,
        "manufacturer": line.manufacturer,
        "mpn": line.mpn,
    })
}

/// Extract every `PART:` line on one page. For each, the "block" of rows
/// from its own `PART:` row up to (but excluding) the next `PART:` row (or
/// the end of the page's rows) is where its Unit Price/Amount and following
/// `MFG :` row are looked for — see the module doc for why a block scan
/// rather than same-row-only, even though this fixture resolves on the
/// first row of the block either way.
fn extract_lines(
    rows: &[Row<'_>],
    bands: &Bands,
    line_offset: u32,
    warnings: &mut Vec<String>,
) -> Vec<ParsedLine> {
    let part_indices: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.iter().any(|t| t.text == "PART:"))
        .map(|(i, _)| i)
        .collect();

    let mut lines = Vec::with_capacity(part_indices.len());
    for (k, &start) in part_indices.iter().enumerate() {
        let end = part_indices.get(k + 1).copied().unwrap_or(rows.len());
        let block = &rows[start..end];
        let part_row = &block[0];

        let mut line = parse_part_row(part_row, bands, line_offset + k as u32 + 1);

        let unit_price_text = find_band_text(block, bands.unit_price_start, bands.amount_start);
        let extended_price_text = find_band_text(block, bands.amount_start, f32::INFINITY);
        line.unit_price = unit_price_text
            .as_deref()
            .and_then(|t| Money::parse(t, CURRENCY));
        line.extended_price = extended_price_text
            .as_deref()
            .and_then(|t| Money::parse(t, CURRENCY));

        if let Some(mfg_row) = block.iter().find(|row| row.iter().any(|t| t.text == "MFG")) {
            fill_manufacturer_mpn(mfg_row, &mut line);
        }

        check_price_consistency(&line, warnings);
        line.raw = build_raw(
            &line,
            unit_price_text.as_deref(),
            extended_price_text.as_deref(),
        );

        lines.push(line);
    }
    lines
}

/// The pure reconstruction core: turns a flat token list (spanning one or
/// more pages) into a [`ParsedInvoice`]. Every unit test in
/// `tests/digikey_pdf.rs` drives this directly via
/// `crate::pdf::load_token_fixture` — no [`PdfTextSource`] involved.
pub fn reconstruct(tokens: &[PositionedToken]) -> ParsedInvoice {
    let mut order = ParsedOrderMeta {
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
    };
    let mut lines: Vec<ParsedLine> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let pages: BTreeSet<u32> = tokens.iter().map(|t| t.page).collect();

    for page in pages {
        let page_tokens: Vec<&PositionedToken> = tokens.iter().filter(|t| t.page == page).collect();
        let rows = group_rows(&page_tokens);

        extract_order_number(&rows, &mut order);
        extract_order_date(&rows, &mut order);
        extract_web_order_id(&rows, &mut order);
        extract_currency(&page_tokens, &mut order);

        match derive_bands(&page_tokens) {
            Some(bands) => {
                extract_totals(&rows, &bands, &mut order);
                let page_lines = extract_lines(&rows, &bands, lines.len() as u32, &mut warnings);
                lines.extend(page_lines);
            }
            None => {
                warnings.push(format!(
                    "page {page}: could not locate the DigiKey table column headers; line items on this page were not extracted"
                ));
            }
        }
    }

    ParsedInvoice {
        supplier: "DigiKey".to_string(),
        source_format: SourceFormat::Pdf,
        order,
        lines,
        warnings,
    }
}
