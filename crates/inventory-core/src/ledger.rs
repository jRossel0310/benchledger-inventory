//! Pure ledger domain model: operation definitions and their exact effect on
//! stock aggregates. No SQL here — `inventory-db` applies these transactionally.

use crate::ids::{PartId, ProjectId};
use crate::quantity::{Quantity, QuantityError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockState {
    Available,
    Reserved,
    CheckedOut,
}

impl StockState {
    pub fn as_sql(&self) -> &'static str {
        match self {
            StockState::Available => "available",
            StockState::Reserved => "reserved",
            StockState::CheckedOut => "checked_out",
        }
    }
}

impl std::str::FromStr for StockState {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "available" => Ok(StockState::Available),
            "reserved" => Ok(StockState::Reserved),
            "checked_out" => Ok(StockState::CheckedOut),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    #[error("adjustments require a non-empty reason note")]
    EmptyAdjustmentNote,
    #[error("cannot transfer a reservation to the same project")]
    TransferSameProject,
    #[error(transparent)]
    Quantity(#[from] QuantityError),
}

/// One requested stock movement. Quantities are always positive; direction is
/// encoded by the variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerOp {
    Receive { part_id: PartId, quantity: Quantity, note: String },
    Reserve { part_id: PartId, quantity: Quantity, project_id: ProjectId },
    ReleaseReservation { part_id: PartId, quantity: Quantity, project_id: ProjectId },
    CheckOut { part_id: PartId, quantity: Quantity, project_id: ProjectId },
    Return { part_id: PartId, quantity: Quantity, project_id: ProjectId },
    ConsumeAvailable { part_id: PartId, quantity: Quantity, project_id: Option<ProjectId>, note: String },
    ConsumeReserved { part_id: PartId, quantity: Quantity, project_id: Option<ProjectId>, note: String },
    ConsumeCheckedOut { part_id: PartId, quantity: Quantity, project_id: Option<ProjectId>, note: String },
    AdjustUp { part_id: PartId, quantity: Quantity, note: String },
    AdjustDown { part_id: PartId, quantity: Quantity, note: String },
    TransferReservation { part_id: PartId, quantity: Quantity, from_project: ProjectId, to_project: ProjectId },
}

impl LedgerOp {
    pub fn part_id(&self) -> &PartId {
        match self {
            LedgerOp::Receive { part_id, .. }
            | LedgerOp::Reserve { part_id, .. }
            | LedgerOp::ReleaseReservation { part_id, .. }
            | LedgerOp::CheckOut { part_id, .. }
            | LedgerOp::Return { part_id, .. }
            | LedgerOp::ConsumeAvailable { part_id, .. }
            | LedgerOp::ConsumeReserved { part_id, .. }
            | LedgerOp::ConsumeCheckedOut { part_id, .. }
            | LedgerOp::AdjustUp { part_id, .. }
            | LedgerOp::AdjustDown { part_id, .. }
            | LedgerOp::TransferReservation { part_id, .. } => part_id,
        }
    }

    pub fn quantity(&self) -> Quantity {
        match self {
            LedgerOp::Receive { quantity, .. }
            | LedgerOp::Reserve { quantity, .. }
            | LedgerOp::ReleaseReservation { quantity, .. }
            | LedgerOp::CheckOut { quantity, .. }
            | LedgerOp::Return { quantity, .. }
            | LedgerOp::ConsumeAvailable { quantity, .. }
            | LedgerOp::ConsumeReserved { quantity, .. }
            | LedgerOp::ConsumeCheckedOut { quantity, .. }
            | LedgerOp::AdjustUp { quantity, .. }
            | LedgerOp::AdjustDown { quantity, .. }
            | LedgerOp::TransferReservation { quantity, .. } => *quantity,
        }
    }

    pub fn note(&self) -> &str {
        match self {
            LedgerOp::Receive { note, .. }
            | LedgerOp::ConsumeAvailable { note, .. }
            | LedgerOp::ConsumeReserved { note, .. }
            | LedgerOp::ConsumeCheckedOut { note, .. }
            | LedgerOp::AdjustUp { note, .. }
            | LedgerOp::AdjustDown { note, .. } => note,
            _ => "",
        }
    }

    pub fn txn_type_sql(&self) -> &'static str {
        match self {
            LedgerOp::Receive { .. } => "receive",
            LedgerOp::Reserve { .. } => "reserve",
            LedgerOp::ReleaseReservation { .. } => "release_reservation",
            LedgerOp::CheckOut { .. } => "check_out",
            LedgerOp::Return { .. } => "return",
            LedgerOp::ConsumeAvailable { .. } => "consume_available",
            LedgerOp::ConsumeReserved { .. } => "consume_reserved",
            LedgerOp::ConsumeCheckedOut { .. } => "consume_checked_out",
            LedgerOp::AdjustUp { .. } => "adjust_up",
            LedgerOp::AdjustDown { .. } => "adjust_down",
            LedgerOp::TransferReservation { .. } => "transfer_reservation",
        }
    }

    /// Movement between states recorded on the ledger row.
    pub fn state_movement(&self) -> (Option<StockState>, Option<StockState>) {
        use StockState::*;
        match self {
            LedgerOp::Receive { .. } => (None, Some(Available)),
            LedgerOp::Reserve { .. } => (Some(Available), Some(Reserved)),
            LedgerOp::ReleaseReservation { .. } => (Some(Reserved), Some(Available)),
            LedgerOp::CheckOut { .. } => (Some(Available), Some(CheckedOut)),
            LedgerOp::Return { .. } => (Some(CheckedOut), Some(Available)),
            LedgerOp::ConsumeAvailable { .. } => (Some(Available), None),
            LedgerOp::ConsumeReserved { .. } => (Some(Reserved), None),
            LedgerOp::ConsumeCheckedOut { .. } => (Some(CheckedOut), None),
            LedgerOp::AdjustUp { .. } => (None, Some(Available)),
            LedgerOp::AdjustDown { .. } => (Some(Available), None),
            LedgerOp::TransferReservation { .. } => (Some(Reserved), Some(Reserved)),
        }
    }

    pub fn validate(&self) -> Result<(), LedgerError> {
        match self {
            LedgerOp::AdjustUp { note, .. } | LedgerOp::AdjustDown { note, .. } => {
                if note.trim().is_empty() {
                    return Err(LedgerError::EmptyAdjustmentNote);
                }
                Ok(())
            }
            LedgerOp::TransferReservation { from_project, to_project, .. } => {
                if from_project == to_project {
                    return Err(LedgerError::TransferSameProject);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Signed milli-unit effect of an operation on the stock aggregates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StockDelta {
    pub available: i64,
    pub reserved: i64,
    pub checked_out: i64,
    pub lifetime_received: i64,
    pub lifetime_consumed: i64,
}

impl StockDelta {
    pub fn inverse(&self) -> StockDelta {
        StockDelta {
            available: -self.available,
            reserved: -self.reserved,
            checked_out: -self.checked_out,
            lifetime_received: -self.lifetime_received,
            lifetime_consumed: -self.lifetime_consumed,
        }
    }
}

pub fn delta_for(op: &LedgerOp) -> StockDelta {
    let q = op.quantity().as_milli();
    let mut d = StockDelta::default();
    match op {
        LedgerOp::Receive { .. } => {
            d.available = q;
            d.lifetime_received = q;
        }
        LedgerOp::Reserve { .. } => {
            d.available = -q;
            d.reserved = q;
        }
        LedgerOp::ReleaseReservation { .. } => {
            d.reserved = -q;
            d.available = q;
        }
        LedgerOp::CheckOut { .. } => {
            d.available = -q;
            d.checked_out = q;
        }
        LedgerOp::Return { .. } => {
            d.checked_out = -q;
            d.available = q;
        }
        LedgerOp::ConsumeAvailable { .. } => {
            d.available = -q;
            d.lifetime_consumed = q;
        }
        LedgerOp::ConsumeReserved { .. } => {
            d.reserved = -q;
            d.lifetime_consumed = q;
        }
        LedgerOp::ConsumeCheckedOut { .. } => {
            d.checked_out = -q;
            d.lifetime_consumed = q;
        }
        LedgerOp::AdjustUp { .. } => {
            d.available = q;
        }
        LedgerOp::AdjustDown { .. } => {
            d.available = -q;
        }
        LedgerOp::TransferReservation { .. } => {
            // net-zero on aggregates: reservation moves between projects
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{PartId, ProjectId};
    use crate::quantity::Quantity;

    fn part() -> PartId {
        PartId::new()
    }
    fn proj() -> ProjectId {
        ProjectId::new()
    }
    fn q(n: i64) -> Quantity {
        Quantity::from_whole(n).unwrap()
    }

    #[test]
    fn receive_increases_available_and_lifetime_received() {
        let d = delta_for(&LedgerOp::Receive { part_id: part(), quantity: q(10), note: String::new() });
        assert_eq!(
            d,
            StockDelta { available: 10_000, reserved: 0, checked_out: 0, lifetime_received: 10_000, lifetime_consumed: 0 }
        );
    }

    #[test]
    fn reserve_moves_available_to_reserved() {
        let d = delta_for(&LedgerOp::Reserve { part_id: part(), quantity: q(3), project_id: proj() });
        assert_eq!(d.available, -3_000);
        assert_eq!(d.reserved, 3_000);
        assert_eq!(d.lifetime_received, 0);
    }

    #[test]
    fn consume_reserved_decrements_reserved_and_bumps_lifetime_consumed() {
        let d = delta_for(&LedgerOp::ConsumeReserved {
            part_id: part(), quantity: q(2), project_id: Some(proj()), note: String::new(),
        });
        assert_eq!(d.reserved, -2_000);
        assert_eq!(d.lifetime_consumed, 2_000);
        assert_eq!(d.available, 0);
    }

    #[test]
    fn adjustments_never_touch_lifetime_counters() {
        let up = delta_for(&LedgerOp::AdjustUp { part_id: part(), quantity: q(5), note: "recount".into() });
        let down = delta_for(&LedgerOp::AdjustDown { part_id: part(), quantity: q(5), note: "recount".into() });
        assert_eq!((up.lifetime_received, up.lifetime_consumed), (0, 0));
        assert_eq!((down.lifetime_received, down.lifetime_consumed), (0, 0));
        assert_eq!(up.available, 5_000);
        assert_eq!(down.available, -5_000);
    }

    #[test]
    fn adjustments_require_a_note() {
        let op = LedgerOp::AdjustUp { part_id: part(), quantity: q(1), note: "  ".into() };
        assert!(matches!(op.validate(), Err(LedgerError::EmptyAdjustmentNote)));
        let ok = LedgerOp::AdjustUp { part_id: part(), quantity: q(1), note: "recount".into() };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn transfer_between_same_project_is_rejected() {
        let p = proj();
        let op = LedgerOp::TransferReservation {
            part_id: part(), quantity: q(1), from_project: p.clone(), to_project: p,
        };
        assert!(matches!(op.validate(), Err(LedgerError::TransferSameProject)));
    }

    #[test]
    fn transfer_has_zero_net_stock_delta() {
        let d = delta_for(&LedgerOp::TransferReservation {
            part_id: part(), quantity: q(4), from_project: proj(), to_project: proj(),
        });
        assert_eq!(
            d,
            StockDelta { available: 0, reserved: 0, checked_out: 0, lifetime_received: 0, lifetime_consumed: 0 }
        );
    }

    #[test]
    fn inverse_delta_negates_every_field() {
        let d = delta_for(&LedgerOp::Receive { part_id: part(), quantity: q(7), note: String::new() });
        let inv = d.inverse();
        assert_eq!(inv.available, -7_000);
        assert_eq!(inv.lifetime_received, -7_000);
    }

    #[test]
    fn every_op_maps_to_the_specced_sql_type() {
        let p = part();
        let pr = proj();
        let cases: Vec<(LedgerOp, &str)> = vec![
            (LedgerOp::Receive { part_id: p.clone(), quantity: q(1), note: String::new() }, "receive"),
            (LedgerOp::Reserve { part_id: p.clone(), quantity: q(1), project_id: pr.clone() }, "reserve"),
            (LedgerOp::ReleaseReservation { part_id: p.clone(), quantity: q(1), project_id: pr.clone() }, "release_reservation"),
            (LedgerOp::CheckOut { part_id: p.clone(), quantity: q(1), project_id: pr.clone() }, "check_out"),
            (LedgerOp::Return { part_id: p.clone(), quantity: q(1), project_id: pr.clone() }, "return"),
            (LedgerOp::ConsumeAvailable { part_id: p.clone(), quantity: q(1), project_id: None, note: String::new() }, "consume_available"),
            (LedgerOp::ConsumeReserved { part_id: p.clone(), quantity: q(1), project_id: Some(pr.clone()), note: String::new() }, "consume_reserved"),
            (LedgerOp::ConsumeCheckedOut { part_id: p.clone(), quantity: q(1), project_id: Some(pr.clone()), note: String::new() }, "consume_checked_out"),
            (LedgerOp::AdjustUp { part_id: p.clone(), quantity: q(1), note: "n".into() }, "adjust_up"),
            (LedgerOp::AdjustDown { part_id: p.clone(), quantity: q(1), note: "n".into() }, "adjust_down"),
            (LedgerOp::TransferReservation { part_id: p, quantity: q(1), from_project: pr.clone(), to_project: proj() }, "transfer_reservation"),
        ];
        for (op, expected) in cases {
            assert_eq!(op.txn_type_sql(), expected);
        }
    }
}
