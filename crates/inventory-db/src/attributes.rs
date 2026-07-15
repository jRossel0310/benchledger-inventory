//! Typed attribute values: parse/validate per definition, store original +
//! normalized. Identity attributes feed 2c duplicate matching.

use inventory_core::ids::PartId;
use inventory_core::units::{parse_with_kind, UnitKind};

use crate::{Database, DbError};

struct AttrDef {
    id: String,
    data_type: String,
    unit_kind: Option<UnitKind>,
}

/// Split a range value like `"1V..2V"` or `"1V to 2V"` into its (untrimmed)
/// low/high substrings. Shared by `set_attribute` and `identity::identity_signature`
/// so both agree on what counts as a range separator.
pub(crate) fn split_range(raw: &str) -> Option<(&str, &str)> {
    raw.split_once("..").or_else(|| raw.split_once(" to "))
}

impl Database {
    fn attr_def(&self, key: &str) -> Result<AttrDef, DbError> {
        let row = self.raw_conn().query_row(
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
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(DbError::AttributeNotFound(key.into()))
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    pub fn set_attribute(&mut self, part_id: &PartId, key: &str, raw: &str) -> Result<(), DbError> {
        let def = self.attr_def(key)?;
        if self.get_part(part_id)?.is_none() {
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
                let parsed =
                    parse_with_kind(raw_trim, kind).map_err(|e| invalid(&e.to_string()))?;
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
                let n: i64 = self.raw_conn().query_row(
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
                let values: Vec<String> =
                    raw_trim.split(',').map(|s| s.trim().to_string()).collect();
                for v in &values {
                    let n: i64 = self.raw_conn().query_row(
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
                let (lo, hi) =
                    split_range(raw_trim).ok_or_else(|| invalid("expected 'low..high'"))?;
                let lo = parse_with_kind(lo.trim(), kind).map_err(|e| invalid(&e.to_string()))?;
                let hi = parse_with_kind(hi.trim(), kind).map_err(|e| invalid(&e.to_string()))?;
                if lo.to_f64() > hi.to_f64() {
                    return Err(invalid("low bound exceeds high bound"));
                }
                (Some(lo.to_f64()), Some(hi.to_f64()), None, None)
            }
            other => return Err(invalid(&format!("unsupported data type {other}"))),
        };
        self.raw_conn().execute(
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
        let def = self.attr_def(key)?;
        self.raw_conn().execute(
            "DELETE FROM part_attribute_values WHERE part_id = ?1 AND attribute_id = ?2",
            rusqlite::params![part_id.as_str(), def.id],
        )?;
        Ok(())
    }
}
