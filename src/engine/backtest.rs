use anyhow::Result;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::exchange::BinanceClient;
use crate::strategies::{ImprovedStrategy, Strategy, StrategySignal, create_improved_strategy};
use crate::types::{Candle, CandleBuffer, Position, PositionStatus, Side, Signal, TimeFrame, TradingPair};

use super::results::{BacktestResults, EquityPoint, ExitReason, MetricsCalculator, TradeRecord};
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
        }
    }
}

/// Core backtesting engine
pub struct BacktestEngine {
    config: BacktestConfig,
    exchange: BinanceClient,
    portfolio: Portfolio,
    strategies: HashMap<TradingPair, ImprovedStrategy>,
    candle_buffers: HashMap<TradingPair, CandleBuffer>,
    current_prices: HashMap<TradingPair, Decimal>,
    equity_curve: Vec<EquityPoint>,
    trades: Vec<TradeRecord>,
    total_fees: Decimal,
    candles_processed: u64,
}

impl BacktestEngine {
    pub fn new(config: BacktestConfig) -> Self {
        let portfolio = Portfolio::new(config.initial_capital);

        let mut strategies = HashMap::new();
        let mut candle_buffers = HashMap::new();
        let mut current_prices = HashMap::new();

        for pair in &config.pairs {
            strategies.insert(*pair, create_improved_strategy(*pair));
            candle_buffers.insert(*pair, CandleBuffer::new(200));
            current_prices.insert(*pair, Decimal::ZERO);
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

        // 5. Calculate and return results
        let final_equity = self.portfolio.total_equity(&self.current_prices);

        let results = MetricsCalculator::calculate(
            self.config.start_date,
            self.config.end_date,
            self.config.initial_capital,
            final_equity,
            &self.trades,
            &self.equity_curve,
        );

        info!(
            "Backtest complete: {} trades, {:.2}% return",
            results.total_trades, results.total_return_pct
        );

        Ok(results)
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

        // 1. Update current price
        self.current_prices.insert(pair, price);

        // 2. Update candle buffer for this pair
        if let Some(buffer) = self.candle_buffers.get_mut(&pair) {
            buffer.push(candle.clone());
        }

        // 3. Update position prices and check stops
        self.portfolio.update_position_price(pair, price);
        self.check_stops(pair, price, candle.open_time)?;

        // 4. Run strategy analysis
        self.run_strategy(pair, price, candle.open_time)?;

        // 5. Update drawdown
        self.portfolio.update_drawdown(&self.current_prices);

        // 6. Record equity point periodically (every 100 candles to save memory)
        self.candles_processed += 1;
        if self.candles_processed % 100 == 0 {
            let equity = self.portfolio.total_equity(&self.current_prices);
            let drawdown = self.portfolio.max_drawdown;
            self.equity_curve.push(EquityPoint {
                timestamp: candle.open_time,
                equity,
                drawdown_pct: drawdown,
            });
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

        match (side, has_position) {
            (Some(Side::Buy), false) => {
                debug!("Opening position for {}", signal.pair);
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
        let quantity = risk_based_qty.min(max_affordable_qty);

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
            opened_at: timestamp,
            closed_at: None,
            strategy_id: signal.strategy_name.clone(),
            order_ids: Vec::new(),
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

    fn check_stops(&mut self, pair: TradingPair, price: Decimal, timestamp: DateTime<Utc>) -> Result<()> {
        let position = match self.portfolio.get_position_for_pair(pair) {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        // Check stop loss
        if let Some(sl) = position.stop_loss {
            if price <= sl {
                debug!("[{}] Stop loss triggered at ${:.2}", pair, price);
                return self.close_position_internal(&position, price, timestamp, ExitReason::StopLoss);
            }
        }

        // Check take profit
        if let Some(tp) = position.take_profit {
            if price >= tp {
                debug!("[{}] Take profit triggered at ${:.2}", pair, price);
                return self.close_position_internal(&position, price, timestamp, ExitReason::TakeProfit);
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

        // Calculate P&L
        let gross_pnl = (execution_price - position.entry_price) * position.quantity;
        let net_pnl = gross_pnl - fee;
        let pnl_pct = (execution_price - position.entry_price) / position.entry_price * dec!(100);

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
            fees: fee,
            strategy: position.strategy_id.clone(),
            exit_reason,
        };

        self.trades.push(trade);

        // Close position in portfolio
        self.portfolio.close_position(&position.id, execution_price);

        // Deduct fee from balance
        self.portfolio.update_balance("USDT", -fee);

        debug!(
            "[{}] Closed position: ${:.2} P&L ({:.2}%)",
            position.pair, net_pnl, pnl_pct
        );

        Ok(())
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
