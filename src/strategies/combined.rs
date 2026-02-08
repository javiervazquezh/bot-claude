use rust_decimal::Decimal;
use crate::indicators::Indicator;
use crate::indicators::atr::{ATR, VolatilityLevel};
use crate::types::{Candle, CandleBuffer, Signal, TradingPair};
use super::{Strategy, StrategySignal};
use super::trend::{TrendStrategy, BreakoutStrategy};
use super::momentum::{MomentumStrategy, VolumeBreakoutStrategy};
use super::mean_reversion::{MeanReversionStrategy, RSIDivergenceStrategy};

/// Strategy index labels for weight adjustment
/// Each asset's strategies are indexed in the order they appear in the `strategies` vec.
/// The regime detection needs to know which index corresponds to "trend" vs "mean_reversion"
/// so it can shift weights appropriately.
#[derive(Debug, Clone, Copy)]
struct StrategyLayout {
    trend_idx: Option<usize>,
    momentum_idx: Option<usize>,
    mean_reversion_idx: Option<usize>,
}

/// Combined Strategy that aggregates signals from multiple strategies
/// with asset-specific weighting and regime-aware dynamic weight adjustment
pub struct CombinedStrategy {
    name: String,
    pair: TradingPair,
    strategies: Vec<Box<dyn Strategy>>,
    base_weights: Vec<Decimal>,
    weights: Vec<Decimal>,
    min_agreement: Decimal,
    layout: StrategyLayout,
    atr: ATR,
    btc_correlation: Option<BTCCorrelationStrategy>,
    btc_correlation_weight: Decimal,
}

impl CombinedStrategy {
    pub fn for_btc() -> Self {
        let pair = TradingPair::BTCUSDT;
        let strategies: Vec<Box<dyn Strategy>> = vec![
            Box::new(TrendStrategy::new(pair)),
            Box::new(BreakoutStrategy::new(pair)),
            Box::new(MeanReversionStrategy::new(pair)),
        ];
        let weights = vec![
            Decimal::new(45, 2), // 45% trend
            Decimal::new(35, 2), // 35% breakout
            Decimal::new(20, 2), // 20% mean reversion
        ];

        Self {
            name: "Combined_BTC".to_string(),
            pair,
            strategies,
            base_weights: weights.clone(),
            weights,
            min_agreement: Decimal::new(60, 2),
            layout: StrategyLayout { trend_idx: Some(0), momentum_idx: None, mean_reversion_idx: Some(2) },
            atr: ATR::new(14),
            btc_correlation: None,
            btc_correlation_weight: Decimal::ZERO,
        }
    }

    pub fn for_eth() -> Self {
        let pair = TradingPair::ETHUSDT;
        let strategies: Vec<Box<dyn Strategy>> = vec![
            Box::new(TrendStrategy::new(pair)),
            Box::new(MomentumStrategy::new(pair)),
            Box::new(MeanReversionStrategy::new(pair)),
        ];
        // Weights sum to 100%; BTC correlation is extra signal that normalizes via total_weight
        let weights = vec![
            Decimal::new(40, 2), // 40% trend
            Decimal::new(35, 2), // 35% momentum
            Decimal::new(25, 2), // 25% mean reversion
        ];

        Self {
            name: "Combined_ETH".to_string(),
            pair,
            strategies,
            base_weights: weights.clone(),
            weights,
            min_agreement: Decimal::new(55, 2),
            layout: StrategyLayout { trend_idx: Some(0), momentum_idx: Some(1), mean_reversion_idx: Some(2) },
            atr: ATR::new(14),
            btc_correlation: Some(BTCCorrelationStrategy::new()),
            btc_correlation_weight: Decimal::new(15, 2), // 15%
        }
    }

    pub fn for_sol() -> Self {
        let pair = TradingPair::SOLUSDT;
        let strategies: Vec<Box<dyn Strategy>> = vec![
            Box::new(MomentumStrategy::new(pair)),
            Box::new(VolumeBreakoutStrategy::new(pair)),
            Box::new(MeanReversionStrategy::new(pair)),
            Box::new(RSIDivergenceStrategy::new(pair)),
        ];
        let weights = vec![
            Decimal::new(35, 2), // 35% momentum
            Decimal::new(25, 2), // 25% volume breakout
            Decimal::new(25, 2), // 25% mean reversion
            Decimal::new(15, 2), // 15% RSI divergence
        ];

        Self {
            name: "Combined_SOL".to_string(),
            pair,
            strategies,
            base_weights: weights.clone(),
            weights,
            min_agreement: Decimal::new(50, 2),
            layout: StrategyLayout { trend_idx: None, momentum_idx: Some(0), mean_reversion_idx: Some(2) },
            atr: ATR::new(14),
            btc_correlation: None,
            btc_correlation_weight: Decimal::ZERO,
        }
    }

    pub fn for_altcoin(pair: TradingPair) -> Self {
        let strategies: Vec<Box<dyn Strategy>> = vec![
            Box::new(TrendStrategy::new(pair)),
            Box::new(MomentumStrategy::new(pair)),
            Box::new(MeanReversionStrategy::new(pair)),
        ];
        let weights = vec![
            Decimal::new(40, 2), // 40% trend
            Decimal::new(35, 2), // 35% momentum
            Decimal::new(25, 2), // 25% mean reversion
        ];

        Self {
            name: format!("Combined_{}", pair.base_asset()),
            pair,
            strategies,
            base_weights: weights.clone(),
            weights,
            min_agreement: Decimal::new(55, 2),
            layout: StrategyLayout { trend_idx: Some(0), momentum_idx: Some(1), mean_reversion_idx: Some(2) },
            atr: ATR::new(14),
            btc_correlation: None,
            btc_correlation_weight: Decimal::ZERO,
        }
    }

    /// Update regime detection and adjust strategy weights based on volatility
    fn update_regime(&mut self, candle: &Candle) {
        self.atr.update(candle.high, candle.low, candle.close);

        let vol_level = match self.atr.volatility_level(candle.close) {
            Some(v) => v,
            None => return, // ATR not ready yet, keep base weights
        };

        // Adjust weights based on regime
        let mut adjusted = self.base_weights.clone();
        let shift = Decimal::new(15, 2); // 15% weight shift

        match vol_level {
            VolatilityLevel::Low => {
                // Ranging market: boost mean reversion, reduce trend
                if let Some(mr_idx) = self.layout.mean_reversion_idx {
                    if let Some(w) = adjusted.get_mut(mr_idx) { *w += shift; }
                }
                if let Some(t_idx) = self.layout.trend_idx {
                    if let Some(w) = adjusted.get_mut(t_idx) { *w -= shift; }
                } else if let Some(m_idx) = self.layout.momentum_idx {
                    if let Some(w) = adjusted.get_mut(m_idx) { *w -= shift; }
                }
            }
            VolatilityLevel::Medium => {
                // Normal: use base weights (no change)
            }
            VolatilityLevel::High => {
                // Trending market: boost trend, reduce mean reversion
                if let Some(t_idx) = self.layout.trend_idx {
                    if let Some(w) = adjusted.get_mut(t_idx) { *w += shift; }
                } else if let Some(m_idx) = self.layout.momentum_idx {
                    if let Some(w) = adjusted.get_mut(m_idx) { *w += shift; }
                }
                if let Some(mr_idx) = self.layout.mean_reversion_idx {
                    if let Some(w) = adjusted.get_mut(mr_idx) { *w -= shift; }
                }
            }
            VolatilityLevel::Extreme => {
                // Extreme: boost momentum, reduce all others proportionally
                if let Some(m_idx) = self.layout.momentum_idx {
                    if let Some(w) = adjusted.get_mut(m_idx) { *w += Decimal::new(10, 2); }
                    // Reduce others proportionally
                    let reduction = Decimal::new(10, 2) / Decimal::from((adjusted.len() - 1) as u32);
                    for (i, w) in adjusted.iter_mut().enumerate() {
                        if i != m_idx {
                            *w -= reduction;
                        }
                    }
                }
            }
        }

        // Ensure no weight goes negative
        for w in adjusted.iter_mut() {
            if *w < Decimal::ZERO {
                *w = Decimal::ZERO;
            }
        }

        self.weights = adjusted;
    }

    fn aggregate_signals(&self, signals: &[StrategySignal], btc_signal: Option<&StrategySignal>) -> Option<StrategySignal> {
        if signals.is_empty() && btc_signal.is_none() {
            return None;
        }

        let mut weighted_strength = Decimal::ZERO;
        let mut total_weight = Decimal::ZERO;
        let mut total_confidence = Decimal::ZERO;
        let mut reasons = Vec::new();

        let mut best_entry = None;
        let mut best_sl = None;
        let mut best_tp = None;
        let mut best_confidence = Decimal::ZERO;

        for (i, signal) in signals.iter().enumerate() {
            let weight = self.weights.get(i).copied().unwrap_or(Decimal::ONE / Decimal::from(signals.len() as u32));

            let strength = Decimal::from(signal.signal.strength() as i32);
            weighted_strength += strength * weight * signal.confidence;
            total_weight += weight;
            total_confidence += signal.confidence * weight;

            reasons.push(format!("{}: {:?} ({:.0}%)",
                signal.strategy_name, signal.signal, signal.confidence * Decimal::from(100)));

            // Use levels from highest confidence signal
            if signal.confidence > best_confidence {
                best_confidence = signal.confidence;
                best_entry = signal.suggested_entry;
                best_sl = signal.suggested_stop_loss;
                best_tp = signal.suggested_take_profit;
            }
        }

        // Include BTC correlation signal if available
        if let Some(btc_sig) = btc_signal {
            let weight = self.btc_correlation_weight;
            let strength = Decimal::from(btc_sig.signal.strength() as i32);
            weighted_strength += strength * weight * btc_sig.confidence;
            total_weight += weight;
            total_confidence += btc_sig.confidence * weight;
            reasons.push(format!("{}: {:?} ({:.0}%)",
                btc_sig.strategy_name, btc_sig.signal, btc_sig.confidence * Decimal::from(100)));
        }

        if total_weight.is_zero() {
            return None;
        }

        let avg_strength = weighted_strength / total_weight;
        let avg_confidence = total_confidence / total_weight;

        // Determine final signal based on weighted average
        let final_signal = if avg_strength > Decimal::new(15, 1) {
            Signal::StrongBuy
        } else if avg_strength > Decimal::new(5, 1) {
            Signal::Buy
        } else if avg_strength < Decimal::new(-15, 1) {
            Signal::StrongSell
        } else if avg_strength < Decimal::new(-5, 1) {
            Signal::Sell
        } else {
            Signal::Neutral
        };

        let combined_reason = reasons.join(" | ");

        let mut result = StrategySignal::new(
            &self.name,
            self.pair,
            final_signal,
            avg_confidence,
            &combined_reason,
        );

        if let (Some(entry), Some(sl), Some(tp)) = (best_entry, best_sl, best_tp) {
            result = result.with_levels(entry, sl, tp);
        }

        Some(result)
    }
}

impl Strategy for CombinedStrategy {
    fn name(&self) -> &str {
        &self.name
    }

    fn pair(&self) -> TradingPair {
        self.pair
    }

    fn analyze(&mut self, candles: &CandleBuffer) -> Option<StrategySignal> {
        // Update regime detection from latest candle
        if let Some(latest) = candles.last() {
            self.update_regime(latest);
        }

        let mut signals = Vec::new();

        for strategy in &mut self.strategies {
            if let Some(signal) = strategy.analyze(candles) {
                signals.push(signal);
            }
        }

        // Get BTC correlation signal if available
        let btc_signal = self.btc_correlation.as_mut()
            .and_then(|btc| btc.analyze(candles));

        self.aggregate_signals(&signals, btc_signal.as_ref())
    }

    fn min_candles_required(&self) -> usize {
        self.strategies
            .iter()
            .map(|s| s.min_candles_required())
            .max()
            .unwrap_or(50)
    }

    fn reset(&mut self) {
        for strategy in &mut self.strategies {
            strategy.reset();
        }
        if let Some(btc) = &mut self.btc_correlation {
            btc.reset();
        }
        self.weights = self.base_weights.clone();
        self.atr.reset();
    }

    fn update_btc_candle(&mut self, candle: Candle) {
        if let Some(btc) = &mut self.btc_correlation {
            btc.update_btc(candle);
        }
    }
}

/// BTC Correlation Strategy for ETH
/// Trades ETH based on BTC's movement with lag
pub struct BTCCorrelationStrategy {
    name: String,
    pair: TradingPair,
    btc_candles: CandleBuffer,
    correlation_lookback: usize,
    lag_periods: usize,
    atr: crate::indicators::ATR,
}

impl BTCCorrelationStrategy {
    pub fn new() -> Self {
        Self {
            name: "BTCCorrelation_ETH".to_string(),
            pair: TradingPair::ETHUSDT,
            btc_candles: CandleBuffer::new(100),
            correlation_lookback: 20,
            lag_periods: 2,
            atr: crate::indicators::ATR::new(14),
        }
    }

    pub fn update_btc(&mut self, candle: Candle) {
        self.btc_candles.push(candle);
    }

    fn calculate_btc_momentum(&self) -> Option<(Decimal, bool)> {
        if self.btc_candles.len() < self.lag_periods + 5 {
            return None;
        }

        let btc_recent = self.btc_candles.last_n(self.lag_periods + 1);
        let btc_change: Decimal = btc_recent.iter()
            .map(|c| c.change_percentage())
            .sum::<Decimal>() / Decimal::from(btc_recent.len() as u32);

        let is_strong = btc_change.abs() > Decimal::from(1); // 1% threshold

        Some((btc_change, is_strong))
    }
}

impl Default for BTCCorrelationStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl Strategy for BTCCorrelationStrategy {
    fn name(&self) -> &str {
        &self.name
    }

    fn pair(&self) -> TradingPair {
        self.pair
    }

    fn analyze(&mut self, candles: &CandleBuffer) -> Option<StrategySignal> {
        let (btc_change, is_strong) = self.calculate_btc_momentum()?;

        if !is_strong {
            return Some(StrategySignal::new(
                &self.name,
                self.pair,
                Signal::Neutral,
                Decimal::new(30, 2),
                "BTC movement not strong enough",
            ));
        }

        let current = candles.last()?;

        // Update ATR from ETH candles
        self.atr.update(current.high, current.low, current.close);

        let (signal, confidence, reason) = if btc_change > Decimal::from(2) {
            (
                Signal::StrongBuy,
                Decimal::new(75, 2),
                format!("Strong BTC rally ({:.1}%), ETH likely to follow", btc_change),
            )
        } else if btc_change > Decimal::ZERO {
            (
                Signal::Buy,
                Decimal::new(60, 2),
                format!("BTC bullish ({:.1}%), ETH correlation play", btc_change),
            )
        } else if btc_change < Decimal::from(-2) {
            (
                Signal::StrongSell,
                Decimal::new(75, 2),
                format!("Strong BTC drop ({:.1}%), ETH likely to follow", btc_change),
            )
        } else {
            (
                Signal::Sell,
                Decimal::new(60, 2),
                format!("BTC bearish ({:.1}%), ETH correlation play", btc_change),
            )
        };

        let mut result = StrategySignal::new(&self.name, self.pair, signal, confidence, &reason);

        // Set price levels from ETH ATR
        if let Some(atr) = self.atr.value() {
            let entry = current.close;
            let (sl, tp) = match signal {
                Signal::StrongBuy | Signal::Buy => {
                    (entry - atr * Decimal::new(15, 1), entry + atr * Decimal::from(3))
                }
                Signal::StrongSell | Signal::Sell => {
                    (entry + atr * Decimal::new(15, 1), entry - atr * Decimal::from(3))
                }
                _ => (entry, entry),
            };
            result = result.with_levels(entry, sl, tp);
        }

        Some(result)
    }

    fn min_candles_required(&self) -> usize {
        10
    }

    fn reset(&mut self) {
        self.btc_candles = CandleBuffer::new(100);
        self.atr.reset();
    }
}

/// Strategy Factory
pub fn create_strategies_for_pair(pair: TradingPair) -> CombinedStrategy {
    match pair {
        TradingPair::BTCUSDT => CombinedStrategy::for_btc(),
        TradingPair::ETHUSDT => CombinedStrategy::for_eth(),
        TradingPair::SOLUSDT => CombinedStrategy::for_sol(),
        TradingPair::BNBUSDT => CombinedStrategy::for_altcoin(pair),
        TradingPair::ADAUSDT => CombinedStrategy::for_altcoin(pair),
        TradingPair::XRPUSDT => CombinedStrategy::for_altcoin(pair),
    }
}
