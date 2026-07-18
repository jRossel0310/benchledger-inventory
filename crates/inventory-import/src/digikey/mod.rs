//! DigiKey supplier parsers: CSV (this task), XLSX and PDF (later Phase 5a
//! tasks). `columns` holds the header/column -> field mapping shared by
//! every DigiKey format so the alias lists cannot drift between them —
//! the CSV and XLSX parsers both build a `columns::ColumnMap` from
//! whatever header row they find and look fields up by name from there.

pub mod columns;
pub mod csv;

pub use csv::DigiKeyCsvParser;
