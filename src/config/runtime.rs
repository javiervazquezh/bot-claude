use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use super::profiles::StrategyProfile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub strategy_profile: StrategyProfile,
    pub risk: RiskSettings,
    pub executor: ExecutorSettings,
    pub strategies: StrategySettings,
    pub general: GeneralSettings,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            strategy_profile: StrategyProfile::Conservative5Year,
            risk: RiskSettings::default(),
            executor: ExecutorSettings::default(),
            strategies: StrategySettings::default(),
            general: GeneralSettings::default(),
        }
    }
}

impl RuntimeConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Risk validation
        if self.risk.max_positions == 0 {
            errors.push("max_positions must be > 0".to_string());
        }
        if self.risk.risk_per_trade_pct <= Decimal::ZERO || self.risk.risk_per_trade_pct > dec!(10) {
            errors.push("risk_per_trade_pct must be between 0 and 10%".to_string());
        }
        if self.risk.default_stop_loss_pct <= Decimal::ZERO {
            errors.push("default_stop_loss_pct must be > 0".to_string());
        }
        if self.risk.max_drawdown_pct <= Decimal::ZERO || self.risk.max_drawdown_pct > dec!(100) {
            errors.push("max_drawdown_pct must be between 0 and 100%".to_string());
        }

        // Executor validation
        if self.executor.min_confidence <= Decimal::ZERO || self.executor.min_confidence > Decimal::ONE {
            errors.push("min_confidence must be between 0 and 1".to_string());
        }
        if self.executor.min_risk_reward < Decimal::ONE {
            errors.push("min_risk_reward must be >= 1".to_string());
        }

        // Strategy validation
        if self.strategies.trend.ema_fast_period >= self.strategies.trend.ema_slow_period {
            errors.push("trend: ema_fast_period must be < ema_slow_period".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskSettings {
    pub max_positions: usize,
    pub max_single_position_pct: Decimal,
    pub max_total_exposure_pct: Decimal,
    pub risk_per_trade_pct: Decimal,
    pub default_stop_loss_pct: Decimal,
    pub default_take_profit_pct: Decimal,
    pub min_risk_reward_ratio: Decimal,
    pub max_drawdown_pct: Decimal,
    pub max_daily_loss_pct: Decimal,
    pub max_holding_hours: u32,
}

impl Default for RiskSettings {
    fn default() -> Self {
        Self {
            max_positions: 3,
            max_single_position_pct: dec!(25),
            max_total_exposure_pct: dec!(60),
            risk_per_trade_pct: dec!(1.5),
            default_stop_loss_pct: dec!(2),
            default_take_profit_pct: dec!(6),
            min_risk_reward_ratio: dec!(1.5),
            max_drawdown_pct: dec!(15),
            max_daily_loss_pct: dec!(5),
            max_holding_hours: 72,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorSettings {
    pub min_confidence: Decimal,
    pub min_risk_reward: Decimal,
}

impl Default for ExecutorSettings {
    fn default() -> Self {
        Self {
            min_confidence: dec!(0.60),
            min_risk_reward: dec!(1.5),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategySettings {
    pub trend: TrendStrategyParams,
    pub momentum: MomentumStrategyParams,
    pub mean_reversion: MeanReversionParams,
    pub breakout: BreakoutParams,
}

impl Default for StrategySettings {
    fn default() -> Self {
        Self {
            trend: TrendStrategyParams::default(),
            momentum: MomentumStrategyParams::default(),
            mean_reversion: MeanReversionParams::default(),
            breakout: BreakoutParams::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendStrategyParams {
    pub ema_fast_period: usize,
    pub ema_slow_period: usize,
    pub atr_period: usize,
    pub min_trend_strength: Decimal,
    pub atr_multiplier_sl: Decimal,
    pub atr_multiplier_tp: Decimal,
}

impl Default for TrendStrategyParams {
    fn default() -> Self {
        Self {
            ema_fast_period: 9,
            ema_slow_period: 21,
            atr_period: 14,
            min_trend_strength: dec!(0.5),
            atr_multiplier_sl: dec!(1.5),
            atr_multiplier_tp: dec!(3.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomentumStrategyParams {
    pub rsi_period: usize,
    pub ema_fast_period: usize,
    pub ema_slow_period: usize,
    pub rsi_overbought: Decimal,
    pub rsi_oversold: Decimal,
    pub volume_threshold: Decimal,
}

impl Default for MomentumStrategyParams {
    fn default() -> Self {
        Self {
            rsi_period: 14,
            ema_fast_period: 8,
            ema_slow_period: 21,
            rsi_overbought: dec!(70),
            rsi_oversold: dec!(30),
            volume_threshold: dec!(1.5),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeanReversionParams {
    pub bollinger_period: usize,
    pub bollinger_std_dev: Decimal,
    pub rsi_period: usize,
    pub rsi_oversold: Decimal,
    pub rsi_overbought: Decimal,
}

impl Default for MeanReversionParams {
    fn default() -> Self {
        Self {
            bollinger_period: 20,
            bollinger_std_dev: dec!(2.0),
            rsi_period: 14,
            rsi_oversold: dec!(25),
            rsi_overbought: dec!(75),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakoutParams {
    pub lookback_period: usize,
    pub breakout_threshold: Decimal,
}

impl Default for BreakoutParams {
    fn default() -> Self {
        Self {
            lookback_period: 20,
            breakout_threshold: dec!(1.5),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub enabled_pairs: Vec<String>,
    pub timeframe: String,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            enabled_pairs: vec![
                "BTCUSDT".to_string(),
                "ETHUSDT".to_string(),
                "SOLUSDT".to_string(),
            ],
            timeframe: "M5".to_string(),
        }
    }
}
