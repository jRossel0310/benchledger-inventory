//! The `InvoiceParser` trait every format-specific supplier parser
//! implements, plus file-format detection shared by all of them.

use crate::model::{ParsedInvoice, SourceFormat};

/// Turns raw file bytes for one supplier/format combination into a
/// `ParsedInvoice`. No parser here does any I/O, DB access, or network —
/// `bytes` is already in memory (read from an upload or `PdfTextSource`
/// output for PDFs).
pub trait InvoiceParser {
    /// The supplier this parser targets (e.g. `"DigiKey"`).
    fn supplier(&self) -> &str;

    /// The file format this parser accepts.
    fn source_format(&self) -> SourceFormat;

    /// Parse `bytes` into a `ParsedInvoice`. Never fabricates data: a field
    /// the document didn't provide is `None`, never guessed.
    fn parse(&self, bytes: &[u8]) -> Result<ParsedInvoice, ImportError>;
}

/// Errors a parser can raise. Never panics on malformed input — every
/// failure mode a caller needs to report is a variant here.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("unsupported file format")]
    UnsupportedFormat,
    #[error("file is empty")]
    Empty,
    #[error("malformed input: {0}")]
    Malformed(String),
    #[error("encoding error: {0}")]
    Encoding(String),
    #[error("pdf error: {0}")]
    Pdf(String),
}

/// Detect a file's `SourceFormat` from its name and/or content, so upload
/// handling doesn't have to trust a possibly-wrong extension.
///
/// Order of checks: file extension first (case-insensitive), then magic
/// bytes (`%PDF` for PDF, the ZIP local-file-header signature `PK\x03\x04`
/// for XLSX, which is a zip archive), then a UTF-8-validity fallback to CSV
/// for plain text. Returns `None` if nothing matches.
pub fn detect_format(filename: &str, bytes: &[u8]) -> Option<SourceFormat> {
    let ext = filename.rsplit('.').next().map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("pdf") => return Some(SourceFormat::Pdf),
        Some("xlsx") => return Some(SourceFormat::Xlsx),
        Some("csv") => return Some(SourceFormat::Csv),
        _ => {}
    }

    if bytes.starts_with(b"%PDF") {
        return Some(SourceFormat::Pdf);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Some(SourceFormat::Xlsx);
    }
    if std::str::from_utf8(bytes).is_ok() {
        return Some(SourceFormat::Csv);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LineKind, Money, ParsedLine, ParsedOrderMeta};

    #[test]
    fn detect_format_from_pdf_magic_bytes() {
        assert_eq!(
            detect_format("upload", b"%PDF-1.7 rest of file"),
            Some(SourceFormat::Pdf)
        );
    }

    #[test]
    fn detect_format_from_xlsx_zip_magic_bytes() {
        assert_eq!(
            detect_format("upload", b"PK\x03\x04rest of zip"),
            Some(SourceFormat::Xlsx)
        );
    }

    #[test]
    fn detect_format_from_plain_csv_text() {
        assert_eq!(
            detect_format("upload", b"a,b,c\n1,2,3"),
            Some(SourceFormat::Csv)
        );
    }

    #[test]
    fn detect_format_unknown_binary_is_none() {
        let bytes: &[u8] = &[0xFF, 0xFE, 0x00, 0x01, 0x80, 0x81, 0x82, 0x83];
        assert_eq!(detect_format("upload", bytes), None);
    }

    #[test]
    fn detect_format_uses_extension_when_present() {
        assert_eq!(detect_format("order.pdf", b""), Some(SourceFormat::Pdf));
        assert_eq!(detect_format("order.xlsx", b""), Some(SourceFormat::Xlsx));
        assert_eq!(detect_format("order.csv", b""), Some(SourceFormat::Csv));
    }

    #[test]
    fn detect_format_extension_check_is_case_insensitive() {
        assert_eq!(detect_format("ORDER.PDF", b""), Some(SourceFormat::Pdf));
        assert_eq!(detect_format("Order.Xlsx", b""), Some(SourceFormat::Xlsx));
        assert_eq!(detect_format("ORDER.CSV", b""), Some(SourceFormat::Csv));
    }

    /// Minimal `InvoiceParser` impl proving the trait is object-safe and
    /// usable through `&dyn InvoiceParser`.
    struct DummyParser;

    impl InvoiceParser for DummyParser {
        fn supplier(&self) -> &str {
            "DigiKey"
        }

        fn source_format(&self) -> SourceFormat {
            SourceFormat::Csv
        }

        fn parse(&self, bytes: &[u8]) -> Result<ParsedInvoice, ImportError> {
            if bytes.is_empty() {
                return Err(ImportError::Empty);
            }
            Ok(ParsedInvoice {
                supplier: self.supplier().to_string(),
                source_format: self.source_format(),
                order: ParsedOrderMeta {
                    order_number: None,
                    invoice_number: None,
                    shipment_number: None,
                    order_date: None,
                    currency: "USD".to_string(),
                    subtotal: None,
                    shipping: None,
                    tax: None,
                    tariff: None,
                    total: None,
                    web_order_id: None,
                },
                lines: vec![ParsedLine {
                    line_number: Some(1),
                    supplier_sku: None,
                    mpn: None,
                    manufacturer: None,
                    description: None,
                    ordered: None,
                    shipped: None,
                    backordered: None,
                    unit_price: Money::parse("1.00", "USD"),
                    extended_price: None,
                    packaging: None,
                    customer_reference: None,
                    kind: LineKind::Unknown,
                    confidence: 0.5,
                    raw: serde_json::json!({}),
                }],
                warnings: vec![],
            })
        }
    }

    fn use_dyn_parser(
        parser: &dyn InvoiceParser,
        bytes: &[u8],
    ) -> Result<ParsedInvoice, ImportError> {
        parser.parse(bytes)
    }

    #[test]
    fn invoice_parser_trait_is_object_safe() {
        let parser = DummyParser;
        let invoice = use_dyn_parser(&parser, b"some bytes").expect("dummy parse should succeed");
        assert_eq!(invoice.supplier, "DigiKey");
        assert_eq!(invoice.source_format, SourceFormat::Csv);
        assert_eq!(invoice.lines.len(), 1);
    }

    #[test]
    fn invoice_parser_trait_usable_via_generic() {
        fn parse_generic<P: InvoiceParser>(
            parser: &P,
            bytes: &[u8],
        ) -> Result<ParsedInvoice, ImportError> {
            parser.parse(bytes)
        }
        let parser = DummyParser;
        let invoice = parse_generic(&parser, b"some bytes").unwrap();
        assert_eq!(invoice.supplier, "DigiKey");
    }

    #[test]
    fn dummy_parser_reports_empty_input() {
        let parser = DummyParser;
        let err = parser.parse(b"").unwrap_err();
        assert!(matches!(err, ImportError::Empty));
        assert_eq!(err.to_string(), "file is empty");
    }
}
