//! DigiKey supplier parsers: CSV, XLSX, and PDF (`pdf`, which reconstructs
//! the invoice table from positioned text — see its module doc). `columns`
//! holds the header/column -> field mapping shared by CSV and XLSX so the
//! alias lists cannot drift between them — every parser builds a
//! `columns::ColumnMap` from whatever header row it finds and looks fields
//! up by name from there. `row` holds the per-row mapping +
//! fee/tariff/no-charge/missing-MPN classification shared by the CSV and
//! XLSX parsers so they cannot drift on those rules either — each reduces
//! its native row representation to `Vec<String>` and calls
//! `row::parse_row`.

pub mod columns;
pub mod csv;
pub mod pdf;
pub mod row;
pub mod xlsx;

pub use csv::DigiKeyCsvParser;
pub use pdf::DigiKeyPdfParser;
pub use xlsx::DigiKeyXlsxParser;
