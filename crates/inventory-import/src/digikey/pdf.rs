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
//! **Scope (Task 8 vs Task 9):** Task 8 relied on noise rows (`ECCN`,
//! `HTSUS`, `ROHS3...`, `Mercury:`, footer/page-number rows) simply never
//! being visited, because line creation is keyed *only* on the literal
//! `PART:` token. Task 9 makes that skipping **explicit** — a battery of
//! `is_*_row` predicates below name each category so the behavior is
//! documented and independently unit-tested rather than an accident of
//! which tokens `extract_lines` happens to look for. Task 9 also adds:
//! `TARIFF` sub-rows now become their own [`LineKind::Tariff`] line (see
//! "TARIFF sub-rows, resolved" below); a description that wraps onto a
//! second row (before the `MFG :` row) is stitched onto the line instead of
//! lost; a `PART:` line whose `MFG :` row is missing/has no `/ MPN` keeps
//! `mpn: None` and drops `confidence` below 1.0 instead of silently
//! pretending it's fully known; and any row inside the line-item table that
//! matches none of the above — but isn't part of the header, totals block,
//! or footer either — becomes a [`LineKind::Unknown`] line plus a
//! `warnings` entry, so nothing is ever dropped without a trace.
//!
//! **TARIFF sub-rows, resolved:** the [`ParsedLine`] model has no
//! "associated tariff" field (Task 8's model, deliberately unmodified — see
//! the plan). So a `TARIFF` row is emitted as its own
//! [`LineKind::Tariff`] line rather than folded into the preceding part:
//! `line_number` is copied from the part line it immediately follows (the
//! only field this crate's model offers for expressing "belongs to line
//! N"), and `extended_price` is the tariff amount read from the Amount
//! x-band. This keeps the invariant simple and testable ("the Tariff line
//! with `line_number == Some(1)` is line 1's tariff") without inventing an
//! unmodeled field, at the cost of the caller needing to join on
//! `line_number` to associate a tariff back to its part — acceptable since
//! 5b (matching/commit) already keys everything off `line_number`.
//!
//! **Unknown-row scope:** to avoid false positives, the Unknown scan is
//! bounded to the rows strictly between a page's column-header rows and its
//! end (see [`table_body_start`]) and explicitly excludes every row shape
//! this module already understands: header, noise/footer, `PART:`, `MFG`,
//! `TARIFF`, a totals-block label row (matched by exact label text, the
//! same set [`extract_totals`] switches on), and the `WEB ORDER ID:` row.
//! Rows *before* the header (the Bill To/Ship To/Buyer address block — PII,
//! deliberately never parsed into fields) are outside this range on
//! purpose, so they never spuriously surface as "unknown" warnings.

use std::collections::BTreeSet;

use crate::digikey::row::CURRENCY;
use crate::model::{LineKind, Money, ParsedInvoice, ParsedLine, ParsedOrderMeta, SourceFormat};
use crate::parser::{ImportError, InvoiceParser};
use crate::pdf::{ExtractionSource, PdfTextSource, PositionedToken, TextExtraction};
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
        let extraction = TextExtraction::classify(tokens);
        Ok(match extraction.source {
            // See `scanned_pdf_invoice`'s doc: 5a ships only this
            // detect-and-warn branch, never a fabricated reconstruction of
            // near-empty input. Real OCR is deferred (docs/known-limitations.md).
            ExtractionSource::Ocr => scanned_pdf_invoice(),
            ExtractionSource::BornDigital => reconstruct(&extraction.tokens),
        })
    }
}

/// The `ParsedInvoice` produced when [`TextExtraction::classify`] decides a
/// PDF's text extraction yielded ~no tokens — almost certainly a scanned
/// image page with no embedded text layer. No order metadata or line items
/// are fabricated (there is nothing to reconstruct from); the single
/// `warnings` entry names the situation and the two ways around it
/// available today (manual correction, or re-exporting/uploading CSV or
/// XLSX instead). Real Windows-OCR is explicitly deferred past Phase 5a —
/// see `docs/known-limitations.md` — this is the whole of 5a's OCR-branch
/// contract: a defined, low-confidence signal for 5d's manual-correction UI
/// to build against, not an actual OCR pass.
fn scanned_pdf_invoice() -> ParsedInvoice {
    ParsedInvoice {
        supplier: "DigiKey".to_string(),
        source_format: SourceFormat::Pdf,
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
        lines: Vec::new(),
        warnings: vec![
            "scanned PDF — OCR not yet available; use manual correction or upload CSV/XLSX"
                .to_string(),
        ],
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

// ---------------------------------------------------------------------
// Row classification (Task 9) — explicit, individually-tested predicates
// for every row shape this document is known to produce. `extract_lines`
// uses the identity-bearing ones (`PART:`/`MFG`/`TARIFF`) to build line
// items; `is_known_table_row` (further below, needs `Bands`) is the
// union used to decide what's *left over* for the Unknown/Fee scan.
// ---------------------------------------------------------------------

/// Column-header words that only ever appear on the two-page-repeated
/// header block, never inside PART/DESC/MFG/totals/footer data. `Line`
/// alone would suffice for the header's first sub-row, but the header is
/// itself wrapped across three separate visual rows (`Line .. Amount` /
/// `Ordered Item Number/ Description` / `Item Qty Qty USD $ USD $` — see
/// the fixture dump in the module doc), so each marker here catches one of
/// those three sub-rows independently.
const HEADER_MARKERS: [&str; 5] = ["Line", "Ordered", "Available", "Backordered", "Qty"];

fn is_header_row(row: &Row<'_>) -> bool {
    row.iter()
        .any(|t| HEADER_MARKERS.contains(&t.text.as_str()))
}

/// Rows that must never become a line and never corrupt the line they sit
/// under: the ECCN/HTSUS export-control row, the ROHS3 compliance row, the
/// Mercury disclosure row, the "All transactions with DigiKey..." /
/// "Page X of Y" footer, and a lone "USD $" echo row (the totals block's
/// trailing currency-unit line — already captured once via
/// [`extract_currency`], so a second bare occurrence carries no new data).
fn is_noise_row(row: &Row<'_>) -> bool {
    let Some(first) = row.first() else {
        return false;
    };
    if matches!(first.text.as_str(), "ECCN:" | "ROHS3" | "Mercury:" | "Page") {
        return true;
    }
    // HTSUS shares ECCN's row in the sample ("ECCN: EAR99  HTSUS: ...") but
    // is checked independently (not just via the ECCN-first-token case
    // above) in case a future layout ever splits it onto its own row.
    if row.iter().any(|t| t.text == "HTSUS:") {
        return true;
    }
    if row.len() >= 2 && row[0].text == "All" && row[1].text == "transactions" {
        return true;
    }
    if row.iter().all(|t| t.text == "USD" || t.text == "$") {
        return true;
    }
    false
}

fn is_part_row(row: &Row<'_>) -> bool {
    row.iter().any(|t| t.text == "PART:")
}

fn is_mfg_row(row: &Row<'_>) -> bool {
    row.iter().any(|t| t.text == "MFG")
}

fn is_tariff_row(row: &Row<'_>) -> bool {
    row.first().is_some_and(|t| t.text == "TARIFF")
}

/// True if `sequence` (e.g. `["WEB", "ORDER", "ID:"]`) appears verbatim,
/// contiguously, anywhere in `row`. Same matching shape as
/// [`value_after_sequence`] but only answers "is this that row", not "what
/// value follows".
fn row_contains_sequence(row: &Row<'_>, sequence: &[&str]) -> bool {
    if sequence.len() > row.len() {
        return false;
    }
    (0..=row.len() - sequence.len()).any(|start| {
        sequence
            .iter()
            .enumerate()
            .all(|(i, expected)| row[start + i].text == *expected)
    })
}

fn is_web_order_id_row(row: &Row<'_>) -> bool {
    row_contains_sequence(row, &["WEB", "ORDER", "ID:"])
}

/// The exact totals-block label strings [`extract_totals`] switches on,
/// plus the one known "distractor" label
/// (`Total Sales and Estimated Tariff Amount`) whose own value is never
/// assigned to any field. Matched here by *exact* joined-label text (not
/// just "has a label and a value shape") specifically so a crafted fee row
/// with a different label (e.g. `SHIPPING CHARGES`) is never mistaken for
/// a totals row — see `is_totals_label_row`.
const TOTALS_LABELS: [&str; 6] = [
    "Sales Amount",
    "Estimated Tariff Amount",
    "Total Sales and Estimated Tariff Amount",
    "Shipping charges applied",
    "Sales Tax",
    "Total",
];

/// True if every token left of the Amount x-band, joined with spaces,
/// exactly equals one of [`TOTALS_LABELS`] — i.e. this row belongs to the
/// totals block (already handled by [`extract_totals`]) and must not be
/// re-classified as a line item.
fn is_totals_label_row(row: &Row<'_>, bands: &Bands) -> bool {
    let label: Vec<&str> = row
        .iter()
        .filter(|t| t.x < bands.amount_start)
        .map(|t| t.text.as_str())
        .collect();
    if label.is_empty() {
        return false;
    }
    TOTALS_LABELS.contains(&label.join(" ").as_str())
}

/// Fee-style keywords a stray line-item-table row might carry (mirrors
/// `crate::digikey::row::FEE_KEYWORDS` minus `TARIFF` — a PDF `TARIFF`
/// sub-row is always per-line and already handled by [`is_tariff_row`],
/// never a standalone fee row in this document; `FREIGHT` stays, it's a
/// legitimate standalone fee keyword just like `SHIPPING`).
const FEE_KEYWORDS: [&str; 3] = ["SHIPPING", "FREIGHT", "TAX"];

/// The first fee keyword found in `row` (case-insensitive substring match
/// against each token), if any.
fn fee_keyword(row: &Row<'_>) -> Option<&'static str> {
    row.iter().find_map(|t| {
        let upper = t.text.to_ascii_uppercase();
        FEE_KEYWORDS.iter().copied().find(|kw| upper.contains(kw))
    })
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

/// The index of the first row of the "table body" — everything from just
/// after the last header sub-row to the end of `rows`. Rows before this
/// index (the Bill To/Ship To/Buyer address block) are never scanned for
/// Unknown/Fee rows; see the module doc's "Unknown-row scope" section.
/// Falls back to `0` (scan everything) if no header row is found at all —
/// this only happens when [`derive_bands`] already returned `None` and the
/// caller skipped line extraction entirely, so it's unreachable in
/// practice, not a silent behavior change.
fn table_body_start(rows: &[Row<'_>]) -> usize {
    rows.iter().rposition(is_header_row).map_or(0, |i| i + 1)
}

/// Union of every row shape this module already gives dedicated meaning
/// to. Anything at/after [`table_body_start`] that does *not* match this is
/// either a fee-style row or a genuinely unrecognized one — see
/// `extract_unclassified_lines`.
fn is_known_table_row(row: &Row<'_>, bands: &Bands) -> bool {
    is_header_row(row)
        || is_noise_row(row)
        || is_part_row(row)
        || is_mfg_row(row)
        || is_tariff_row(row)
        || is_totals_label_row(row, bands)
        || is_web_order_id_row(row)
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

    // Bound the qty digit-scan to tokens left of `PART:` (the Item
    // Number/Description column's own start). The Line/Ordered/Available/
    // Backordered columns are all strictly left of `PART:` in this table,
    // while the description that follows it can itself contain bare-digit
    // tokens (e.g. "GP 2 CIRCUIT"). Without this bound, `classify_qty_x`'s
    // wide Backordered band (`available_end..unit_price_start`, which spans
    // the entire Item Number/Description column — see [`Bands`]) would
    // misread such a description numeral as the Backordered quantity,
    // silently corrupting it. `row` is x-ascending (see [`group_rows`]), so
    // slicing up to `part_idx` is exactly "every token left of `PART:`".
    let qty_scan_end = part_idx.unwrap_or(row.len());
    for tok in row[..qty_scan_end].iter() {
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

/// Stitch a description that wraps onto a second row onto `line`. Scans
/// `block[1..]` (the rows right after the `PART:` row) and, for each row
/// that is *not* itself a recognized `MFG`/noise/`TARIFF` row, appends its
/// text (tokens left of `bands.unit_price_start`, same bound
/// [`parse_part_row`] uses for the first description row) to
/// `line.description`, then records that row's absolute index in
/// `consumed` (`block_start + offset`) so the Task 9 Unknown/Fee scan never
/// re-visits it. Stops at the first row it doesn't recognize as
/// continuation text (an `MFG`/noise/`TARIFF` row, or an empty one) —
/// for the base fixture that's always `block[1]` itself (the `MFG :` row
/// sits immediately under every `PART:` row), so this is a no-op there.
fn stitch_wrapped_description(
    block: &[Row<'_>],
    block_start: usize,
    bands: &Bands,
    line: &mut ParsedLine,
    consumed: &mut BTreeSet<usize>,
) {
    for (offset, row) in block.iter().enumerate().skip(1) {
        if is_mfg_row(row) || is_noise_row(row) || is_tariff_row(row) {
            break;
        }
        let words: Vec<&str> = row
            .iter()
            .filter(|t| t.x < bands.unit_price_start)
            .map(|t| t.text.as_str())
            .collect();
        if words.is_empty() {
            break;
        }
        let extra = words.join(" ");
        line.description = Some(match line.description.take() {
            Some(existing) => format!("{existing} {extra}"),
            None => extra,
        });
        consumed.insert(block_start + offset);
    }
}

/// Build the per-line [`LineKind::Tariff`] line for a `TARIFF` sub-row —
/// see the module doc's "TARIFF sub-rows, resolved" section for why this
/// is a separate line rather than a field on the part. `part_line_number`
/// is the preceding part's own `line_number`, copied here so the two rows
/// can be joined back together by the caller. Returns `None` if the row
/// carries no parseable Amount-band value (nothing to report).
fn build_tariff_line(
    row: &Row<'_>,
    bands: &Bands,
    part_line_number: Option<u32>,
) -> Option<ParsedLine> {
    let amount_token = row.iter().find(|t| t.x >= bands.amount_start)?;
    let extended_price = Money::parse(&amount_token.text, CURRENCY)?;
    Some(ParsedLine {
        line_number: part_line_number,
        supplier_sku: None,
        mpn: None,
        manufacturer: None,
        description: Some("TARIFF".to_string()),
        ordered: None,
        shipped: None,
        backordered: None,
        unit_price: None,
        extended_price: Some(extended_price),
        packaging: None,
        customer_reference: None,
        kind: LineKind::Tariff,
        confidence: 1.0,
        raw: serde_json::json!({
            "line_number": part_line_number,
            "amount": amount_token.text,
        }),
    })
}

/// Extract every `PART:` line on one page. For each, the "block" of rows
/// from its own `PART:` row up to (but excluding) the next `PART:` row (or
/// the end of the page's rows) is where its Unit Price/Amount, wrapped
/// description continuation, following `MFG :` row, and per-line `TARIFF`
/// row are looked for — see the module doc for why a block scan rather
/// than same-row-only, even though this fixture resolves on the first row
/// of the block either way.
///
/// Every row this function consumes as part of a block (the `PART:` row
/// itself, a wrapped-description continuation row, the `MFG` row, the
/// `TARIFF` row) has its absolute index recorded in `consumed` so
/// `extract_unclassified_lines` never re-classifies it. Returns the part
/// lines and the tariff lines separately — the caller decides how to merge
/// them into `ParsedInvoice.lines`.
fn extract_lines(
    rows: &[Row<'_>],
    bands: &Bands,
    line_offset: u32,
    consumed: &mut BTreeSet<usize>,
    warnings: &mut Vec<String>,
) -> (Vec<ParsedLine>, Vec<ParsedLine>) {
    let part_indices: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| is_part_row(row))
        .map(|(i, _)| i)
        .collect();

    let mut lines = Vec::with_capacity(part_indices.len());
    let mut tariff_lines = Vec::new();
    for (k, &start) in part_indices.iter().enumerate() {
        let end = part_indices.get(k + 1).copied().unwrap_or(rows.len());
        let block = &rows[start..end];
        let part_row = &block[0];

        let mut line = parse_part_row(part_row, bands, line_offset + k as u32 + 1);
        consumed.insert(start);

        stitch_wrapped_description(block, start, bands, &mut line, consumed);

        let unit_price_text = find_band_text(block, bands.unit_price_start, bands.amount_start);
        let extended_price_text = find_band_text(block, bands.amount_start, f32::INFINITY);
        line.unit_price = unit_price_text
            .as_deref()
            .and_then(|t| Money::parse(t, CURRENCY));
        line.extended_price = extended_price_text
            .as_deref()
            .and_then(|t| Money::parse(t, CURRENCY));

        if let Some((mfg_offset, mfg_row)) =
            block.iter().enumerate().find(|(_, row)| is_mfg_row(row))
        {
            fill_manufacturer_mpn(mfg_row, &mut line);
            consumed.insert(start + mfg_offset);
        }
        // A part row that never resolved an MPN (missing/malformed `MFG :`
        // row, or none at all) is still captured as a `Part` line — never
        // dropped — but flagged with reduced confidence, mirroring the
        // CSV/XLSX parsers' same convention in `crate::digikey::row`.
        if line.mpn.is_none() {
            line.confidence = line.confidence.min(0.8);
        }

        if let Some((tariff_offset, tariff_row)) =
            block.iter().enumerate().find(|(_, row)| is_tariff_row(row))
        {
            consumed.insert(start + tariff_offset);
            if let Some(tariff_line) = build_tariff_line(tariff_row, bands, line.line_number) {
                tariff_lines.push(tariff_line);
            }
        }

        check_price_consistency(&line, warnings);
        line.raw = build_raw(
            &line,
            unit_price_text.as_deref(),
            extended_price_text.as_deref(),
        );

        lines.push(line);
    }
    (lines, tariff_lines)
}

/// A standalone fee-style row (no `PART:`, a fee keyword like `SHIPPING`,
/// and a parseable Amount-band value) → [`LineKind::Fee`]. Returns `None`
/// if either half is missing, in which case the caller falls back to
/// `LineKind::Unknown` rather than guessing.
fn build_fee_line(row: &Row<'_>, bands: &Bands) -> Option<ParsedLine> {
    let keyword = fee_keyword(row)?;
    let amount_token = row.iter().find(|t| t.x >= bands.amount_start)?;
    let extended_price = Money::parse(&amount_token.text, CURRENCY)?;
    let label = row
        .iter()
        .filter(|t| t.x < bands.amount_start)
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Some(ParsedLine {
        line_number: None,
        supplier_sku: None,
        mpn: None,
        manufacturer: None,
        description: (!label.is_empty()).then(|| label.clone()),
        ordered: None,
        shipped: None,
        backordered: None,
        unit_price: None,
        extended_price: Some(extended_price),
        packaging: None,
        customer_reference: None,
        kind: LineKind::Fee,
        confidence: 1.0,
        raw: serde_json::json!({
            "row_text": label,
            "amount": amount_token.text,
            "fee_keyword": keyword,
        }),
    })
}

/// A leftover row inside the table body that matches no known shape and
/// carries no fee keyword — captured, never dropped, with reduced
/// confidence and a `warnings` entry the caller pushes alongside it.
fn build_unknown_line(row_text: &str) -> ParsedLine {
    ParsedLine {
        line_number: None,
        supplier_sku: None,
        mpn: None,
        manufacturer: None,
        description: Some(row_text.to_string()),
        ordered: None,
        shipped: None,
        backordered: None,
        unit_price: None,
        extended_price: None,
        packaging: None,
        customer_reference: None,
        kind: LineKind::Unknown,
        confidence: 0.3,
        raw: serde_json::json!({ "row_text": row_text }),
    }
}

/// Scan the table body (from [`table_body_start`] to the end of the page's
/// rows) for anything `extract_lines` didn't already consume and
/// [`is_known_table_row`] doesn't already explain. Each such row becomes
/// either a [`LineKind::Fee`] line (fee keyword + a parseable amount) or a
/// [`LineKind::Unknown`] line plus a `warnings` entry — see the module
/// doc's "Unknown-row scope" section for why the scan is bounded to the
/// table body rather than the whole page.
fn extract_unclassified_lines(
    rows: &[Row<'_>],
    bands: &Bands,
    scan_start: usize,
    consumed: &BTreeSet<usize>,
    page: u32,
    warnings: &mut Vec<String>,
) -> Vec<ParsedLine> {
    let mut out = Vec::new();
    for (idx, row) in rows.iter().enumerate().skip(scan_start) {
        if consumed.contains(&idx) || is_known_table_row(row, bands) {
            continue;
        }
        if let Some(fee_line) = build_fee_line(row, bands) {
            out.push(fee_line);
            continue;
        }
        let row_text = row
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        warnings.push(format!(
            "page {page}: unrecognized row inside the line-item table, classified as Unknown: \"{row_text}\""
        ));
        out.push(build_unknown_line(&row_text));
    }
    out
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

                // Fallback line numbering counts only `Part` lines seen so
                // far (not tariff/fee/unknown lines), so it stays aligned
                // with the document's own "Line Item" numbering even once
                // this function starts appending other kinds to `lines`.
                let part_line_count =
                    lines.iter().filter(|l| l.kind == LineKind::Part).count() as u32;
                let mut consumed: BTreeSet<usize> = BTreeSet::new();
                let (page_lines, tariff_lines) =
                    extract_lines(&rows, &bands, part_line_count, &mut consumed, &mut warnings);
                lines.extend(page_lines);
                lines.extend(tariff_lines);

                let scan_start = table_body_start(&rows);
                let leftover = extract_unclassified_lines(
                    &rows,
                    &bands,
                    scan_start,
                    &consumed,
                    page,
                    &mut warnings,
                );
                lines.extend(leftover);
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
