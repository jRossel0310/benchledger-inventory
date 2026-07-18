# Sanitized invoice fixtures

Committed, **PII-free** DigiKey documents the parser tests read. Each is a real
document's structure with every personal detail replaced by a synthetic
placeholder; catalog data (part numbers, manufacturers, descriptions,
quantities, prices, order/web-order numbers) is preserved.

Sanitization mapping applied to the source (`samples/digikey/private/`, gitignored):

| Real (personal) | Placeholder |
|---|---|
| Customer name | `TEST CUSTOMER` |
| Street address | `1 EXAMPLE ST` |
| City / State / ZIP | `ANYTOWN CA 90001` |
| Customer account number | `00000000` |

DigiKey's own public company address, and non-personal order data, are kept.

## Fixtures

- `digikey_po_100353602.txt` — `pdftotext -layout` dump of the 2-page,
  6-line PO Acknowledgement sample, sanitized. Human-readable reference for the
  DigiKey PDF layout.
- `digikey_po_100353602.tokens.json` — the **positioned-token** fixture the PDF
  parser's unit tests run against: an array of `{text, x, y, width, height,
  page}` (origin top-left, y down, PDF points). Generated from the private
  sample by `samples/digikey/tools/dump_tokens.py` (PyMuPDF) with the same PII
  sanitization applied token-by-token, and a denylist hard-gate in the
  generator so no personal string can survive. 423 tokens, 2 pages. The runtime
  `PdfiumTextSource` normalizes pdfium's coordinates to this same convention, so
  the reconstruction logic is identical for the fixture and for live PDFs.

The guard test `../no_pii.rs` fails if any file here contains a string listed in
`samples/digikey/private/pii-denylist.txt` (present only on the authoring
machine; the test passes with a note when the denylist is absent).
