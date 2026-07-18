//! Pure data model for a parsed supplier invoice.
//!
//! `ParsedInvoice` is the single vocabulary every format-specific parser
//! (DigiKey CSV/XLSX/PDF, later tasks) produces and the import repository
//! (`inventory-db`) persists. Nothing here fabricates data: a field the
//! source document didn't provide is `None`, never a guess. Money is exact
//! integer micros (i64, x1_000_000) — never a float — so DigiKey's five
//! fractional digits on unit prices (`1.82000`) round-trip exactly.

use inventory_core::quantity::Quantity;

/// A fully parsed supplier invoice/order: metadata + line items, plus any
/// non-fatal warnings surfaced while parsing (unrecognized rows, scanned
/// PDFs, etc.) so nothing is silently dropped.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParsedInvoice {
    pub supplier: String,
    pub source_format: SourceFormat,
    pub order: ParsedOrderMeta,
    pub lines: Vec<ParsedLine>,
    pub warnings: Vec<String>,
}

/// Order-level metadata. Every field is optional except `currency`, which
/// defaults to the invoice's stated currency (e.g. `"USD"`) rather than
/// being fabricated when absent — parsers should default it explicitly.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParsedOrderMeta {
    pub order_number: Option<String>,
    pub invoice_number: Option<String>,
    pub shipment_number: Option<String>,
    pub order_date: Option<String>,
    pub currency: String,
    pub subtotal: Option<Money>,
    pub shipping: Option<Money>,
    pub tax: Option<Money>,
    pub tariff: Option<Money>,
    pub total: Option<Money>,
    pub web_order_id: Option<String>,
}

/// A single invoice line. `raw` preserves the original extracted fields
/// (CSV row cells, XLSX row cells, or PDF token text) as JSON so review and
/// debugging can always see exactly what the parser saw, independent of how
/// well the typed fields above were filled in.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParsedLine {
    pub line_number: Option<u32>,
    pub supplier_sku: Option<String>,
    pub mpn: Option<String>,
    pub manufacturer: Option<String>,
    pub description: Option<String>,
    pub ordered: Option<Quantity>,
    pub shipped: Option<Quantity>,
    pub backordered: Option<Quantity>,
    pub unit_price: Option<Money>,
    pub extended_price: Option<Money>,
    pub packaging: Option<String>,
    pub customer_reference: Option<String>,
    pub kind: LineKind,
    pub confidence: f32,
    pub raw: serde_json::Value,
}

/// Which file format a `ParsedInvoice` was extracted from. Round-trips
/// through the `imports.source_format` CHECK column via `as_db_str`/
/// `from_db_str` ('pdf'/'csv'/'xlsx' — migration 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    Pdf,
    Csv,
    Xlsx,
}

impl SourceFormat {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            SourceFormat::Pdf => "pdf",
            SourceFormat::Csv => "csv",
            SourceFormat::Xlsx => "xlsx",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        Some(match s {
            "pdf" => SourceFormat::Pdf,
            "csv" => SourceFormat::Csv,
            "xlsx" => SourceFormat::Xlsx,
            _ => return None,
        })
    }
}

/// What kind of invoice row a `ParsedLine` represents. Round-trips through
/// the `import_lines.line_kind` CHECK column via `as_db_str`/`from_db_str`
/// ('part'/'fee'/'tariff'/'no_charge'/'unknown' — migration 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Part,
    Fee,
    Tariff,
    NoCharge,
    Unknown,
}

impl LineKind {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            LineKind::Part => "part",
            LineKind::Fee => "fee",
            LineKind::Tariff => "tariff",
            LineKind::NoCharge => "no_charge",
            LineKind::Unknown => "unknown",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        Some(match s {
            "part" => LineKind::Part,
            "fee" => LineKind::Fee,
            "tariff" => LineKind::Tariff,
            "no_charge" => LineKind::NoCharge,
            "unknown" => LineKind::Unknown,
            _ => return None,
        })
    }
}

/// An exact monetary amount: integer micros (i64, x1_000_000) plus an ISO
/// currency code string. Never a float, so DigiKey's five-decimal unit
/// prices (`1.82000`) and two-decimal extended amounts (`5.46`) both
/// round-trip exactly.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Money {
    pub micros: i64,
    pub currency: String,
}

impl Money {
    /// Parse decimal money text into exact integer micros. No `f64` is
    /// involved anywhere in this path.
    ///
    /// Accepts an optional leading `$` and surrounding whitespace, an
    /// optional leading `-` for negative amounts (credits/no-charge lines),
    /// an integer part, and an optional `.`-separated fractional part of
    /// any length. The fractional part is padded with trailing zeros (if
    /// shorter than 6 digits) or truncated (if longer than 6 digits) to
    /// scale it to micros — digits beyond the 6th are dropped, not rounded.
    ///
    /// Empty text, a bare `-`, or an em dash `—` (DigiKey's "no value"
    /// placeholder) all return `None`, as does anything non-numeric.
    pub fn parse(text: &str, currency: &str) -> Option<Money> {
        let s = text.trim();
        if s.is_empty() || s == "—" || s == "-" {
            return None;
        }

        let s = s.strip_prefix('$').map(str::trim).unwrap_or(s);
        let (negative, s) = match s.strip_prefix('-') {
            Some(rest) => (true, rest.trim()),
            None => (false, s),
        };

        let (int_part, frac_part) = match s.split_once('.') {
            Some((i, f)) => (i, f),
            None => (s, ""),
        };
        if int_part.is_empty() && frac_part.is_empty() {
            return None;
        }
        if !int_part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if !frac_part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }

        let whole: i64 = if int_part.is_empty() {
            0
        } else {
            int_part.parse().ok()?
        };

        let mut frac = frac_part.to_string();
        if frac.len() > 6 {
            frac.truncate(6);
        } else {
            while frac.len() < 6 {
                frac.push('0');
            }
        }
        let frac_micros: i64 = frac.parse().ok()?;

        let micros = whole.checked_mul(1_000_000)?.checked_add(frac_micros)?;
        let micros = if negative { -micros } else { micros };

        Some(Money {
            micros,
            currency: currency.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_parse_exactness_table() {
        let cases: &[(&str, i64)] = &[
            ("1.82000", 1_820_000),
            ("5.46", 5_460_000),
            ("23.38", 23_380_000),
            ("$4.99", 4_990_000),
            ("0.44000", 440_000),
        ];
        for (input, expected) in cases {
            let m = Money::parse(input, "USD")
                .unwrap_or_else(|| panic!("'{input}' should have parsed"));
            assert_eq!(m.micros, *expected, "'{input}' parsed to {}", m.micros);
            assert_eq!(m.currency, "USD");
        }
    }

    #[test]
    fn money_parse_rejects_empty_and_placeholders() {
        assert_eq!(Money::parse("", "USD"), None);
        assert_eq!(Money::parse("—", "USD"), None);
        assert_eq!(Money::parse("-", "USD"), None);
        assert_eq!(Money::parse("N/A", "USD"), None);
    }

    #[test]
    fn money_parse_handles_negative_amounts() {
        // Credits / no-charge lines can be negative.
        let m = Money::parse("-1.50", "USD").unwrap();
        assert_eq!(m.micros, -1_500_000);
    }

    #[test]
    fn money_parse_truncates_beyond_six_fractional_digits() {
        // DECISION: extra fractional digits beyond micro precision are
        // truncated, not rounded.
        let m = Money::parse("1.2345678", "USD").unwrap();
        assert_eq!(m.micros, 1_234_567);
    }

    #[test]
    fn money_parse_pads_short_fractional_parts() {
        let m = Money::parse("5", "USD").unwrap();
        assert_eq!(m.micros, 5_000_000);
        let m = Money::parse(".5", "USD").unwrap();
        assert_eq!(m.micros, 500_000);
    }

    #[test]
    fn source_format_db_string_round_trips() {
        for fmt in [SourceFormat::Pdf, SourceFormat::Csv, SourceFormat::Xlsx] {
            let s = fmt.as_db_str();
            assert_eq!(SourceFormat::from_db_str(s), Some(fmt));
        }
        assert_eq!(SourceFormat::from_db_str("bogus"), None);
    }

    #[test]
    fn line_kind_db_string_round_trips() {
        for kind in [
            LineKind::Part,
            LineKind::Fee,
            LineKind::Tariff,
            LineKind::NoCharge,
            LineKind::Unknown,
        ] {
            let s = kind.as_db_str();
            assert_eq!(LineKind::from_db_str(s), Some(kind));
        }
        assert_eq!(LineKind::from_db_str("bogus"), None);
    }

    fn sample_invoice() -> ParsedInvoice {
        ParsedInvoice {
            supplier: "DigiKey".to_string(),
            source_format: SourceFormat::Csv,
            order: ParsedOrderMeta {
                order_number: Some("100353602".to_string()),
                invoice_number: None,
                shipment_number: Some("1".to_string()),
                order_date: Some("2026-01-01".to_string()),
                currency: "USD".to_string(),
                subtotal: Money::parse("14.28", "USD"),
                shipping: Money::parse("4.99", "USD"),
                tax: Money::parse("1.24", "USD"),
                tariff: Money::parse("2.87", "USD"),
                total: Money::parse("23.38", "USD"),
                web_order_id: Some("373838988".to_string()),
            },
            lines: vec![ParsedLine {
                line_number: Some(1),
                supplier_sku: Some("296-1234-ND".to_string()),
                mpn: Some("TPS7A4700RGWR".to_string()),
                manufacturer: Some("Texas Instruments".to_string()),
                description: Some("LDO REG".to_string()),
                ordered: Some(Quantity::from_whole(10).unwrap()),
                shipped: Some(Quantity::from_whole(8).unwrap()),
                backordered: Some(Quantity::from_whole(2).unwrap()),
                unit_price: Money::parse("1.82000", "USD"),
                extended_price: Money::parse("14.56", "USD"),
                packaging: Some("Cut Tape".to_string()),
                customer_reference: None,
                kind: LineKind::Part,
                confidence: 1.0,
                raw: serde_json::json!({"Part Number": "296-1234-ND"}),
            }],
            warnings: vec!["example warning".to_string()],
        }
    }

    #[test]
    fn parsed_invoice_serde_round_trips() {
        let invoice = sample_invoice();
        let json = serde_json::to_string(&invoice).unwrap();
        let back: ParsedInvoice = serde_json::from_str(&json).unwrap();
        assert_eq!(back, invoice);
    }

    #[test]
    fn parsed_line_serde_round_trips() {
        let line = sample_invoice().lines.remove(0);
        let json = serde_json::to_string(&line).unwrap();
        let back: ParsedLine = serde_json::from_str(&json).unwrap();
        assert_eq!(back, line);
    }
}
