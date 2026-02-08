use rust_decimal::Decimal;
use crate::indicators::{ATR, DoubleEMA, MACD, Indicator};
use crate::types::{CandleBuffer, Signal, TradingPair};
use super::{Strategy, StrategySignal};

/// Trend Following Strategy
/// Best for: BTC, ETH
/// Captures sustained directional moves using EMA crossovers and MACD confirmation
pub struct TrendStrategy {
    name: String,
    pair: TradingPair,
    ema: DoubleEMA,
    macd: MACD,
    atr: ATR,
    min_trend_strength: Decimal,
    atr_multiplier_sl: Decimal,
    atr_multiplier_tp: Decimal,
    candles_processed: usize,
}

impl TrendStrategy {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            name: format!("Trend_{}", pair),
            pair,
            ema: DoubleEMA::new(9, 21),
            macd: MACD::default_params(),
            atr: ATR::new(14),
            min_trend_strength: Decimal::new(5, 1), // 0.5% minimum spread
            atr_multiplier_sl: Decimal::new(15, 1), // 1.5x ATR for stop loss
            atr_multiplier_tp: Decimal::new(30, 1), // 3x ATR for take profit
            candles_processed: 0,
        }
    }

    pub fn with_params(
        pair: TradingPair,
        fast_ema: usize,
        slow_ema: usize,
        atr_period: usize,
    ) -> Self {
        Self {
            name: format!("Trend_{}_{}/{}", pair, fast_ema, slow_ema),
            pair,
            ema: DoubleEMA::new(fast_ema, slow_ema),
            macd: MACD::default_params(),
            atr: ATR::new(atr_period),
            min_trend_strength: Decimal::new(5, 1),
            atr_multiplier_sl: Decimal::new(15, 1),
            atr_multiplier_tp: Decimal::new(30, 1),
            candles_processed: 0,
        }
    }

    fn calculate_signal(&self, is_bullish: bool, macd_confirms: bool) -> Signal {
        if is_bullish && macd_confirms {
            Signal::StrongBuy
        } else if is_bullish {
            Signal::Buy
        } else if !is_bullish && macd_confirms {
            Signal::StrongSell
        } else if !is_bullish {
            Signal::Sell
        } else {
            Signal::Neutral
        }
    }

    fn calculate_confidence(&self, spread_pct: Decimal, macd_confirms: bool, trend_aligned: bool) -> Decimal {
        let mut confidence = Decimal::new(50, 2); // Base 50%

        // Add confidence for spread strength
        if spread_pct.abs() > Decimal::ONE {
            confidence += Decimal::new(15, 2);
        }

        // Add confidence for MACD confirmation
        if macd_confirms {
            confidence += Decimal::new(20, 2);
        }

        // Add confidence for trend alignment
        if trend_aligned {
            confidence += Decimal::new(15, 2);
        }

        confidence.min(Decimal::new(95, 2))
    }
}

impl Strategy for TrendStrategy {
    fn name(&self) -> &str {
        &self.name
    }

    fn pair(&self) -> TradingPair {
        self.pair
    }

    fn analyze(&mut self, candles: &CandleBuffer) -> Option<StrategySignal> {
        if candles.len() < self.min_candles_required() {
            return None;
        }

        // Update indicators with NEW candles only (incremental)
        let len = candles.len();
        let start = if self.candles_processed == 0 {
            0
        } else if self.candles_processed < len {
            self.candles_processed
        } else {
            len - 1
        };
        for i in start..len {
            let c = &candles.candles[i];
            self.ema.update(c.close);
            self.macd.update(c.close);
            self.atr.update(c.high, c.low, c.close);
        }
        self.candles_processed = len;

        // Check if indicators are ready
        if !self.ema.is_ready() || !self.macd.is_ready() || !self.atr.is_ready() {
            return None;
        }

        let current_price = candles.last()?.close;
        let fast_ema = self.ema.fast_value()?;
        let slow_ema = self.ema.slow_value()?;
        let spread_pct = self.ema.spread_percentage()?;
        let atr = self.atr.value()?;

        // Determine trend direction
        let is_bullish = fast_ema > slow_ema;
        let is_above_both = current_price > fast_ema && current_price > slow_ema;
        let is_below_both = current_price < fast_ema && current_price < slow_ema;

        // Check MACD confirmation
        let macd_trend = self.macd.trend()?;
        let macd_confirms = if is_bullish {
            macd_trend.is_bullish()
        } else {
            macd_trend.is_bearish()
        };

        // Check trend alignment
        let trend_aligned = (is_bullish && is_above_both) || (!is_bullish && is_below_both);

        // Require minimum trend strength
        if spread_pct.abs() < self.min_trend_strength {
            return Some(StrategySignal::new(
                &self.name,
                self.pair,
                Signal::Neutral,
                Decimal::new(30, 2),
                "Trend not strong enough",
            ));
        }

        let signal = self.calculate_signal(is_bullish, macd_confirms);
        let confidence = self.calculate_confidence(spread_pct, macd_confirms, trend_aligned);

        // Calculate entry, stop loss, and take profit
        let entry = current_price;
        let (stop_loss, take_profit) = if is_bullish {
            let sl = entry - (atr * self.atr_multiplier_sl);
            let tp = entry + (atr * self.atr_multiplier_tp);
            (sl, tp)
        } else {
            let sl = entry + (atr * self.atr_multiplier_sl);
            let tp = entry - (atr * self.atr_multiplier_tp);
            (sl, tp)
        };

        let reason = format!(
            "EMA crossover: fast={:.2} slow={:.2} spread={:.2}%, MACD: {:?}",
            fast_ema, slow_ema, spread_pct, macd_trend
        );

        Some(
            StrategySignal::new(&self.name, self.pair, signal, confidence, &reason)
                .with_levels(entry, stop_loss, take_profit),
        )
    }

    fn min_candles_required(&self) -> usize {
        50 // Need enough for slow EMA and MACD
    }

    fn reset(&mut self) {
        self.ema.reset();
        self.macd.reset();
        self.atr.reset();
        self.candles_processed = 0;
    }
}

/// Breakout Strategy
/// Identifies breakouts from consolidation zones
pub struct BreakoutStrategy {
    name: String,
    pair: TradingPair,
    lookback_period: usize,
    atr: ATR,
    breakout_threshold: Decimal,
    candles_processed: usize,
}

impl BreakoutStrategy {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            name: format!("Breakout_{}", pair),
            pair,
            lookback_period: 20,
            atr: ATR::new(14),
            breakout_threshold: Decimal::new(15, 1), // 1.5x ATR
            candles_processed: 0,
        }
    }
}

impl Strategy for BreakoutStrategy {
    fn name(&self) -> &str {
        &self.name
    }

    fn pair(&self) -> TradingPair {
        self.pair
    }

    fn analyze(&mut self, candles: &CandleBuffer) -> Option<StrategySignal> {
        if candles.len() < self.min_candles_required() {
            return None;
        }

        // Update ATR with NEW candles only (incremental)
        let len = candles.len();
        let start = if self.candles_processed == 0 {
            0
        } else if self.candles_processed < len {
            self.candles_processed
        } else {
            len - 1
        };
        for i in start..len {
            let c = &candles.candles[i];
            self.atr.update(c.high, c.low, c.close);
        }
        self.candles_processed = len;

        if !self.atr.is_ready() {
            return None;
        }

        let current = candles.last()?;
        let recent = candles.last_n(self.lookback_period);

        let highest = recent.iter().map(|c| c.high).max()?;
        let lowest = recent.iter().map(|c| c.low).min()?;
        let atr = self.atr.value()?;
        let threshold = atr * self.breakout_threshold;

        let (signal, confidence, reason) = if current.close > highest {
            let breakout_strength = (current.close - highest) / atr;
            let conf = Decimal::new(60, 2) + (breakout_strength * Decimal::new(10, 2)).min(Decimal::new(30, 2));
            (
                Signal::StrongBuy,
                conf,
                format!("Bullish breakout above {:.2}", highest),
            )
        } else if current.close < lowest {
            let breakout_strength = (lowest - current.close) / atr;
            let conf = Decimal::new(60, 2) + (breakout_strength * Decimal::new(10, 2)).min(Decimal::new(30, 2));
            (
                Signal::StrongSell,
                conf,
                format!("Bearish breakout below {:.2}", lowest),
            )
        } else {
            (
                Signal::Neutral,
                Decimal::new(30, 2),
                "No breakout detected".to_string(),
            )
        };

        let entry = current.close;
        let (stop_loss, take_profit) = match signal {
            Signal::StrongBuy | Signal::Buy => {
                let sl = highest - threshold;
                let tp = entry + (threshold * Decimal::from(2));
                (sl, tp)
            }
            Signal::StrongSell | Signal::Sell => {
                let sl = lowest + threshold;
                let tp = entry - (threshold * Decimal::from(2));
                (sl, tp)
            }
            _ => (entry, entry),
        };

        Some(
            StrategySignal::new(&self.name, self.pair, signal, confidence, &reason)
                .with_levels(entry, stop_loss, take_profit),
        )
    }

    fn min_candles_required(&self) -> usize {
        self.lookback_period + 14
    }

    fn reset(&mut self) {
        self.atr.reset();
        self.candles_processed = 0;
    }
}
