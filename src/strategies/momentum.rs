use rust_decimal::Decimal;
use crate::indicators::{ATR, EMA, RSI, VolumeProfile, Indicator};
use crate::indicators::volume::OBV;
use crate::indicators::rsi::StochasticRSI;
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
    obv: OBV,
    prev_obv: Option<Decimal>,
    stoch_rsi: StochasticRSI,
    last_stoch_k: Option<Decimal>,
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
            rsi: RSI::new(21),
            ema_fast: EMA::new(16),
            ema_slow: EMA::new(42),
            volume_profile: VolumeProfile::new(40),
            atr: ATR::new(28),
            obv: OBV::new(),
            prev_obv: None,
            stoch_rsi: StochasticRSI::new(21, 21, 5, 5),
            last_stoch_k: None,
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
            obv: OBV::new(),
            prev_obv: None,
            stoch_rsi: StochasticRSI::new(14, 14, 3, 3),
            last_stoch_k: None,
            rsi_overbought: Decimal::from(65),
            rsi_oversold: Decimal::from(35),
            volume_threshold: Decimal::new(12, 1),
            candles_processed: 0,
        }
    }

    fn analyze_momentum(&self, rsi: Decimal, price_vs_fast: bool, price_vs_slow: bool) -> (Signal, &str) {
        let is_strong_bullish = rsi > Decimal::from(55) && rsi < self.rsi_overbought
            && price_vs_fast && price_vs_slow;
        let is_bullish = rsi > Decimal::from(50) && price_vs_fast;
        let is_strong_bearish = rsi < Decimal::from(45) && rsi > self.rsi_oversold
            && !price_vs_fast && !price_vs_slow;
        let is_bearish = rsi < Decimal::from(50) && !price_vs_fast;

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
            self.prev_obv = Some(self.obv.value());
            self.obv.update(c.close, c.volume);
            if let Some((k, _d)) = self.stoch_rsi.update(c.close) {
                self.last_stoch_k = Some(k);
            }
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

        // OBV confirmation: rising OBV with rising price = confirmation, divergence = warning
        if let Some(prev) = self.prev_obv {
            let obv_rising = self.obv.value() > prev;
            let price_rising = price_above_fast;
            if obv_rising == price_rising {
                confidence += Decimal::new(10, 2); // OBV confirms direction
            } else {
                confidence -= Decimal::new(10, 2); // OBV divergence warning
            }
        }

        // StochasticRSI overbought/oversold warning
        if let Some(k) = self.last_stoch_k {
            match signal {
                Signal::StrongBuy | Signal::Buy => {
                    if k > Decimal::from(80) {
                        confidence -= Decimal::new(15, 2); // Overbought warning
                    }
                }
                Signal::StrongSell | Signal::Sell => {
                    if k < Decimal::from(20) {
                        confidence -= Decimal::new(15, 2); // Oversold warning
                    }
                }
                _ => {}
            }
        }

        // Clamp confidence
        confidence = confidence.max(Decimal::new(10, 2)).min(Decimal::new(95, 2));

        let entry = current.close;
        let (stop_loss, take_profit) = match signal {
            Signal::StrongBuy | Signal::Buy => {
                let sl = entry - (atr * Decimal::new(24, 1));
                let tp = entry + (atr * Decimal::new(48, 1));
                (sl, tp)
            }
            Signal::StrongSell | Signal::Sell => {
                let sl = entry + (atr * Decimal::new(24, 1));
                let tp = entry - (atr * Decimal::new(48, 1));
                (sl, tp)
            }
            _ => (entry, entry),
        };

        let obv_str = if let Some(prev) = self.prev_obv {
            let dir = if self.obv.value() > prev { "rising" } else { "falling" };
            format!(", OBV: {}", dir)
        } else {
            String::new()
        };
        let stoch_str = self.last_stoch_k
            .map(|k| format!(", StochRSI K: {:.0}", k))
            .unwrap_or_default();
        let full_reason = format!(
            "{} - RSI: {:.1}, Fast EMA: {:.2}, Slow EMA: {:.2}, High Volume: {}{}{}",
            reason, rsi, fast_ema, slow_ema, high_volume, obv_str, stoch_str
        );

        Some(
            StrategySignal::new(&self.name, self.pair, signal, confidence, &full_reason)
                .with_levels(entry, stop_loss, take_profit),
        )
    }

    fn min_candles_required(&self) -> usize {
        60
    }

    fn reset(&mut self) {
        self.rsi.reset();
        self.ema_fast.reset();
        self.ema_slow.reset();
        self.volume_profile.reset();
        self.atr.reset();
        self.obv.reset();
        self.prev_obv = None;
        self.stoch_rsi.reset();
        self.last_stoch_k = None;
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
            volume_profile: VolumeProfile::new(40),
            atr: ATR::new(28),
            lookback: 20,
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
                let sl = current.low - (atr * Decimal::ONE);
                let tp = entry + (atr * Decimal::from(4));
                (sl, tp)
            }
            _ => {
                let sl = current.high + (atr * Decimal::ONE);
                let tp = entry - (atr * Decimal::from(4));
                (sl, tp)
            }
        };

        Some(
            StrategySignal::new(&self.name, self.pair, signal, confidence, &reason)
                .with_levels(entry, stop_loss, take_profit),
        )
    }

    fn min_candles_required(&self) -> usize {
        50
    }

    fn reset(&mut self) {
        self.volume_profile.reset();
        self.atr.reset();
        self.candles_processed = 0;
    }
}
