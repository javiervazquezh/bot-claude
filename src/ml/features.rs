use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::strategies::StrategySignal;
use crate::types::{CandleBuffer, Signal, TradingPair};

/// Fixed-size feature vector for ML prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeFeatures {
    pub signal_strength: f64,
    pub confidence: f64,
    pub rsi_14: f64,
    pub atr_pct: f64,
    pub ema_spread_pct: f64,
    pub bb_position: f64,
    pub volume_ratio: f64,
    pub volatility_regime: f64,
    pub recent_win_rate: f64,
    pub recent_avg_pnl_pct: f64,
    pub streak: f64,
    pub hour_of_day: f64,
    pub day_of_week: f64,
    pub pair_id: f64,
    // Derived technical features
    pub macd_line: f64,
    pub macd_histogram: f64,
    pub stochastic_rsi_k: f64,
    pub mfi_14: f64,
    pub roc_10: f64,
    pub bb_width_pct: f64,
    pub atr_normalized_return: f64,
}

impl TradeFeatures {
    pub const NUM_FEATURES: usize = 21;

    pub fn to_array(&self) -> [f64; Self::NUM_FEATURES] {
        [
            self.signal_strength,
            self.confidence,
            self.rsi_14,
            self.atr_pct,
            self.ema_spread_pct,
            self.bb_position,
            self.volume_ratio,
            self.volatility_regime,
            self.recent_win_rate,
            self.recent_avg_pnl_pct,
            self.streak,
            self.hour_of_day,
            self.day_of_week,
            self.pair_id,
            self.macd_line,
            self.macd_histogram,
            self.stochastic_rsi_k,
            self.mfi_14,
            self.roc_10,
            self.bb_width_pct,
            self.atr_normalized_return,
        ]
    }

    fn pair_to_id(pair: TradingPair) -> f64 {
        match pair {
            TradingPair::BTCUSDT => 0.0,
            TradingPair::ETHUSDT => 1.0,
            TradingPair::SOLUSDT => 2.0,
            TradingPair::BNBUSDT => 3.0,
            TradingPair::ADAUSDT => 4.0,
            TradingPair::XRPUSDT => 5.0,
        }
    }
}

/// Recent trade outcome for computing performance features
#[derive(Debug, Clone)]
pub struct RecentTrade {
    pub is_win: bool,
    pub pnl_pct: f64,
}

/// Extract features from a signal and current market state
pub fn extract_features(
    signal: &StrategySignal,
    candles: &CandleBuffer,
    recent_trades: &[RecentTrade],
    _macro_ema_value: Option<Decimal>,
) -> Option<TradeFeatures> {
    let current = candles.last()?;

    // Signal features
    let signal_strength = signal.signal.strength() as f64;
    let confidence: f64 = signal.confidence.try_into().unwrap_or(0.0);

    // RSI from candle data (approximate from recent closes)
    let rsi_14 = approximate_rsi(candles, 14).unwrap_or(50.0);

    // ATR as % of price
    let atr_pct = approximate_atr_pct(candles, 14).unwrap_or(2.0);

    // EMA spread (fast vs slow)
    let ema_spread_pct = approximate_ema_spread(candles, 9, 21).unwrap_or(0.0);

    // Bollinger Band position (0 = lower, 1 = upper)
    let bb_position = approximate_bb_position(candles, 20).unwrap_or(0.5);

    // Volume ratio (current vs 20-period avg)
    let volume_ratio = approximate_volume_ratio(candles, 20).unwrap_or(1.0);

    // Volatility regime from ATR
    let volatility_regime = if atr_pct < 0.5 { 0.0 }
        else if atr_pct < 1.0 { 1.0 }
        else if atr_pct < 2.0 { 2.0 }
        else { 3.0 };

    // Recent performance features
    let (recent_win_rate, recent_avg_pnl_pct, streak) = compute_recent_stats(recent_trades);

    // Time features
    let hour_of_day = current.open_time.hour() as f64;
    let day_of_week = current.open_time.weekday().num_days_from_monday() as f64;

    // Derived technical features
    let (macd_line, macd_histogram) = approximate_macd(candles, 12, 26, 9)
        .unwrap_or((0.0, 0.0));
    let stochastic_rsi_k = approximate_stochastic_rsi(candles, 14, 14, 3)
        .unwrap_or(50.0);
    let mfi_14 = approximate_mfi(candles, 14).unwrap_or(50.0);
    let roc_10 = approximate_roc(candles, 10).unwrap_or(0.0);
    let bb_width_pct = approximate_bb_width_pct(candles, 20).unwrap_or(2.0);
    let atr_normalized_return = approximate_atr_normalized_return(candles, 14)
        .unwrap_or(0.0);

    Some(TradeFeatures {
        signal_strength,
        confidence,
        rsi_14,
        atr_pct,
        ema_spread_pct,
        bb_position,
        volume_ratio,
        volatility_regime,
        recent_win_rate,
        recent_avg_pnl_pct,
        streak,
        hour_of_day,
        day_of_week,
        pair_id: TradeFeatures::pair_to_id(signal.pair),
        macd_line,
        macd_histogram,
        stochastic_rsi_k,
        mfi_14,
        roc_10,
        bb_width_pct,
        atr_normalized_return,
    })
}

fn approximate_rsi(candles: &CandleBuffer, period: usize) -> Option<f64> {
    if candles.len() < period + 1 { return None; }
    let recent = candles.last_n(period + 1);
    let mut gains = 0.0;
    let mut losses = 0.0;
    for i in 1..recent.len() {
        let change: f64 = (recent[i].close - recent[i - 1].close).try_into().unwrap_or(0.0);
        if change > 0.0 { gains += change; } else { losses += -change; }
    }
    let avg_gain = gains / period as f64;
    let avg_loss = losses / period as f64;
    if avg_loss == 0.0 { return Some(100.0); }
    let rs = avg_gain / avg_loss;
    Some(100.0 - (100.0 / (1.0 + rs)))
}

fn approximate_atr_pct(candles: &CandleBuffer, period: usize) -> Option<f64> {
    if candles.len() < period { return None; }
    let recent = candles.last_n(period);
    let price: f64 = recent.last()?.close.try_into().ok()?;
    if price == 0.0 { return None; }
    let avg_range: f64 = recent.iter()
        .map(|c| {
            let h: f64 = c.high.try_into().unwrap_or(0.0);
            let l: f64 = c.low.try_into().unwrap_or(0.0);
            h - l
        })
        .sum::<f64>() / period as f64;
    Some(avg_range / price * 100.0)
}

fn approximate_ema_spread(candles: &CandleBuffer, fast: usize, slow: usize) -> Option<f64> {
    if candles.len() < slow { return None; }
    let recent = candles.last_n(slow);
    let closes: Vec<f64> = recent.iter()
        .map(|c| c.close.try_into().unwrap_or(0.0))
        .collect();
    let fast_avg: f64 = closes[closes.len() - fast..].iter().sum::<f64>() / fast as f64;
    let slow_avg: f64 = closes.iter().sum::<f64>() / slow as f64;
    if slow_avg == 0.0 { return None; }
    Some((fast_avg - slow_avg) / slow_avg * 100.0)
}

fn approximate_bb_position(candles: &CandleBuffer, period: usize) -> Option<f64> {
    if candles.len() < period { return None; }
    let recent = candles.last_n(period);
    let closes: Vec<f64> = recent.iter()
        .map(|c| c.close.try_into().unwrap_or(0.0))
        .collect();
    let mean = closes.iter().sum::<f64>() / period as f64;
    let variance = closes.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / period as f64;
    let std_dev = variance.sqrt();
    let upper = mean + 2.0 * std_dev;
    let lower = mean - 2.0 * std_dev;
    let price = *closes.last()?;
    let band_width = upper - lower;
    if band_width == 0.0 { return Some(0.5); }
    Some(((price - lower) / band_width).clamp(0.0, 1.0))
}

fn approximate_volume_ratio(candles: &CandleBuffer, period: usize) -> Option<f64> {
    if candles.len() < period { return None; }
    let recent = candles.last_n(period);
    let avg_vol: f64 = recent.iter()
        .map(|c| c.volume.try_into().unwrap_or(0.0f64))
        .sum::<f64>() / period as f64;
    let current_vol: f64 = recent.last()?.volume.try_into().ok()?;
    if avg_vol == 0.0 { return Some(1.0); }
    Some(current_vol / avg_vol)
}

fn compute_recent_stats(recent_trades: &[RecentTrade]) -> (f64, f64, f64) {
    if recent_trades.is_empty() {
        return (0.5, 0.0, 0.0);
    }
    let n = recent_trades.len() as f64;
    let wins = recent_trades.iter().filter(|t| t.is_win).count() as f64;
    let win_rate = wins / n;
    let avg_pnl = recent_trades.iter().map(|t| t.pnl_pct).sum::<f64>() / n;

    // Compute streak
    let mut streak = 0.0f64;
    for trade in recent_trades.iter().rev() {
        if trade.is_win {
            if streak >= 0.0 { streak += 1.0; } else { break; }
        } else {
            if streak <= 0.0 { streak -= 1.0; } else { break; }
        }
    }

    (win_rate, avg_pnl, streak)
}

/// MACD(fast, slow, signal) — returns (macd_line_pct, histogram_pct) as % of price
fn approximate_macd(candles: &CandleBuffer, fast: usize, slow: usize, signal_period: usize) -> Option<(f64, f64)> {
    let needed = slow + signal_period;
    if candles.len() < needed { return None; }
    let recent = candles.last_n(needed);
    let closes: Vec<f64> = recent.iter()
        .map(|c| c.close.try_into().unwrap_or(0.0))
        .collect();
    let price = *closes.last()?;
    if price == 0.0 { return None; }

    // Compute EMA using exponential smoothing
    let ema = |data: &[f64], period: usize| -> Vec<f64> {
        let k = 2.0 / (period as f64 + 1.0);
        let mut ema_vals = Vec::with_capacity(data.len());
        ema_vals.push(data[..period].iter().sum::<f64>() / period as f64); // seed with SMA
        for i in period..data.len() {
            let prev = *ema_vals.last().unwrap();
            ema_vals.push(data[i] * k + prev * (1.0 - k));
        }
        ema_vals
    };

    let ema_fast = ema(&closes, fast);
    let ema_slow = ema(&closes, slow);

    // MACD line = EMA_fast - EMA_slow (aligned to end)
    let fast_offset = slow - fast; // ema_fast starts earlier
    let macd_len = ema_slow.len().min(ema_fast.len() - fast_offset);
    let macd_line: Vec<f64> = (0..macd_len)
        .map(|i| ema_fast[i + fast_offset] - ema_slow[i])
        .collect();

    if macd_line.len() < signal_period { return None; }

    // Signal line = EMA of MACD line
    let signal_ema = ema(&macd_line, signal_period);
    let last_macd = *macd_line.last()?;
    let last_signal = *signal_ema.last()?;
    let histogram = last_macd - last_signal;

    Some((last_macd / price * 100.0, histogram / price * 100.0))
}

/// StochasticRSI %K — RSI applied to RSI, then stochastic normalized (0-100)
fn approximate_stochastic_rsi(candles: &CandleBuffer, rsi_period: usize, stoch_period: usize, k_smooth: usize) -> Option<f64> {
    let needed = rsi_period + stoch_period + k_smooth + 1;
    if candles.len() < needed { return None; }
    let recent = candles.last_n(needed);
    let closes: Vec<f64> = recent.iter()
        .map(|c| c.close.try_into().unwrap_or(0.0))
        .collect();

    // Compute RSI series
    let mut rsi_series = Vec::new();
    for end in (rsi_period + 1)..=closes.len() {
        let window = &closes[end - rsi_period - 1..end];
        let mut gains = 0.0;
        let mut losses = 0.0;
        for i in 1..window.len() {
            let change = window[i] - window[i - 1];
            if change > 0.0 { gains += change; } else { losses += -change; }
        }
        let avg_gain = gains / rsi_period as f64;
        let avg_loss = losses / rsi_period as f64;
        let rsi = if avg_loss == 0.0 { 100.0 } else {
            100.0 - (100.0 / (1.0 + avg_gain / avg_loss))
        };
        rsi_series.push(rsi);
    }

    if rsi_series.len() < stoch_period + k_smooth { return None; }

    // Stochastic of RSI
    let mut raw_k_series = Vec::new();
    for end in stoch_period..=rsi_series.len() {
        let window = &rsi_series[end - stoch_period..end];
        let min_rsi = window.iter().cloned().fold(f64::MAX, f64::min);
        let max_rsi = window.iter().cloned().fold(f64::MIN, f64::max);
        let range = max_rsi - min_rsi;
        let raw_k = if range == 0.0 { 50.0 } else {
            (window.last().unwrap() - min_rsi) / range * 100.0
        };
        raw_k_series.push(raw_k);
    }

    if raw_k_series.len() < k_smooth { return None; }

    // Smooth %K with SMA
    let k: f64 = raw_k_series[raw_k_series.len() - k_smooth..].iter().sum::<f64>() / k_smooth as f64;
    Some(k.clamp(0.0, 100.0))
}

/// Money Flow Index (0-100) — volume-weighted RSI
fn approximate_mfi(candles: &CandleBuffer, period: usize) -> Option<f64> {
    if candles.len() < period + 1 { return None; }
    let recent = candles.last_n(period + 1);

    let mut positive_flow = 0.0;
    let mut negative_flow = 0.0;

    for i in 1..recent.len() {
        let tp_curr = {
            let h: f64 = recent[i].high.try_into().unwrap_or(0.0);
            let l: f64 = recent[i].low.try_into().unwrap_or(0.0);
            let c: f64 = recent[i].close.try_into().unwrap_or(0.0);
            (h + l + c) / 3.0
        };
        let tp_prev = {
            let h: f64 = recent[i - 1].high.try_into().unwrap_or(0.0);
            let l: f64 = recent[i - 1].low.try_into().unwrap_or(0.0);
            let c: f64 = recent[i - 1].close.try_into().unwrap_or(0.0);
            (h + l + c) / 3.0
        };
        let vol: f64 = recent[i].volume.try_into().unwrap_or(0.0);
        let raw_money_flow = tp_curr * vol;

        if tp_curr > tp_prev {
            positive_flow += raw_money_flow;
        } else {
            negative_flow += raw_money_flow;
        }
    }

    if negative_flow == 0.0 { return Some(100.0); }
    let money_ratio = positive_flow / negative_flow;
    Some(100.0 - (100.0 / (1.0 + money_ratio)))
}

/// Rate of change over N periods (%)
fn approximate_roc(candles: &CandleBuffer, period: usize) -> Option<f64> {
    if candles.len() < period + 1 { return None; }
    let recent = candles.last_n(period + 1);
    let close_now: f64 = recent.last()?.close.try_into().ok()?;
    let close_ago: f64 = recent.first()?.close.try_into().ok()?;
    if close_ago == 0.0 { return None; }
    Some((close_now - close_ago) / close_ago * 100.0)
}

/// Bollinger Band width as % of middle band
fn approximate_bb_width_pct(candles: &CandleBuffer, period: usize) -> Option<f64> {
    if candles.len() < period { return None; }
    let recent = candles.last_n(period);
    let closes: Vec<f64> = recent.iter()
        .map(|c| c.close.try_into().unwrap_or(0.0))
        .collect();
    let mean = closes.iter().sum::<f64>() / period as f64;
    if mean == 0.0 { return None; }
    let variance = closes.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / period as f64;
    let std_dev = variance.sqrt();
    let width = 4.0 * std_dev; // upper - lower = 2*2*std_dev
    Some(width / mean * 100.0)
}

/// ATR-normalized return: (close - prev_close) / ATR
fn approximate_atr_normalized_return(candles: &CandleBuffer, atr_period: usize) -> Option<f64> {
    if candles.len() < atr_period + 1 { return None; }
    let recent = candles.last_n(atr_period + 1);
    let close_now: f64 = recent.last()?.close.try_into().ok()?;
    let close_prev: f64 = recent[recent.len() - 2].close.try_into().ok()?;

    // ATR over last N candles
    let avg_range: f64 = recent[1..].iter()
        .map(|c| {
            let h: f64 = c.high.try_into().unwrap_or(0.0);
            let l: f64 = c.low.try_into().unwrap_or(0.0);
            h - l
        })
        .sum::<f64>() / atr_period as f64;

    if avg_range == 0.0 { return None; }
    Some((close_now - close_prev) / avg_range)
}

use chrono::Datelike;
use chrono::Timelike;
