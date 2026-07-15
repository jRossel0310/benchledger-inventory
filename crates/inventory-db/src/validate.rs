//! Third defense line: recompute stock aggregates from the full ledger and
//! compare with `part_stock`. Run quietly at startup, before backups/restores,
//! and in tests.

use std::collections::HashMap;

use inventory_core::ids::PartId;
use inventory_core::ledger::StockDelta;

use crate::ledger::delta_from_stored;
use crate::{Database, DbError};

#[derive(Debug, Clone)]
pub struct Discrepancy {
    pub part_id: PartId,
    pub field: String,
    pub stored: i64,
    pub recomputed: i64,
}

#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub parts_checked: usize,
    pub discrepancies: Vec<Discrepancy>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.discrepancies.is_empty()
    }
}

impl Database {
    pub fn validate_invariants(&self) -> Result<ValidationReport, DbError> {
        // Recompute per-part totals by replaying every ledger row.
        let mut recomputed: HashMap<String, StockDelta> = HashMap::new();
        {
            // Replay order is immaterial (pure summation) — rowid kept for deterministic debugging output.
            let mut stmt = self.raw_conn().prepare(
                "SELECT t.id, t.part_id, t.group_id, t.txn_type, t.quantity_milli, t.from_state,
                        t.to_state, t.project_id, t.to_project_id, t.note, t.reversed_txn_id,
                        t.created_at, o.txn_type
                 FROM transactions t
                 LEFT JOIN transactions o ON o.id = t.reversed_txn_id
                 ORDER BY t.rowid",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let mut txn = crate::ledger::row_to_txn(row)?;
                let delta = if txn.txn_type == "reverse" {
                    let original_type: Option<String> = row.get(12)?;
                    let original_type = original_type.ok_or_else(|| {
                        DbError::Corrupt(format!("reversal {} lacks its original", txn.id.as_str()))
                    })?;
                    txn.txn_type = original_type;
                    delta_from_stored(&txn)?.inverse()
                } else {
                    delta_from_stored(&txn)?
                };
                let entry = recomputed.entry(txn.part_id.as_str().to_string()).or_default();
                entry.available += delta.available;
                entry.reserved += delta.reserved;
                entry.checked_out += delta.checked_out;
                entry.lifetime_received += delta.lifetime_received;
                entry.lifetime_consumed += delta.lifetime_consumed;
            }
        }

        // Compare against stored aggregates for every part.
        let mut discrepancies = Vec::new();
        let mut parts_checked = 0usize;
        let mut stmt = self.raw_conn().prepare(
            "SELECT part_id, available_milli, reserved_milli, checked_out_milli,
                    lifetime_received_milli, lifetime_consumed_milli
             FROM part_stock ORDER BY part_id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            parts_checked += 1;
            let part_raw: String = row.get(0)?;
            let part_id = PartId::from_string(part_raw.clone())
                .map_err(|_| DbError::Corrupt("bad part id in part_stock".into()))?;
            let expected = recomputed.remove(&part_raw).unwrap_or_default();
            let stored = [
                ("available_milli", row.get::<_, i64>(1)?, expected.available),
                ("reserved_milli", row.get::<_, i64>(2)?, expected.reserved),
                ("checked_out_milli", row.get::<_, i64>(3)?, expected.checked_out),
                ("lifetime_received_milli", row.get::<_, i64>(4)?, expected.lifetime_received),
                ("lifetime_consumed_milli", row.get::<_, i64>(5)?, expected.lifetime_consumed),
            ];
            for (field, stored_v, recomputed_v) in stored {
                if stored_v != recomputed_v {
                    discrepancies.push(Discrepancy {
                        part_id: part_id.clone(),
                        field: field.to_string(),
                        stored: stored_v,
                        recomputed: recomputed_v,
                    });
                }
            }
        }
        // Any leftover recomputed entries mean ledger rows exist for parts
        // missing a part_stock row.
        for (part_raw, expected) in recomputed {
            let part_id = PartId::from_string(part_raw)
                .map_err(|_| DbError::Corrupt("bad part id in transactions".into()))?;
            let fields = [
                ("available_milli (part_stock row missing)", expected.available),
                ("reserved_milli (part_stock row missing)", expected.reserved),
                ("checked_out_milli (part_stock row missing)", expected.checked_out),
                ("lifetime_received_milli (part_stock row missing)", expected.lifetime_received),
                ("lifetime_consumed_milli (part_stock row missing)", expected.lifetime_consumed),
            ];
            for (field, recomputed_v) in fields {
                discrepancies.push(Discrepancy {
                    part_id: part_id.clone(),
                    field: field.to_string(),
                    stored: 0,
                    recomputed: recomputed_v,
                });
            }
        }

        Ok(ValidationReport { parts_checked, discrepancies })
    }
}
