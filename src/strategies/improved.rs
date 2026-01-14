use rust_decimal_macros::dec;
use crate::indicators::{ATR, EMA, RSI, MACD, Indicator};
use crate::types::{CandleBuffer, Signal, TradingPair};
use super::{Strategy, StrategySignal};

/// Mean Reversion Strategy
///
/// Buy when price is oversold but still in overall uptrend:
/// 1. Overall uptrend (price > EMA 100)
/// 2. Temporary pullback (price < EMA 20, or RSI oversold)
/// 3. Entry when momentum starts recovering
pub struct ImprovedStrategy {
    name: String,
    pair: TradingPair,

    // Trend (multi-timeframe)
    ema_20: EMA,
    ema_50: EMA,
    ema_100: EMA,

    // Momentum
    rsi: RSI,
    macd: MACD,

    // Volatility
    atr: ATR,

    // State
    candles_processed: usize,
    total_candles_seen: usize,  // Global counter that never resets
    last_signal_candle: usize,  // Uses total_candles_seen, not buffer index
}

impl ImprovedStrategy {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            name: format!("Conservative_{}", pair),
            pair,
            ema_20: EMA::new(20),
            ema_50: EMA::new(50),
            ema_100: EMA::new(100),
            rsi: RSI::new(14),
            macd: MACD::default_params(),
            atr: ATR::new(14),
            candles_processed: 0,
            total_candles_seen: 0,
            last_signal_candle: 0,
        }
    }
}

impl Strategy for ImprovedStrategy {
    fn name(&self) -> &str {
        &self.name
    }

    fn pair(&self) -> TradingPair {
        self.pair
    }

    fn analyze(&mut self, candles: &CandleBuffer) -> Option<StrategySignal> {
        let len = candles.len();
        if len < 120 {
            return None;
        }

        // Handle incremental updates - we only want to process NEW candles
        // The key insight is:
        // - candles_processed tracks how many candles we've seen in the current buffer
        // - When buffer is full (200), each new push replaces the oldest candle
        // - So if len == candles_processed and both are 200, we have 1 new candle at the end
        let (start, new_candles_count) = if self.candles_processed == 0 {
            // First time: process all candles
            (0, len)
        } else if self.candles_processed < len {
            // Buffer is still growing: process from where we left off
            (self.candles_processed, len - self.candles_processed)
        } else {
            // Buffer is full (len == candles_processed): exactly 1 new candle at the end
            // This happens when buffer size is maxed out and rotating
            (len - 1, 1)
        };

        // Update indicators with new candles
        for i in start..len {
            let c = &candles.candles[i];
            self.ema_20.update(c.close);
            self.ema_50.update(c.close);
            self.ema_100.update(c.close);
            self.rsi.update(c.close);
            self.macd.update(c.close);
            self.atr.update(c.high, c.low, c.close);
        }

        // Track buffer position and global candle count
        self.candles_processed = len;
        self.total_candles_seen += new_candles_count;

        // Ensure all indicators are ready
        if !self.ema_100.is_ready() || !self.rsi.is_ready() ||
           !self.macd.is_ready() || !self.atr.is_ready() {
            return None;
        }

        // Cooldown adjusted for timeframe
        // For 4-hour TF: 8 candles = ~1.3 days (ultra aggressive)
        // For 1-hour TF: 50 candles = ~2 days
        // For 5-min TF: 800 candles = ~66 hours
        let cooldown = 8;  // Ultra aggressive
        if self.last_signal_candle > 0 && (self.total_candles_seen - self.last_signal_candle) < cooldown {
            return None;
        }

        let current = candles.last()?;
        let price = current.close;
        let ema20 = self.ema_20.value()?;
        let ema50 = self.ema_50.value()?;
        let ema100 = self.ema_100.value()?;
        let rsi = self.rsi.value()?;
        let atr = self.atr.value()?;

        // TREND FOLLOWING WITH MOMENTUM
        // 1. Price above both EMAs (uptrend)
        let in_uptrend = price > ema50 && price > ema100;

        // 2. EMAs aligned (50 > 100)
        let trend_aligned = ema50 > ema100;

        // 3. RSI showing momentum but not extreme (40-70)
        let rsi_good = rsi >= dec!(40) && rsi <= dec!(70);

        // 4. MACD bullish
        let macd = &self.macd;
        let macd_bullish = macd.histogram_increasing() ||
                           self.macd.trend().map(|t| t.is_bullish()).unwrap_or(false);

        // Entry: all conditions met
        let buy_signal = in_uptrend && trend_aligned && rsi_good && macd_bullish;

        if !buy_signal {
            return None;
        }

        // Wide targets to capture big trends
        // Stop at 2 ATR below
        // Target at 5 ATR above (2.5:1 ratio)
        let sl_distance = atr * dec!(2.0);
        let tp_distance = atr * dec!(5.0);

        let stop_loss = price - sl_distance;
        let take_profit = price + tp_distance;

        self.last_signal_candle = self.total_candles_seen;

        let reason = format!(
            "Trend up, EMAs aligned, RSI {:.0}, MACD bull",
            rsi
        );

        Some(
            StrategySignal::new(&self.name, self.pair, Signal::Buy, dec!(0.70), &reason)
                .with_levels(price, stop_loss, take_profit)
        )
    }

    fn min_candles_required(&self) -> usize {
        120
    }

    fn reset(&mut self) {
        self.ema_20.reset();
        self.ema_50.reset();
        self.ema_100.reset();
        self.rsi.reset();
        self.macd.reset();
        self.atr.reset();
        self.candles_processed = 0;
        self.total_candles_seen = 0;
        self.last_signal_candle = 0;
    }
}

pub fn create_improved_strategy(pair: TradingPair) -> ImprovedStrategy {
    ImprovedStrategy::new(pair)
}
