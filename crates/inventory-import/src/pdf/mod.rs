//! PDF positioned-text abstraction.
//!
//! PDFs don't carry rows/columns — a supplier's PDF table (DigiKey's PO
//! Acknowledgement, `crate::digikey::pdf`, Tasks 8/9) has to be
//! reconstructed from where each word sits on the page. This module owns
//! the shared vocabulary for that: [`PositionedToken`] (one word plus its
//! rectangle) and the [`text_source::PdfTextSource`] trait that produces a
//! `Vec<PositionedToken>` from raw PDF bytes.
//!
//! The one production `PdfTextSource` is [`text_source::PdfiumTextSource`],
//! backed by `pdfium-render` and gated behind the `pdfium` Cargo feature —
//! see `Cargo.toml` and `docs/build.md`. It is deliberately kept out of the
//! default build/test path: `pdfium-render`'s dynamic loading means the
//! crate still *compiles* without a `pdfium.dll` present, but *calling*
//! `PdfiumTextSource::new()` does need one, so unit tests never exercise it.
//! Instead, every reconstruction test (this module's own tests, and Task
//! 8/9's `crate::digikey::pdf` tests) runs against the committed
//! `PositionedToken` fixture read by [`load_token_fixture`] — a real
//! extraction, dumped once (by `samples/digikey/tools/dump_tokens.py`,
//! PyMuPDF) from a sanitized sample PDF, and frozen as JSON. Because both
//! `PdfiumTextSource` and that fixture-generating script normalize to the
//! exact same coordinate convention (see [`PositionedToken`]'s doc), the
//! DigiKey row/column reconstruction logic is source-independent: it cannot
//! tell whether it's looking at a live extraction or the fixture.

pub mod text_source;

pub use text_source::PdfTextSource;
#[cfg(feature = "pdfium")]
pub use text_source::PdfiumTextSource;

use serde::{Deserialize, Serialize};

/// One extracted, positioned word of text from a PDF page.
///
/// **Coordinate convention (binding on every [`PdfTextSource`]
/// implementation):** origin top-left, `y` increases DOWNWARD, units are
/// PDF points (1/72 inch). This is deliberately NOT pdfium's native
/// coordinate space (origin bottom-left, `y` increases upward) — every
/// source normalizes to *this* convention on the way out, so
/// `crate::digikey::pdf`'s row/column reconstruction never has to know or
/// care which source produced a given `Vec<PositionedToken>`. See
/// `text_source::pdfium_source` for the pdfium-side conversion math.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionedToken {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub page: u32,
}

/// Read a committed positioned-token JSON fixture for tests.
///
/// `name` is just the fixture's file name (e.g.
/// `"digikey_po_100353602.tokens.json"`); it's resolved
/// `CARGO_MANIFEST_DIR`-relative to `tests/fixtures/<name>` — the same
/// convention every other fixture in this crate uses (see
/// `digikey::csv`'s `fixture_path` helper).
///
/// This is a plain `pub fn`, not `#[cfg(test)]`, on purpose: an integration
/// test under `tests/` (a separate compilation unit from this library
/// crate, e.g. Task 8's `tests/digikey_pdf.rs`) cannot see `#[cfg(test)]`
/// items in the library, only genuinely public ones. Panics (rather than
/// returning `Result`) on a missing/malformed fixture, matching every other
/// fixture-loading test helper in this crate — a broken test fixture should
/// fail loudly and immediately, not be handled as recoverable data.
pub fn load_token_fixture(name: &str) -> Vec<PositionedToken> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "failed to parse fixture {} as Vec<PositionedToken>: {e}",
            path.display()
        )
    })
}

/// Below this many tokens on a whole document, a "PDF" is almost certainly
/// an image-only/scanned page with no embedded text layer at all, rather
/// than a genuinely thin born-digital document — the committed DigiKey PO
/// Acknowledgement fixture alone carries 423 tokens across two pages (see
/// `crate::pdf::tests::fixture_deserializes_with_expected_token_count_and_pages`),
/// and even a minimal one-line invoice has header labels, an order number,
/// and totals well past this. A scanned page routed through
/// [`PdfTextSource::extract`] with no OCR step yields zero tokens (or, at
/// most, a stray one from a rendered page number/watermark that happens to
/// carry a real text object) — comfortably under this threshold.
pub const SCANNED_PDF_TOKEN_THRESHOLD: usize = 10;

/// What produced a [`TextExtraction`]'s tokens: a born-digital PDF (a real
/// embedded text layer, read directly) or an image-only/scanned page that
/// yielded ~no tokens and would need OCR to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionSource {
    BornDigital,
    Ocr,
}

/// The result of extracting text from one PDF: the tokens themselves (empty
/// for the [`ExtractionSource::Ocr`] branch — there is nothing to reconstruct
/// from), which path produced them, and a rough confidence in the result.
///
/// This crate does **not** implement OCR — the real Windows-OCR
/// implementation for scanned PDFs is explicitly deferred past Phase 5a (see
/// `docs/known-limitations.md`). What 5a ships is [`TextExtraction::classify`]
/// detecting the scanned-PDF case from a raw token count and reporting it
/// through this type, so a caller (`crate::digikey::pdf::DigiKeyPdfParser`)
/// can produce a low-confidence, clearly-labeled `ParsedInvoice` instead of
/// attempting table reconstruction against near-empty input — giving 5d's
/// manual-correction UI a defined contract to build against later.
#[derive(Debug, Clone, PartialEq)]
pub struct TextExtraction {
    pub tokens: Vec<PositionedToken>,
    pub source: ExtractionSource,
    pub confidence: f32,
}

impl TextExtraction {
    /// Classify a raw [`PdfTextSource::extract`] result: fewer than
    /// [`SCANNED_PDF_TOKEN_THRESHOLD`] tokens on the whole document is
    /// treated as a scanned/image-only PDF ([`ExtractionSource::Ocr`], low
    /// confidence); anything at or above it is treated as normal
    /// born-digital text ([`ExtractionSource::BornDigital`], full
    /// confidence). This is a coarse count-based heuristic, not layout
    /// analysis — sufficient for the 5a contract (detect the scanned case,
    /// don't fabricate a reconstruction from it), not a general scanned-vs-
    /// digital classifier.
    pub fn classify(tokens: Vec<PositionedToken>) -> TextExtraction {
        if tokens.len() < SCANNED_PDF_TOKEN_THRESHOLD {
            TextExtraction {
                tokens,
                source: ExtractionSource::Ocr,
                confidence: 0.1,
            }
        } else {
            TextExtraction {
                tokens,
                source: ExtractionSource::BornDigital,
                confidence: 1.0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn positioned_token_serde_round_trips() {
        let token = PositionedToken {
            text: "PART:".to_string(),
            x: 10.5,
            y: 20.25,
            width: 30.0,
            height: 8.0,
            page: 1,
        };
        let json = serde_json::to_string(&token).unwrap();
        let back: PositionedToken = serde_json::from_str(&json).unwrap();
        assert_eq!(back, token);
    }

    #[test]
    fn fixture_deserializes_with_expected_token_count_and_pages() {
        let tokens = load_token_fixture("digikey_po_100353602.tokens.json");
        assert_eq!(tokens.len(), 423);

        let pages: BTreeSet<u32> = tokens.iter().map(|t| t.page).collect();
        assert_eq!(
            pages,
            BTreeSet::from([0, 1]),
            "fixture should only have pages 0 and 1"
        );
    }

    #[test]
    fn fixture_contains_known_supplier_sku_token_with_plausible_position() {
        let tokens = load_token_fixture("digikey_po_100353602.tokens.json");
        let sku = tokens
            .iter()
            .find(|t| t.text == "475-BPW34-ND")
            .expect("expected the 475-BPW34-ND supplier-sku token in the fixture");
        assert!(sku.x > 0.0, "x should be a plausible positive coordinate");
        assert!(sku.y > 0.0, "y should be a plausible positive coordinate");
        assert!(sku.width > 0.0);
        assert!(sku.height > 0.0);
    }

    #[test]
    fn fixture_contains_known_order_number_token_with_plausible_position() {
        let tokens = load_token_fixture("digikey_po_100353602.tokens.json");
        let order_number = tokens
            .iter()
            .find(|t| t.text == "100353602")
            .expect("expected the 100353602 order-number token in the fixture");
        assert!(order_number.x > 0.0);
        assert!(order_number.y > 0.0);
    }

    #[test]
    fn classify_empty_tokens_as_ocr_with_low_confidence() {
        let extraction = TextExtraction::classify(Vec::new());
        assert_eq!(extraction.source, ExtractionSource::Ocr);
        assert!(
            extraction.confidence < 0.5,
            "scanned-PDF confidence should be low, got {}",
            extraction.confidence
        );
        assert!(extraction.tokens.is_empty());
    }

    #[test]
    fn classify_a_few_stray_tokens_below_threshold_as_ocr() {
        // A scanned page can still carry a stray real text object (e.g. a
        // rendered page number) without being a born-digital document.
        let tokens = vec![PositionedToken {
            text: "1".to_string(),
            x: 1.0,
            y: 1.0,
            width: 1.0,
            height: 1.0,
            page: 0,
        }];
        let extraction = TextExtraction::classify(tokens);
        assert_eq!(extraction.source, ExtractionSource::Ocr);
        assert!(extraction.confidence < 0.5);
    }

    #[test]
    fn classify_the_real_fixture_as_born_digital_with_full_confidence() {
        let tokens = load_token_fixture("digikey_po_100353602.tokens.json");
        let extraction = TextExtraction::classify(tokens);
        assert_eq!(extraction.source, ExtractionSource::BornDigital);
        assert_eq!(extraction.confidence, 1.0);
        assert_eq!(extraction.tokens.len(), 423);
    }
}
