//! [`PdfTextSource`]: an abstraction over "get positioned text out of a
//! PDF's bytes," so `crate::digikey::pdf`'s row/column reconstruction
//! (Tasks 8/9) can be unit-tested against the committed
//! [`crate::pdf::load_token_fixture`] fixture without ever loading a native
//! PDF-rendering library. [`PdfiumTextSource`] is the one production
//! implementation, gated behind the `pdfium` Cargo feature (off by
//! default — see `Cargo.toml`).

use crate::parser::ImportError;
use crate::pdf::PositionedToken;

/// Extracts positioned text tokens from a PDF's raw bytes. Every
/// implementation MUST normalize to the coordinate convention documented on
/// [`PositionedToken`] (origin top-left, `y` increases downward, units are
/// PDF points) regardless of what convention the underlying library
/// natively uses — the DigiKey reconstruction logic relies on that
/// normalization to treat a live extraction and the committed fixture
/// identically.
pub trait PdfTextSource {
    fn extract(&self, bytes: &[u8]) -> Result<Vec<PositionedToken>, ImportError>;
}

#[cfg(feature = "pdfium")]
pub use pdfium_source::PdfiumTextSource;

#[cfg(feature = "pdfium")]
mod pdfium_source {
    use super::PdfTextSource;
    use crate::parser::ImportError;
    use crate::pdf::PositionedToken;
    use pdfium_render::prelude::*;

    /// Horizontal gap (as a fraction of character height, in PDF points)
    /// beyond which two adjacent characters are treated as separate words
    /// even when no literal space glyph sits between them. Some PDF
    /// generators position glyphs purely by coordinate and never emit a
    /// space character at all, relying entirely on gaps — this is a
    /// fallback word-boundary signal for that case (the primary signal is
    /// an actual whitespace `char`, handled directly in
    /// [`words_from_chars`]).
    ///
    /// ASSUMPTION, not yet validated against a live `pdfium.dll` (this
    /// environment has none — see `docs/build.md`): scaled to a fraction of
    /// the character's own height because most fonts' space-glyph advance
    /// is roughly a quarter to a third of the em size. If real extractions
    /// show words splitting or merging incorrectly, tune this constant
    /// first (Task 8's `#[ignore]`d integration path is where that should
    /// be validated).
    const GAP_FRACTION_OF_HEIGHT: f32 = 0.35;

    /// Vertical drift (as a fraction of character height) beyond which two
    /// characters are treated as being on different lines even though
    /// pdfium's document order placed them consecutively. Same
    /// UNVALIDATED-assumption caveat as [`GAP_FRACTION_OF_HEIGHT`].
    const LINE_DRIFT_FRACTION_OF_HEIGHT: f32 = 0.5;

    /// [`PdfTextSource`] backed by `pdfium-render`'s dynamic (runtime-loaded)
    /// binding to Google's pdfium C++ library (`pdfium.dll` on Windows,
    /// `libpdfium.so`/`.dylib` elsewhere).
    ///
    /// # Library discovery
    /// The library binary is NOT bundled with this crate — with the
    /// `pdfium` feature's default (non-`static`) configuration,
    /// `pdfium-render` links it at runtime via `libloading`, so the crate
    /// compiles and links fine without the DLL present; only calling
    /// [`PdfiumTextSource::new`] needs it to actually exist on disk. This
    /// mirrors how `docs/build.md` documents obtaining it. Discovery order:
    /// 1. The `PDFIUM_DLL_DIR` environment variable, if set: the platform
    ///    library file name (`pdfium.dll` etc., via
    ///    `Pdfium::pdfium_platform_library_name_at_path`) is looked up
    ///    inside that directory.
    /// 2. Otherwise, the system library search path
    ///    (`Pdfium::bind_to_system_library`) — e.g. a copy placed next to
    ///    the application executable or elsewhere on `PATH`.
    ///
    /// # Coordinate normalization (CRITICAL — read before touching this file)
    /// pdfium's native page coordinate space has its origin at the
    /// BOTTOM-LEFT of the page with `y` increasing UPWARD (the standard
    /// PDF/PostScript convention). [`PositionedToken`] and every other part
    /// of this crate use top-left origin / `y`-DOWN instead (the
    /// screen/typography convention — also what
    /// `samples/digikey/tools/dump_tokens.py` already produces via
    /// PyMuPDF, since that script is what generated the committed
    /// fixture). For a character/word rectangle:
    ///
    /// ```text
    /// y_top_down = page_height_points - pdfium_rect.top().value
    /// ```
    ///
    /// `pdfium_rect.top()` is the rectangle edge with the LARGER pdfium
    /// `y` — i.e. visually the highest point on the page — so subtracting
    /// it from the page height gives the distance down from the page's
    /// visual top, which is exactly `y` in the top-left/`y`-down
    /// convention. `width`/`height` are unaffected by the flip: they're
    /// differences between two coordinates, and negating both terms of a
    /// difference leaves the difference's magnitude unchanged.
    pub struct PdfiumTextSource {
        pdfium: Pdfium,
    }

    impl PdfiumTextSource {
        /// Load pdfium per the discovery order documented on this type.
        /// Never panics: any load failure becomes
        /// `Err(ImportError::Pdf(_))` with the underlying `libloading`/
        /// pdfium error message attached, so a caller (5d's upload flow)
        /// can surface "install pdfium.dll" rather than crashing the app.
        pub fn new() -> Result<Self, ImportError> {
            let bindings = match std::env::var("PDFIUM_DLL_DIR") {
                Ok(dir) => {
                    let library_path = Pdfium::pdfium_platform_library_name_at_path(&dir);
                    Pdfium::bind_to_library(&library_path).map_err(|e| {
                        ImportError::Pdf(format!(
                            "pdfium library unavailable: failed to load '{}' \
                             (from PDFIUM_DLL_DIR={dir}): {e}",
                            library_path.display()
                        ))
                    })?
                }
                Err(_) => Pdfium::bind_to_system_library().map_err(|e| {
                    ImportError::Pdf(format!(
                        "pdfium library unavailable: PDFIUM_DLL_DIR is not set and no \
                         system pdfium library was found: {e}"
                    ))
                })?,
            };
            Ok(PdfiumTextSource {
                pdfium: Pdfium::new(bindings),
            })
        }
    }

    impl PdfTextSource for PdfiumTextSource {
        fn extract(&self, bytes: &[u8]) -> Result<Vec<PositionedToken>, ImportError> {
            let document = self
                .pdfium
                .load_pdf_from_byte_slice(bytes, None)
                .map_err(|e| ImportError::Pdf(format!("failed to open PDF: {e}")))?;

            let mut tokens = Vec::new();
            for (page_index, page) in document.pages().iter().enumerate() {
                // Page height in PDF points — the constant every character
                // rectangle on this page is flipped against (see the
                // struct-level doc's coordinate-normalization derivation).
                let page_height = page.height().value;

                let text = page.text().map_err(|e| {
                    ImportError::Pdf(format!(
                        "page {page_index}: failed to read text objects: {e}"
                    ))
                })?;

                words_from_chars(&text.chars(), page_height, page_index as u32, &mut tokens);
            }
            Ok(tokens)
        }
    }

    /// Accumulates one in-progress word's text and bounding rectangle
    /// (already flipped to the top-left/`y`-down convention) while
    /// [`words_from_chars`] scans a page's characters in document order.
    struct WordBuilder {
        text: String,
        // Accumulated bounds of the whole word so far.
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        // The immediately preceding character's own right edge / top / height
        // — kept separate from the accumulated word bounds above so the
        // gap/drift tests in `words_from_chars` always compare a new
        // character against the character right before it, not against the
        // word's overall (potentially much wider) bounding box.
        prev_right: f32,
        prev_top: f32,
        prev_height: f32,
    }

    /// Emit `current` as a [`PositionedToken`] (if it holds any non-blank
    /// text) and reset it to `None`. Shared by every word-boundary case in
    /// [`words_from_chars`] (whitespace, a large gap/line drift, or an
    /// unpositionable character) so they all behave identically.
    fn flush_word(current: &mut Option<WordBuilder>, page: u32, tokens: &mut Vec<PositionedToken>) {
        if let Some(word) = current.take() {
            if !word.text.trim().is_empty() {
                tokens.push(PositionedToken {
                    text: word.text,
                    x: word.x0,
                    y: word.y0,
                    width: word.x1 - word.x0,
                    height: word.y1 - word.y0,
                    page,
                });
            }
        }
    }

    /// Group one page's characters into word-level [`PositionedToken`]s by
    /// proximity, converting each character's rectangle to the
    /// top-left/`y`-down convention as it goes (see
    /// [`PdfiumTextSource`]'s doc for the coordinate math).
    ///
    /// Two consecutive (in pdfium document order) characters join the same
    /// word when: neither is whitespace, they sit on approximately the
    /// same line (vertical drift small relative to character height), and
    /// the horizontal gap between them is small relative to character
    /// height. Both thresholds are the named constants at the top of this
    /// module — UNVALIDATED assumptions, see their docs.
    ///
    /// A character whose bounds pdfium cannot report (`loose_bounds()`
    /// returns `Err`, which can happen for invisible/control glyphs) is
    /// dropped rather than guessed at; like whitespace, this flushes
    /// whatever word was in progress.
    ///
    /// KNOWN LIMITATION: this groups characters strictly in pdfium's
    /// per-page document order. Most PDF generators (including DigiKey's,
    /// per the sanitized `.txt` fixture's layout) emit each text run
    /// left-to-right/top-to-bottom, so this holds in practice — but a PDF
    /// whose text objects are defined out of visual order (e.g.
    /// interleaved columns) could produce a token whose characters are not
    /// contiguous on the page. `crate::digikey::pdf`'s row reconstruction
    /// (Task 8/9) re-sorts tokens into rows by `y`-band regardless, which
    /// bounds the blast radius of this to within-row token order.
    fn words_from_chars(
        chars: &PdfPageTextChars,
        page_height: f32,
        page: u32,
        tokens: &mut Vec<PositionedToken>,
    ) {
        let mut current: Option<WordBuilder> = None;

        for ch in chars.iter() {
            let Some(unicode_char) = ch.unicode_char() else {
                // No Unicode representation available for this glyph —
                // can't meaningfully include it in any word's text.
                flush_word(&mut current, page, tokens);
                continue;
            };
            if unicode_char.is_whitespace() {
                flush_word(&mut current, page, tokens);
                continue;
            }
            let Ok(bounds) = ch.loose_bounds() else {
                flush_word(&mut current, page, tokens);
                continue;
            };

            let left = bounds.left().value;
            let right = bounds.right().value;
            // Coordinate flip: pdfium's bottom-left/y-up -> this crate's
            // top-left/y-down. `top()` (larger pdfium y = visually higher)
            // becomes the smaller y-down value.
            let top_down = page_height - bounds.top().value;
            let bottom_down = page_height - bounds.bottom().value;
            let height = (bottom_down - top_down).max(0.0);

            let continues_word = current.as_ref().is_some_and(|w| {
                let gap = left - w.prev_right;
                let drift = (top_down - w.prev_top).abs();
                let scale = height.max(w.prev_height).max(1.0);
                gap <= GAP_FRACTION_OF_HEIGHT * scale
                    && drift <= LINE_DRIFT_FRACTION_OF_HEIGHT * scale
            });
            if !continues_word {
                flush_word(&mut current, page, tokens);
            }

            match current.as_mut() {
                Some(word) => {
                    word.text.push(unicode_char);
                    word.x0 = word.x0.min(left);
                    word.y0 = word.y0.min(top_down);
                    word.x1 = word.x1.max(right);
                    word.y1 = word.y1.max(bottom_down);
                    word.prev_right = right;
                    word.prev_top = top_down;
                    word.prev_height = height;
                }
                None => {
                    current = Some(WordBuilder {
                        text: unicode_char.to_string(),
                        x0: left,
                        y0: top_down,
                        x1: right,
                        y1: bottom_down,
                        prev_right: right,
                        prev_top: top_down,
                        prev_height: height,
                    });
                }
            }
        }

        flush_word(&mut current, page, tokens);
    }

    // No unit tests here: exercising `PdfiumTextSource` needs a real
    // `pdfium.dll`, which this environment does not have (see
    // `docs/build.md`). `words_from_chars`'s grouping logic is pure but is
    // still driven entirely by pdfium-provided types (`PdfPageTextChars`),
    // so it cannot be unit-tested without pdfium either. Live validation is
    // deferred to a manual run once a `pdfium.dll` is available (Task 8
    // consumes the committed token fixture instead, which is unaffected by
    // any of this).
}
