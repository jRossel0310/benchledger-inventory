//! Typed attribute values: parse/validate per definition, store original +
//! normalized. Identity attributes feed 2c duplicate matching.

use inventory_core::ids::PartId;
use inventory_core::units::{parse_with_kind, UnitKind};
use rusqlite::Transaction;

use crate::{Database, DbError};

struct AttrDef {
    id: String,
    data_type: String,
    unit_kind: Option<UnitKind>,
}

/// Look up an attribute definition by key against any connection-like value
/// (`&Connection` or `&Transaction`, which derefs to `Connection`) — the
/// shared lookup both `Database::set_attribute` and the in-tx
/// `set_attribute_in_tx` (Phase 5c Task 5's `apply_enrichment`) use, so the
/// two never drift.
fn attr_def(conn: &rusqlite::Connection, key: &str) -> Result<AttrDef, DbError> {
    let row = conn.query_row(
        "SELECT id, data_type, unit_kind FROM attribute_defs WHERE key = ?1",
        [key],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    );
    match row {
        Ok((id, data_type, unit)) => Ok(AttrDef {
            id,
            data_type,
            unit_kind: unit.as_deref().and_then(UnitKind::from_sql),
        }),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(DbError::AttributeNotFound(key.into())),
        Err(e) => Err(DbError::Sqlite(e)),
    }
}

/// In-transaction body of `Database::set_attribute`: validates+parses `raw`
/// against `key`'s definition and upserts `part_attribute_values`, but does
/// NOT commit and does NOT call `refresh_search_text` — mirrors
/// `parts::create_part_in_tx`/`add_variant_in_tx`. Phase 5c Task 5's
/// `apply_enrichment` calls this directly so an attribute write composes
/// into the same one-transaction, all-or-nothing apply as the other field
/// writes and the provenance upsert.
pub(crate) fn set_attribute_in_tx(
    tx: &Transaction<'_>,
    part_id: &PartId,
    key: &str,
    raw: &str,
) -> Result<(), DbError> {
    let def = attr_def(tx, key)?;
    let exists: i64 = tx.query_row(
        "SELECT COUNT(*) FROM parts WHERE id = ?1",
        [part_id.as_str()],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return Err(DbError::PartNotFound);
    }
    let invalid = |reason: &str| DbError::InvalidAttributeValue {
        key: key.into(),
        reason: reason.into(),
    };
    let raw_trim = raw.trim();
    if raw_trim.is_empty() {
        return Err(invalid("empty value"));
    }
    let (value_num, value_num_hi, value_text, value_bool): (
        Option<f64>,
        Option<f64>,
        Option<String>,
        Option<bool>,
    ) = match def.data_type.as_str() {
        "text" | "url" => (None, None, Some(raw_trim.to_string()), None),
        "number" => {
            let n: f64 = raw_trim.parse().map_err(|_| invalid("not a number"))?;
            (Some(n), None, None, None)
        }
        "number_unit" => {
            let kind = def
                .unit_kind
                .ok_or_else(|| invalid("definition lacks unit kind"))?;
            let parsed = parse_with_kind(raw_trim, kind).map_err(|e| invalid(&e.to_string()))?;
            (Some(parsed.to_f64()), None, None, None)
        }
        "boolean" => {
            let b = match raw_trim.to_lowercase().as_str() {
                "true" | "yes" | "1" => true,
                "false" | "no" | "0" => false,
                _ => return Err(invalid("expected true/false")),
            };
            (None, None, None, Some(b))
        }
        "choice" => {
            let n: i64 = tx.query_row(
                "SELECT COUNT(*) FROM attribute_choices WHERE attribute_id = ?1 AND value = ?2",
                rusqlite::params![def.id, raw_trim],
                |r| r.get(0),
            )?;
            if n == 0 {
                return Err(invalid("not one of the defined choices"));
            }
            (None, None, Some(raw_trim.to_string()), None)
        }
        "multi_choice" => {
            let values: Vec<String> = raw_trim.split(',').map(|s| s.trim().to_string()).collect();
            for v in &values {
                let n: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM attribute_choices WHERE attribute_id = ?1 AND value = ?2",
                    rusqlite::params![def.id, v],
                    |r| r.get(0),
                )?;
                if n == 0 {
                    return Err(invalid(&format!("'{v}' is not one of the defined choices")));
                }
            }
            (
                None,
                None,
                Some(serde_json::to_string(&values).expect("json")),
                None,
            )
        }
        "range" => {
            let kind = def
                .unit_kind
                .ok_or_else(|| invalid("definition lacks unit kind"))?;
            let (lo, hi) = split_range(raw_trim).ok_or_else(|| invalid("expected 'low..high'"))?;
            let lo = parse_with_kind(lo.trim(), kind).map_err(|e| invalid(&e.to_string()))?;
            let hi = parse_with_kind(hi.trim(), kind).map_err(|e| invalid(&e.to_string()))?;
            if lo.to_f64() > hi.to_f64() {
                return Err(invalid("low bound exceeds high bound"));
            }
            (Some(lo.to_f64()), Some(hi.to_f64()), None, None)
        }
        other => return Err(invalid(&format!("unsupported data type {other}"))),
    };
    tx.execute(
        "INSERT INTO part_attribute_values (part_id, attribute_id, original_text, value_num, value_num_hi, value_text, value_bool)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(part_id, attribute_id) DO UPDATE SET
           original_text = excluded.original_text,
           value_num = excluded.value_num,
           value_num_hi = excluded.value_num_hi,
           value_text = excluded.value_text,
           value_bool = excluded.value_bool",
        rusqlite::params![part_id.as_str(), def.id, raw_trim, value_num, value_num_hi, value_text, value_bool],
    )?;
    Ok(())
}

/// Split a range value like `"1V..2V"` or `"1V to 2V"` into its (untrimmed)
/// low/high substrings. Shared by `set_attribute` and `identity::identity_signature`
/// so both agree on what counts as a range separator.
pub(crate) fn split_range(raw: &str) -> Option<(&str, &str)> {
    raw.split_once("..").or_else(|| raw.split_once(" to "))
}

/// Format `raw` under `unit_kind`'s parsing rules into its canonical display
/// form (e.g. `unit_kind: "resistance", raw: "10k"` -> `"10 kΩ"`) — the same
/// `parse_with_kind`/`ParsedValue::format` primitive `set_attribute`'s
/// `number_unit`/`range` branches use to normalize a stored value, exposed
/// standalone (no `Database`/part needed) so a `number_unit` field in the
/// part create/edit form can show a live normalized preview as the user
/// types, before the value is ever attached to a part. Stateless: never
/// touches the database, so it can't fail on a missing attribute definition
/// the way `set_attribute` can — only on an unrecognized `unit_kind` or an
/// unparsable `raw`.
pub fn preview_unit_value(unit_kind: &str, raw: &str) -> Result<String, DbError> {
    let invalid = |key: &str, reason: String| DbError::InvalidAttributeValue {
        key: key.to_string(),
        reason,
    };
    let kind = UnitKind::from_sql(unit_kind)
        .ok_or_else(|| invalid("unit_kind", format!("unknown unit kind '{unit_kind}'")))?;
    let raw_trim = raw.trim();
    if raw_trim.is_empty() {
        return Err(invalid("value", "empty value".to_string()));
    }
    let parsed = parse_with_kind(raw_trim, kind).map_err(|e| invalid("value", e.to_string()))?;
    Ok(parsed.format())
}

/// One part's attribute value joined with enough of its definition to
/// render or export it without a second lookup: `label` (display name) and
/// `canonical_unit` (the fixed unit `normalized_value` is expressed in —
/// `attribute_defs.canonical_unit`, set only for `number_unit`/`range`
/// attributes) alongside the plain `(key, original_text, value_num)` triple
/// `get_attributes` already returns. Added for the Phase 6 public snapshot
/// builder (`inventory-sync`), which needs the label and canonical unit to
/// render a part's specs without depending on `inventory-db`'s internal
/// schema — but it's a generally useful join, not snapshot-specific.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AttributeValueRow {
    pub key: String,
    pub label: String,
    pub original_text: String,
    pub normalized_value: Option<f64>,
    pub canonical_unit: Option<String>,
}

impl Database {
    /// Every attribute value set on `part_id`, joined with its definition's
    /// label and canonical unit, ordered by key (same order as
    /// `get_attributes`).
    pub fn list_attribute_values(
        &self,
        part_id: &PartId,
    ) -> Result<Vec<AttributeValueRow>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT a.key, a.label, v.original_text, v.value_num, a.canonical_unit
             FROM part_attribute_values v JOIN attribute_defs a ON a.id = v.attribute_id
             WHERE v.part_id = ?1 ORDER BY a.key",
        )?;
        let mut rows = stmt.query([part_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(AttributeValueRow {
                key: row.get(0)?,
                label: row.get(1)?,
                original_text: row.get(2)?,
                normalized_value: row.get(3)?,
                canonical_unit: row.get(4)?,
            });
        }
        Ok(out)
    }

    pub fn set_attribute(&mut self, part_id: &PartId, key: &str, raw: &str) -> Result<(), DbError> {
        let tx = self.conn_mut().transaction()?;
        set_attribute_in_tx(&tx, part_id, key, raw)?;
        tx.commit()?;
        self.refresh_search_text(part_id)?;
        Ok(())
    }

    pub fn get_attributes(
        &self,
        part_id: &PartId,
    ) -> Result<Vec<(String, String, Option<f64>)>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT a.key, v.original_text, v.value_num
             FROM part_attribute_values v JOIN attribute_defs a ON a.id = v.attribute_id
             WHERE v.part_id = ?1 ORDER BY a.key",
        )?;
        let mut rows = stmt.query([part_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get(0)?, row.get(1)?, row.get(2)?));
        }
        Ok(out)
    }

    pub fn identity_attributes(
        &self,
        part_id: &PartId,
    ) -> Result<Vec<(String, Option<f64>, String)>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT a.key, v.value_num, v.original_text
             FROM part_attribute_values v JOIN attribute_defs a ON a.id = v.attribute_id
             WHERE v.part_id = ?1 AND a.identity = 1 ORDER BY a.key",
        )?;
        let mut rows = stmt.query([part_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get(0)?, row.get(1)?, row.get(2)?));
        }
        Ok(out)
    }

    pub fn clear_attribute(&mut self, part_id: &PartId, key: &str) -> Result<(), DbError> {
        let def = attr_def(self.raw_conn(), key)?;
        self.raw_conn().execute(
            "DELETE FROM part_attribute_values WHERE part_id = ?1 AND attribute_id = ?2",
            rusqlite::params![part_id.as_str(), def.id],
        )?;
        self.refresh_search_text(part_id)?;
        Ok(())
    }
}
