//! Shared per-row parsing + classification for DigiKey line-item tables.
//!
//! CSV and XLSX exports differ only in *how* they arrive at a `Vec<String>`
//! row of cell text (a `csv::StringRecord` vs. a stringified calamine
//! `Data` row). Once a row is reduced to plain strings, the mapping from a
//! header row to logical fields and the classification of a data row into a
//! [`ParsedLine`] happens exactly once here — both format-specific parsers
//! call [`parse_row`] so they cannot drift apart on fee/tariff/no-charge/
//! missing-MPN rules.

use crate::digikey::columns::{ColumnMap, Field};
use crate::model::{LineKind, Money, ParsedLine};
use inventory_core::quantity::Quantity;

/// Currency every DigiKey CSV/XLSX line-item export is assumed to use —
/// neither format carries a per-file currency cell (unlike the PDF PO
/// acknowledgement, which states `USD $` explicitly).
pub const CURRENCY: &str = "USD";

const FEE_KEYWORDS: [&str; 4] = ["SHIPPING", "TARIFF", "TAX", "FREIGHT"];

/// Build the `raw` JSON object of original header -> cell pairs for one row,
/// verbatim, so nothing the source contained is lost even when the typed
/// fields below couldn't capture it.
pub fn raw_json(header: &[String], row: &[String]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (h, c) in header.iter().zip(row.iter()) {
        map.insert(h.clone(), serde_json::Value::String(c.clone()));
    }
    serde_json::Value::Object(map)
}

/// Trimmed, empty-string-as-None lookup of a field's cell by name.
fn cell(row: &[String], map: &ColumnMap, field: Field) -> Option<String> {
    map.index_of(field)
        .and_then(|idx| row.get(idx))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Whole-number DigiKey quantity ("3", "1,000") -> a `Quantity` of `each`.
fn parse_quantity(text: &str) -> Option<Quantity> {
    let cleaned = text.replace(',', "");
    let n: i64 = cleaned.trim().parse().ok()?;
    Quantity::from_whole(n).ok()
}

fn is_zero(money: &Option<Money>) -> bool {
    matches!(money, Some(m) if m.micros == 0)
}

/// Map one data row (already reduced to plain cell strings, whatever format
/// it came from) through `map` into a classified [`ParsedLine`]. Never
/// drops a row: an unrecognizable one becomes [`LineKind::Unknown`] with
/// reduced confidence and a warning appended to `warnings`.
pub fn parse_row(
    header: &[String],
    row: &[String],
    map: &ColumnMap,
    warnings: &mut Vec<String>,
) -> Option<ParsedLine> {
    let line_number = cell(row, map, Field::LineNumber).and_then(|s| s.parse::<u32>().ok());
    let supplier_sku = cell(row, map, Field::SupplierSku);
    let mpn = cell(row, map, Field::Mpn);
    let manufacturer = cell(row, map, Field::Manufacturer);
    let description = cell(row, map, Field::Description);
    let packaging = cell(row, map, Field::Packaging);
    let customer_reference = cell(row, map, Field::CustomerReference);

    let shipped = cell(row, map, Field::Shipped).and_then(|s| parse_quantity(&s));
    let ordered = cell(row, map, Field::Ordered).and_then(|s| parse_quantity(&s));
    let backordered = cell(row, map, Field::Backordered).and_then(|s| parse_quantity(&s));

    let unit_price = cell(row, map, Field::UnitPrice).and_then(|s| Money::parse(&s, CURRENCY));
    let extended_price =
        cell(row, map, Field::ExtendedPrice).and_then(|s| Money::parse(&s, CURRENCY));

    let has_part_id = supplier_sku.is_some() || mpn.is_some();
    let fee_keyword = description
        .as_deref()
        .map(|d| d.to_ascii_uppercase())
        .and_then(|d| FEE_KEYWORDS.iter().find(|kw| d.contains(*kw)).copied());

    let (kind, confidence) = if has_part_id {
        if is_zero(&unit_price) || is_zero(&extended_price) {
            (LineKind::NoCharge, 1.0)
        } else if mpn.is_none() {
            // Missing MPN on an otherwise-valid part row: keep it as Part,
            // just with reduced confidence — captured, not dropped.
            (LineKind::Part, 0.8)
        } else {
            (LineKind::Part, 1.0)
        }
    } else if let Some(kw) = fee_keyword {
        if kw == "TARIFF" {
            (LineKind::Tariff, 1.0)
        } else {
            (LineKind::Fee, 1.0)
        }
    } else {
        // Quantity/price present but no SKU, no MPN, and no recognizable
        // fee keyword — never drop it, just flag it as unrecognized.
        warnings.push(format!(
            "row without a supplier SKU/MPN or fee keyword classified as Unknown: {:?}",
            row
        ));
        (LineKind::Unknown, 0.3)
    };

    Some(ParsedLine {
        line_number,
        supplier_sku,
        mpn,
        manufacturer,
        description,
        ordered,
        shipped,
        backordered,
        unit_price,
        extended_price,
        packaging,
        customer_reference,
        kind,
        confidence,
        raw: raw_json(header, row),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Vec<String> {
        vec![
            "Index".to_string(),
            "DigiKey Part #".to_string(),
            "Manufacturer Part Number".to_string(),
            "Manufacturer".to_string(),
            "Description".to_string(),
            "Quantity".to_string(),
            "Unit Price".to_string(),
            "Extended Price".to_string(),
        ]
    }

    #[test]
    fn parse_row_maps_a_normal_part_row() {
        let header = header();
        let map = ColumnMap::from_header(&header);
        let row = vec![
            "1".to_string(),
            "475-BPW34-ND".to_string(),
            "BPW34".to_string(),
            "AMS-OSRAM USA INC.".to_string(),
            "SENSOR PHOTODIODE 850NM 2DIP".to_string(),
            "3".to_string(),
            "1.82000".to_string(),
            "5.46".to_string(),
        ];
        let mut warnings = Vec::new();
        let line = parse_row(&header, &row, &map, &mut warnings).unwrap();
        assert_eq!(line.supplier_sku.as_deref(), Some("475-BPW34-ND"));
        assert_eq!(line.mpn.as_deref(), Some("BPW34"));
        assert_eq!(line.shipped, Some(Quantity::from_whole(3).unwrap()));
        assert_eq!(line.unit_price.as_ref().unwrap().micros, 1_820_000);
        assert_eq!(line.kind, LineKind::Part);
        assert_eq!(line.confidence, 1.0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn parse_row_classifies_fee_row_without_part_id() {
        let header = header();
        let map = ColumnMap::from_header(&header);
        let row = vec![
            "8".to_string(),
            String::new(),
            String::new(),
            String::new(),
            "SHIPPING CHARGES".to_string(),
            String::new(),
            String::new(),
            "4.99".to_string(),
        ];
        let mut warnings = Vec::new();
        let line = parse_row(&header, &row, &map, &mut warnings).unwrap();
        assert_eq!(line.kind, LineKind::Fee);
        assert!(line.supplier_sku.is_none());
    }
}
