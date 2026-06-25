use crate::median::{calculate_median, MedianError};
use soroban_sdk::{contracttype, Vec};

/// Discrete slashing tiers used to differentiate small communication noise from deliberate manipulation.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SlashingTier {
    NoPenalty,
    Low,
    Medium,
    High,
    Critical,
}

/// The result of comparing a faulty provider's submitted price against the consensus median.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviationAnalysis {
    pub submitted_price: i128,
    pub finalized_median_price: i128,
    pub deviation_bps: u128,
    pub slashing_bps: u32,
    pub tier: SlashingTier,
}

impl SlashingTier {
    pub fn from_deviation_bps(deviation_bps: u128) -> Self {
        match deviation_bps {
            0..=100 => SlashingTier::NoPenalty,
            101..=250 => SlashingTier::Low,
            251..=500 => SlashingTier::Medium,
            501..=1_000 => SlashingTier::High,
            _ => SlashingTier::Critical,
        }
    }

    pub fn burn_rate_bps(self) -> u32 {
        match self {
            SlashingTier::NoPenalty => 0,
            SlashingTier::Low => 50,
            SlashingTier::Medium => 150,
            SlashingTier::High => 400,
            SlashingTier::Critical => 1_000,
        }
    }
}

/// Calculate the absolute price deviation from the finalized consensus median in basis points.
/// Returns `None` when the consensus median is zero or when the result cannot be computed safely.
pub fn calculate_price_deviation_bps(submitted_price: i128, finalized_median_price: i128) -> Option<u128> {
    if finalized_median_price <= 0 {
        return None;
    }

    let deviation = if submitted_price >= finalized_median_price {
        submitted_price - finalized_median_price
    } else {
        finalized_median_price - submitted_price
    };

    let numerator = (deviation as u128).checked_mul(10_000)?;
    let denominator = finalized_median_price as u128;
    Some(numerator / denominator)
}

/// Convert a deviation into a slashing burn rate in basis points using a tiered scale.
pub fn calculate_slashing_bps(deviation_bps: u128) -> u32 {
    SlashingTier::from_deviation_bps(deviation_bps).burn_rate_bps()
}

/// Analyze a faulty node price submission against a finalized median consensus price set.
///
/// This returns the computed median, the absolute deviation in basis points, and
/// a burn rate that grows with the magnitude of the deviation.
pub fn analyze_deviation_against_finalized_median(
    submitted_price: i128,
    consensus_prices: Vec<i128>,
) -> Result<DeviationAnalysis, MedianError> {
    let finalized_median_price = calculate_median(consensus_prices)?;
    let deviation_bps = calculate_price_deviation_bps(submitted_price, finalized_median_price)
        .unwrap_or(0);
    let tier = SlashingTier::from_deviation_bps(deviation_bps);
    let slashing_bps = tier.burn_rate_bps();

    Ok(DeviationAnalysis {
        submitted_price,
        finalized_median_price,
        deviation_bps,
        slashing_bps,
        tier,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{vec, Env};

    #[test]
    fn test_calculate_price_deviation_bps_returns_none_for_zero_median() {
        assert_eq!(calculate_price_deviation_bps(1_000_000, 0), None);
    }

    #[test]
    fn test_calculate_price_deviation_bps_small_deviation() {
        assert_eq!(calculate_price_deviation_bps(1_001_000, 1_000_000), Some(100));
        assert_eq!(calculate_price_deviation_bps(999_000, 1_000_000), Some(100));
    }

    #[test]
    fn test_calculate_slashing_bps_tiers() {
        assert_eq!(calculate_slashing_bps(0), 0);
        assert_eq!(calculate_slashing_bps(150), 50);
        assert_eq!(calculate_slashing_bps(300), 150);
        assert_eq!(calculate_slashing_bps(750), 400);
        assert_eq!(calculate_slashing_bps(2_500), 1_000);
    }

    #[test]
    fn test_analyze_deviation_against_finalized_median() {
        let env = Env::default();
        let prices = vec![&env, 10_000_i128, 10_100_i128, 9_900_i128, 11_000_i128];

        let analysis = analyze_deviation_against_finalized_median(11_500, prices).unwrap();

        assert_eq!(analysis.finalized_median_price, 10_050);
        assert_eq!(analysis.deviation_bps, 1_447);
        assert_eq!(analysis.tier, SlashingTier::Critical);
        assert_eq!(analysis.slashing_bps, 1_000);
    }

    #[test]
    fn test_slashing_tier_for_minor_node_hiccup() {
        assert_eq!(SlashingTier::from_deviation_bps(100), SlashingTier::NoPenalty);
        assert_eq!(SlashingTier::from_deviation_bps(180), SlashingTier::Low);
    }
}
