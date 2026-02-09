#![allow(dead_code)]
use anyhow::{anyhow, Result};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use tracing::{debug, info};
use uuid::Uuid;

use crate::exchange::BinanceClient;
use crate::ml::features::{self, TradeFeatures, RecentTrade};
use crate::strategies::{create_strategies_for_pair, Strategy};
use crate::types::{Candle, CandleBuffer, Signal, TimeFrame, TradingPair};

/// Configuration for signal collection
#[derive(Debug, Clone)]
pub struct SignalCollectionConfig {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub timeframe: TimeFrame,
    pub pairs: Vec<TradingPair>,
    pub lookahead_candles: usize,
    pub win_threshold_pct: f64,
}

/// A single signal record with features and forward return
#[derive(Debug, Clone)]
pub struct SignalRecord {
    pub signal_id: String,
    pub pair: TradingPair,
    pub timestamp: DateTime<Utc>,
    pub signal_type: Signal,
    pub confidence: Decimal,
    pub features: TradeFeatures,
    pub entry_price: Decimal,
    pub lookahead_price: Decimal,
    pub forward_return_pct: f64,
    pub is_win: bool,
}

/// Signal collector that processes M1 candles and collects all Buy/StrongBuy signals
pub struct SignalCollector {
    pub config: SignalCollectionConfig,
    exchange: BinanceClient,
    strategies: HashMap<TradingPair, Box<dyn Strategy>>,
    candle_buffers: HashMap<TradingPair, CandleBuffer>,
    all_candles: HashMap<TradingPair, Vec<Candle>>,
    collected_signals: Vec<SignalRecord>,
}

impl SignalCollector {
    pub fn new(config: SignalCollectionConfig) -> Self {
        let mut strategies: HashMap<TradingPair, Box<dyn Strategy>> = HashMap::new();
        let mut candle_buffers = HashMap::new();

        for &pair in &config.pairs {
            strategies.insert(pair, Box::new(create_strategies_for_pair(pair)));
            candle_buffers.insert(pair, CandleBuffer::new(500));
        }

        Self {
            config,
            exchange: BinanceClient::public_only(),
            strategies,
            candle_buffers,
            all_candles: HashMap::new(),
            collected_signals: Vec::new(),
        }
    }

    /// Run signal collection - fetch candles, process signals, compute forward returns
    pub async fn run(&mut self) -> Result<Vec<SignalRecord>> {
        info!("Starting signal collection on {} timeframe", self.config.timeframe.as_str());
        info!("Date range: {} to {}", self.config.start_date, self.config.end_date);
        info!("Pairs: {:?}", self.config.pairs);

        // Convert dates to DateTime
        let start_time = self.config.start_date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("Invalid start date"))?
            .and_utc();
        let end_time = self.config.end_date
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| anyhow!("Invalid end date"))?
            .and_utc();

        // Fetch all candles for all pairs
        info!("Fetching historical candles...");
        for &pair in &self.config.pairs {
            info!("  Fetching {} {} candles...", pair.as_str(), self.config.timeframe.as_str());
            let candles = self.exchange.get_historical_candles(
                pair,
                self.config.timeframe,
                start_time,
                end_time,
            ).await?;

            info!("    Fetched {} candles", candles.len());
            self.all_candles.insert(pair, candles);
        }

        // Process candles and collect signals
        info!("Processing signals...");
        let pairs = self.config.pairs.clone(); // Clone to avoid borrow checker issue
        for &pair in &pairs {
            let total_candles = self.all_candles[&pair].len();
            let process_until = total_candles.saturating_sub(self.config.lookahead_candles);

            info!("  Processing {} (will process {} of {} candles, reserving {} for lookahead)",
                pair.as_str(), process_until, total_candles, self.config.lookahead_candles);

            for idx in 0..process_until {
                self.process_candle_batch(pair, idx)?;

                // Progress logging every 10000 candles
                if idx > 0 && idx % 10000 == 0 {
                    info!("    Processed {}/{} candles ({} signals collected)",
                        idx, process_until, self.collected_signals.len());
                }
            }

            let pair_signals = self.collected_signals.iter()
                .filter(|s| s.pair == pair)
                .count();
            info!("  Completed {}: {} signals collected", pair.as_str(), pair_signals);
        }

        info!("Signal collection complete: {} total signals", self.collected_signals.len());
        Ok(self.collected_signals.clone())
    }

    /// Process a single candle - generate signal, extract features, record if Buy/StrongBuy
    fn process_candle_batch(&mut self, pair: TradingPair, candle_idx: usize) -> Result<()> {
        let candle = self.all_candles[&pair][candle_idx].clone();

        // Update candle buffer for indicator computation
        self.candle_buffers.get_mut(&pair).unwrap().push(candle.clone());

        // Get strategy and analyze
        let strategy = self.strategies.get_mut(&pair).unwrap();
        let buffer = self.candle_buffers.get(&pair).unwrap();

        // Skip if buffer not warmed up
        if buffer.len() < strategy.min_candles_required() {
            return Ok(());
        }

        // Generate signal
        if let Some(signal) = strategy.analyze(buffer) {
            // Only collect Buy/StrongBuy signals
            if matches!(signal.signal, Signal::Buy | Signal::StrongBuy) {
                // Extract features (empty recent_trades since we're not tracking real trades)
                let empty_trades: Vec<RecentTrade> = vec![];
                if let Some(features) = features::extract_features(&signal, buffer, &empty_trades, None) {
                    // Compute forward return
                    if let Some((lookahead_price, forward_return_pct, is_win)) =
                        self.compute_forward_return(pair, candle_idx, candle.close)
                    {
                        let record = SignalRecord {
                            signal_id: Uuid::new_v4().to_string(),
                            pair,
                            timestamp: candle.open_time,
                            signal_type: signal.signal,
                            confidence: signal.confidence,
                            features,
                            entry_price: candle.close,
                            lookahead_price,
                            forward_return_pct,
                            is_win,
                        };

                        debug!("Collected signal: {} {:?} at ${:.2} (forward return: {:.2}%, win: {})",
                            pair.as_str(), signal.signal, candle.close,
                            forward_return_pct, is_win);

                        self.collected_signals.push(record);
                    }
                }
            }
        }

        Ok(())
    }

    /// Compute forward return by looking ahead N candles and finding highest price
    fn compute_forward_return(
        &self,
        pair: TradingPair,
        signal_idx: usize,
        entry_price: Decimal,
    ) -> Option<(Decimal, f64, bool)> {
        let candles = &self.all_candles[&pair];
        let lookahead_end = signal_idx + self.config.lookahead_candles;

        if lookahead_end >= candles.len() {
            return None; // Not enough lookahead data
        }

        // Find highest price in lookahead window
        let max_price = candles[signal_idx + 1..=lookahead_end]
            .iter()
            .map(|c| c.high)
            .max()
            .unwrap_or(entry_price);

        // Compute forward return
        let entry_f64: f64 = entry_price.try_into().unwrap_or(0.0);
        let max_f64: f64 = max_price.try_into().unwrap_or(0.0);

        if entry_f64 == 0.0 {
            return None;
        }

        let forward_return_pct = (max_f64 - entry_f64) / entry_f64 * 100.0;
        let is_win = forward_return_pct > self.config.win_threshold_pct;

        Some((max_price, forward_return_pct, is_win))
    }

    /// Export collected signals to CSV
    pub fn export_to_csv(&self, output_path: &str) -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(output_path)?;

        // Header: metadata + 21 features
        writeln!(
            file,
            "signal_id,timestamp,pair,signal_type,confidence,entry_price,lookahead_price,forward_return_pct,is_win,signal_strength,confidence,rsi_14,atr_pct,ema_spread_pct,bb_position,volume_ratio,volatility_regime,recent_win_rate,recent_avg_pnl_pct,streak,hour_of_day,day_of_week,pair_id,macd_line,macd_histogram,stochastic_rsi_k,mfi_14,roc_10,bb_width_pct,atr_normalized_return"
        )?;

        // Data rows
        for record in &self.collected_signals {
            let feat_arr = record.features.to_array();
            let feat_str: Vec<String> = feat_arr.iter().map(|v| format!("{:.6}", v)).collect();

            writeln!(
                file,
                "{},{},{},{},{:.4},{:.8},{:.8},{:.6},{},{}",
                record.signal_id,
                record.timestamp.format("%Y-%m-%d %H:%M:%S"),
                record.pair.as_str(),
                format!("{:?}", record.signal_type),
                record.confidence,
                record.entry_price,
                record.lookahead_price,
                record.forward_return_pct,
                if record.is_win { 1 } else { 0 },
                feat_str.join(",")
            )?;
        }

        info!("Exported {} signal records to {}", self.collected_signals.len(), output_path);

        let wins = self.collected_signals.iter().filter(|r| r.is_win).count();
        let losses = self.collected_signals.len() - wins;
        let win_rate = wins as f64 / self.collected_signals.len() as f64 * 100.0;

        info!("  Wins: {} ({:.1}%)", wins, win_rate);
        info!("  Losses: {} ({:.1}%)", losses, 100.0 - win_rate);

        Ok(())
    }

    pub fn signal_count(&self) -> usize {
        self.collected_signals.len()
    }
}
