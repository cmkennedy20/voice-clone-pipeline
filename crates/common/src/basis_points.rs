//! Exact integer basis-point (`1/10_000`) representation and arithmetic.

use std::fmt;

/// Exact integer basis points where `10_000` represents `100.00%` (`1` bps = `0.01%`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct BasisPoints(u32);

impl BasisPoints {
    /// Zero basis points (`0.00%`).
    pub const ZERO: Self = Self(0);

    /// Maximum standard proportion (`10_000` bps = `100.00%`).
    pub const MAX_PROPORTION: Self = Self(10_000);

    /// Alias for `100.00%` (`10_000` bps).
    pub const ONE_HUNDRED_PERCENT: Self = Self(10_000);

    /// Creates a `BasisPoints` value from raw integer basis points without clamping.
    #[must_use]
    pub const fn from_raw(bps: u32) -> Self {
        Self(bps)
    }

    /// Creates a `BasisPoints` value clamped into `0..=10_000`.
    #[must_use]
    pub const fn new_clamped(bps: u32) -> Self {
        if bps > Self::MAX_PROPORTION.0 {
            Self::MAX_PROPORTION
        } else {
            Self(bps)
        }
    }

    /// Returns the raw `u32` basis-point integer.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }

    /// Computes `(numerator / denominator) * 10_000` using exact integer round-half-up.
    ///
    /// Returns [`Self::ZERO`] when `denominator == 0` and saturates to [`u32::MAX`]
    /// if the resulting ratio exceeds `u32::MAX` basis points.
    #[must_use]
    pub fn from_ratio_round_half_up(numerator: u64, denominator: u64) -> Self {
        if denominator == 0 {
            return Self::ZERO;
        }
        let num = u128::from(numerator);
        let den = u128::from(denominator);
        let rounded = (num * 10_000 + (den / 2)) / den;
        Self(u32::try_from(rounded).unwrap_or(u32::MAX))
    }

    /// Saturating addition in basis points.
    #[must_use]
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    /// Saturating subtraction in basis points.
    #[must_use]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// Returns `true` when `self <= max`.
    #[must_use]
    pub const fn is_within(self, max: Self) -> bool {
        self.0 <= max.0
    }
}

impl fmt::Display for BasisPoints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / 100;
        let frac = self.0 % 100;
        write!(f, "{whole}.{frac:02}%")
    }
}

#[cfg(test)]
mod tests {
    use super::BasisPoints;

    #[test]
    fn zero_denominator_returns_zero_without_panicking() {
        assert_eq!(
            BasisPoints::from_ratio_round_half_up(0, 0),
            BasisPoints::ZERO
        );
        assert_eq!(
            BasisPoints::from_ratio_round_half_up(42, 0),
            BasisPoints::ZERO
        );
    }

    #[test]
    fn ratio_rounds_half_up_across_even_and_odd_denominators() {
        // 1 / 10 = 10.00% = 1000 bps
        assert_eq!(BasisPoints::from_ratio_round_half_up(1, 10).value(), 1_000);
        // 1 / 8 = 12.50% = 1250 bps
        assert_eq!(BasisPoints::from_ratio_round_half_up(1, 8).value(), 1_250);
        // Exact 0.5 bps tie: 1 / 20_000 = 0.5 bps -> rounds half-up to 1 bps
        assert_eq!(BasisPoints::from_ratio_round_half_up(1, 20_000).value(), 1);
        // Just below 0.5 bps tie: 9_999 / 200_000_000 = 0.49995 bps -> rounds down to 0 bps
        assert_eq!(
            BasisPoints::from_ratio_round_half_up(9_999, 200_000_000).value(),
            0
        );
        // Odd denominator: 1 / 3 = 3333.33... -> 3333 bps; 2 / 3 = 6666.66... -> 6667 bps
        assert_eq!(BasisPoints::from_ratio_round_half_up(1, 3).value(), 3_333);
        assert_eq!(BasisPoints::from_ratio_round_half_up(2, 3).value(), 6_667);
    }

    #[test]
    fn ratio_saturates_to_u32_max_even_at_u64_max_numerator() {
        assert_eq!(
            BasisPoints::from_ratio_round_half_up(u64::MAX, 1).value(),
            u32::MAX
        );
        // 6 / 4 = 150.00% = 15_000 bps (exceeds 10_000 without saturating prematurely)
        assert_eq!(BasisPoints::from_ratio_round_half_up(6, 4).value(), 15_000);
    }

    #[test]
    fn clamping_and_saturating_ops_respect_boundaries() {
        assert_eq!(BasisPoints::new_clamped(9_999).value(), 9_999);
        assert_eq!(
            BasisPoints::new_clamped(10_000),
            BasisPoints::ONE_HUNDRED_PERCENT
        );
        assert_eq!(
            BasisPoints::new_clamped(25_000),
            BasisPoints::MAX_PROPORTION
        );

        let a = BasisPoints::from_raw(300);
        let b = BasisPoints::from_raw(500);
        assert_eq!(a.saturating_sub(b), BasisPoints::ZERO);
        assert_eq!(b.saturating_sub(a).value(), 200);
        assert_eq!(
            BasisPoints::from_raw(u32::MAX - 5)
                .saturating_add(BasisPoints::from_raw(20))
                .value(),
            u32::MAX
        );
    }

    #[test]
    fn ordering_and_is_within_evaluate_inclusively() {
        let limit = BasisPoints::from_raw(1_200);
        assert!(BasisPoints::from_raw(1_199).is_within(limit));
        assert!(BasisPoints::from_raw(1_200).is_within(limit));
        assert!(!BasisPoints::from_raw(1_201).is_within(limit));
        assert!(BasisPoints::from_raw(50) < BasisPoints::from_raw(100));
    }

    #[test]
    fn display_formats_exact_two_decimal_percentage() {
        assert_eq!(BasisPoints::ZERO.to_string(), "0.00%");
        assert_eq!(BasisPoints::from_raw(5).to_string(), "0.05%");
        assert_eq!(BasisPoints::from_raw(50).to_string(), "0.50%");
        assert_eq!(BasisPoints::from_raw(1_234).to_string(), "12.34%");
        assert_eq!(BasisPoints::ONE_HUNDRED_PERCENT.to_string(), "100.00%");
        assert_eq!(BasisPoints::from_raw(15_000).to_string(), "150.00%");
    }
}
