//! Dashboard aggregates (Phase 3 Task 3): a single cheap summary query plus
//! a recent-activity feed, so the desktop dashboard never has to run N
//! client-side queries (one per part) to answer "what's in inventory".

use inventory_core::ids::{GroupId, PartId, TransactionId};
use inventory_core::quantity::{Quantity, QuantityUnit};

use crate::{Database, DbError};

/// Inventory-wide counts for the dashboard's summary cards. Unit sums
/// (`available_units`/`reserved_units`/`checked_out_units`) are raw
/// milli-unit totals summed across every non-archived part regardless of
/// that part's own `quantity_unit` — a mixed sum of "each"/"meter"/"foot"
/// stock has no single true unit, so the dashboard renders it as a plain
/// count rather than claiming a unit suffix that would be wrong for most of
/// the parts it covers.
///
/// Every count here excludes archived parts (matching `list_parts(false)`
/// and the search command's default), because archived stock is no longer
/// part of the "what's in inventory right now" picture the dashboard
/// answers. `active_project_count` is the one exception: the `projects`
/// table is still a Phase 4 stub (just `id`/`name`, no status column), so it
/// counts every project until that lands.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DashboardSummary {
    pub available_units: i64,
    pub part_count: i64,
    pub reserved_units: i64,
    pub checked_out_units: i64,
    pub low_stock_count: i64,
    pub active_project_count: i64,
    pub metadata_incomplete_count: i64,
    pub unbinned_count: i64,
}

/// One row of the dashboard's recent-activity feed: a ledger transaction
/// joined to its part's display name and unit (so the frontend can format
/// the quantity correctly without a further per-row query), plus a
/// backend-computed `reversible` flag — the same reversal rules
/// `reverse_transaction` enforces (not a reversal itself, not part of a
/// group, not already reversed) — so the UI can safely show/hide the
/// "reverse" action without re-deriving that business logic client-side.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RecentTxn {
    pub id: TransactionId,
    pub part_id: PartId,
    pub display_name: String,
    pub txn_type: String,
    pub quantity: Quantity,
    pub quantity_unit: QuantityUnit,
    pub created_at: String,
    pub group_id: Option<GroupId>,
    pub reversible: bool,
}

impl Database {
    /// One aggregate query for the stock/part-level counts plus one more for
    /// the project count — cheap and constant in round trips regardless of
    /// how many parts exist.
    pub fn dashboard_summary(&self) -> Result<DashboardSummary, DbError> {
        #[allow(clippy::type_complexity)]
        let (
            part_count,
            available_units,
            reserved_units,
            checked_out_units,
            low_stock_count,
            metadata_incomplete_count,
            unbinned_count,
        ): (i64, i64, i64, i64, i64, i64, i64) = self.raw_conn().query_row(
            "SELECT
                COUNT(*),
                COALESCE(SUM(s.available_milli), 0),
                COALESCE(SUM(s.reserved_milli), 0),
                COALESCE(SUM(s.checked_out_milli), 0),
                COALESCE(SUM(CASE WHEN p.low_stock_threshold_milli IS NOT NULL
                                   AND s.available_milli < p.low_stock_threshold_milli
                              THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.metadata_complete = 0 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN p.bin_label IS NULL THEN 1 ELSE 0 END), 0)
             FROM parts p JOIN part_stock s ON s.part_id = p.id
             WHERE p.archived = 0",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )?;

        let active_project_count: i64 =
            self.raw_conn()
                .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;

        Ok(DashboardSummary {
            available_units,
            part_count,
            reserved_units,
            checked_out_units,
            low_stock_count,
            active_project_count,
            metadata_incomplete_count,
            unbinned_count,
        })
    }

    /// The `limit` most recent ledger rows across every part, newest first
    /// (via `rowid`, same ordering rationale as `list_transactions`: it's
    /// insertion order, and `created_at` is only second-granular).
    pub fn recent_transactions(&self, limit: i64) -> Result<Vec<RecentTxn>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT t.id, t.part_id, p.display_name, t.txn_type, t.quantity_milli,
                    p.quantity_unit, t.created_at, t.group_id,
                    (t.txn_type != 'reverse' AND t.group_id IS NULL
                     AND NOT EXISTS (SELECT 1 FROM transactions r WHERE r.reversed_txn_id = t.id))
             FROM transactions t
             JOIN parts p ON p.id = t.part_id
             ORDER BY t.rowid DESC
             LIMIT ?1",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_recent_txn(row)?);
        }
        Ok(out)
    }
}

fn row_to_recent_txn(row: &rusqlite::Row<'_>) -> Result<RecentTxn, DbError> {
    let bad = |what: &str| DbError::Corrupt(format!("bad {what} in transactions row"));
    let unit_raw: String = row.get(5)?;
    let quantity_unit = QuantityUnit::from_sql(&unit_raw).ok_or_else(|| bad("quantity_unit"))?;
    Ok(RecentTxn {
        id: TransactionId::from_string(row.get(0)?).map_err(|_| bad("id"))?,
        part_id: PartId::from_string(row.get(1)?).map_err(|_| bad("part_id"))?,
        display_name: row.get(2)?,
        txn_type: row.get(3)?,
        quantity: Quantity::from_milli(row.get(4)?, quantity_unit).map_err(|_| bad("quantity"))?,
        quantity_unit,
        created_at: row.get(6)?,
        group_id: row
            .get::<_, Option<String>>(7)?
            .map(GroupId::from_string)
            .transpose()
            .map_err(|_| bad("group_id"))?,
        reversible: row.get(8)?,
    })
}
