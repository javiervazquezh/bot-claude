use rust_decimal::Decimal;
use crate::indicators::{ATR, EMA, RSI, VolumeProfile, Indicator};
use crate::types::{CandleBuffer, Signal, TradingPair};
use super::{Strategy, StrategySignal};

/// Momentum Strategy
/// Best for: SOL (high-beta momentum asset)
/// Captures strong directional moves with volume confirmation
pub struct MomentumStrategy {
    name: String,
    pair: TradingPair,
    rsi: RSI,
    ema_fast: EMA,
    ema_slow: EMA,
    volume_profile: VolumeProfile,
    atr: ATR,
    rsi_overbought: Decimal,
    rsi_oversold: Decimal,
    volume_threshold: Decimal,
    candles_processed: usize,
}

impl MomentumStrategy {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            name: format!("Momentum_{}", pair),
            pair,
            rsi: RSI::new(14),
            ema_fast: EMA::new(8),
            ema_slow: EMA::new(21),
            volume_profile: VolumeProfile::new(20),
            atr: ATR::new(14),
            rsi_overbought: Decimal::from(70),
            rsi_oversold: Decimal::from(30),
            volume_threshold: Decimal::new(15, 1), // 1.5x average volume
            candles_processed: 0,
        }
    }

    pub fn aggressive() -> Self {
        Self {
            name: "Momentum_Aggressive".to_string(),
            pair: TradingPair::SOLUSDT,
            rsi: RSI::new(9),
            ema_fast: EMA::new(5),
            ema_slow: EMA::new(13),
            volume_profile: VolumeProfile::new(14),
            atr: ATR::new(10),
            rsi_overbought: Decimal::from(65),
            rsi_oversold: Decimal::from(35),
            volume_threshold: Decimal::new(12, 1),
            candles_processed: 0,
        }
    }

    fn analyze_momentum(&self, rsi: Decimal, price_vs_fast: bool, price_vs_slow: bool) -> (Signal, &str) {
        let is_strong_bullish = rsi > Decimal::from(50) && rsi < self.rsi_overbought
            && price_vs_fast && price_vs_slow;
        let is_bullish = rsi > Decimal::from(45) && price_vs_fast;
        let is_strong_bearish = rsi < Decimal::from(50) && rsi > self.rsi_oversold
            && !price_vs_fast && !price_vs_slow;
        let is_bearish = rsi < Decimal::from(55) && !price_vs_fast;

        if is_strong_bullish {
            (Signal::StrongBuy, "Strong bullish momentum with volume")
        } else if is_bullish {
            (Signal::Buy, "Bullish momentum building")
        } else if is_strong_bearish {
            (Signal::StrongSell, "Strong bearish momentum with volume")
        } else if is_bearish {
            (Signal::Sell, "Bearish momentum building")
        } else {
            (Signal::Neutral, "No clear momentum")
        }
    }
}

impl Strategy for MomentumStrategy {
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
            self.rsi.update(c.close);
            self.ema_fast.update(c.close);
            self.ema_slow.update(c.close);
            self.volume_profile.update(c.volume);
            self.atr.update(c.high, c.low, c.close);
        }
        self.candles_processed = len;

        if !self.rsi.is_ready() || !self.ema_fast.is_ready() || !self.ema_slow.is_ready() {
            return None;
        }

        let current = candles.last()?;
        let rsi = self.rsi.value()?;
        let fast_ema = self.ema_fast.value()?;
        let slow_ema = self.ema_slow.value()?;
        let atr = self.atr.value()?;

        let price_above_fast = current.close > fast_ema;
        let price_above_slow = current.close > slow_ema;

        // Check volume confirmation
        let high_volume = self.volume_profile.is_high_volume(current.volume, self.volume_threshold);

        let (signal, reason) = self.analyze_momentum(rsi, price_above_fast, price_above_slow);

        // Calculate confidence
        let mut confidence = Decimal::new(50, 2);
        if high_volume {
            confidence += Decimal::new(20, 2);
        }
        if fast_ema > slow_ema && price_above_fast {
            confidence += Decimal::new(15, 2);
        } else if fast_ema < slow_ema && !price_above_fast {
            confidence += Decimal::new(15, 2);
        }

        // Reduce confidence near RSI extremes (potential reversal)
        if rsi > Decimal::from(75) || rsi < Decimal::from(25) {
            confidence -= Decimal::new(10, 2);
        }

        let entry = current.close;
        let (stop_loss, take_profit) = match signal {
            Signal::StrongBuy | Signal::Buy => {
                let sl = entry - (atr * Decimal::new(12, 1));
                let tp = entry + (atr * Decimal::new(24, 1));
                (sl, tp)
            }
            Signal::StrongSell | Signal::Sell => {
                let sl = entry + (atr * Decimal::new(12, 1));
                let tp = entry - (atr * Decimal::new(24, 1));
                (sl, tp)
            }
            _ => (entry, entry),
        };

        let full_reason = format!(
            "{} - RSI: {:.1}, Fast EMA: {:.2}, Slow EMA: {:.2}, High Volume: {}",
            reason, rsi, fast_ema, slow_ema, high_volume
        );

        Some(
            StrategySignal::new(&self.name, self.pair, signal, confidence, &full_reason)
                .with_levels(entry, stop_loss, take_profit),
        )
    }

    fn min_candles_required(&self) -> usize {
        30
    }

    fn reset(&mut self) {
        self.rsi.reset();
        self.ema_fast.reset();
        self.ema_slow.reset();
        self.volume_profile.reset();
        self.atr.reset();
        self.candles_processed = 0;
    }
}

/// Volume Breakout Strategy
/// Identifies breakouts confirmed by significant volume spikes
pub struct VolumeBreakoutStrategy {
    name: String,
    pair: TradingPair,
    volume_profile: VolumeProfile,
    atr: ATR,
    lookback: usize,
    volume_multiplier: Decimal,
    candles_processed: usize,
}

impl VolumeBreakoutStrategy {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            name: format!("VolumeBreakout_{}", pair),
            pair,
            volume_profile: VolumeProfile::new(20),
            atr: ATR::new(14),
            lookback: 10,
            volume_multiplier: Decimal::from(2),
            candles_processed: 0,
        }
    }
}

impl Strategy for VolumeBreakoutStrategy {
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
            self.volume_profile.update(c.volume);
            self.atr.update(c.high, c.low, c.close);
        }
        self.candles_processed = len;

        let current = candles.last()?;
        let recent = candles.last_n(self.lookback);

        let avg_volume = self.volume_profile.avg_volume()?;
        let atr = self.atr.value()?;

        // Check for volume spike
        let volume_ratio = if !avg_volume.is_zero() {
            current.volume / avg_volume
        } else {
            Decimal::ONE
        };

        let is_volume_spike = volume_ratio > self.volume_multiplier;

        if !is_volume_spike {
            return Some(StrategySignal::new(
                &self.name,
                self.pair,
                Signal::Neutral,
                Decimal::new(30, 2),
                "No volume spike detected",
            ));
        }

        // Determine direction based on candle
        let (signal, confidence, reason) = if current.is_bullish() {
            let price_strength = current.change_percentage().abs();
            let conf = Decimal::new(65, 2) + (price_strength * Decimal::new(5, 2)).min(Decimal::new(25, 2));
            (
                Signal::StrongBuy,
                conf,
                format!("Bullish volume breakout, {:.1}x avg volume", volume_ratio),
            )
        } else {
            let price_strength = current.change_percentage().abs();
            let conf = Decimal::new(65, 2) + (price_strength * Decimal::new(5, 2)).min(Decimal::new(25, 2));
            (
                Signal::StrongSell,
                conf,
                format!("Bearish volume breakout, {:.1}x avg volume", volume_ratio),
            )
        };

        let entry = current.close;
        let (stop_loss, take_profit) = match signal {
            Signal::StrongBuy | Signal::Buy => {
                let sl = current.low - (atr * Decimal::new(5, 1));
                let tp = entry + (atr * Decimal::from(2));
                (sl, tp)
            }
            _ => {
                let sl = current.high + (atr * Decimal::new(5, 1));
                let tp = entry - (atr * Decimal::from(2));
                (sl, tp)
            }
        };

        Some(
            StrategySignal::new(&self.name, self.pair, signal, confidence, &reason)
                .with_levels(entry, stop_loss, take_profit),
        )
    }

    fn min_candles_required(&self) -> usize {
        25
    }

    fn reset(&mut self) {
        self.volume_profile.reset();
        self.atr.reset();
        self.candles_processed = 0;
    }
}
