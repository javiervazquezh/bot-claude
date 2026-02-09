use anyhow::Result;
use ndarray::Array2;

use crate::types::CandleBuffer;

/// Extract regime features from candle data
/// Returns 8 features normalized for HMM input:
/// 1. Log returns (normalized)
/// 2. Volatility (20-period std of returns)
/// 3. Volume ratio (current vs 20-period avg)
/// 4. RSI (normalized to [-1, 1])
/// 5. EMA spread % (9 vs 21)
/// 6. MACD histogram (normalized)
/// 7. Price momentum (5-period % change)
/// 8. Volume momentum (5-period % change)
pub fn extract_regime_features(candles: &CandleBuffer) -> Result<Vec<f64>> {
    if candles.len() < 30 {
        return Err(anyhow::anyhow!("Need at least 30 candles for regime features"));
    }

    let recent = candles.last_n(30);

    // 1. Log returns
    let last_close: f64 = recent.last().unwrap().close.try_into().unwrap_or(0.0);
    let prev_close: f64 = recent[recent.len() - 2].close.try_into().unwrap_or(last_close);
    let log_return = if prev_close > 0.0 {
        (last_close / prev_close).ln()
    } else {
        0.0
    };

    // 2. Volatility (20-period std of returns)
    let mut returns = Vec::new();
    for i in 1..recent.len().min(21) {
        let curr: f64 = recent[recent.len() - i].close.try_into().unwrap_or(0.0);
        let prev: f64 = recent[recent.len() - i - 1].close.try_into().unwrap_or(curr);
        if prev > 0.0 {
            returns.push((curr / prev).ln());
        }
    }
    let volatility = if returns.len() > 1 {
        let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance: f64 = returns.iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>() / returns.len() as f64;
        variance.sqrt()
    } else {
        0.0
    };

    // 3. Volume ratio (current vs 20-period avg)
    let current_vol: f64 = recent.last().unwrap().volume.try_into().unwrap_or(0.0);
    let avg_vol: f64 = recent.iter()
        .rev()
        .take(20)
        .map(|c| c.volume.try_into().unwrap_or(0.0))
        .sum::<f64>() / 20.0;
    let volume_ratio = if avg_vol > 0.0 {
        (current_vol / avg_vol - 1.0).max(-1.0).min(1.0) // Clamp to [-1, 1]
    } else {
        0.0
    };

    // 4. RSI (normalized to [-1, 1])
    let rsi = calculate_rsi(&recent, 14);
    let rsi_normalized = (rsi - 50.0) / 50.0; // Convert 0-100 to [-1, 1]

    // 5. EMA spread % (9 vs 21)
    let ema_spread = calculate_ema_spread(&recent, 9, 21);

    // 6. MACD histogram (normalized)
    let macd_hist = calculate_macd_histogram(&recent);

    // 7. Price momentum (5-period % change)
    let price_momentum = if recent.len() >= 6 {
        let current: f64 = recent.last().unwrap().close.try_into().unwrap_or(0.0);
        let past: f64 = recent[recent.len() - 6].close.try_into().unwrap_or(current);
        if past > 0.0 {
            ((current - past) / past).max(-0.5).min(0.5) * 2.0 // Normalize to ~[-1, 1]
        } else {
            0.0
        }
    } else {
        0.0
    };

    // 8. Volume momentum (5-period % change)
    let volume_momentum = if recent.len() >= 6 {
        let current_vol: f64 = recent.last().unwrap().volume.try_into().unwrap_or(0.0);
        let past_vol: f64 = recent[recent.len() - 6].volume.try_into().unwrap_or(current_vol);
        if past_vol > 0.0 {
            ((current_vol - past_vol) / past_vol).max(-0.5).min(0.5) * 2.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    Ok(vec![
        log_return * 100.0, // Scale up for better HMM learning
        volatility * 100.0,
        volume_ratio,
        rsi_normalized,
        ema_spread,
        macd_hist,
        price_momentum,
        volume_momentum,
    ])
}

/// Extract features for multiple time periods (creates observation matrix)
pub fn extract_regime_features_batch(candles: &CandleBuffer, window_size: usize) -> Result<Array2<f64>> {
    if candles.len() < window_size + 30 {
        return Err(anyhow::anyhow!("Not enough candles for batch feature extraction"));
    }

    let mut observations = Vec::new();

    // Extract features for each window
    for i in 0..(candles.len() - 30) {
        let mut window_candles = CandleBuffer::new(30);
        for candle in &candles.candles[i..i + 30] {
            window_candles.push(candle.clone());
        }

        if let Ok(features) = extract_regime_features(&window_candles) {
            observations.push(features);
        }
    }

    if observations.is_empty() {
        return Err(anyhow::anyhow!("No valid observations extracted"));
    }

    let n_obs = observations.len();
    let n_features = observations[0].len();

    let mut data = Array2::zeros((n_obs, n_features));
    for (i, obs) in observations.iter().enumerate() {
        for (j, &val) in obs.iter().enumerate() {
            data[[i, j]] = val;
        }
    }

    Ok(data)
}

fn calculate_rsi(candles: &[crate::types::Candle], period: usize) -> f64 {
    if candles.len() < period + 1 {
        return 50.0; // Neutral
    }

    let recent = &candles[candles.len() - period - 1..];
    let mut gains = 0.0;
    let mut losses = 0.0;

    for i in 1..recent.len() {
        let change: f64 = (recent[i].close - recent[i - 1].close)
            .try_into()
            .unwrap_or(0.0);
        if change > 0.0 {
            gains += change;
        } else {
            losses += -change;
        }
    }

    let avg_gain = gains / period as f64;
    let avg_loss = losses / period as f64;

    if avg_loss == 0.0 {
        return 100.0;
    }

    let rs = avg_gain / avg_loss;
    100.0 - (100.0 / (1.0 + rs))
}

fn calculate_ema_spread(candles: &[crate::types::Candle], fast: usize, slow: usize) -> f64 {
    if candles.len() < slow {
        return 0.0;
    }

    let closes: Vec<f64> = candles.iter()
        .map(|c| c.close.try_into().unwrap_or(0.0))
        .collect();

    // Simple moving average as approximation
    let fast_avg: f64 = closes[closes.len() - fast..].iter().sum::<f64>() / fast as f64;
    let slow_avg: f64 = closes[closes.len() - slow..].iter().sum::<f64>() / slow as f64;

    if slow_avg > 0.0 {
        ((fast_avg - slow_avg) / slow_avg).max(-0.2).min(0.2) * 5.0 // Normalize to ~[-1, 1]
    } else {
        0.0
    }
}

fn calculate_macd_histogram(candles: &[crate::types::Candle]) -> f64 {
    if candles.len() < 26 {
        return 0.0;
    }

    let closes: Vec<f64> = candles.iter()
        .map(|c| c.close.try_into().unwrap_or(0.0))
        .collect();

    // Simple EMA approximation
    let ema12: f64 = closes[closes.len() - 12..].iter().sum::<f64>() / 12.0;
    let ema26: f64 = closes[closes.len() - 26..].iter().sum::<f64>() / 26.0;
    let macd = ema12 - ema26;

    // Signal line (9-period EMA of MACD - simplified)
    let signal = macd; // Simplified - in real implementation would track MACD history

    let histogram = macd - signal;

    // Normalize
    let current_price = closes.last().unwrap_or(&1.0);
    if *current_price > 0.0 {
        (histogram / current_price * 1000.0).max(-1.0).min(1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Candle, TradingPair, TimeFrame};
    use chrono::Utc;
    use rust_decimal::Decimal;

    fn create_test_candles(n: usize) -> CandleBuffer {
        let mut buffer = CandleBuffer::new(n);
        let base_price = Decimal::from(50000);

        for i in 0..n {
            buffer.push(Candle {
                pair: TradingPair::BTCUSDT,
                timeframe: TimeFrame::H1,
                open_time: Utc::now(),
                close_time: Utc::now(),
                open: base_price + Decimal::from(i as i64 * 10),
                high: base_price + Decimal::from(i as i64 * 15),
                low: base_price + Decimal::from(i as i64 * 5),
                close: base_price + Decimal::from(i as i64 * 10),
                volume: Decimal::from(100),
                quote_volume: Decimal::from(5000000),
                trades: 1000,
                is_closed: true,
            });
        }

        buffer
    }

    #[test]
    fn test_extract_regime_features() {
        let candles = create_test_candles(50);
        let features = extract_regime_features(&candles).unwrap();

        assert_eq!(features.len(), 8);
        // All features should be finite
        for feat in features {
            assert!(feat.is_finite());
        }
    }

    #[test]
    fn test_extract_regime_features_batch() {
        let candles = create_test_candles(100);
        let batch = extract_regime_features_batch(&candles, 50).unwrap();

        assert!(batch.shape()[0] > 0); // Has observations
        assert_eq!(batch.shape()[1], 8); // 8 features
    }
}
