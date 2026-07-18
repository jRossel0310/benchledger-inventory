# DigiKey sample documents

This directory holds DigiKey order documents used to build and validate the
invoice parsers (see `docs/parsers.md`).

## Layout

- `private/` — **gitignored, never committed.** Drop raw DigiKey files here
  (PDF / CSV / XLSX order acknowledgements, invoices, shipment notices). These
  contain personal information (name, address, account number) and must never
  enter a commit, log, or the public snapshot.
- `private/pii-denylist.txt` — **gitignored.** One personal-information string
  per line. The fixture guard test (`crates/inventory-import/tests/no_pii.rs`)
  reads this on your machine and fails if any committed fixture contains one of
  these strings, so a sanitization slip is caught before it is committed.

## Committed, sanitized fixtures

The parser tests read sanitized fixtures from
`crates/inventory-import/tests/fixtures/`. A fixture is a real document's
structure with every personal detail replaced by a synthetic placeholder
(the customer name → `TEST CUSTOMER`, the street/city/ZIP → a made-up address,
the account number → `00000000`). Catalog data — part numbers, manufacturers,
descriptions, quantities, prices, order/web-order numbers — is preserved so the
fixtures exercise the real layout.

## Adding a new sample

1. Put the raw file in `private/`.
2. Add any new personal strings it contains to `private/pii-denylist.txt`.
3. Create a sanitized fixture under `crates/inventory-import/tests/fixtures/`
   (replace personal details; keep catalog/layout data).
4. Run `cargo test -p inventory-import` — the guard test confirms no denylisted
   string leaked into a committed fixture.
