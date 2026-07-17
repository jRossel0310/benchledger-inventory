//! History screen aggregates (Phase 3 Task 9): a paged, filtered view over
//! every ledger transaction, joined to the human context the screen needs
//! (part display name/unit/archived state, resolved project name, the
//! owning group's kind) so it never has to fire a further per-row query.
//!
//! Grouped transactions are inserted consecutively within `apply_group`'s/
//! `reverse_group`'s single database transaction (see `ledger.rs`), so
//! ordering by `rowid DESC` — the same ordering rationale `list_transactions`
//! and `recent_transactions` use — keeps every member of a group contiguous
//! in the result. `list_history`'s callers (the History screen) rely on this
//! to cluster consecutive same-`group_id` rows into one expandable group
//! header rather than needing a second "group rollup" query; see
//! `tests/history.rs`'s `group_rollup_reports_kind_and_keeps_members_contiguous`.
//! A group can in principle straddle a page boundary if `limit` doesn't
//! align with a group's member count — an accepted, documented limitation
//! rather than a correctness bug (the group is still fully reversible by its
//! `group_id`; only the visual clustering on that one page is incomplete).
//!
//! `reversible` reuses the exact rule `reverse_transaction` enforces and
//! `dashboard.rs`'s `recent_transactions` already flags with (not a reversal
//! itself, not part of a group, not already reversed) — see that module's
//! doc comment for why this is computed server-side rather than re-derived
//! per screen.

use inventory_core::ids::{GroupId, PartId, ProjectId, TransactionId};
use inventory_core::quantity::{Quantity, QuantityUnit};

use crate::{Database, DbError};

/// Every field is an optional AND-composed narrowing filter over
/// `list_history`'s otherwise-unfiltered "every transaction" result.
/// `date_from`/`date_to` are `YYYY-MM-DD` (compared against
/// `date(transactions.created_at)`, inclusive on both ends). `project_id`
/// matches either leg of a transfer (`project_id` OR `to_project_id`) — the
/// same "either side of the move" semantics `search.rs`'s `project:` filter
/// uses. `limit`/`offset` page the (already filtered) result; `total` on the
/// returned `HistoryPage` is the filtered count before paging, for a
/// pagination control to render against.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct HistoryFilter {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub txn_type: Option<String>,
    pub part_id: Option<PartId>,
    pub project_id: Option<ProjectId>,
    pub group_id: Option<GroupId>,
    pub limit: u32,
    pub offset: u32,
}

/// One ledger transaction plus enough joined context to render a History row
/// without a further per-row query.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct HistoryRow {
    pub id: TransactionId,
    pub part_id: PartId,
    pub display_name: String,
    pub quantity_unit: QuantityUnit,
    /// The part this row's transaction belongs to is currently archived —
    /// drives the History screen's "restore" affordance without a further
    /// per-row `get_part` call.
    pub part_archived: bool,
    pub txn_type: String,
    pub quantity: Quantity,
    pub from_state: Option<String>,
    pub to_state: Option<String>,
    pub project_id: Option<ProjectId>,
    pub to_project_id: Option<ProjectId>,
    /// Resolved name of whichever project leg is set (`to_project_id` takes
    /// priority when both are — a transfer's destination — matching the
    /// same preference `PartDetailTransactions.tsx` already uses client-side
    /// for `TransactionRecord`), or `None` when neither leg is set.
    pub project_name: Option<String>,
    pub note: String,
    pub group_id: Option<GroupId>,
    /// The owning group's `kind` (e.g. `"receive_batch"`,
    /// `"reverse:receive_batch"`), or `None` for an ungrouped row — lets the
    /// History screen label a group header without a further `get_group`
    /// call per group.
    pub group_kind: Option<String>,
    pub reversed_txn_id: Option<TransactionId>,
    /// Set once Phase 5's import tables exist and an import writes it; the
    /// column already exists on `transactions` (reserved ahead of time, see
    /// migration 0002's `import_id` comment) but nothing populates it yet,
    /// so this is always `None` today. Passed through now rather than added
    /// later so the History screen's "view original import" affordance can
    /// honestly render "nothing to show" instead of fabricating a link.
    pub import_id: Option<String>,
    pub created_at: String,
    /// The same reversibility rule `reverse_transaction` enforces and
    /// `dashboard.rs::recent_transactions` already flags with: not a
    /// reversal itself, not part of a group, not already reversed.
    pub reversible: bool,
}

/// A page of `list_history` results plus the total row count the filter
/// matched (independent of `limit`/`offset`), for a pagination control.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct HistoryPage {
    pub rows: Vec<HistoryRow>,
    pub total: u32,
}

impl Database {
    /// Newest-first (`rowid DESC`, the same ordering rationale
    /// `list_transactions`/`recent_transactions` use), filtered, paged view
    /// over every ledger transaction. See the module doc for the group
    /// clustering and reversibility rules this joins in.
    pub fn list_history(&self, filter: &HistoryFilter) -> Result<HistoryPage, DbError> {
        let (where_sql, where_params) = build_where(filter);

        let count_sql = format!(
            "SELECT COUNT(*) FROM transactions t JOIN parts p ON p.id = t.part_id {where_sql}"
        );
        let total: i64 = self.raw_conn().query_row(
            &count_sql,
            rusqlite::params_from_iter(where_params.iter().map(|p| p.as_ref())),
            |r| r.get(0),
        )?;

        let rows_sql = format!(
            "SELECT t.id, t.part_id, p.display_name, p.quantity_unit, p.archived,
                    t.txn_type, t.quantity_milli, t.from_state, t.to_state,
                    t.project_id, t.to_project_id, COALESCE(pj_to.name, pj_from.name),
                    t.note, t.group_id, g.kind, t.reversed_txn_id, t.import_id, t.created_at,
                    (t.txn_type != 'reverse' AND t.group_id IS NULL
                     AND NOT EXISTS (SELECT 1 FROM transactions r WHERE r.reversed_txn_id = t.id))
             FROM transactions t
             JOIN parts p ON p.id = t.part_id
             LEFT JOIN transaction_groups g ON g.id = t.group_id
             LEFT JOIN projects pj_from ON pj_from.id = t.project_id
             LEFT JOIN projects pj_to ON pj_to.id = t.to_project_id
             {where_sql}
             ORDER BY t.rowid DESC
             LIMIT ? OFFSET ?"
        );
        let mut params = where_params;
        params.push(Box::new(filter.limit as i64));
        params.push(Box::new(filter.offset as i64));

        let mut stmt = self.raw_conn().prepare(&rows_sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(
            params.iter().map(|p| p.as_ref()),
        ))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_history(row)?);
        }
        Ok(HistoryPage {
            rows: out,
            total: total as u32,
        })
    }
}

/// Build the shared `WHERE ...` fragment (empty string when no filter is
/// set) and its bound parameters, in the same order the `?` placeholders
/// appear — reused by both the count query and the row query in
/// `list_history` so the two can never drift apart on which rows they
/// consider "matching".
#[allow(clippy::type_complexity)]
fn build_where(filter: &HistoryFilter) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(from) = &filter.date_from {
        clauses.push("date(t.created_at) >= date(?)".to_string());
        params.push(Box::new(from.clone()));
    }
    if let Some(to) = &filter.date_to {
        clauses.push("date(t.created_at) <= date(?)".to_string());
        params.push(Box::new(to.clone()));
    }
    if let Some(txn_type) = &filter.txn_type {
        clauses.push("t.txn_type = ?".to_string());
        params.push(Box::new(txn_type.clone()));
    }
    if let Some(part_id) = &filter.part_id {
        clauses.push("t.part_id = ?".to_string());
        params.push(Box::new(part_id.as_str().to_string()));
    }
    if let Some(project_id) = &filter.project_id {
        clauses.push("(t.project_id = ? OR t.to_project_id = ?)".to_string());
        params.push(Box::new(project_id.as_str().to_string()));
        params.push(Box::new(project_id.as_str().to_string()));
    }
    if let Some(group_id) = &filter.group_id {
        clauses.push("t.group_id = ?".to_string());
        params.push(Box::new(group_id.as_str().to_string()));
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (where_sql, params)
}

fn row_to_history(row: &rusqlite::Row<'_>) -> Result<HistoryRow, DbError> {
    let bad = |what: &str| DbError::Corrupt(format!("bad {what} in history row"));

    let unit_raw: String = row.get(3)?;
    let quantity_unit = QuantityUnit::from_sql(&unit_raw).ok_or_else(|| bad("quantity_unit"))?;

    Ok(HistoryRow {
        id: TransactionId::from_string(row.get(0)?).map_err(|_| bad("id"))?,
        part_id: PartId::from_string(row.get(1)?).map_err(|_| bad("part_id"))?,
        display_name: row.get(2)?,
        quantity_unit,
        part_archived: row.get(4)?,
        txn_type: row.get(5)?,
        quantity: Quantity::from_milli(row.get(6)?, quantity_unit).map_err(|_| bad("quantity"))?,
        from_state: row.get(7)?,
        to_state: row.get(8)?,
        project_id: row
            .get::<_, Option<String>>(9)?
            .map(ProjectId::from_string)
            .transpose()
            .map_err(|_| bad("project_id"))?,
        to_project_id: row
            .get::<_, Option<String>>(10)?
            .map(ProjectId::from_string)
            .transpose()
            .map_err(|_| bad("to_project_id"))?,
        project_name: row.get(11)?,
        note: row.get(12)?,
        group_id: row
            .get::<_, Option<String>>(13)?
            .map(GroupId::from_string)
            .transpose()
            .map_err(|_| bad("group_id"))?,
        group_kind: row.get(14)?,
        reversed_txn_id: row
            .get::<_, Option<String>>(15)?
            .map(TransactionId::from_string)
            .transpose()
            .map_err(|_| bad("reversed_txn_id"))?,
        import_id: row.get(16)?,
        created_at: row.get(17)?,
        reversible: row.get(18)?,
    })
}
