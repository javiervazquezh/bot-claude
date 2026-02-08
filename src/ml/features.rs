use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::strategies::StrategySignal;
use crate::types::{CandleBuffer, Signal, TradingPair};

/// Fixed-size feature vector for ML prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeFeatures {
    pub signal_strength: f64,
    pub confidence: f64,
    pub risk_reward_ratio: f64,
    pub rsi_14: f64,
    pub atr_pct: f64,
    pub ema_spread_pct: f64,
    pub bb_position: f64,
    pub price_vs_200ema: f64,
    pub volume_ratio: f64,
    pub volatility_regime: f64,
    pub recent_win_rate: f64,
    pub recent_avg_pnl_pct: f64,
    pub streak: f64,
    pub hour_of_day: f64,
    pub day_of_week: f64,
    pub pair_id: f64,
}

impl TradeFeatures {
    pub const NUM_FEATURES: usize = 16;

    pub fn to_array(&self) -> [f64; Self::NUM_FEATURES] {
        [
            self.signal_strength,
            self.confidence,
            self.risk_reward_ratio,
            self.rsi_14,
            self.atr_pct,
            self.ema_spread_pct,
            self.bb_position,
            self.price_vs_200ema,
            self.volume_ratio,
            self.volatility_regime,
            self.recent_win_rate,
            self.recent_avg_pnl_pct,
            self.streak,
            self.hour_of_day,
            self.day_of_week,
            self.pair_id,
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
    macro_ema_value: Option<Decimal>,
) -> Option<TradeFeatures> {
    let current = candles.last()?;
    let price: f64 = current.close.try_into().ok()?;

    // Signal features
    let signal_strength = signal.signal.strength() as f64;
    let confidence: f64 = signal.confidence.try_into().unwrap_or(0.0);
    let risk_reward_ratio = signal.risk_reward_ratio()
        .and_then(|rr| rr.try_into().ok())
        .unwrap_or(0.0);

    // RSI from candle data (approximate from recent closes)
    let rsi_14 = approximate_rsi(candles, 14).unwrap_or(50.0);

    // ATR as % of price
    let atr_pct = approximate_atr_pct(candles, 14).unwrap_or(2.0);

    // EMA spread (fast vs slow)
    let ema_spread_pct = approximate_ema_spread(candles, 9, 21).unwrap_or(0.0);

    // Bollinger Band position (0 = lower, 1 = upper)
    let bb_position = approximate_bb_position(candles, 20).unwrap_or(0.5);

    // Price vs 200-EMA
    let price_vs_200ema = macro_ema_value
        .and_then(|v| {
            let v_f64: f64 = v.try_into().ok()?;
            if v_f64 > 0.0 { Some((price - v_f64) / v_f64 * 100.0) } else { None }
        })
        .unwrap_or(0.0);

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

    Some(TradeFeatures {
        signal_strength,
        confidence,
        risk_reward_ratio,
        rsi_14,
        atr_pct,
        ema_spread_pct,
        bb_position,
        price_vs_200ema,
        volume_ratio,
        volatility_regime,
        recent_win_rate,
        recent_avg_pnl_pct,
        streak,
        hour_of_day,
        day_of_week,
        pair_id: TradeFeatures::pair_to_id(signal.pair),
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

use chrono::Datelike;
use chrono::Timelike;
