use anyhow::Result;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::RuntimeConfig;
use crate::notifications::{NotificationManager, position_opened, position_closed, stop_loss_triggered, take_profit_triggered, AlertType};
use crate::risk::RiskManager;
use crate::strategies::{Strategy, StrategySignal};
use crate::types::{CandleBuffer, OrderRequest, Side, Signal, TradingPair};
use super::{BotController, PaperTradingEngine, Portfolio};

pub struct TradeExecutor {
    engine: Arc<PaperTradingEngine>,
    risk_manager: Arc<RiskManager>,
    config: Arc<RwLock<RuntimeConfig>>,
    controller: Arc<BotController>,
    notifications: Arc<NotificationManager>,
}

impl TradeExecutor {
    pub fn new(
        engine: Arc<PaperTradingEngine>,
        risk_manager: Arc<RiskManager>,
        config: Arc<RwLock<RuntimeConfig>>,
        controller: Arc<BotController>,
        notifications: Arc<NotificationManager>,
    ) -> Self {
        Self {
            engine,
            risk_manager,
            config,
            controller,
            notifications,
        }
    }

    pub async fn process_signal(&self, signal: StrategySignal) -> Result<Option<String>> {
        // Check if bot is running and not paused
        if !self.controller.should_process_signals() {
            debug!("Signal ignored: bot is paused or stopped");
            return Ok(None);
        }

        // Check if signal meets minimum requirements
        if !self.should_trade(&signal).await {
            debug!(
                "Signal rejected: {} {:?} confidence={:.0}%",
                signal.pair, signal.signal, signal.confidence * Decimal::from(100)
            );
            return Ok(None);
        }

        let portfolio = self.engine.get_portfolio().await;
        let current_price = self.engine.get_price(signal.pair).await
            .ok_or_else(|| anyhow::anyhow!("No price for {}", signal.pair))?;

        // Check risk management
        if !self.risk_manager.can_open_position(&portfolio, signal.pair).await {
            info!(
                "Risk check failed for {}: max positions or exposure reached",
                signal.pair
            );
            return Ok(None);
        }

        // Determine action based on signal and current positions
        let has_position = portfolio.has_open_position(signal.pair);

        match (signal.side(), has_position) {
            (Some(Side::Buy), false) => {
                // Open new long position
                self.open_position(&signal, &portfolio, current_price).await
            }
            (Some(Side::Sell), true) => {
                // Close existing position
                self.close_position(&signal, &portfolio).await
            }
            (Some(Side::Sell), false) => {
                // Could open short (if supported) - skip for spot trading
                debug!("Sell signal but no position to close for {}", signal.pair);
                Ok(None)
            }
            (Some(Side::Buy), true) => {
                // Already have position, could add (DCA) but skip for now
                debug!("Buy signal but already have position for {}", signal.pair);
                Ok(None)
            }
            (None, _) => {
                // Neutral signal, no action
                Ok(None)
            }
        }
    }

    async fn should_trade(&self, signal: &StrategySignal) -> bool {
        // Must have actionable signal
        if matches!(signal.signal, Signal::Neutral) {
            return false;
        }

        let config = self.config.read().await;
        let executor_settings = &config.executor;

        // Must meet confidence threshold
        if signal.confidence < executor_settings.min_confidence {
            return false;
        }

        // Check risk/reward ratio if available
        if let Some(rr) = signal.risk_reward_ratio() {
            if rr < executor_settings.min_risk_reward {
                debug!(
                    "Signal rejected: R:R {:.2} below minimum {:.2}",
                    rr, executor_settings.min_risk_reward
                );
                return false;
            }
        }

        true
    }

    async fn open_position(
        &self,
        signal: &StrategySignal,
        portfolio: &Portfolio,
        price: Decimal,
    ) -> Result<Option<String>> {
        // Calculate position size based on risk
        let position_size = self.risk_manager.calculate_position_size(
            portfolio,
            signal.pair,
            price,
            signal.suggested_stop_loss,
        ).await;

        if position_size.is_zero() {
            warn!("Position size is zero for {}", signal.pair);
            return Ok(None);
        }

        // Create order
        let order = OrderRequest::market(signal.pair, Side::Buy, position_size);

        info!(
            "Opening position: {} {} {} @ ~${:.2} | Confidence: {:.0}% | {}",
            Side::Buy,
            position_size,
            signal.pair,
            price,
            signal.confidence * Decimal::from(100),
            signal.reason
        );

        let result = self.engine.place_order(order).await?;

        // Increment trade count in controller
        self.controller.increment_trades();

        // Send notification
        self.notifications.notify(position_opened(
            signal.pair,
            format!("{:?}", Side::Buy),
            position_size,
            price,
        )).await;

        // Place stop loss if suggested
        if let Some(sl_price) = signal.suggested_stop_loss {
            let sl_order = OrderRequest::stop_loss(signal.pair, Side::Sell, position_size, sl_price);
            if let Err(e) = self.engine.place_order(sl_order).await {
                warn!("Failed to place stop loss: {}", e);
            }
        }

        Ok(Some(result.client_order_id))
    }

    async fn close_position(
        &self,
        signal: &StrategySignal,
        portfolio: &Portfolio,
    ) -> Result<Option<String>> {
        let position = portfolio.get_position_for_pair(signal.pair)
            .ok_or_else(|| anyhow::anyhow!("No position to close for {}", signal.pair))?;

        let order = OrderRequest::market(signal.pair, Side::Sell, position.quantity);

        info!(
            "Closing position: {} {} {} | P&L: ${:.2} ({:.2}%) | {}",
            Side::Sell,
            position.quantity,
            signal.pair,
            position.unrealized_pnl,
            position.pnl_percentage(),
            signal.reason
        );

        let result = self.engine.place_order(order).await?;

        // Send notification
        self.notifications.notify(position_closed(
            signal.pair,
            position.unrealized_pnl,
            position.pnl_percentage(),
            signal.reason.clone(),
        )).await;

        // Record loss if applicable
        if position.unrealized_pnl < Decimal::ZERO {
            self.risk_manager.record_loss(position.unrealized_pnl).await;
        }

        Ok(Some(result.client_order_id))
    }

    /// Legacy method - delegates to check_position_exits
    pub async fn check_stop_losses(&self) -> Result<Vec<String>> {
        self.check_position_exits().await
    }

    /// Check all position exits (stop loss and take profit)
    pub async fn check_position_exits(&self) -> Result<Vec<String>> {
        // Don't check if bot is stopped (but do check if paused - to protect capital)
        if !self.controller.is_running() {
            return Ok(Vec::new());
        }

        let mut portfolio = self.engine.get_portfolio_mut().await;
        let mut closed = Vec::new();

        // Get current prices for all trading pairs
        let mut prices = HashMap::new();
        for pair in [TradingPair::BTCUSDT, TradingPair::ETHUSDT, TradingPair::SOLUSDT] {
            if let Some(price) = self.engine.get_price(pair).await {
                prices.insert(pair, price);
            }
        }

        // Process each position
        let position_ids: Vec<String> = portfolio.get_open_positions()
            .iter()
            .map(|p| p.id.clone())
            .collect();

        for position_id in position_ids {
            if let Some(position) = portfolio.get_position_mut(&position_id) {
                let current_price = prices.get(&position.pair).copied().unwrap_or(position.current_price);

                // Update position price
                position.update_price(current_price);

                // Check stop loss
                if position.should_stop_loss() {
                    info!(
                        "Stop loss triggered for {}: {} @ {}",
                        position.pair, position.stop_loss.unwrap_or_default(), current_price
                    );

                    // Send notification
                    self.notifications.notify(stop_loss_triggered(
                        position.pair,
                        current_price,
                        position.unrealized_pnl,
                    )).await;

                    let order = OrderRequest::market(position.pair, Side::Sell, position.quantity);
                    if let Ok(result) = self.engine.place_order(order).await {
                        closed.push(result.client_order_id);
                        if position.unrealized_pnl < Decimal::ZERO {
                            self.risk_manager.record_loss(position.unrealized_pnl).await;
                        }
                    }
                    continue;
                }

                // Check take profit
                if position.should_take_profit() {
                    info!(
                        "Take profit triggered for {}: {} @ {}",
                        position.pair, position.take_profit.unwrap_or_default(), current_price
                    );

                    // Send notification
                    self.notifications.notify(take_profit_triggered(
                        position.pair,
                        current_price,
                        position.unrealized_pnl,
                    )).await;

                    let order = OrderRequest::market(position.pair, Side::Sell, position.quantity);
                    if let Ok(result) = self.engine.place_order(order).await {
                        closed.push(result.client_order_id);
                    }
                    continue;
                }
            }
        }

        Ok(closed)
    }
}

pub struct StrategyRunner {
    strategies: Vec<Box<dyn Strategy>>,
    executor: Arc<TradeExecutor>,
}

impl StrategyRunner {
    pub fn new(executor: Arc<TradeExecutor>) -> Self {
        Self {
            strategies: Vec::new(),
            executor,
        }
    }

    pub fn add_strategy(&mut self, strategy: Box<dyn Strategy>) {
        info!("Added strategy: {} for {}", strategy.name(), strategy.pair());
        self.strategies.push(strategy);
    }

    pub async fn run_analysis(&mut self, candles: &CandleBuffer) -> Vec<StrategySignal> {
        let mut signals = Vec::new();

        for strategy in &mut self.strategies {
            if candles.len() >= strategy.min_candles_required() {
                if let Some(signal) = strategy.analyze(candles) {
                    signals.push(signal);
                }
            }
        }

        signals
    }

    pub async fn execute_signals(&self, signals: Vec<StrategySignal>) -> Vec<String> {
        let mut order_ids = Vec::new();

        for signal in signals {
            match self.executor.process_signal(signal).await {
                Ok(Some(id)) => order_ids.push(id),
                Ok(None) => {}
                Err(e) => warn!("Failed to process signal: {}", e),
            }
        }

        order_ids
    }
}
