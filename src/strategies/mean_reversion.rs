use rust_decimal::Decimal;
use crate::indicators::{ATR, BollingerBands, RSI, Indicator};
use crate::indicators::volume::MoneyFlowIndex;
use crate::types::{CandleBuffer, Signal, TradingPair};
use super::{Strategy, StrategySignal};

/// Mean Reversion Strategy
/// Best for: SOL on extreme moves, ETH during range-bound periods
/// Trades reversals when price extends too far from mean
pub struct MeanReversionStrategy {
    name: String,
    pair: TradingPair,
    bollinger: BollingerBands,
    rsi: RSI,
    atr: ATR,
    mfi: MoneyFlowIndex,
    rsi_oversold: Decimal,
    rsi_overbought: Decimal,
    candles_processed: usize,
}

impl MeanReversionStrategy {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            name: format!("MeanReversion_{}", pair),
            pair,
            bollinger: BollingerBands::new(40, Decimal::from(2)),
            rsi: RSI::new(21),
            atr: ATR::new(28),
            mfi: MoneyFlowIndex::new(28),
            rsi_oversold: Decimal::from(25),
            rsi_overbought: Decimal::from(75),
            candles_processed: 0,
        }
    }

    pub fn conservative(pair: TradingPair) -> Self {
        Self {
            name: format!("MeanReversion_Conservative_{}", pair),
            pair,
            bollinger: BollingerBands::new(40, Decimal::new(25, 1)), // 2.5 std dev
            rsi: RSI::new(21),
            atr: ATR::new(28),
            mfi: MoneyFlowIndex::new(28),
            rsi_oversold: Decimal::from(20),
            rsi_overbought: Decimal::from(80),
            candles_processed: 0,
        }
    }

    fn check_reversal_conditions(
        &self,
        price: Decimal,
        bb_upper: Decimal,
        bb_lower: Decimal,
        bb_middle: Decimal,
        rsi: Decimal,
    ) -> (Signal, Decimal, String) {
        let is_at_lower = price <= bb_lower;
        let is_at_upper = price >= bb_upper;
        let rsi_oversold = rsi <= self.rsi_oversold;
        let rsi_overbought = rsi >= self.rsi_overbought;

        if is_at_lower && rsi_oversold {
            (
                Signal::StrongBuy,
                Decimal::new(80, 2),
                format!(
                    "Price at lower BB ({:.2}) with RSI oversold ({:.1})",
                    bb_lower, rsi
                ),
            )
        } else if is_at_lower {
            (
                Signal::Buy,
                Decimal::new(60, 2),
                format!("Price at lower BB ({:.2}), RSI: {:.1}", bb_lower, rsi),
            )
        } else if is_at_upper && rsi_overbought {
            (
                Signal::StrongSell,
                Decimal::new(80, 2),
                format!(
                    "Price at upper BB ({:.2}) with RSI overbought ({:.1})",
                    bb_upper, rsi
                ),
            )
        } else if is_at_upper {
            (
                Signal::Sell,
                Decimal::new(60, 2),
                format!("Price at upper BB ({:.2}), RSI: {:.1}", bb_upper, rsi),
            )
        } else {
            (
                Signal::Neutral,
                Decimal::new(40, 2),
                format!(
                    "Price between bands, waiting for extreme. BB: {:.2}-{:.2}, RSI: {:.1}",
                    bb_lower, bb_upper, rsi
                ),
            )
        }
    }
}

impl Strategy for MeanReversionStrategy {
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
            self.bollinger.update(c.close);
            self.rsi.update(c.close);
            self.atr.update(c.high, c.low, c.close);
            self.mfi.update(c.high, c.low, c.close, c.volume);
        }
        self.candles_processed = len;

        if !self.bollinger.is_ready() || !self.rsi.is_ready() || !self.atr.is_ready() || !self.mfi.is_ready() {
            return None;
        }

        let current = candles.last()?;
        let price = current.close;
        let bb_upper = self.bollinger.upper()?;
        let bb_lower = self.bollinger.lower()?;
        let bb_middle = self.bollinger.middle()?;
        let rsi = self.rsi.value()?;
        let atr = self.atr.value()?;

        let (signal, mut confidence, reason) =
            self.check_reversal_conditions(price, bb_upper, bb_lower, bb_middle, rsi);

        // MFI confirmation: MFI < 20 confirms oversold on buy, MFI > 80 confirms overbought on sell
        if let Some(mfi_val) = self.mfi.value() {
            match signal {
                Signal::StrongBuy | Signal::Buy => {
                    if mfi_val < Decimal::from(20) {
                        confidence += Decimal::new(10, 2); // MFI confirms oversold
                    }
                }
                Signal::StrongSell | Signal::Sell => {
                    if mfi_val > Decimal::from(80) {
                        confidence += Decimal::new(10, 2); // MFI confirms overbought
                    }
                }
                _ => {}
            }
            confidence = confidence.min(Decimal::new(95, 2));
        }

        // Calculate targets: entry at current, target past middle band for better R:R
        let entry = price;
        let (stop_loss, take_profit) = match signal {
            Signal::StrongBuy | Signal::Buy => {
                let sl = bb_lower - (atr * Decimal::ONE);
                // Target midpoint between middle and upper band for better R:R
                let tp = bb_middle + (bb_upper - bb_middle) * Decimal::new(5, 1);
                (sl, tp)
            }
            Signal::StrongSell | Signal::Sell => {
                let sl = bb_upper + (atr * Decimal::ONE);
                // Target midpoint between middle and lower band for better R:R
                let tp = bb_middle - (bb_middle - bb_lower) * Decimal::new(5, 1);
                (sl, tp)
            }
            _ => (price, price),
        };

        let mfi_str = self.mfi.value()
            .map(|v| format!(", MFI: {:.1}", v))
            .unwrap_or_default();
        let full_reason = format!("{}{}", reason, mfi_str);

        Some(
            StrategySignal::new(&self.name, self.pair, signal, confidence, &full_reason)
                .with_levels(entry, stop_loss, take_profit),
        )
    }

    fn min_candles_required(&self) -> usize {
        60
    }

    fn reset(&mut self) {
        self.bollinger.reset();
        self.rsi.reset();
        self.atr.reset();
        self.mfi.reset();
        self.candles_processed = 0;
    }
}

/// RSI Divergence Strategy
/// Identifies potential reversals through RSI divergence
pub struct RSIDivergenceStrategy {
    name: String,
    pair: TradingPair,
    rsi: RSI,
    atr: ATR,
    lookback: usize,
    price_history: Vec<(Decimal, Decimal)>, // (price, rsi)
    candles_processed: usize,
}

impl RSIDivergenceStrategy {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            name: format!("RSIDivergence_{}", pair),
            pair,
            rsi: RSI::new(21),
            atr: ATR::new(28),
            lookback: 48,
            price_history: Vec::with_capacity(20),
            candles_processed: 0,
        }
    }

    fn detect_divergence(&self, current_price: Decimal, current_rsi: Decimal) -> Option<(bool, bool)> {
        if self.price_history.len() < self.lookback {
            return None;
        }

        let recent = &self.price_history[self.price_history.len() - self.lookback..];

        // Find local minimum and maximum in recent history
        let (min_price, min_rsi) = recent.iter().min_by(|a, b| a.0.cmp(&b.0))?;
        let (max_price, max_rsi) = recent.iter().max_by(|a, b| a.0.cmp(&b.0))?;

        // Bullish divergence: price making lower lows, RSI making higher lows
        let bullish_div = current_price < *min_price && current_rsi > *min_rsi;

        // Bearish divergence: price making higher highs, RSI making lower highs
        let bearish_div = current_price > *max_price && current_rsi < *max_rsi;

        Some((bullish_div, bearish_div))
    }
}

impl Strategy for RSIDivergenceStrategy {
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
            if let Some(rsi) = self.rsi.update(c.close) {
                self.price_history.push((c.close, rsi));
                if self.price_history.len() > 50 {
                    self.price_history.remove(0);
                }
            }
            self.atr.update(c.high, c.low, c.close);
        }
        self.candles_processed = len;

        if !self.rsi.is_ready() || !self.atr.is_ready() {
            return None;
        }

        let current = candles.last()?;
        let price = current.close;
        let rsi = self.rsi.value()?;
        let atr = self.atr.value()?;

        let (bullish_div, bearish_div) = self.detect_divergence(price, rsi)?;

        let (signal, confidence, reason) = if bullish_div {
            (
                Signal::Buy,
                Decimal::new(70, 2),
                format!("Bullish RSI divergence detected at {:.2}", price),
            )
        } else if bearish_div {
            (
                Signal::Sell,
                Decimal::new(70, 2),
                format!("Bearish RSI divergence detected at {:.2}", price),
            )
        } else {
            (
                Signal::Neutral,
                Decimal::new(30, 2),
                "No divergence detected".to_string(),
            )
        };

        let entry = price;
        let (stop_loss, take_profit) = match signal {
            Signal::Buy => {
                let sl = entry - (atr * Decimal::from(3));
                let tp = entry + (atr * Decimal::from(6));
                (sl, tp)
            }
            Signal::Sell => {
                let sl = entry + (atr * Decimal::from(3));
                let tp = entry - (atr * Decimal::from(6));
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
        60
    }

    fn reset(&mut self) {
        self.rsi.reset();
        self.atr.reset();
        self.price_history.clear();
        self.candles_processed = 0;
    }
}
