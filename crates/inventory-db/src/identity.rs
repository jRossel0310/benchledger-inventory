//! Exact-form identity signatures for duplicate matching (Phase 2c).
//!
//! An identity signature is the set of a part's identity-flagged attribute
//! values, each re-parsed into its exact form so that equivalent notations
//! ("10k" vs "10000 ohm", "0603" vs "1608 metric") compare equal. Comparison
//! NEVER goes through `f64` — see `inventory_core::units::ParsedValue`'s
//! doc comment for why lossy float comparison is unsound here.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;

use inventory_core::ids::PartId;
use inventory_core::packages::normalize_package;
use inventory_core::units::{parse_with_kind, ParsedValue, UnitKind};

use crate::attributes::split_range;
use crate::{Database, DbError};

/// One identity-attribute value, held in its exact comparable form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdentityValue {
    /// `number_unit` attributes: the re-parsed exact value.
    Exact(ParsedValue),
    /// `range` attributes: re-parsed (low, high) bounds.
    Range(ParsedValue, ParsedValue),
    /// The `package` attribute: `normalize_package(...).canonical`.
    Package(String),
    /// `choice`/`text`/`number` attributes: the trimmed original text.
    Text(String),
}

struct IdentityDef {
    key: String,
    data_type: String,
    unit_kind: Option<UnitKind>,
}

/// Build a part's identity signature: one `IdentityValue` per identity
/// attribute linked (and not hidden) on the part's category, keyed by
/// attribute key.
///
/// Returns `Ok(None)` when:
/// - the part's category has no identity attributes (nothing defines identity), or
/// - any identity attribute is missing on the part, or
/// - any identity attribute's stored value fails to re-parse under its def
///   (shouldn't happen for values that passed `set_attribute`, but a
///   signature is only as trustworthy as its weakest field, so any
///   unparsable value makes identity indeterminate).
///
/// Non-identity attributes are ignored entirely.
pub fn identity_signature(
    db: &Database,
    part_id: &PartId,
) -> Result<Option<BTreeMap<String, IdentityValue>>, DbError> {
    let part = db.get_part(part_id)?.ok_or(DbError::PartNotFound)?;

    let mut stmt = db.raw_conn().prepare(
        "SELECT a.key, a.data_type, a.unit_kind
         FROM category_attributes ca
         JOIN attribute_defs a ON a.id = ca.attribute_id
         WHERE ca.category_id = ?1 AND a.identity = 1 AND ca.hidden = 0
         ORDER BY a.key",
    )?;
    let defs: Vec<IdentityDef> = stmt
        .query_map([part.category_id.as_str()], |r| {
            let unit_kind: Option<String> = r.get(2)?;
            Ok(IdentityDef {
                key: r.get(0)?,
                data_type: r.get(1)?,
                unit_kind: unit_kind.as_deref().and_then(UnitKind::from_sql),
            })
        })?
        .collect::<Result<_, _>>()?;

    if defs.is_empty() {
        return Ok(None);
    }

    let mut out = BTreeMap::new();
    for def in &defs {
        let original: Option<String> = db
            .raw_conn()
            .query_row(
                "SELECT v.original_text
                 FROM part_attribute_values v
                 JOIN attribute_defs a ON a.id = v.attribute_id
                 WHERE v.part_id = ?1 AND a.key = ?2",
                rusqlite::params![part_id.as_str(), def.key],
                |r| r.get(0),
            )
            .optional()?;
        let Some(original) = original else {
            return Ok(None); // missing identity attribute -> incomplete identity
        };
        let original = original.trim();

        let value = if def.key == "package" {
            match normalize_package(original) {
                Some(np) => IdentityValue::Package(np.canonical),
                None => return Ok(None),
            }
        } else {
            match def.data_type.as_str() {
                "number_unit" => {
                    let Some(kind) = def.unit_kind else {
                        return Ok(None);
                    };
                    match parse_with_kind(original, kind) {
                        Ok(v) => IdentityValue::Exact(v),
                        Err(_) => return Ok(None),
                    }
                }
                "range" => {
                    let Some(kind) = def.unit_kind else {
                        return Ok(None);
                    };
                    let Some((lo, hi)) = split_range(original) else {
                        return Ok(None);
                    };
                    let (Ok(lo), Ok(hi)) = (
                        parse_with_kind(lo.trim(), kind),
                        parse_with_kind(hi.trim(), kind),
                    ) else {
                        return Ok(None);
                    };
                    IdentityValue::Range(lo, hi)
                }
                _ => IdentityValue::Text(original.to_string()),
            }
        };
        out.insert(def.key.clone(), value);
    }
    Ok(Some(out))
}

/// Whether two identity signatures denote the same part identity. Plain
/// `==`; exists so call sites read as domain logic rather than a stray
/// equality check.
pub fn signatures_equal(
    a: &BTreeMap<String, IdentityValue>,
    b: &BTreeMap<String, IdentityValue>,
) -> bool {
    a == b
}
