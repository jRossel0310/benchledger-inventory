//! BOM (bill of materials) repository: one row per part required to build a
//! project. `reserved` and `consumed` are never stored — they are derived
//! from the ledger on every read (see `derive_reserved_consumed` for the
//! exact formula), the same "third line of defense" philosophy `validate.rs`
//! uses to reconcile `part_stock` from the ledger. This avoids a second
//! source of truth for per-line quantities that could drift from the
//! append-only ledger (Global Constraints: "derived-not-stored").

use inventory_core::ids::{BomItemId, PartId, ProjectId};
use inventory_core::quantity::{Quantity, QuantityError, QuantityUnit};

use crate::{Database, DbError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct BomItemDraft {
    pub part_id: PartId,
    pub quantity_per_build: Quantity,
    pub reference_designators: String,
    pub required: bool,
    pub notes: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct BomItemRecord {
    pub id: BomItemId,
    pub project_id: ProjectId,
    pub part_id: PartId,
    pub part_display_name: String,
    pub quantity_per_build: Quantity,
    /// `quantity_per_build * project.build_quantity`, recomputed on every
    /// read so it always reflects the project's current build_quantity.
    pub total_required: Quantity,
    pub reference_designators: String,
    pub required: bool,
    pub notes: String,
    pub substitutes: Vec<PartId>,
    /// Derived from the ledger: see `derive_reserved_consumed`.
    pub reserved: Quantity,
    /// Derived from the ledger: see `derive_reserved_consumed`.
    pub consumed: Quantity,
    /// The part's free (fungible, not-yet-reserved) stock — `part_stock.available_milli`.
    pub available: Quantity,
    /// What still needs to be obtained to complete the build: total required
    /// minus what's already consumed, reserved, or freely available (clamped
    /// at 0), per the spec's BOM "Missing" column. Note `available` is
    /// global/fungible part stock while `consumed`/`reserved` are
    /// project-scoped, so this slightly overstates "missing" when another
    /// project is also drawing on the same free stock — a pre-existing
    /// imprecision, not something this formula fixes.
    pub missing: Quantity,
}

const BOM_ITEM_JOIN_COLUMNS: &str =
    "bi.id, bi.part_id, p.display_name, bi.quantity_per_build_milli,
    bi.reference_designators, bi.required, bi.notes, p.quantity_unit, ps.available_milli";
const BOM_ITEM_JOIN_FROM: &str = "FROM bom_items bi
    JOIN parts p ON p.id = bi.part_id
    JOIN part_stock ps ON ps.part_id = bi.part_id";

type BomItemJoinRow = (
    String,
    String,
    String,
    i64,
    String,
    bool,
    String,
    String,
    i64,
);

impl Database {
    /// Adds a BOM line. `project_id` and `draft.part_id` must both exist;
    /// `(project_id, part_id)` must be unique (one BOM line per part per
    /// project — `bom_items.UNIQUE(project_id, part_id)`).
    pub fn add_bom_item(
        &mut self,
        project_id: &ProjectId,
        draft: &BomItemDraft,
    ) -> Result<BomItemRecord, DbError> {
        if self.get_project(project_id)?.is_none() {
            return Err(DbError::ProjectNotFound);
        }
        if self.get_part(&draft.part_id)?.is_none() {
            return Err(DbError::PartNotFound);
        }
        let id = BomItemId::new();
        self.raw_conn()
            .execute(
                "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli,
                                        reference_designators, required, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    id.as_str(),
                    project_id.as_str(),
                    draft.part_id.as_str(),
                    draft.quantity_per_build.as_milli(),
                    draft.reference_designators,
                    draft.required,
                    draft.notes,
                ],
            )
            .map_err(map_unique_to_bom_item_exists)?;
        self.get_bom_item(&id)?.ok_or(DbError::BomItemNotFound)
    }

    /// Persists `quantity_per_build`/`reference_designators`/`required`/`notes`.
    /// `draft.part_id` is ignored — a BOM line's part cannot be changed after
    /// creation (remove the line and add a new one instead).
    pub fn update_bom_item(
        &mut self,
        id: &BomItemId,
        draft: &BomItemDraft,
    ) -> Result<BomItemRecord, DbError> {
        let n = self.raw_conn().execute(
            "UPDATE bom_items SET quantity_per_build_milli = ?2, reference_designators = ?3,
                                  required = ?4, notes = ?5
             WHERE id = ?1",
            rusqlite::params![
                id.as_str(),
                draft.quantity_per_build.as_milli(),
                draft.reference_designators,
                draft.required,
                draft.notes,
            ],
        )?;
        if n == 0 {
            return Err(DbError::BomItemNotFound);
        }
        self.get_bom_item(id)?.ok_or(DbError::BomItemNotFound)
    }

    /// Deletes a BOM line. `bom_substitutes` rows for it cascade
    /// (`ON DELETE CASCADE`); ledger rows are untouched (the ledger is
    /// append-only and outlives the BOM line that once referenced it).
    pub fn remove_bom_item(&mut self, id: &BomItemId) -> Result<(), DbError> {
        let n = self
            .raw_conn()
            .execute("DELETE FROM bom_items WHERE id = ?1", [id.as_str()])?;
        if n == 0 {
            return Err(DbError::BomItemNotFound);
        }
        Ok(())
    }

    /// Replaces the full substitute-parts list for a BOM line (delete then
    /// reinsert, in one transaction) rather than diffing.
    pub fn set_bom_substitutes(
        &mut self,
        bom_item_id: &BomItemId,
        part_ids: &[PartId],
    ) -> Result<(), DbError> {
        let exists: i64 = self.raw_conn().query_row(
            "SELECT COUNT(*) FROM bom_items WHERE id = ?1",
            [bom_item_id.as_str()],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(DbError::BomItemNotFound);
        }
        for part_id in part_ids {
            if self.get_part(part_id)?.is_none() {
                return Err(DbError::PartNotFound);
            }
        }
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "DELETE FROM bom_substitutes WHERE bom_item_id = ?1",
            [bom_item_id.as_str()],
        )?;
        for part_id in part_ids {
            tx.execute(
                "INSERT INTO bom_substitutes (bom_item_id, part_id) VALUES (?1, ?2)",
                rusqlite::params![bom_item_id.as_str(), part_id.as_str()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_bom_item(&self, id: &BomItemId) -> Result<Option<BomItemRecord>, DbError> {
        let result = self.raw_conn().query_row(
            &format!(
                "SELECT bi.project_id, {BOM_ITEM_JOIN_COLUMNS} {BOM_ITEM_JOIN_FROM} WHERE bi.id = ?1"
            ),
            [id.as_str()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (
                        r.get::<_, String>(1)?, // bi.id
                        r.get::<_, String>(2)?, // bi.part_id
                        r.get::<_, String>(3)?, // p.display_name
                        r.get::<_, i64>(4)?,    // bi.quantity_per_build_milli
                        r.get::<_, String>(5)?, // bi.reference_designators
                        r.get::<_, bool>(6)?,   // bi.required
                        r.get::<_, String>(7)?, // bi.notes
                        r.get::<_, String>(8)?, // p.quantity_unit
                        r.get::<_, i64>(9)?,    // ps.available_milli
                    ),
                ))
            },
        );
        let (project_id_raw, row): (String, BomItemJoinRow) = match result {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(DbError::Sqlite(e)),
        };
        let project_id = ProjectId::from_string(project_id_raw)
            .map_err(|_| DbError::Corrupt("bad project id in bom_items".into()))?;
        // bom_items.project_id REFERENCES projects(id) ON DELETE CASCADE, so a
        // row here always has a live parent project; a missing one is corruption.
        let project = self
            .get_project(&project_id)?
            .ok_or_else(|| DbError::Corrupt("bom_item references a missing project".into()))?;
        Ok(Some(self.row_to_bom_item(
            project_id,
            row,
            project.build_quantity,
        )?))
    }

    /// The core BOM query: every line for `project_id`, ordered by part
    /// display name, with `total_required`/`substitutes`/`reserved`/
    /// `consumed`/`available`/`missing` all computed fresh.
    pub fn list_bom(&self, project_id: &ProjectId) -> Result<Vec<BomItemRecord>, DbError> {
        let project = self
            .get_project(project_id)?
            .ok_or(DbError::ProjectNotFound)?;

        let rows: Vec<BomItemJoinRow> = {
            let mut stmt = self.raw_conn().prepare(&format!(
                "SELECT {BOM_ITEM_JOIN_COLUMNS} {BOM_ITEM_JOIN_FROM}
                 WHERE bi.project_id = ?1
                 ORDER BY p.display_name"
            ))?;
            let mapped = stmt.query_map([project_id.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, bool>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            })?;
            mapped.collect::<Result<_, _>>()?
        };

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_bom_item(project_id.clone(), row, project.build_quantity)?);
        }
        Ok(out)
    }

    /// Bulk-add: inserts every draft for `project_id` in one transaction,
    /// silently skipping rows whose `part_id` doesn't exist and rows that
    /// collide with an existing `(project_id, part_id)` BOM line (the caller
    /// gets back only the lines that were actually added — this is the
    /// "simple" structured import the Task 3 brief allows as a stub; a
    /// richer import matching parts by MPN/SKU with a per-row skip report
    /// is Phase 5 scope).
    pub fn import_bom(
        &mut self,
        project_id: &ProjectId,
        rows: Vec<BomItemDraft>,
    ) -> Result<Vec<BomItemRecord>, DbError> {
        if self.get_project(project_id)?.is_none() {
            return Err(DbError::ProjectNotFound);
        }
        let mut added_ids = Vec::new();
        {
            let tx = self.conn_mut().transaction()?;
            for draft in &rows {
                let part_exists: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM parts WHERE id = ?1",
                    [draft.part_id.as_str()],
                    |r| r.get(0),
                )?;
                if part_exists == 0 {
                    continue;
                }
                let id = BomItemId::new();
                let result = tx.execute(
                    "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli,
                                            reference_designators, required, notes)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        id.as_str(),
                        project_id.as_str(),
                        draft.part_id.as_str(),
                        draft.quantity_per_build.as_milli(),
                        draft.reference_designators,
                        draft.required,
                        draft.notes,
                    ],
                );
                match result {
                    Ok(_) => added_ids.push(id),
                    Err(e) if is_unique_violation(&e) => continue,
                    Err(e) => return Err(DbError::Sqlite(e)),
                }
            }
            tx.commit()?;
        }
        let mut out = Vec::with_capacity(added_ids.len());
        for id in added_ids {
            if let Some(rec) = self.get_bom_item(&id)? {
                out.push(rec);
            }
        }
        Ok(out)
    }

    /// Assembles a full `BomItemRecord` from a joined `bom_items` row plus
    /// the project's `build_quantity`, filling in the fields that must be
    /// computed rather than read: `total_required`, `substitutes`,
    /// `reserved`/`consumed` (ledger-derived), and `missing`.
    fn row_to_bom_item(
        &self,
        project_id: ProjectId,
        row: BomItemJoinRow,
        build_quantity: i64,
    ) -> Result<BomItemRecord, DbError> {
        let (
            id_raw,
            part_id_raw,
            part_display_name,
            qty_milli,
            reference_designators,
            required,
            notes,
            unit_raw,
            available_milli,
        ) = row;
        let id = BomItemId::from_string(id_raw)
            .map_err(|_| DbError::Corrupt("bad bom_item id".into()))?;
        let part_id = PartId::from_string(part_id_raw)
            .map_err(|_| DbError::Corrupt("bad part id in bom_items".into()))?;
        let unit = QuantityUnit::from_sql(&unit_raw)
            .ok_or_else(|| DbError::Corrupt(format!("unknown quantity_unit '{unit_raw}'")))?;

        let quantity_per_build = Quantity::from_milli(qty_milli, unit)?;
        let total_required_milli = qty_milli
            .checked_mul(build_quantity)
            .ok_or(DbError::Domain(QuantityError::Overflow))?;
        let total_required = Quantity::from_milli(total_required_milli, unit)?;
        let available = Quantity::from_milli(available_milli, unit)?;

        let substitutes = self.list_substitutes(&id)?;
        let (reserved, consumed) = self.derive_reserved_consumed(&project_id, &part_id, unit)?;

        let missing_milli =
            (total_required_milli - consumed.as_milli() - reserved.as_milli() - available_milli)
                .max(0);
        let missing = Quantity::from_milli(missing_milli, unit)?;

        Ok(BomItemRecord {
            id,
            project_id,
            part_id,
            part_display_name,
            quantity_per_build,
            total_required,
            reference_designators,
            required,
            notes,
            substitutes,
            reserved,
            consumed,
            available,
            missing,
        })
    }

    fn list_substitutes(&self, bom_item_id: &BomItemId) -> Result<Vec<PartId>, DbError> {
        let mut stmt = self.raw_conn().prepare(
            "SELECT part_id FROM bom_substitutes WHERE bom_item_id = ?1 ORDER BY part_id",
        )?;
        let mut out = Vec::new();
        let mut rows = stmt.query([bom_item_id.as_str()])?;
        while let Some(row) = rows.next()? {
            let raw: String = row.get(0)?;
            out.push(PartId::from_string(raw).map_err(|_| DbError::Corrupt("bad part id".into()))?);
        }
        Ok(out)
    }

    /// Derives `(reserved, consumed)` for `part_id` attributed to
    /// `project_id`, straight from the ledger — never stored.
    ///
    /// `bom_items` does not (yet) get `bom_item_id` threaded onto the ledger
    /// rows it produces: `apply_in_tx` (crates/inventory-db/src/ledger.rs)
    /// only ever writes `project_id`/`to_project_id`, and threading
    /// `bom_item_id` through `LedgerOp`/`apply_group` is Phase 4 Task 4's
    /// job. So this derives from `(part_id, project_id)` instead of
    /// `bom_item_id`. Because `bom_items` enforces
    /// `UNIQUE(project_id, part_id)`, there is at most one BOM line per
    /// (project, part) pair, so summing by `(project_id, part_id)` is
    /// exactly equivalent to summing by `bom_item_id` for this schema —
    /// no information is lost by keying on the pair instead.
    ///
    /// `reserved` mirrors the same accounting `delta_for` (inventory-core)
    /// uses for `part_stock.reserved_milli`, filtered to rows carrying this
    /// project's id: `Reserve` adds, `ReleaseReservation` AND
    /// `ConsumeReserved` both subtract (consuming a reservation ends it,
    /// exactly like it decrements `part_stock.reserved_milli`) — so
    /// `reserved` always reads "currently reserved and not yet released or
    /// consumed" rather than a cumulative reserve count that would overstate
    /// the outstanding reservation once some of it has been built.
    ///
    /// `consumed` sums `consume_reserved` + `consume_available` +
    /// `consume_checked_out` quantity for this project+part, per the spec's
    /// BOM "Consumed" column (`consume_available`/`consume_checked_out`
    /// carry an `Option<ProjectId>` in `LedgerOp` — when a caller omits the
    /// project on those ops, that consumption simply isn't attributable to
    /// any project and won't appear here, which is the documented limit of
    /// project-attributed consumption).
    ///
    /// `reverse` rows (from `reverse_transaction`/`reverse_group`) are
    /// re-attributed to their original operation's effect, inverted — the
    /// same technique `validate.rs`'s ledger replay uses — so a reversed
    /// reserve/release/consume nets out correctly instead of silently
    /// double-counting.
    ///
    /// Known gap: `TransferReservation` (moving a reservation between two
    /// projects) is not reflected here. Task 4's `reserve_bom`/
    /// `build_from_bom` never emit transfers for BOM lines, so this is out
    /// of scope for now; a future project-to-project BOM transfer feature
    /// would need to extend this query.
    ///
    /// Known gap: a `ConsumeReserved` that consumes a reservation under a
    /// different `project_id` (or `None`) than the reservation was made
    /// under would leave the original project's derived `reserved`
    /// overstated (the consumption never subtracts from it). This relies on
    /// callers — Task 4's build-from-BOM — always passing the same
    /// `project_id` that made the reservation.
    fn derive_reserved_consumed(
        &self,
        project_id: &ProjectId,
        part_id: &PartId,
        unit: QuantityUnit,
    ) -> Result<(Quantity, Quantity), DbError> {
        let (reserved_milli, consumed_milli): (i64, i64) = self.raw_conn().query_row(
            "SELECT
                COALESCE(SUM(CASE
                    WHEN t.txn_type = 'reserve' THEN t.quantity_milli
                    WHEN t.txn_type IN ('release_reservation', 'consume_reserved') THEN -t.quantity_milli
                    WHEN t.txn_type = 'reverse' AND o.txn_type = 'reserve' THEN -t.quantity_milli
                    WHEN t.txn_type = 'reverse' AND o.txn_type IN ('release_reservation', 'consume_reserved') THEN t.quantity_milli
                    ELSE 0
                END), 0),
                COALESCE(SUM(CASE
                    WHEN t.txn_type IN ('consume_reserved', 'consume_available', 'consume_checked_out')
                        THEN t.quantity_milli
                    WHEN t.txn_type = 'reverse'
                        AND o.txn_type IN ('consume_reserved', 'consume_available', 'consume_checked_out')
                        THEN -t.quantity_milli
                    ELSE 0
                END), 0)
             FROM transactions t
             LEFT JOIN transactions o ON o.id = t.reversed_txn_id
             WHERE t.part_id = ?1 AND t.project_id = ?2",
            rusqlite::params![part_id.as_str(), project_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((
            Quantity::from_milli(reserved_milli, unit)?,
            Quantity::from_milli(consumed_milli, unit)?,
        ))
    }
}

/// `bom_items.UNIQUE(project_id, part_id)` (extended code 2067); translate a
/// violation into the typed error. Mirrors `categories::map_unique_to_category_name_taken`.
fn map_unique_to_bom_item_exists(e: rusqlite::Error) -> DbError {
    if is_unique_violation(&e) {
        return DbError::BomItemExists;
    }
    DbError::Sqlite(e)
}

fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _) if err.extended_code == 2067
    )
}
