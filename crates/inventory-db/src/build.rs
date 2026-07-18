//! Reserve-BOM and atomic build-from-BOM — Phase 4 Task 4, the core of
//! Phase 4. Every mutating entry point here is a thin wrapper that computes
//! a `Vec<LedgerOp>` from the BOM's current (ledger-derived) state and hands
//! it to the existing `apply_group` (`crates/inventory-db/src/ledger.rs`,
//! Phase 2a) as ONE atomic, all-or-nothing transaction group. No parallel
//! transaction path is introduced here.
//!
//! ## `bom_item_id` threading — NOT done, by design
//!
//! The Task 1 migration added a bare, FK-less `transactions.bom_item_id`
//! column anticipating that Task 4 would populate it. Doing so would mean
//! adding an `Option<BomItemId>` field to most `LedgerOp` variants, threading
//! it through `apply_in_tx`'s INSERT (`crates/inventory-db/src/ledger.rs`),
//! and updating every existing call site across the workspace (other
//! `inventory-db` modules, `dev_seed`, and eventually the Tauri commands) —
//! a large, invasive change to a tested, load-bearing signature for a
//! precision Task 3 doesn't actually need. `bom_items` already enforces
//! `UNIQUE(project_id, part_id)`, so Task 3's `derive_reserved_consumed`
//! (`crates/inventory-db/src/bom.rs`) sums ledger rows by `(project_id,
//! part_id)` instead of `bom_item_id` — provably equivalent for this schema,
//! since there is at most one BOM line per (project, part) pair. Every op
//! this module emits carries `project_id`, which is exactly what that
//! derivation keys on, so BOM-line attribution works correctly without
//! `bom_item_id` ever reaching the ledger. This is the smaller, documented
//! choice the Task 4 brief explicitly allows.
//!
//! ## Required vs. optional lines
//!
//! `required` is informational only here — it does not gate which ops get
//! generated. Every line contributes whatever is coverable (its reserved
//! quantity is always consumed; an available-draw is consumed only if
//! approved; a reusable part is checked out if stock allows); `missing`
//! reports what's still short regardless of the `required` flag so the UI
//! can flag required-but-unmet lines more prominently. "Build what can be
//! built, flag the rest" (see the Phase 4 plan's Task 7 self-review note),
//! rather than silently dropping an optional line's already-reserved stock.
//!
//! ## Reserved vs. available-draw: different safety postures, on purpose
//!
//! `reserve_bom` clamps every reservation to `min(needed, available)` — it
//! can never fail on insufficient stock; partial reservation is the documented
//! happy path. A build's *approved* available-draw is deliberately NOT
//! clamped to current available stock: `available_needed` is the full
//! remaining shortfall after reserved stock, and it is the `part_stock`
//! CHECK constraint (`available_milli >= 0`, enforced inside the same
//! `apply_group` transaction) that is the real gate, exactly the "second
//! line of defense" pattern `ledger.rs` already documents. This is what
//! makes the atomicity guarantee actually exercisable: an approved line that
//! turns out to be short fails that single op, and `apply_group` rolls back
//! the entire build — nothing partially applied.
//!
//! ## Reusable parts (`usage_behavior = 'usually_checked_out'`)
//!
//! Reusable equipment is never pre-reserved by `reserve_bom` (fungible
//! `available -> reserved` holding doesn't fit tools that get checked out
//! for a project's duration) — it is checked out directly from `available`
//! at build time (or ad hoc, via `associate_checkout`). Unlike a consumable
//! line's available-draw, a reusable line's checkout IS clamped to current
//! `available` (`min(total_required, available)`): checkout isn't gated
//! behind the caller's per-line approval the way a consumable draw is, so it
//! should not be able to sink an otherwise-successful build on a shortfall
//! the caller never got to review — the shortfall is simply reported via
//! `missing`.

use inventory_core::ids::{BomItemId, PartId, ProjectId};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::{Quantity, QuantityUnit};

use crate::bom::BomItemRecord;
use crate::ledger::{GroupRecord, TransactionRecord};
use crate::projects::ProjectStatus;
use crate::{Database, DbError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct BuildPlanLine {
    pub bom_item_id: BomItemId,
    pub part_id: PartId,
    pub part_display_name: String,
    pub required: bool,
    /// `true` when the part's `usage_behavior` is `usually_checked_out`:
    /// this line is checked out at build time rather than consumed.
    pub is_reusable: bool,
    /// Quantity that will be consumed from this line's current reservation
    /// (`consume_reserved`) — always applied, never gated by approval.
    pub reserved_qty: Quantity,
    /// Additional quantity, beyond `reserved_qty`, that would be drawn from
    /// free available stock (`consume_available`) IF the caller approves
    /// this line. Not clamped to current available stock — see the module
    /// doc comment for why.
    pub available_needed: Quantity,
    /// For a reusable line: quantity that will be checked out
    /// (`check_out`) rather than consumed. Zero for consumable lines.
    pub checkout_qty: Quantity,
    /// Still short of `total_required` even after `reserved_qty` +
    /// `available_needed` (consumable lines) or `checkout_qty` (reusable
    /// lines) — informational; never gates op generation.
    pub missing: Quantity,
    pub planned_ops_preview: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct BuildPlan {
    pub lines: Vec<BuildPlanLine>,
}

impl Database {
    /// Reserves `min(needed, available)` for every REQUIRED BOM line, where
    /// `needed = total_required - already_reserved_for_project -
    /// already_consumed_for_project`. Reusable-equipment lines
    /// (`usage_behavior = 'usually_checked_out'`) are skipped — see the
    /// module doc comment. Partial reservation is the documented happy path
    /// (never fails on insufficient stock); a line contributing nothing
    /// (already fully reserved, nothing available, optional, or reusable)
    /// simply emits no op. Applied as ONE `apply_group("reserve_bom", ...)`;
    /// if no line can reserve anything, that call receives an empty op
    /// vector and returns the existing `DbError::EmptyGroup` — the same
    /// clean "nothing to do" signal `apply_group` already gives any other
    /// caller, rather than a bespoke variant.
    pub fn reserve_bom(&mut self, project_id: &ProjectId) -> Result<GroupRecord, DbError> {
        let items = self.list_bom(project_id)?;
        let mut ops = Vec::new();
        for item in items {
            if !item.required {
                continue;
            }
            let needed_milli = (item.total_required.as_milli()
                - item.reserved.as_milli()
                - item.consumed.as_milli())
            .max(0);
            if needed_milli == 0 {
                continue;
            }
            let take_milli = needed_milli.min(item.available.as_milli());
            if take_milli <= 0 {
                continue;
            }
            let part = self.get_part(&item.part_id)?.ok_or(DbError::PartNotFound)?;
            if part.usage_behavior == "usually_checked_out" {
                continue;
            }
            ops.push(LedgerOp::Reserve {
                part_id: item.part_id.clone(),
                quantity: Quantity::from_milli(take_milli, part.quantity_unit)?,
                project_id: project_id.clone(),
            });
        }
        let note = format!("reserve BOM for project {}", project_id.as_str());
        self.apply_group("reserve_bom", &note, &ops)
    }

    /// Releases every part of `project_id`'s current BOM reservation
    /// (derived the same way `list_bom`'s `reserved` column is), regardless
    /// of a line's `required` flag — this is cleanup, not the same
    /// required-only filter `reserve_bom` applies going in. Applied as ONE
    /// `apply_group("release_bom", ...)`; nothing reserved yields the same
    /// `DbError::EmptyGroup` "nothing to do" signal.
    pub fn release_bom_reservations(
        &mut self,
        project_id: &ProjectId,
    ) -> Result<GroupRecord, DbError> {
        let items = self.list_bom(project_id)?;
        let mut ops = Vec::new();
        for item in items {
            if item.reserved == Quantity::ZERO {
                continue;
            }
            ops.push(LedgerOp::ReleaseReservation {
                part_id: item.part_id.clone(),
                quantity: item.reserved,
                project_id: project_id.clone(),
            });
        }
        let note = format!(
            "release BOM reservations for project {}",
            project_id.as_str()
        );
        self.apply_group("release_bom", &note, &ops)
    }

    /// Dry run: computes exactly what `build_from_bom` would do, without
    /// mutating anything. See the module doc comment for the exact
    /// per-field semantics (consumable vs. reusable lines, why
    /// `available_needed` isn't clamped to current stock).
    pub fn plan_build(&self, project_id: &ProjectId) -> Result<BuildPlan, DbError> {
        let items = self.list_bom(project_id)?;
        let mut lines = Vec::with_capacity(items.len());
        for item in items {
            let part = self.get_part(&item.part_id)?.ok_or(DbError::PartNotFound)?;
            let is_reusable = part.usage_behavior == "usually_checked_out";
            lines.push(plan_line(&item, part.quantity_unit, is_reusable)?);
        }
        Ok(BuildPlan { lines })
    }

    /// Executes `plan_build`, filtered by `approved_available_lines`, as ONE
    /// `apply_group("build_from_bom", ...)`: `consume_reserved` for every
    /// line's `reserved_qty` (unconditional), `consume_available` for a
    /// line's `available_needed` ONLY if its `bom_item_id` is in
    /// `approved_available_lines`, and `check_out` for a reusable line's
    /// `checkout_qty` (unconditional, clamped-safe). All-or-nothing:
    /// `apply_group` rolls back the whole group if any single op fails
    /// (e.g. an approved line whose available-draw exceeds real stock —
    /// this is the mechanism the atomicity tests exercise). On success, a
    /// `planned` project auto-activates; a failed build never touches
    /// project status. An empty op vector (nothing to build) yields the
    /// same `DbError::EmptyGroup` "nothing to do" signal as `reserve_bom`.
    pub fn build_from_bom(
        &mut self,
        project_id: &ProjectId,
        approved_available_lines: &[BomItemId],
    ) -> Result<GroupRecord, DbError> {
        let plan = self.plan_build(project_id)?;
        let mut ops = Vec::new();
        for line in &plan.lines {
            if line.reserved_qty > Quantity::ZERO {
                ops.push(LedgerOp::ConsumeReserved {
                    part_id: line.part_id.clone(),
                    quantity: line.reserved_qty,
                    project_id: Some(project_id.clone()),
                    note: "build from BOM".into(),
                });
            }
            if line.is_reusable {
                if line.checkout_qty > Quantity::ZERO {
                    ops.push(LedgerOp::CheckOut {
                        part_id: line.part_id.clone(),
                        quantity: line.checkout_qty,
                        project_id: project_id.clone(),
                    });
                }
            } else if line.available_needed > Quantity::ZERO
                && approved_available_lines.contains(&line.bom_item_id)
            {
                ops.push(LedgerOp::ConsumeAvailable {
                    part_id: line.part_id.clone(),
                    quantity: line.available_needed,
                    project_id: Some(project_id.clone()),
                    note: "build from BOM (drawn from available)".into(),
                });
            }
        }

        let note = format!("build from BOM for project {}", project_id.as_str());
        let group = self.apply_group("build_from_bom", &note, &ops)?;

        if let Some(project) = self.get_project(project_id)? {
            if project.status == ProjectStatus::Planned {
                self.set_project_status(project_id, ProjectStatus::Active)?;
            }
        }

        Ok(group)
    }

    /// Ad-hoc reusable-item checkout to a project (not tied to a BOM line):
    /// a single `CheckOut` op tagged with `project_id`, applied via the
    /// existing single-op `apply` path.
    pub fn associate_checkout(
        &mut self,
        project_id: &ProjectId,
        part_id: &PartId,
        quantity: Quantity,
    ) -> Result<TransactionRecord, DbError> {
        self.apply(&LedgerOp::CheckOut {
            part_id: part_id.clone(),
            quantity,
            project_id: project_id.clone(),
        })
    }
}

fn plan_line(
    item: &BomItemRecord,
    unit: QuantityUnit,
    is_reusable: bool,
) -> Result<BuildPlanLine, DbError> {
    let total_required = item.total_required.as_milli();
    let available = item.available.as_milli();
    let reserved = item.reserved.as_milli();

    let (reserved_qty_milli, available_needed_milli, checkout_qty_milli, missing_milli);
    if is_reusable {
        // Clamped to current available: checkout isn't behind per-line
        // caller approval, so it must not be able to fail the whole group.
        let take = total_required.min(available).max(0);
        reserved_qty_milli = 0;
        checkout_qty_milli = take;
        available_needed_milli = 0;
        missing_milli = (total_required - take).max(0);
    } else {
        let r = reserved.min(total_required).max(0);
        let remaining = (total_required - r).max(0);
        reserved_qty_milli = r;
        // Deliberately NOT clamped to `available` -- see module doc comment.
        available_needed_milli = remaining;
        checkout_qty_milli = 0;
        missing_milli = (remaining - available).max(0);
    }

    let reserved_qty = Quantity::from_milli(reserved_qty_milli, unit)?;
    let available_needed = Quantity::from_milli(available_needed_milli, unit)?;
    let checkout_qty = Quantity::from_milli(checkout_qty_milli, unit)?;
    let missing = Quantity::from_milli(missing_milli, unit)?;

    let planned_ops_preview = preview_for(
        reserved_qty,
        available_needed,
        checkout_qty,
        missing,
        is_reusable,
    );

    Ok(BuildPlanLine {
        bom_item_id: item.id.clone(),
        part_id: item.part_id.clone(),
        part_display_name: item.part_display_name.clone(),
        required: item.required,
        is_reusable,
        reserved_qty,
        available_needed,
        checkout_qty,
        missing,
        planned_ops_preview,
    })
}

fn fmt_qty(q: Quantity) -> String {
    let milli = q.as_milli();
    if milli % Quantity::SCALE == 0 {
        (milli / Quantity::SCALE).to_string()
    } else {
        format!("{:.3}", milli as f64 / Quantity::SCALE as f64)
    }
}

fn preview_for(
    reserved_qty: Quantity,
    available_needed: Quantity,
    checkout_qty: Quantity,
    missing: Quantity,
    is_reusable: bool,
) -> String {
    let mut parts = Vec::new();
    if reserved_qty > Quantity::ZERO {
        parts.push(format!("consume {} reserved", fmt_qty(reserved_qty)));
    }
    if is_reusable {
        if checkout_qty > Quantity::ZERO {
            parts.push(format!("check out {}", fmt_qty(checkout_qty)));
        }
    } else if available_needed > Quantity::ZERO {
        parts.push(format!(
            "consume {} available (requires approval)",
            fmt_qty(available_needed)
        ));
    }
    if missing > Quantity::ZERO {
        parts.push(format!("{} still missing", fmt_qty(missing)));
    }
    if parts.is_empty() {
        parts.push("nothing to do".to_string());
    }
    parts.join("; ")
}
