use anyhow::Result;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::exchange::BinanceClient;
use crate::ml::{TradeFeatures, TradePredictor, OutcomeTracker};
use crate::ml::features::{self, RecentTrade};
use crate::strategies::{Strategy, StrategySignal, create_improved_strategy, create_strategies_for_pair};
use crate::strategies::combined::CombinedStrategy;
use crate::types::{Candle, CandleBuffer, Position, PositionStatus, Side, Signal, TimeFrame, TradingPair};

use super::results::{BacktestResults, EquityPoint, ExitReason, MetricsCalculator, TradeRecord, WalkForwardResult, WindowResult};
use super::Portfolio;

/// Configuration for backtesting
#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub initial_capital: Decimal,
    pub timeframe: TimeFrame,
    pub pairs: Vec<TradingPair>,
    pub fee_rate: Decimal,
    pub slippage_rate: Decimal,
    pub min_confidence: Decimal,
    pub min_risk_reward: Decimal,
    pub risk_per_trade: Decimal,      // Risk % per trade (e.g., 0.05 for 5%, 0.12 for 12%)
    pub max_allocation: Decimal,      // Max % of capital per position (e.g., 0.60 for 60%, 0.90 for 90%)
    pub max_correlated_positions: usize, // Max positions in same correlation group
    pub max_drawdown_pct: Decimal,           // Emergency stop drawdown threshold
    pub walk_forward_windows: Option<usize>, // None = standard backtest, Some(n) = n windows
    pub walk_forward_oos_pct: Decimal,       // Out-of-sample percentage (default 0.25)
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            start_date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
            initial_capital: dec!(2000),
            timeframe: TimeFrame::M5,
            pairs: TradingPair::all(),
            fee_rate: dec!(0.001),      // 0.1%
            slippage_rate: dec!(0.0005), // 0.05%
            min_confidence: dec!(0.65),  // Higher threshold for mixed markets
            min_risk_reward: dec!(2.0),  // Require good R:R for quality trades
            risk_per_trade: dec!(0.05),  // Conservative 5% risk per trade
            max_allocation: dec!(0.60),  // Conservative 60% max allocation per position
            max_correlated_positions: 2, // Max 2 positions in same correlation group
            max_drawdown_pct: dec!(15),  // Emergency stop at 15% drawdown
            walk_forward_windows: None,  // Standard backtest by default
            walk_forward_oos_pct: dec!(0.25), // 25% out-of-sample
        }
    }
}

/// Core backtesting engine
pub struct BacktestEngine {
    config: BacktestConfig,
    exchange: BinanceClient,
    portfolio: Portfolio,
    strategies: HashMap<TradingPair, Box<dyn Strategy>>,
    candle_buffers: HashMap<TradingPair, CandleBuffer>,
    current_prices: HashMap<TradingPair, Decimal>,
    equity_curve: Vec<EquityPoint>,
    trades: Vec<TradeRecord>,
    total_fees: Decimal,
    candles_processed: u64,
    last_equity_date: Option<NaiveDate>,
    first_prices: HashMap<TradingPair, Decimal>,
    atr_indicators: HashMap<TradingPair, crate::indicators::atr::ATR>,
    emergency_stopped: bool,
    /// Cooldown: candle number of last exit per pair (to prevent churn after stop-loss)
    last_exit_candle: HashMap<TradingPair, u64>,
    /// Whether the last exit for a pair was a stop-loss (cooldown only applies after losses)
    last_exit_was_stoploss: HashMap<TradingPair, bool>,
    /// ML trade predictor for signal quality filtering
    trade_predictor: TradePredictor,
    /// ML outcome tracker for collecting training data
    outcome_tracker: OutcomeTracker,
    /// Number of trades since last ML retrain
    trades_since_retrain: usize,
    /// ML retrain interval (every N completed trades)
    retrain_interval: usize,
    /// Maps pair → ML trade_id for matching entry features to exit outcomes
    ml_trade_ids: HashMap<TradingPair, String>,
    /// Volatility targeting: rolling daily returns for realized vol calculation
    daily_returns_window: Vec<f64>,
    /// Previous day's ending equity (for computing daily returns)
    prev_day_equity: Option<Decimal>,
    /// Current volatility scaling factor (adjusts position sizes to target constant vol)
    vol_scale: Decimal,
}

impl BacktestEngine {
    /// Create engine with CombinedStrategy (matches live trading path)
    pub fn new(config: BacktestConfig) -> Self {
        Self::with_strategy_factory(config, |pair| Box::new(create_strategies_for_pair(pair)))
    }

    /// Create engine with ImprovedStrategy (legacy backtest path)
    pub fn with_improved_strategy(config: BacktestConfig) -> Self {
        Self::with_strategy_factory(config, |pair| Box::new(create_improved_strategy(pair)))
    }

    fn with_strategy_factory(
        config: BacktestConfig,
        factory: impl Fn(TradingPair) -> Box<dyn Strategy>,
    ) -> Self {
        let portfolio = Portfolio::new(config.initial_capital);

        let mut strategies = HashMap::new();
        let mut candle_buffers = HashMap::new();
        let mut current_prices = HashMap::new();

        let mut atr_indicators = HashMap::new();
        for pair in &config.pairs {
            strategies.insert(*pair, factory(*pair));
            candle_buffers.insert(*pair, CandleBuffer::new(500));
            current_prices.insert(*pair, Decimal::ZERO);
            atr_indicators.insert(*pair, crate::indicators::atr::ATR::new(28));
        }

        Self {
            config,
            exchange: BinanceClient::public_only(),
            portfolio,
            strategies,
            candle_buffers,
            current_prices,
            equity_curve: Vec::new(),
            trades: Vec::new(),
            total_fees: Decimal::ZERO,
            candles_processed: 0,
            last_equity_date: None,
            first_prices: HashMap::new(),
            atr_indicators,
            emergency_stopped: false,
            last_exit_candle: HashMap::new(),
            last_exit_was_stoploss: HashMap::new(),
            trade_predictor: TradePredictor::new(),
            outcome_tracker: OutcomeTracker::new(),
            trades_since_retrain: 0,
            retrain_interval: 20,
            ml_trade_ids: HashMap::new(),
            daily_returns_window: Vec::new(),
            prev_day_equity: None,
            vol_scale: Decimal::ONE,
        }
    }

    /// Main entry point for running backtest
    pub async fn run(&mut self) -> Result<BacktestResults> {
        info!(
            "Starting backtest: {} to {} with ${:.2}",
            self.config.start_date, self.config.end_date, self.config.initial_capital
        );

        // 1. Fetch all historical data
        let historical_data = self.fetch_all_historical_data().await?;

        // 2. Merge and sort candles chronologically
        let timeline = self.create_timeline(&historical_data);
        info!("Processing {} candles in timeline", timeline.len());

        // 3. Process each candle in order
        for candle in timeline {
            self.process_candle(candle)?;
        }

        // 4. Close any remaining positions at end
        self.close_all_positions()?;

        // 5. Calculate benchmark (buy & hold equal-weight)
        let benchmark_final = self.calculate_benchmark();

        // 6. Calculate and return results
        let final_equity = self.portfolio.total_equity(&self.current_prices);

        let results = MetricsCalculator::calculate(
            self.config.start_date,
            self.config.end_date,
            self.config.initial_capital,
            final_equity,
            &self.trades,
            &self.equity_curve,
            benchmark_final,
        );

        info!(
            "Backtest complete: {} trades, {:.2}% return",
            results.total_trades, results.total_return_pct
        );

        Ok(results)
    }

    /// Run walk-forward validation with n windows
    pub async fn run_walk_forward(config: BacktestConfig, n_windows: usize) -> Result<WalkForwardResult> {
        let total_days = (config.end_date - config.start_date).num_days();
        let window_days = total_days / n_windows as i64;
        let oos_pct_f64: f64 = config.walk_forward_oos_pct.try_into().unwrap_or(0.25);
        let oos_days = (window_days as f64 * oos_pct_f64) as i64;
        let is_days = window_days - oos_days;

        info!("Walk-forward validation: {} windows, {} days each (IS: {}, OOS: {})",
            n_windows, window_days, is_days, oos_days);

        let mut windows = Vec::new();

        for i in 0..n_windows {
            let window_start = config.start_date + chrono::Duration::days(i as i64 * window_days);
            let is_end = window_start + chrono::Duration::days(is_days);
            let oos_start = is_end + chrono::Duration::days(1);
            let oos_end = if i == n_windows - 1 {
                config.end_date // Last window extends to end
            } else {
                window_start + chrono::Duration::days(window_days)
            };

            info!("Window {}: IS {} to {}, OOS {} to {}", i + 1, window_start, is_end, oos_start, oos_end);

            // Run in-sample backtest
            let is_config = BacktestConfig {
                start_date: window_start,
                end_date: is_end,
                ..config.clone()
            };
            let mut is_engine = BacktestEngine::new(is_config);
            let is_results = is_engine.run().await?;

            // Run out-of-sample backtest (fresh strategies)
            let oos_config = BacktestConfig {
                start_date: oos_start,
                end_date: oos_end,
                ..config.clone()
            };
            let mut oos_engine = BacktestEngine::new(oos_config);
            let oos_results = oos_engine.run().await?;

            windows.push(WindowResult {
                window_num: i + 1,
                is_start: window_start,
                is_end,
                oos_start,
                oos_end,
                is_results,
                oos_results,
            });
        }

        // Aggregate results
        let n = windows.len() as u32;
        let aggregate_is_return_pct = windows.iter()
            .map(|w| w.is_results.total_return_pct)
            .sum::<Decimal>() / Decimal::from(n);
        let aggregate_is_sharpe = windows.iter()
            .map(|w| w.is_results.sharpe_ratio)
            .sum::<Decimal>() / Decimal::from(n);
        let aggregate_oos_return_pct = windows.iter()
            .map(|w| w.oos_results.total_return_pct)
            .sum::<Decimal>() / Decimal::from(n);
        let aggregate_oos_sharpe = windows.iter()
            .map(|w| w.oos_results.sharpe_ratio)
            .sum::<Decimal>() / Decimal::from(n);

        let overfitting_ratio = if !aggregate_oos_sharpe.is_zero() {
            aggregate_is_sharpe / aggregate_oos_sharpe
        } else if aggregate_is_sharpe > Decimal::ZERO {
            Decimal::from(100) // OOS sharpe is 0 but IS isn't — extreme overfit
        } else {
            Decimal::ONE
        };

        Ok(WalkForwardResult {
            windows,
            aggregate_oos_return_pct,
            aggregate_oos_sharpe,
            aggregate_is_return_pct,
            aggregate_is_sharpe,
            overfitting_ratio,
        })
    }

    async fn fetch_all_historical_data(&self) -> Result<HashMap<TradingPair, Vec<Candle>>> {
        let mut data = HashMap::new();

        let start = self
            .config
            .start_date
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_utc();
        let end = self
            .config
            .end_date
            .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
            .and_utc();

        for pair in &self.config.pairs {
            info!("Fetching historical data for {}...", pair);
            let candles = self
                .exchange
                .get_historical_candles(*pair, self.config.timeframe, start, end)
                .await?;
            info!("Fetched {} candles for {}", candles.len(), pair);
            data.insert(*pair, candles);
        }

        Ok(data)
    }

    fn create_timeline(&self, data: &HashMap<TradingPair, Vec<Candle>>) -> Vec<Candle> {
        // Iterate pairs in a fixed order to ensure determinism
        let mut all_candles: Vec<Candle> = Vec::new();
        for pair in &self.config.pairs {
            if let Some(candles) = data.get(pair) {
                all_candles.extend(candles.iter().cloned());
            }
        }

        // Sort by open_time, then by pair for deterministic ordering at same timestamp
        all_candles.sort_by(|a, b| {
            a.open_time.cmp(&b.open_time)
                .then_with(|| a.pair.cmp(&b.pair))
        });
        all_candles
    }

    fn process_candle(&mut self, candle: Candle) -> Result<()> {
        let pair = candle.pair;
        let price = candle.close;

        // 1. Update current price and track first price per pair (for benchmark)
        self.current_prices.insert(pair, price);
        self.first_prices.entry(pair).or_insert(price);

        // 2. Update candle buffer and ATR for this pair
        if let Some(buffer) = self.candle_buffers.get_mut(&pair) {
            buffer.push(candle.clone());
        }
        if let Some(atr) = self.atr_indicators.get_mut(&pair) {
            atr.update(candle.high, candle.low, candle.close);
        }

        // 3. Feed BTC candles to cross-asset correlation strategies
        if pair == TradingPair::BTCUSDT {
            // Clone candle before mutable borrow of strategies
            let btc_candle = candle.clone();
            for (strat_pair, strategy) in self.strategies.iter_mut() {
                if *strat_pair != TradingPair::BTCUSDT {
                    strategy.update_btc_candle(btc_candle.clone());
                }
            }
        }

        // 4. Update position prices and check stops using candle high/low
        self.portfolio.update_position_price(pair, price);
        self.check_stops(pair, &candle)?;

        // 5. Run strategy analysis (skip if emergency stopped)
        if !self.emergency_stopped {
            self.run_strategy(pair, price, candle.open_time)?;
        }

        // 6. Update drawdown and check emergency stop
        self.portfolio.update_drawdown(&self.current_prices);
        if !self.emergency_stopped && self.portfolio.max_drawdown > self.config.max_drawdown_pct {
            warn!("Emergency stop triggered at {:.2}% drawdown (limit: {:.2}%)",
                self.portfolio.max_drawdown, self.config.max_drawdown_pct);
            self.close_all_positions()?;
            self.emergency_stopped = true;
        }

        // 7. Record/update equity point for current calendar day (end-of-day value for proper Sharpe)
        self.candles_processed += 1;
        let candle_date = candle.open_time.date_naive();
        let equity = self.portfolio.total_equity(&self.current_prices);
        let drawdown = self.portfolio.max_drawdown;
        if self.last_equity_date.map_or(true, |d| d != candle_date) {
            // New day: compute daily return for volatility targeting
            if let Some(prev_equity) = self.prev_day_equity {
                if prev_equity > Decimal::ZERO {
                    let daily_ret: f64 = ((equity - prev_equity) / prev_equity)
                        .try_into().unwrap_or(0.0);
                    self.daily_returns_window.push(daily_ret);
                    // Rolling 20-day window
                    if self.daily_returns_window.len() > 20 {
                        self.daily_returns_window.remove(0);
                    }
                    // Recompute vol scale when we have enough data
                    if self.daily_returns_window.len() >= 10 {
                        let n = self.daily_returns_window.len() as f64;
                        let mean: f64 = self.daily_returns_window.iter().sum::<f64>() / n;
                        let variance: f64 = self.daily_returns_window.iter()
                            .map(|r| (r - mean).powi(2))
                            .sum::<f64>() / (n - 1.0);
                        let daily_vol = variance.sqrt();
                        let annualized_vol = daily_vol * 365.0f64.sqrt();
                        // Target 18% annualized portfolio volatility
                        if annualized_vol > 0.01 {
                            let scale = (0.18 / annualized_vol).clamp(0.4, 1.8);
                            self.vol_scale = Decimal::try_from(scale).unwrap_or(Decimal::ONE);
                        }
                    }
                }
            }
            self.prev_day_equity = Some(equity);

            // Push new equity point
            self.equity_curve.push(EquityPoint {
                timestamp: candle.open_time,
                equity,
                drawdown_pct: drawdown,
            });
            self.last_equity_date = Some(candle_date);
        } else {
            // Same day: overwrite with latest (end-of-day) values
            if let Some(last) = self.equity_curve.last_mut() {
                last.timestamp = candle.open_time;
                last.equity = equity;
                last.drawdown_pct = drawdown;
            }
        }

        Ok(())
    }

    fn run_strategy(&mut self, pair: TradingPair, price: Decimal, timestamp: DateTime<Utc>) -> Result<()> {
        let buffer = match self.candle_buffers.get(&pair) {
            Some(b) => b.clone(),
            None => return Ok(()),
        };

        let strategy = match self.strategies.get_mut(&pair) {
            Some(s) => s,
            None => return Ok(()),
        };

        if buffer.len() < strategy.min_candles_required() {
            return Ok(());
        }

        if let Some(signal) = strategy.analyze(&buffer) {
            self.process_signal(signal, price, timestamp)?;
        }

        Ok(())
    }

    fn process_signal(
        &mut self,
        signal: StrategySignal,
        price: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        debug!(
            "[{}] Signal: {:?} confidence={:.0}% reason={}",
            signal.pair, signal.signal, signal.confidence * dec!(100), signal.reason
        );

        // Check confidence threshold
        if signal.confidence < self.config.min_confidence {
            debug!("Signal rejected: confidence {:.0}% < {:.0}%", signal.confidence * dec!(100), self.config.min_confidence * dec!(100));
            return Ok(());
        }

        // Check risk/reward ratio
        if let Some(rr) = signal.risk_reward_ratio() {
            if rr < self.config.min_risk_reward {
                debug!("Signal rejected: R:R {:.2} < {:.2}", rr, self.config.min_risk_reward);
                return Ok(());
            }
        }

        let has_position = self.portfolio.has_open_position(signal.pair);
        let side = signal.side();

        debug!(
            "Processing: side={:?} has_position={} pair={}",
            side, has_position, signal.pair
        );

        // Check correlation limits before opening
        if side == Some(Side::Buy) && !has_position {
            let group = signal.pair.correlation_group();
            let correlated_count = self.portfolio.get_open_positions().iter()
                .filter(|p| p.pair.correlation_group() == group)
                .count();
            if correlated_count >= self.config.max_correlated_positions {
                debug!("Signal rejected: {} correlated positions in group '{}' >= max {}",
                    correlated_count, group, self.config.max_correlated_positions);
                return Ok(());
            }
        }

        // Check cooldown: skip re-entry within 24 candles after a stop-loss exit (~24h at 1H)
        if side == Some(Side::Buy) && !has_position {
            if let Some(&exit_candle) = self.last_exit_candle.get(&signal.pair) {
                let is_stoploss = self.last_exit_was_stoploss.get(&signal.pair).copied().unwrap_or(false);
                if is_stoploss && self.candles_processed < exit_candle + 6 {
                    debug!("Signal rejected: cooldown after stop-loss ({} candles remaining)",
                        exit_candle + 6 - self.candles_processed);
                    return Ok(());
                }
            }
        }

        // ML signal gating: extract features and check predictor
        if side == Some(Side::Buy) && !has_position {
            if let Some(buffer) = self.candle_buffers.get(&signal.pair) {
                let recent = self.outcome_tracker.recent_trades(10);
                let feats = features::extract_features(&signal, buffer, &recent, None);
                if let Some(ref f) = feats {
                    if !self.trade_predictor.should_trade(f) {
                        debug!("ML model rejected signal for {}", signal.pair);
                        return Ok(());
                    }
                }
            }
        }

        match (side, has_position) {
            (Some(Side::Buy), false) => {
                debug!("Opening position for {}", signal.pair);
                // Record ML features for this trade
                if let Some(buffer) = self.candle_buffers.get(&signal.pair) {
                    let recent = self.outcome_tracker.recent_trades(10);
                    if let Some(feats) = features::extract_features(&signal, buffer, &recent, None) {
                        let trade_id = format!("{}_{}", signal.pair, self.candles_processed);
                        self.outcome_tracker.record_entry(&trade_id, feats);
                        self.ml_trade_ids.insert(signal.pair, trade_id);
                    }
                }
                // Open new long position
                self.open_position(&signal, price, timestamp)?;
            }
            (Some(Side::Sell), true) => {
                debug!("Closing position for {}", signal.pair);
                // Close existing position
                self.close_position_by_signal(&signal, price, timestamp)?;
            }
            _ => {
                debug!("No action: side={:?} has_position={}", side, has_position);
            }
        }

        Ok(())
    }

    fn open_position(
        &mut self,
        signal: &StrategySignal,
        price: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let available = self.portfolio.available_usdt();

        // Apply slippage to get execution price
        let execution_price = price * (Decimal::ONE + self.config.slippage_rate);

        // Calculate stop distance
        let stop_distance = signal
            .suggested_stop_loss
            .map(|sl| (price - sl).abs())
            .unwrap_or(price * dec!(0.02));

        // Calculate position size based on strategy-specific risk per trade
        let risk_amount = available * self.config.risk_per_trade;

        // Position size = risk amount / stop distance
        let risk_based_qty = if !stop_distance.is_zero() {
            risk_amount / stop_distance
        } else {
            risk_amount / (price * dec!(0.02))
        };

        // Also cap at strategy-specific max allocation
        let max_allocation = available * self.config.max_allocation;
        let fee_multiplier = Decimal::ONE + self.config.fee_rate;
        let max_affordable_qty = max_allocation / (execution_price * fee_multiplier);

        // Use the smaller of risk-based or max affordable
        let mut quantity = risk_based_qty.min(max_affordable_qty);

        // Apply volatility-adjusted sizing via ATR
        if let Some(atr) = self.atr_indicators.get(&signal.pair) {
            if let Some(vol_level) = atr.volatility_level(price) {
                let factor = vol_level.position_size_factor();
                quantity = quantity * factor;
                debug!("[{}] ATR sizing: {:?} -> {:.1}x", signal.pair, vol_level, factor);
            }
        }

        // Volatility targeting: scale positions to target constant portfolio vol
        quantity = quantity * self.vol_scale;

        // Drawdown-based position scaling: reduce exposure during drawdowns
        let current_dd = self.portfolio.max_drawdown;
        if current_dd > dec!(5) {
            // Linear scale-down from 5% DD (1.0x) to 15% DD (0.3x)
            let dd_scale = (Decimal::ONE - (current_dd - dec!(5)) / dec!(10)).max(dec!(0.3));
            quantity = quantity * dd_scale;
            debug!("Drawdown scaling: {:.1}% DD -> {:.2}x size", current_dd, dd_scale);
        }

        // Calculate fee
        let notional = quantity * execution_price;
        let fee = notional * self.config.fee_rate;
        self.total_fees += fee;

        // Check if we have enough balance
        let total_cost = notional + fee;

        if total_cost > available {
            debug!("[BACKTEST] REJECTED: Insufficient balance - need {:.2} have {:.2}", total_cost, available);
            return Ok(());
        }

        // Minimum position size check
        if notional < dec!(10) {
            debug!("[BACKTEST] REJECTED: Position too small (notional {:.2})", notional);
            return Ok(());
        }

        // Create position
        let position = Position {
            id: Uuid::new_v4().to_string(),
            pair: signal.pair,
            side: Side::Buy,
            status: PositionStatus::Open,
            entry_price: execution_price,
            current_price: execution_price,
            quantity,
            stop_loss: signal.suggested_stop_loss,
            take_profit: signal.suggested_take_profit,
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            peak_pnl_pct: Decimal::ZERO,
            opened_at: timestamp,
            closed_at: None,
            strategy_id: signal.strategy_name.clone(),
            order_ids: Vec::new(),
            oco_order_id: None,
            entry_fee: fee,
        };

        debug!(
            "[{}] Opening: {:.4} @ ${:.2} (${:.2} notional)",
            signal.pair,
            quantity,
            execution_price,
            notional
        );

        // Deduct fee from balance
        self.portfolio.update_balance("USDT", -fee);
        self.portfolio.open_position(position);

        Ok(())
    }

    fn close_position_by_signal(
        &mut self,
        signal: &StrategySignal,
        price: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let position = match self.portfolio.get_position_for_pair(signal.pair) {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        self.close_position_internal(&position, price, timestamp, ExitReason::Signal)
    }

    fn check_stops(&mut self, pair: TradingPair, candle: &Candle) -> Result<()> {
        let position = match self.portfolio.get_position_for_pair(pair) {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        // Update peak PnL% using candle.high (best price during this candle for longs)
        let best_pnl_pct = (candle.high - position.entry_price) / position.entry_price * dec!(100);
        if best_pnl_pct > position.peak_pnl_pct {
            if let Some(pos) = self.portfolio.get_position_for_pair_mut(pair) {
                pos.peak_pnl_pct = best_pnl_pct;
            }
        }

        // Check stop loss against candle LOW (worst case for longs)
        if let Some(sl) = position.stop_loss {
            if candle.low <= sl {
                debug!("[{}] Stop loss triggered: low={:.2} <= sl={:.2}", pair, candle.low, sl);
                // Execute at stop price, not at the low
                return self.close_position_internal(&position, sl, candle.open_time, ExitReason::StopLoss);
            }
        }

        // Check take profit against candle HIGH (best case for longs)
        if let Some(tp) = position.take_profit {
            if candle.high >= tp {
                debug!("[{}] Take profit triggered: high={:.2} >= tp={:.2}", pair, candle.high, tp);
                // Execute at take profit price, not at the high
                return self.close_position_internal(&position, tp, candle.open_time, ExitReason::TakeProfit);
            }
        }

        // Trailing stop: if position reached 8%+ profit, trail at 3% from peak
        // Balanced thresholds for H1 candles
        let current_peak = if best_pnl_pct > position.peak_pnl_pct {
            best_pnl_pct
        } else {
            position.peak_pnl_pct
        };
        if current_peak >= dec!(15) {
            let current_pnl_pct = (candle.low - position.entry_price) / position.entry_price * dec!(100);
            let drawdown_from_peak = current_peak - current_pnl_pct;
            if drawdown_from_peak >= dec!(6) {
                let trail_price = position.entry_price * (Decimal::ONE + (current_peak - dec!(6)) / dec!(100));
                debug!("[{}] Trailing stop triggered: peak={:.2}%, current low PnL={:.2}%, trail price={:.2}",
                    pair, current_peak, current_pnl_pct, trail_price);
                return self.close_position_internal(&position, trail_price, candle.open_time, ExitReason::TrailingStop);
            }
        }

        Ok(())
    }

    fn close_position_internal(
        &mut self,
        position: &Position,
        price: Decimal,
        timestamp: DateTime<Utc>,
        exit_reason: ExitReason,
    ) -> Result<()> {
        // Apply slippage (negative for sells)
        let execution_price = price * (Decimal::ONE - self.config.slippage_rate);

        // Calculate fee
        let notional = position.quantity * execution_price;
        let fee = notional * self.config.fee_rate;
        self.total_fees += fee;

        // Calculate P&L (including both entry and exit fees)
        let gross_pnl = (execution_price - position.entry_price) * position.quantity;
        let total_fees = position.entry_fee + fee;
        let net_pnl = gross_pnl - total_fees;
        let pnl_pct = (execution_price - position.entry_price) / position.entry_price * dec!(100);

        // Record cooldown data for this pair (before exit_reason is moved)
        self.last_exit_candle.insert(position.pair, self.candles_processed);
        self.last_exit_was_stoploss.insert(position.pair, matches!(exit_reason, ExitReason::StopLoss));

        // Create trade record
        let trade = TradeRecord {
            id: position.id.clone(),
            pair: position.pair,
            side: position.side,
            entry_time: position.opened_at,
            exit_time: timestamp,
            entry_price: position.entry_price,
            exit_price: execution_price,
            quantity: position.quantity,
            pnl: net_pnl,
            pnl_pct,
            fees: total_fees,
            strategy: position.strategy_id.clone(),
            exit_reason,
        };

        self.trades.push(trade);

        // Close position in portfolio
        self.portfolio.close_position(&position.id, execution_price);

        // Deduct fee from balance
        self.portfolio.update_balance("USDT", -fee);

        // ML: record outcome and retrain if interval reached
        if let Some(trade_id) = self.ml_trade_ids.remove(&position.pair) {
            self.outcome_tracker.record_exit(&trade_id, pnl_pct);
            self.trades_since_retrain += 1;

            if self.trades_since_retrain >= self.retrain_interval {
                let training_data = self.outcome_tracker.get_training_data();
                match self.trade_predictor.train(&training_data) {
                    Ok(report) => {
                        info!("ML model retrained: {} samples, {:.1}% accuracy ({} wins / {} losses)",
                            report.samples, report.accuracy * 100.0,
                            report.wins_in_data, report.losses_in_data);
                    }
                    Err(e) => {
                        debug!("ML retrain skipped: {}", e);
                    }
                }
                self.trades_since_retrain = 0;
            }
        }

        debug!(
            "[{}] Closed position: ${:.2} P&L ({:.2}%)",
            position.pair, net_pnl, pnl_pct
        );

        Ok(())
    }

    /// Calculate buy-and-hold benchmark: equal-weight allocation across all pairs
    fn calculate_benchmark(&self) -> Decimal {
        let n_pairs = self.config.pairs.len();
        if n_pairs == 0 {
            return self.config.initial_capital;
        }

        let allocation_per_pair = self.config.initial_capital / Decimal::from(n_pairs as u32);
        let mut benchmark_equity = Decimal::ZERO;

        for pair in &self.config.pairs {
            let first = self.first_prices.get(pair).copied().unwrap_or(Decimal::ONE);
            let last = self.current_prices.get(pair).copied().unwrap_or(first);
            if !first.is_zero() {
                benchmark_equity += allocation_per_pair * (last / first);
            }
        }

        benchmark_equity
    }

    fn close_all_positions(&mut self) -> Result<()> {
        let positions: Vec<Position> = self
            .portfolio
            .get_open_positions()
            .iter()
            .map(|p| (*p).clone())
            .collect();

        if !positions.is_empty() {
            debug!("Closing {} remaining positions at end of backtest", positions.len());
        }

        let end_time = self
            .config
            .end_date
            .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
            .and_utc();

        for position in positions {
            let price = self.current_prices.get(&position.pair).copied().unwrap_or(position.current_price);
            self.close_position_internal(&position, price, end_time, ExitReason::EndOfBacktest)?;
        }

        Ok(())
    }
}
