//! Transactional application of ledger operations. Every stock change inserts
//! a ledger row and updates `part_stock` in the same SQLite transaction; the
//! CHECK constraints are the second line of defense against negative stock.

use inventory_core::ids::{GroupId, PartId, ProjectId, TransactionId};
use inventory_core::ledger::{delta_for, LedgerOp};
use inventory_core::quantity::Quantity;
use rusqlite::Transaction;

use crate::{Database, DbError};

#[derive(Debug, Clone)]
pub struct GroupRecord {
    pub id: GroupId,
    pub kind: String,
    pub note: String,
    pub reversed_group_id: Option<GroupId>,
    pub created_at: String,
    pub transactions: Vec<TransactionRecord>,
}

#[derive(Debug, Clone)]
pub struct TransactionRecord {
    pub id: TransactionId,
    pub part_id: PartId,
    pub group_id: Option<GroupId>,
    pub txn_type: String,
    pub quantity: Quantity,
    pub from_state: Option<String>,
    pub to_state: Option<String>,
    pub project_id: Option<ProjectId>,
    pub to_project_id: Option<ProjectId>,
    pub note: String,
    pub reversed_txn_id: Option<TransactionId>,
    pub created_at: String,
}

impl Database {
    pub fn apply(&mut self, op: &LedgerOp) -> Result<TransactionRecord, DbError> {
        let tx = self.conn_mut().transaction()?;
        let record = apply_in_tx(&tx, op, None)?;
        tx.commit()?;
        Ok(record)
    }

    pub fn list_transactions(&self, part_id: &PartId) -> Result<Vec<TransactionRecord>, DbError> {
        // rowid preserves insertion order; created_at is second-granular and ULIDs don't sort by creation time
        let mut stmt = self.raw_conn().prepare(
            "SELECT id, part_id, group_id, txn_type, quantity_milli, from_state, to_state,
                    project_id, to_project_id, note, reversed_txn_id, created_at
             FROM transactions WHERE part_id = ?1
             ORDER BY rowid DESC",
        )?;
        let mut rows = stmt.query([part_id.as_str()])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_txn(row)?);
        }
        Ok(out)
    }

    /// Minimal helper until Phase 4 builds real project management.
    pub fn create_project(&mut self, name: &str) -> Result<ProjectId, DbError> {
        let id = ProjectId::new();
        self.raw_conn().execute(
            "INSERT INTO projects (id, name) VALUES (?1, ?2)",
            rusqlite::params![id.as_str(), name],
        )?;
        Ok(id)
    }

    pub fn apply_group(&mut self, kind: &str, note: &str, ops: &[LedgerOp]) -> Result<GroupRecord, DbError> {
        if ops.is_empty() {
            return Err(DbError::EmptyGroup);
        }
        let tx = self.conn_mut().transaction()?;
        let group_id = GroupId::new();
        tx.execute(
            "INSERT INTO transaction_groups (id, kind, note) VALUES (?1, ?2, ?3)",
            rusqlite::params![group_id.as_str(), kind, note],
        )?;
        let mut transactions = Vec::with_capacity(ops.len());
        for op in ops {
            transactions.push(apply_in_tx(&tx, op, Some(&group_id))?);
        }
        let created_at: String = tx.query_row(
            "SELECT created_at FROM transaction_groups WHERE id = ?1",
            [group_id.as_str()],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(GroupRecord {
            id: group_id,
            kind: kind.to_string(),
            note: note.to_string(),
            reversed_group_id: None,
            created_at,
            transactions,
        })
    }

    pub fn get_group(&self, id: &GroupId) -> Result<Option<GroupRecord>, DbError> {
        let header = self
            .raw_conn()
            .query_row(
                "SELECT kind, note, reversed_group_id, created_at FROM transaction_groups WHERE id = ?1",
                [id.as_str()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            );
        let (kind, note, reversed_raw, created_at) = match header {
            Ok(h) => h,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(DbError::Sqlite(e)),
        };
        // rowid preserves insertion order; created_at is second-granular and ULIDs don't sort by creation time
        let mut stmt = self.raw_conn().prepare(
            "SELECT id, part_id, group_id, txn_type, quantity_milli, from_state, to_state,
                    project_id, to_project_id, note, reversed_txn_id, created_at
             FROM transactions WHERE group_id = ?1 ORDER BY rowid",
        )?;
        let mut rows = stmt.query([id.as_str()])?;
        let mut transactions = Vec::new();
        while let Some(row) = rows.next()? {
            transactions.push(row_to_txn(row)?);
        }
        Ok(Some(GroupRecord {
            id: id.clone(),
            kind,
            note,
            reversed_group_id: reversed_raw
                .map(GroupId::from_string)
                .transpose()
                .map_err(|_| DbError::Corrupt("bad reversed_group_id".into()))?,
            created_at,
            transactions,
        }))
    }
}

/// Shared by single ops (Task 4), groups (Task 6), and reversals (Task 7).
pub(crate) fn apply_in_tx(
    tx: &Transaction<'_>,
    op: &LedgerOp,
    group_id: Option<&GroupId>,
) -> Result<TransactionRecord, DbError> {
    op.validate()?;

    let archived: bool = tx
        .query_row("SELECT archived FROM parts WHERE id = ?1", [op.part_id().as_str()], |r| r.get(0))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::PartNotFound,
            other => DbError::Sqlite(other),
        })?;
    if archived && !op_allowed_on_archived(op) {
        return Err(DbError::PartArchived);
    }

    let delta = delta_for(op);
    update_stock(tx, op.part_id(), &delta)?;

    let id = TransactionId::new();
    let (from_state, to_state) = op.state_movement();
    let (project_id, to_project_id) = op_projects(op);
    tx.execute(
        "INSERT INTO transactions (id, part_id, group_id, txn_type, quantity_milli,
                                   from_state, to_state, project_id, to_project_id, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            id.as_str(),
            op.part_id().as_str(),
            group_id.map(|g| g.as_str()),
            op.txn_type_sql(),
            op.quantity().as_milli(),
            from_state.map(|s| s.as_sql()),
            to_state.map(|s| s.as_sql()),
            project_id.as_ref().map(|p| p.as_str()),
            to_project_id.as_ref().map(|p| p.as_str()),
            op.note(),
        ],
    )?;

    Ok(TransactionRecord {
        id: id.clone(),
        part_id: op.part_id().clone(),
        group_id: group_id.cloned(),
        txn_type: op.txn_type_sql().to_string(),
        quantity: op.quantity(),
        from_state: from_state.map(|s| s.as_sql().to_string()),
        to_state: to_state.map(|s| s.as_sql().to_string()),
        project_id,
        to_project_id,
        note: op.note().to_string(),
        reversed_txn_id: None,
        created_at: tx.query_row(
            "SELECT created_at FROM transactions WHERE id = ?1",
            [id.as_str()],
            |r| r.get(0),
        )?,
    })
}

pub(crate) fn update_stock(
    tx: &Transaction<'_>,
    part_id: &PartId,
    delta: &inventory_core::ledger::StockDelta,
) -> Result<(), DbError> {
    let result = tx.execute(
        "UPDATE part_stock SET
            available_milli = available_milli + ?2,
            reserved_milli = reserved_milli + ?3,
            checked_out_milli = checked_out_milli + ?4,
            lifetime_received_milli = lifetime_received_milli + ?5,
            lifetime_consumed_milli = lifetime_consumed_milli + ?6
         WHERE part_id = ?1",
        rusqlite::params![
            part_id.as_str(),
            delta.available,
            delta.reserved,
            delta.checked_out,
            delta.lifetime_received,
            delta.lifetime_consumed,
        ],
    );
    match result {
        Ok(0) => Err(DbError::PartNotFound),
        Ok(_) => Ok(()),
        Err(e) if is_check_violation(&e) => Err(DbError::InsufficientStock(format!(
            "operation would drive stock negative for part {}",
            part_id.as_str()
        ))),
        Err(e) => Err(DbError::Sqlite(e)),
    }
}

fn is_check_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

fn op_allowed_on_archived(op: &LedgerOp) -> bool {
    matches!(op, LedgerOp::ReleaseReservation { .. } | LedgerOp::Return { .. })
}

fn op_projects(op: &LedgerOp) -> (Option<ProjectId>, Option<ProjectId>) {
    match op {
        LedgerOp::Reserve { project_id, .. }
        | LedgerOp::ReleaseReservation { project_id, .. }
        | LedgerOp::CheckOut { project_id, .. }
        | LedgerOp::Return { project_id, .. } => (Some(project_id.clone()), None),
        LedgerOp::ConsumeAvailable { project_id, .. }
        | LedgerOp::ConsumeReserved { project_id, .. }
        | LedgerOp::ConsumeCheckedOut { project_id, .. } => (project_id.clone(), None),
        LedgerOp::TransferReservation { from_project, to_project, .. } => {
            (Some(from_project.clone()), Some(to_project.clone()))
        }
        _ => (None, None),
    }
}

pub(crate) fn row_to_txn(row: &rusqlite::Row<'_>) -> Result<TransactionRecord, DbError> {
    let bad = |what: &str| DbError::Corrupt(format!("bad {what} in transactions row"));
    let opt_id = |v: Option<String>, what: &str| -> Result<Option<TransactionId>, DbError> {
        v.map(TransactionId::from_string).transpose().map_err(|_| bad(what))
    };
    // NOTE: quantity uses Meter here only to bypass the discrete-fraction check
    // when reading; stored values were validated on write against the part's
    // real unit. 2b revisits by joining the part's unit into ledger reads.
    Ok(TransactionRecord {
        id: TransactionId::from_string(row.get(0)?).map_err(|_| bad("id"))?,
        part_id: PartId::from_string(row.get(1)?).map_err(|_| bad("part_id"))?,
        group_id: row
            .get::<_, Option<String>>(2)?
            .map(GroupId::from_string)
            .transpose()
            .map_err(|_| bad("group_id"))?,
        txn_type: row.get(3)?,
        quantity: Quantity::from_milli(row.get(4)?, inventory_core::quantity::QuantityUnit::Meter)
            .map_err(|_| bad("quantity"))?,
        from_state: row.get(5)?,
        to_state: row.get(6)?,
        project_id: row
            .get::<_, Option<String>>(7)?
            .map(ProjectId::from_string)
            .transpose()
            .map_err(|_| bad("project_id"))?,
        to_project_id: row
            .get::<_, Option<String>>(8)?
            .map(ProjectId::from_string)
            .transpose()
            .map_err(|_| bad("to_project_id"))?,
        note: row.get(9)?,
        reversed_txn_id: opt_id(row.get(10)?, "reversed_txn_id")?,
        created_at: row.get(11)?,
    })
}
