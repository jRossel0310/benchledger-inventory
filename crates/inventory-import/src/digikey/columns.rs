//! Format-agnostic DigiKey header -> field mapping.
//!
//! DigiKey order/invoice exports (CSV now, XLSX in a later task) use varying
//! header text for the same logical column across export tools and account
//! settings. This module owns the alias lists and the header-row lookup so
//! every DigiKey format-specific parser maps headers to fields the exact
//! same way — they cannot drift apart.

use std::collections::HashMap;

/// A logical DigiKey invoice-line field, independent of which header text a
/// particular export uses to label it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    LineNumber,
    SupplierSku,
    Mpn,
    Manufacturer,
    Description,
    /// Shipped/received quantity — DigiKey labels this column "Quantity" on
    /// most exports, but it is the *shipped* amount, not ordered.
    Shipped,
    Ordered,
    Backordered,
    UnitPrice,
    ExtendedPrice,
    Packaging,
    CustomerReference,
}

impl Field {
    /// Every field this module knows how to locate, in a stable order.
    pub const ALL: [Field; 12] = [
        Field::LineNumber,
        Field::SupplierSku,
        Field::Mpn,
        Field::Manufacturer,
        Field::Description,
        Field::Shipped,
        Field::Ordered,
        Field::Backordered,
        Field::UnitPrice,
        Field::ExtendedPrice,
        Field::Packaging,
        Field::CustomerReference,
    ];

    /// Recognized header aliases for this field: lowercase, trimmed, exact
    /// match against a normalized header cell.
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Field::LineNumber => &["index", "line", "line item"],
            Field::SupplierSku => &[
                "digikey part #",
                "digi-key part number",
                "part number",
                "dk part #",
            ],
            Field::Mpn => &[
                "manufacturer part number",
                "mfr part #",
                "manufacturer part #",
                "mpn",
            ],
            Field::Manufacturer => &["manufacturer", "mfr", "mfg"],
            Field::Description => &["description"],
            Field::Shipped => &[
                "quantity",
                "shipped quantity",
                "qty shipped",
                "quantity shipped",
            ],
            Field::Ordered => &["ordered quantity", "qty ordered"],
            Field::Backordered => &["backorder quantity", "backordered quantity", "backorder"],
            Field::UnitPrice => &["unit price"],
            Field::ExtendedPrice => &["extended price", "ext price", "amount"],
            Field::Packaging => &["packaging"],
            Field::CustomerReference => &["customer reference", "customer ref"],
        }
    }
}

/// Normalize a header cell for alias matching: trim surrounding whitespace,
/// lowercase. DigiKey exports vary casing ("Quantity" vs "QUANTITY") and
/// sometimes pad cells with whitespace.
pub fn normalize_header(header: &str) -> String {
    header.trim().to_lowercase()
}

/// Maps logical fields to the column index they were found at in one
/// header row. Built once per file and reused for every data row.
#[derive(Debug, Clone, Default)]
pub struct ColumnMap {
    fields: HashMap<Field, usize>,
}

impl ColumnMap {
    /// Build a map from a header row by matching each cell (case/whitespace
    /// insensitive) against every field's alias list. Matching is by NAME
    /// only, so a header with columns in any order maps identically. The
    /// first column matching a field's alias wins if a header repeats an
    /// alias (shouldn't happen in practice).
    pub fn from_header<S: AsRef<str>>(header: &[S]) -> Self {
        let mut fields = HashMap::new();
        for (idx, cell) in header.iter().enumerate() {
            let normalized = normalize_header(cell.as_ref());
            for field in Field::ALL {
                if fields.contains_key(&field) {
                    continue;
                }
                if field.aliases().contains(&normalized.as_str()) {
                    fields.insert(field, idx);
                }
            }
        }
        ColumnMap { fields }
    }

    /// The column index a field was found at, if the header row contained a
    /// recognized alias for it.
    pub fn index_of(&self, field: Field) -> Option<usize> {
        self.fields.get(&field).copied()
    }

    /// True if `header` contains a cell identifying it as a DigiKey
    /// line-item table — a supplier-SKU or description column alias. Used
    /// to locate the header row when preamble lines (title/address blocks)
    /// precede it.
    pub fn looks_like_header<S: AsRef<str>>(header: &[S]) -> bool {
        header.iter().any(|cell| {
            let normalized = normalize_header(cell.as_ref());
            Field::SupplierSku.aliases().contains(&normalized.as_str())
                || Field::Description.aliases().contains(&normalized.as_str())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_aliases_to_column_indices() {
        let header = vec![
            "Index".to_string(),
            "DigiKey Part #".to_string(),
            "Manufacturer Part Number".to_string(),
            "Quantity".to_string(),
        ];
        let map = ColumnMap::from_header(&header);
        assert_eq!(map.index_of(Field::LineNumber), Some(0));
        assert_eq!(map.index_of(Field::SupplierSku), Some(1));
        assert_eq!(map.index_of(Field::Mpn), Some(2));
        assert_eq!(map.index_of(Field::Shipped), Some(3));
        assert_eq!(map.index_of(Field::Manufacturer), None);
    }

    #[test]
    fn header_matching_is_case_and_whitespace_insensitive() {
        let header = vec![
            "  DIGI-KEY PART NUMBER  ".to_string(),
            " qty ordered ".to_string(),
        ];
        let map = ColumnMap::from_header(&header);
        assert_eq!(map.index_of(Field::SupplierSku), Some(0));
        assert_eq!(map.index_of(Field::Ordered), Some(1));
    }

    #[test]
    fn mapping_is_order_independent() {
        let header_a = vec!["Part Number".to_string(), "Manufacturer".to_string()];
        let header_b = vec!["Manufacturer".to_string(), "Part Number".to_string()];
        let map_a = ColumnMap::from_header(&header_a);
        let map_b = ColumnMap::from_header(&header_b);
        assert_eq!(map_a.index_of(Field::SupplierSku), Some(0));
        assert_eq!(map_b.index_of(Field::SupplierSku), Some(1));
        assert_eq!(map_a.index_of(Field::Manufacturer), Some(1));
        assert_eq!(map_b.index_of(Field::Manufacturer), Some(0));
    }

    #[test]
    fn looks_like_header_detects_sku_or_description_alias() {
        assert!(ColumnMap::looks_like_header(&["Description".to_string()]));
        assert!(ColumnMap::looks_like_header(&[
            "Digi-Key Part Number".to_string()
        ]));
        assert!(!ColumnMap::looks_like_header(&[
            "TEST CUSTOMER".to_string(),
            "1 EXAMPLE ST".to_string()
        ]));
    }

    #[test]
    fn unrecognized_header_yields_empty_map() {
        let header = vec!["Foo".to_string(), "Bar".to_string()];
        let map = ColumnMap::from_header(&header);
        for field in Field::ALL {
            assert_eq!(map.index_of(field), None);
        }
    }
}
