#![allow(dead_code)]
pub mod trend;
pub mod momentum;
pub mod mean_reversion;
pub mod combined;
pub mod improved;

pub use combined::*;
pub use improved::*;

use rust_decimal::Decimal;
use crate::types::{Candle, CandleBuffer, Signal, Side, TradingPair};

pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    fn pair(&self) -> TradingPair;
    fn analyze(&mut self, candles: &CandleBuffer) -> Option<StrategySignal>;
    fn min_candles_required(&self) -> usize;
    fn reset(&mut self);
    /// Feed a BTC candle for cross-asset correlation strategies (default: no-op)
    fn update_btc_candle(&mut self, _candle: Candle) {}
    /// Allow downcasting to concrete types for HMM injection
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct StrategySignal {
    pub strategy_name: String,
    pub pair: TradingPair,
    pub signal: Signal,
    pub confidence: Decimal,
    pub suggested_entry: Option<Decimal>,
    pub suggested_stop_loss: Option<Decimal>,
    pub suggested_take_profit: Option<Decimal>,
    pub reason: String,
}

impl StrategySignal {
    pub fn new(
        strategy_name: &str,
        pair: TradingPair,
        signal: Signal,
        confidence: Decimal,
        reason: &str,
    ) -> Self {
        Self {
            strategy_name: strategy_name.to_string(),
            pair,
            signal,
            confidence,
            suggested_entry: None,
            suggested_stop_loss: None,
            suggested_take_profit: None,
            reason: reason.to_string(),
        }
    }

    pub fn with_levels(
        mut self,
        entry: Decimal,
        stop_loss: Decimal,
        take_profit: Decimal,
    ) -> Self {
        self.suggested_entry = Some(entry);
        self.suggested_stop_loss = Some(stop_loss);
        self.suggested_take_profit = Some(take_profit);
        self
    }

    pub fn side(&self) -> Option<Side> {
        match self.signal {
            Signal::StrongBuy | Signal::Buy => Some(Side::Buy),
            Signal::StrongSell | Signal::Sell => Some(Side::Sell),
            Signal::Neutral => None,
        }
    }

    pub fn should_trade(&self, min_confidence: Decimal) -> bool {
        self.confidence >= min_confidence && !matches!(self.signal, Signal::Neutral)
    }

    pub fn risk_reward_ratio(&self) -> Option<Decimal> {
        match (self.suggested_entry, self.suggested_stop_loss, self.suggested_take_profit) {
            (Some(entry), Some(sl), Some(tp)) => {
                let risk = (entry - sl).abs();
                let reward = (tp - entry).abs();
                if risk.is_zero() {
                    None
                } else {
                    Some(reward / risk)
                }
            }
            _ => None,
        }
    }
}
