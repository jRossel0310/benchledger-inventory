# Import parsing (`inventory-import`)

Phase 5a's "Upload → Extract" half of the spec §10 import pipeline: turn a
supplier order file (PDF / CSV / XLSX) into a `ParsedInvoice` — order
metadata + line items, with every field's original extracted text preserved
— without touching inventory. Matching, enrichment, review, and commit are
5b–5d; see `docs/architecture.md` and the phase plans under
`docs/superpowers/plans/`.

## The model (`src/model.rs`)

`ParsedInvoice { supplier, source_format, order: ParsedOrderMeta, lines:
Vec<ParsedLine>, warnings: Vec<String> }` is the single vocabulary every
parser produces and `inventory-db::imports` persists. Two rules hold across
every parser in this crate:

- **Never fabricate.** A field the source document didn't provide is
  `None`, never a guess. An unparseable/unrecognized row is still captured
  (as `LineKind::Unknown`, low confidence, plus a `warnings` entry) —
  nothing is silently dropped.
- **Money is exact.** `Money { micros: i64, currency: String }` — never a
  float, so DigiKey's five-decimal unit prices (`1.82000`) and two-decimal
  extended amounts (`5.46`) both round-trip exactly. `Money::parse` handles
  a leading `$`, a leading `-` (credits/no-charge lines), and pads/truncates
  the fractional part to exactly 6 digits (micros) — truncated, not
  rounded, beyond the 6th digit. Quantities use the existing
  `inventory_core::quantity::Quantity` (milli-units, x1000).

`LineKind` classifies every line: `Part` (inventory-affecting — the only
kind that should ever become a receive transaction in 5b), `Fee`
(shipping/tax-style rows with no part identity), `Tariff` (a TARIFF
sub-row, see below), `NoCharge` (zero-price promotional rows), `Unknown`
(captured but unrecognized). **`ParsedInvoice::part_lines()`** returns an
iterator over `LineKind::Part` lines only — the canonical selection any
consumer that turns a `ParsedInvoice` into stock movement should filter
through, so a `Fee`/`Tariff`/`NoCharge`/`Unknown` row can never accidentally
create a receive.

## The `InvoiceParser` trait (`src/parser.rs`)

```rust
trait InvoiceParser {
    fn supplier(&self) -> &str;
    fn source_format(&self) -> SourceFormat;
    fn parse(&self, bytes: &[u8]) -> Result<ParsedInvoice, ImportError>;
}
```

Pure: no I/O, no DB access, no network. `detect_format(filename, bytes)`
picks a `SourceFormat` for an upload — extension first (case-insensitive,
trusted outright when recognized), falling back to magic-byte/content
sniffing (`%PDF`, the zip signature `PK\x03\x04` for XLSX, UTF-8 validity
for CSV) only when the extension is missing or unrecognized. A mislabeled
extension (e.g. an XLSX saved as `.csv`) is therefore misdetected — see
`docs/known-limitations.md`.

## DigiKey CSV/XLSX (`src/digikey/{csv,xlsx,columns,row}.rs`)

DigiKey's line-item exports vary by export tool and account settings:
column order and exact header text differ, some exports carry a
title/address preamble above the header row, and some rows have no part
identifier at all (shipping/tax/tariff/no-charge rows). Both formats share
two modules so they cannot drift apart:

- **`digikey::columns`** — the header-text → logical-`Field` alias table
  (`ColumnMap::from_header` matches by name, case/whitespace-insensitive, so
  column order never matters) and `ColumnMap::looks_like_header`, which
  scans down past any preamble rows to find the real header.
- **`digikey::row`** — `parse_row`, the one function that turns a
  `(header, row)` pair into a classified `ParsedLine`: a row with a
  SKU/MPN is `Part` (missing MPN lowers confidence but still `Part`; a zero
  unit/extended price is `NoCharge`); a row without a part identity but a
  `SHIPPING`/`TARIFF`/`TAX`/`FREIGHT` keyword in its description is
  `Fee`/`Tariff`; anything else is `Unknown` plus a warning. `raw_json`
  preserves every original header→cell pair verbatim.

`DigiKeyCsvParser` reads records with the `csv` crate (`has_headers: false`
so the preamble-scan can locate the real header itself), tolerates a UTF-8
BOM. `DigiKeyXlsxParser` reads the first worksheet via `calamine`, converts
every cell to a string (`cell_to_string` — critically, an integral float
cell like `Data::Float(3.0)` becomes `"3"`, not `"3.0"`, or the shared
integer-quantity parser in `row::parse_row` would silently drop it), then
locates the header and classifies rows through the exact same `row::parse_row`
helper the CSV parser uses.

Neither CSV nor XLSX line-item export carries a per-file currency cell or
order metadata (order/invoice number, dates, totals) — `ParsedOrderMeta` is
left mostly `None` from these two parsers; richer metadata comes from the
PDF parser when available.

## PDF: the `PdfTextSource` abstraction (`src/pdf/`)

A PDF carries no rows/columns, only positioned words on a page. Every
`PdfTextSource` implementation turns raw PDF bytes into
`Vec<PositionedToken>` (`{ text, x, y, width, height, page }`), normalized
to **one coordinate convention regardless of source**: origin top-left, `y`
increases downward, units are PDF points (1/72 inch).

`PdfiumTextSource` (feature-gated behind Cargo feature `pdfium`, off by
default) is the one production implementation, backed by `pdfium-render`'s
dynamic (runtime-loaded, via `libloading`) binding to Google's pdfium C++
library. See `docs/build.md` for obtaining `pdfium.dll` and the
`PDFIUM_DLL_DIR` discovery path. Because the feature links dynamically, the
crate always *compiles* without the DLL present — only calling
`PdfiumTextSource::new()` needs it at runtime, and a load failure returns
`ImportError::Pdf(...)` rather than panicking.

### Why unit tests use committed token fixtures, never live pdfium

`crates/inventory-import/tests/fixtures/digikey_po_100353602.tokens.json` is
a **real** extraction — dumped once, on the authoring machine, from a
sanitized sample PDF, and frozen as JSON via
`samples/digikey/tools/dump_tokens.py` (PyMuPDF, a separate Python
dependency). `load_token_fixture(name)` reads it back
(`CARGO_MANIFEST_DIR`-relative to `tests/fixtures/<name>`) as a plain `pub
fn` (not `#[cfg(test)]`) so both the library's own unit tests and the
separate-compilation-unit integration tests under `tests/` can use it.

This is the same shared-fixture pattern the units engine uses elsewhere in
the repo, and for the same reason: `PdfiumTextSource` needs a native DLL
that is not available in this dev/CI environment, and even where it is
available, native rendering is not something a fast, hermetic unit-test
suite should depend on. PyMuPDF's own word extraction already normalizes to
the same top-left/`y`-down/points convention `PdfiumTextSource` produces
(see the coordinate-flip math in `src/pdf/text_source.rs`'s module doc), so
the fixture is interchangeable with a live extraction as far as
`crate::digikey::pdf::reconstruct` — the pure, fixture-driven core — is
concerned. It cannot tell whether it's looking at a live extraction or the
frozen fixture. Only one `#[cfg(feature = "pdfium")]`-gated, opt-in path
(documented in `docs/build.md`) exercises the real DLL; `cargo test -p
inventory-import` (default features) never touches it.

## DigiKey PDF table reconstruction (`src/digikey/pdf.rs`)

`DigiKeyPdfParser<S: PdfTextSource>::parse` calls `source.extract(bytes)`
then hands the tokens to `reconstruct(tokens) -> ParsedInvoice` — the pure
core every unit test drives directly via `load_token_fixture`, with no
`PdfTextSource` involved at all.

**Row/column reconstruction**, per page:

1. **Rows by y-band** (`group_rows`): tokens are sorted by `(y, x)` and
   clustered into rows whenever consecutive `y` values are within
   `ROW_Y_TOLERANCE` (4.5pt — derived empirically from the fixture: a
   `PART:` row's own label/data-number lines sit ~1.0–1.1pt apart, while
   distinct visual rows are always ≥8.6pt apart). Each row is then
   re-sorted left-to-right by `x`.
2. **Column x-bands** (`derive_bands`): located from the header row's own
   token positions (`Line`, `Ordered`, `Available`, `Backordered`, `Unit`,
   `Amount`) — not hardcoded pixel offsets — so the table survives minor
   layout drift. The Item Number/Description column has no band of its
   own; its content is found by the literal `PART:`/`DESC:` tokens instead
   (see below).
3. **Metadata**: `PO Acknowledgement <n>` → `order_number`; `Order Date:
   <d>`; `WEB ORDER ID: <n>`; a `USD` token anywhere → `currency`. Bill
   To/Ship To/Buyer address blocks are read as rows but deliberately never
   parsed into fields — PII.
4. **Line items** (`extract_lines`): a row containing the literal token
   `PART:` opens a line — `supplier_sku` is the token right after `PART:`,
   `description` is every token after `DESC:` up to the Unit Price band.
   Qty digits left of `PART:` are classified into Line/Ordered/
   Available(→shipped)/Backordered by x-band (bounded to left of `PART:`
   specifically so a bare digit inside the description, e.g. "GP 2
   CIRCUIT", is never misread as a quantity). The following `MFG :
   <manufacturer> / <mpn>` row fills manufacturer/mpn — a missing/malformed
   one leaves `mpn: None` and lowers `confidence`, but the line is still
   captured as `Part`. Unit price/extended price are read from the whole
   block (the `PART:` row through just before the next one), not just the
   `PART:` row's own line — DigiKey's Amount-band token can, in principle,
   land on a later row within the same block; on the real sample it always
   resolves on the first row, but the algorithm doesn't assume that.
5. **TARIFF sub-rows**: a row starting with the literal token `TARIFF`
   becomes its own `LineKind::Tariff` line (there is no per-line tariff
   field on `ParsedLine`), with `line_number` copied from the
   part line it immediately follows, so a caller re-associates a tariff to
   its part by joining on `line_number`.
6. **Noise skipping**: `is_noise_row` explicitly recognizes and skips
   ECCN/HTSUS export-control rows, the ROHS3 compliance row, the Mercury
   disclosure row, the "All transactions with DigiKey..."/`Page X of Y`
   footer, and a lone `USD $` echo row — these never become lines and never
   corrupt the line they sit under.
7. **Totals** (`extract_totals`): `Sales Amount` → subtotal, `Estimated
   Tariff Amount` → tariff, `Shipping charges applied` → shipping, `Sales
   Tax` → tax, `Total` → total — matched by *exact* joined label text (so
   the distractor label `Total Sales and Estimated Tariff Amount` is never
   mistaken for `Total`), value = the first token at/past the Amount
   x-band on the same row.
8. **Unknown rows**: anything left inside the table body (between the
   header and the page's end) that isn't one of the shapes above, and
   carries no fee keyword either, becomes a `LineKind::Unknown` line plus a
   `warnings` entry — bounded to the table body specifically so the
   PII address block above the header never spuriously surfaces as
   "unknown".

Multi-page documents re-derive bands per page (a repeated header is
expected) and continue the same `Part`-line-number sequence across pages.

## Fixtures: generating and sanitizing

- `samples/digikey/private/` — gitignored, **never committed**. Raw
  supplier files (PDF/CSV/XLSX) live here; they may contain a real name,
  address, or account number.
- `samples/digikey/tools/dump_tokens.py` — run on the authoring machine
  only. Reads a raw PDF from `private/` with PyMuPDF, replaces personal
  tokens per `private/sanitize-map.txt` (gitignored: `real=placeholder`
  pairs, e.g. the real name → `TEST CUSTOMER`), and hard-fails if any
  string from `private/pii-denylist.txt` (gitignored) survives into the
  output — so a generated fixture is never a leak surface by construction.
  The script itself is committed (it contains no personal data).
- `crates/inventory-import/tests/no_pii.rs` — a guard test that reads
  `private/pii-denylist.txt` (skips with a note if absent, e.g. on a fresh
  clone or CI) and fails if any committed file under
  `crates/inventory-import/tests/fixtures/` or `samples/digikey/` contains
  a denylisted string, case-insensitive.
- CSV/XLSX fixtures (`digikey_order.csv`/`.xlsx` + variants) are
  **synthesized** from DigiKey's documented export columns, not dumped from
  a real export — see `docs/known-limitations.md`.

## How to add a new supplier

1. Add `crates/inventory-import/src/<supplier>/` with a `mod.rs` and one
   file per format you support, mirroring `digikey/`'s split
   (`columns.rs`/`row.rs` shared between CSV/XLSX if the supplier has both;
   a `pdf.rs` with its own `reconstruct` if it has a PDF).
2. Implement `InvoiceParser` per format — `supplier()` returns the new
   supplier's name, `source_format()` the format, `parse()` never
   fabricates data and never drops a row (fall back to `LineKind::Unknown`
   + a warning).
3. If the format is PDF, build column x-bands from that supplier's own
   header tokens (don't hardcode positions from DigiKey's layout) and drop
   a sanitized token fixture under `tests/fixtures/` the same way DigiKey's
   was produced (see "Fixtures" above) — write a `dump_tokens`-equivalent
   script only if the existing one doesn't already fit (it's PyMuPDF-only,
   no DigiKey-specific logic).
4. Wire the new parser into whatever dispatches on `detect_format` +
   supplier (5b, when the review/commit pipeline exists) — nothing in this
   crate hardcodes DigiKey as the only supplier; `InvoiceParser` is
   supplier-agnostic on purpose.

## How to add a new sample (existing supplier)

See `samples/digikey/README.md`'s "Adding a new sample" section: drop the
raw file in `private/`, add any new personal strings to
`private/pii-denylist.txt`, create a sanitized fixture under
`tests/fixtures/`, then `cargo test -p inventory-import` to confirm the
PII guard passes.

## OCR fallback (scanned PDFs)

See `crate::pdf::TextExtraction`/`ExtractionSource` and
`docs/known-limitations.md` — a PDF whose text extraction yields ~no tokens
is treated as scanned/image-only and produces a `ParsedInvoice` with no
fabricated lines/metadata and a warning, rather than attempting (and
silently failing) table reconstruction against near-empty input.
