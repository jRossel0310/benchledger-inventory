//! Exact fixed-point quantities: milli-units (x1000). No floats, no negatives.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Quantity(i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityUnit {
    Each,
    Meter,
    Foot,
}

impl QuantityUnit {
    pub fn is_discrete(self) -> bool {
        matches!(self, QuantityUnit::Each)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum QuantityError {
    #[error("quantity cannot be negative")]
    Negative,
    #[error("discrete quantities must be whole units")]
    FractionalDiscrete,
    #[error("quantity overflow")]
    Overflow,
}

impl Quantity {
    pub const ZERO: Quantity = Quantity(0);
    pub const SCALE: i64 = 1000;

    pub fn from_milli(milli: i64, unit: QuantityUnit) -> Result<Self, QuantityError> {
        if milli < 0 {
            return Err(QuantityError::Negative);
        }
        if unit.is_discrete() && milli % Self::SCALE != 0 {
            return Err(QuantityError::FractionalDiscrete);
        }
        Ok(Quantity(milli))
    }

    pub fn from_whole(units: i64) -> Result<Self, QuantityError> {
        if units < 0 {
            return Err(QuantityError::Negative);
        }
        units
            .checked_mul(Self::SCALE)
            .map(Quantity)
            .ok_or(QuantityError::Overflow)
    }

    pub fn as_milli(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Quantity) -> Result<Quantity, QuantityError> {
        self.0.checked_add(other.0).map(Quantity).ok_or(QuantityError::Overflow)
    }

    pub fn checked_sub(self, other: Quantity) -> Result<Quantity, QuantityError> {
        let v = self.0 - other.0;
        if v < 0 {
            Err(QuantityError::Negative)
        } else {
            Ok(Quantity(v))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_quantities_scale_by_1000() {
        assert_eq!(Quantity::from_whole(30).unwrap().as_milli(), 30_000);
    }

    #[test]
    fn negative_quantities_are_rejected() {
        assert_eq!(Quantity::from_whole(-1), Err(QuantityError::Negative));
        assert_eq!(
            Quantity::from_milli(-5, QuantityUnit::Meter),
            Err(QuantityError::Negative)
        );
    }

    #[test]
    fn discrete_units_reject_fractions() {
        assert_eq!(
            Quantity::from_milli(1500, QuantityUnit::Each),
            Err(QuantityError::FractionalDiscrete)
        );
        assert!(Quantity::from_milli(2000, QuantityUnit::Each).is_ok());
    }

    #[test]
    fn continuous_units_accept_fractions() {
        assert_eq!(Quantity::from_milli(1500, QuantityUnit::Meter).unwrap().as_milli(), 1500);
    }

    #[test]
    fn subtraction_cannot_go_negative() {
        let a = Quantity::from_whole(3).unwrap();
        let b = Quantity::from_whole(5).unwrap();
        assert_eq!(a.checked_sub(b), Err(QuantityError::Negative));
        assert_eq!(b.checked_sub(a).unwrap(), Quantity::from_whole(2).unwrap());
    }

    #[test]
    fn addition_detects_overflow() {
        let max = Quantity::from_milli(i64::MAX - (i64::MAX % 1000), QuantityUnit::Meter).unwrap();
        assert_eq!(max.checked_add(Quantity::from_whole(1).unwrap()), Err(QuantityError::Overflow));
    }
}
