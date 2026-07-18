//! DigiKey supplier parsers: CSV, XLSX (this task), PDF (later Phase 5a
//! tasks). `columns` holds the header/column -> field mapping shared by
//! every DigiKey format so the alias lists cannot drift between them —
//! every parser builds a `columns::ColumnMap` from whatever header row it
//! finds and looks fields up by name from there. `row` holds the per-row
//! mapping + fee/tariff/no-charge/missing-MPN classification shared by the
//! CSV and XLSX parsers so they cannot drift on those rules either — each
//! reduces its native row representation to `Vec<String>` and calls
//! `row::parse_row`.

pub mod columns;
pub mod csv;
pub mod row;
pub mod xlsx;

pub use csv::DigiKeyCsvParser;
pub use xlsx::DigiKeyXlsxParser;
