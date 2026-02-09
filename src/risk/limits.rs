#![allow(dead_code)]
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    // Position limits
    pub max_positions: usize,
    pub max_single_position_pct: Decimal, // % of capital for single position
    pub max_total_exposure_pct: Decimal,  // % of capital in all positions

    // Risk per trade
    pub risk_per_trade_pct: Decimal, // % of capital risked per trade

    // Stop loss / Take profit defaults
    pub default_stop_loss_pct: Decimal,
    pub default_take_profit_pct: Decimal,
    pub min_risk_reward_ratio: Decimal,

    // Drawdown limits
    pub max_drawdown_pct: Decimal,
    pub max_daily_loss_pct: Decimal,

    // Time limits
    pub max_holding_hours: u32,

    // Correlation limits
    pub max_correlated_positions: usize,
}

impl RiskLimits {
    pub fn conservative() -> Self {
        Self {
            max_positions: 2,
            max_single_position_pct: Decimal::from(20),
            max_total_exposure_pct: Decimal::from(40),
            risk_per_trade_pct: Decimal::ONE,
            default_stop_loss_pct: Decimal::from(2),
            default_take_profit_pct: Decimal::from(4),
            min_risk_reward_ratio: Decimal::from(2),
            max_drawdown_pct: Decimal::from(10),
            max_daily_loss_pct: Decimal::from(3),
            max_holding_hours: 48,
            max_correlated_positions: 1,
        }
    }

    pub fn moderate() -> Self {
        Self {
            max_positions: 3,
            max_single_position_pct: Decimal::from(25),
            max_total_exposure_pct: Decimal::from(60),
            risk_per_trade_pct: Decimal::new(15, 1), // 1.5%
            default_stop_loss_pct: Decimal::from(3),
            default_take_profit_pct: Decimal::from(5),
            min_risk_reward_ratio: Decimal::new(15, 1), // 1.5:1
            max_drawdown_pct: Decimal::from(15),
            max_daily_loss_pct: Decimal::from(5),
            max_holding_hours: 72,
            max_correlated_positions: 2,
        }
    }

    pub fn aggressive() -> Self {
        Self {
            max_positions: 4,
            max_single_position_pct: Decimal::from(30),
            max_total_exposure_pct: Decimal::from(80),
            risk_per_trade_pct: Decimal::from(2),
            default_stop_loss_pct: Decimal::from(4),
            default_take_profit_pct: Decimal::from(8),
            min_risk_reward_ratio: Decimal::new(12, 1), // 1.2:1
            max_drawdown_pct: Decimal::from(20),
            max_daily_loss_pct: Decimal::from(7),
            max_holding_hours: 168, // 1 week
            max_correlated_positions: 3,
        }
    }

    pub fn custom(
        max_positions: usize,
        risk_per_trade: Decimal,
        max_drawdown: Decimal,
    ) -> Self {
        Self {
            max_positions,
            max_single_position_pct: Decimal::from(100) / Decimal::from(max_positions as u32),
            max_total_exposure_pct: Decimal::from(70),
            risk_per_trade_pct: risk_per_trade,
            default_stop_loss_pct: risk_per_trade * Decimal::from(2),
            default_take_profit_pct: risk_per_trade * Decimal::from(4),
            min_risk_reward_ratio: Decimal::new(15, 1),
            max_drawdown_pct: max_drawdown,
            max_daily_loss_pct: max_drawdown / Decimal::from(3),
            max_holding_hours: 72,
            max_correlated_positions: 2,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.max_positions == 0 {
            return Err("Max positions must be > 0".to_string());
        }
        if self.risk_per_trade_pct <= Decimal::ZERO || self.risk_per_trade_pct > Decimal::from(10) {
            return Err("Risk per trade must be between 0 and 10%".to_string());
        }
        if self.max_drawdown_pct <= Decimal::ZERO || self.max_drawdown_pct > Decimal::from(50) {
            return Err("Max drawdown must be between 0 and 50%".to_string());
        }
        if self.min_risk_reward_ratio < Decimal::ONE {
            return Err("Risk/reward ratio should be >= 1".to_string());
        }
        Ok(())
    }
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self::moderate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSizeConfig {
    pub btc_allocation: Decimal,
    pub eth_allocation: Decimal,
    pub sol_allocation: Decimal,
}

impl Default for PositionSizeConfig {
    fn default() -> Self {
        Self {
            btc_allocation: Decimal::from(40),
            eth_allocation: Decimal::from(35),
            sol_allocation: Decimal::from(25),
        }
    }
}

impl PositionSizeConfig {
    pub fn balanced() -> Self {
        Self {
            btc_allocation: Decimal::new(33, 0),
            eth_allocation: Decimal::new(33, 0),
            sol_allocation: Decimal::new(34, 0),
        }
    }

    pub fn btc_heavy() -> Self {
        Self {
            btc_allocation: Decimal::from(50),
            eth_allocation: Decimal::from(30),
            sol_allocation: Decimal::from(20),
        }
    }

    pub fn altcoin_heavy() -> Self {
        Self {
            btc_allocation: Decimal::from(30),
            eth_allocation: Decimal::from(35),
            sol_allocation: Decimal::from(35),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_limits_validation() {
        let limits = RiskLimits::moderate();
        assert!(limits.validate().is_ok());

        let invalid = RiskLimits {
            max_positions: 0,
            ..RiskLimits::moderate()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_position_size_config() {
        let config = PositionSizeConfig::default();
        let total = config.btc_allocation + config.eth_allocation + config.sol_allocation;
        assert_eq!(total, Decimal::from(100));
    }
}
