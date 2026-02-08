use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::exchange::BinanceClient;
use crate::types::{
    Candle, CandleBuffer, Order, OrderRequest, OrderStatus, OrderType, Position, Side,
    Ticker, TimeFrame, TradingPair,
};
use super::Portfolio;

fn taker_fee() -> Decimal { Decimal::new(1, 3) } // 0.1% taker fee
fn maker_fee() -> Decimal { Decimal::new(1, 3) } // 0.1% maker fee
fn slippage() -> Decimal { Decimal::new(5, 4) }  // 0.05% slippage simulation

pub struct PaperTradingEngine {
    portfolio: Arc<RwLock<Portfolio>>,
    exchange: BinanceClient,
    prices: Arc<RwLock<HashMap<TradingPair, Decimal>>>,
    candle_buffers: Arc<RwLock<HashMap<(TradingPair, TimeFrame), CandleBuffer>>>,
    pending_orders: Arc<RwLock<Vec<Order>>>,
}

impl PaperTradingEngine {
    pub fn new(initial_capital: Decimal) -> Self {
        Self {
            portfolio: Arc::new(RwLock::new(Portfolio::new(initial_capital))),
            exchange: BinanceClient::public_only(),
            prices: Arc::new(RwLock::new(HashMap::new())),
            candle_buffers: Arc::new(RwLock::new(HashMap::new())),
            pending_orders: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn with_exchange(mut self, exchange: BinanceClient) -> Self {
        self.exchange = exchange;
        self
    }

    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing paper trading engine...");

        // Fetch initial prices for all pairs
        for pair in TradingPair::all() {
            match self.exchange.get_ticker(pair).await {
                Ok(ticker) => {
                    let mut prices = self.prices.write().await;
                    prices.insert(pair, ticker.price);
                    info!("{}: ${:.2}", pair, ticker.price);
                }
                Err(e) => {
                    warn!("Failed to fetch ticker for {}: {}", pair, e);
                }
            }
        }

        // Fetch initial historical candles (200 to cover all strategy requirements)
        // This allows the bot to start trading immediately without waiting 20-30 minutes
        info!("Fetching historical candles for all pairs...");
        for pair in TradingPair::all() {
            for timeframe in [TimeFrame::M5, TimeFrame::M15, TimeFrame::H1] {
                match self.exchange.get_candles(pair, timeframe, 200).await {
                    Ok(candles) => {
                        let mut buffers = self.candle_buffers.write().await;
                        let buffer = buffers
                            .entry((pair, timeframe))
                            .or_insert_with(|| CandleBuffer::new(200));
                        for candle in candles {
                            buffer.push(candle);
                        }
                        info!("  {} {} - {} candles loaded", pair, timeframe, buffer.len());
                    }
                    Err(e) => {
                        warn!("Failed to fetch candles for {} {}: {}", pair, timeframe, e);
                    }
                }
            }
        }

        info!("Paper trading engine initialized - ready to trade!");
        Ok(())
    }

    pub async fn update_price(&self, pair: TradingPair, price: Decimal) {
        // Acquire and release prices lock in its own scope to prevent lock ordering deadlock
        // (portfolio_summary acquires portfolio→prices; we must not hold prices→portfolio)
        {
            let mut prices = self.prices.write().await;
            prices.insert(pair, price);
        }

        // Now safely acquire portfolio lock without holding prices
        {
            let mut portfolio = self.portfolio.write().await;
            portfolio.update_position_price(pair, price);
        }

        // Check pending orders
        self.check_pending_orders(pair, price).await;
    }

    pub async fn update_candle(&self, candle: Candle) {
        if candle.is_closed {
            let mut buffers = self.candle_buffers.write().await;
            let buffer = buffers
                .entry((candle.pair, candle.timeframe))
                .or_insert_with(|| CandleBuffer::new(200));
            buffer.push(candle.clone());
        }

        // Update price from candle
        self.update_price(candle.pair, candle.close).await;
    }

    pub async fn get_price(&self, pair: TradingPair) -> Option<Decimal> {
        let prices = self.prices.read().await;
        prices.get(&pair).copied()
    }

    pub async fn get_candles(&self, pair: TradingPair, timeframe: TimeFrame) -> Option<CandleBuffer> {
        let buffers = self.candle_buffers.read().await;
        buffers.get(&(pair, timeframe)).cloned()
    }

    pub async fn place_order(&self, request: OrderRequest) -> Result<Order> {
        let mut order = Order::from_request(&request);

        let price = self.get_price(request.pair).await
            .ok_or_else(|| anyhow::anyhow!("No price available for {}", request.pair))?;

        // Simulate order execution
        match request.order_type {
            OrderType::Market => {
                // Market orders execute immediately with slippage
                let execution_price = self.simulate_execution_price(price, request.side);
                self.execute_order(&mut order, execution_price).await?;
            }
            OrderType::Limit => {
                // Limit orders go to pending
                let limit_price = request.price.unwrap_or(price);
                let should_execute = match request.side {
                    Side::Buy => price <= limit_price,
                    Side::Sell => price >= limit_price,
                };

                if should_execute {
                    self.execute_order(&mut order, limit_price).await?;
                } else {
                    order.status = OrderStatus::Open;
                    let mut pending = self.pending_orders.write().await;
                    pending.push(order.clone());
                    info!("Limit order placed: {} {} {} @ {}", request.side, request.quantity, request.pair, limit_price);
                }
            }
            OrderType::StopLoss | OrderType::StopLossLimit => {
                // Stop orders wait for trigger
                order.status = OrderStatus::Open;
                let mut pending = self.pending_orders.write().await;
                pending.push(order.clone());
                info!("Stop order placed: {} {} {} trigger @ {:?}",
                    request.side, request.quantity, request.pair, request.stop_price);
            }
            _ => {
                order.status = OrderStatus::Open;
                let mut pending = self.pending_orders.write().await;
                pending.push(order.clone());
            }
        }

        // Add to portfolio
        let mut portfolio = self.portfolio.write().await;
        portfolio.add_order(order.clone());

        Ok(order)
    }

    async fn execute_order(&self, order: &mut Order, price: Decimal) -> Result<()> {
        let mut portfolio = self.portfolio.write().await;

        // Calculate commission
        let notional = price * order.quantity;
        let fee_rate = if matches!(order.order_type, OrderType::Market) {
            taker_fee()
        } else {
            maker_fee()
        };
        let commission = notional * fee_rate;

        // Check if we have enough balance
        match order.side {
            Side::Buy => {
                let required = notional + commission;
                let available = portfolio.available_usdt();
                if available < required {
                    order.status = OrderStatus::Rejected;
                    return Err(anyhow::anyhow!(
                        "Insufficient balance: need {} USDT, have {}",
                        required,
                        available
                    ));
                }
            }
            Side::Sell => {
                // For sells, check if we have a position or asset to sell
                let asset = order.pair.base_asset();
                let balance = portfolio.get_balance(asset);
                if balance < order.quantity {
                    // Check positions
                    if !portfolio.has_open_position(order.pair) {
                        order.status = OrderStatus::Rejected;
                        return Err(anyhow::anyhow!(
                            "Insufficient {}: need {}, have {}",
                            asset,
                            order.quantity,
                            balance
                        ));
                    }
                }
            }
        }

        // Execute the order
        order.status = OrderStatus::Filled;
        order.filled_quantity = order.quantity;
        order.average_fill_price = Some(price);
        order.updated_at = Utc::now();

        info!(
            "Order filled: {} {} {} @ {} (fee: {} USDT)",
            order.side, order.quantity, order.pair, price, commission
        );

        // Update portfolio based on order type
        match order.side {
            Side::Buy => {
                // Deduct USDT and add position
                let position = Position::new(
                    order.pair,
                    Side::Buy,
                    price,
                    order.quantity,
                    order.strategy_id.clone().unwrap_or_else(|| "manual".to_string()),
                );
                portfolio.open_position(position);
                portfolio.update_balance("USDT", -commission);
            }
            Side::Sell => {
                // Close existing position if any
                if let Some(pos) = portfolio.get_position_for_pair(order.pair) {
                    let pos_id = pos.id.clone();
                    drop(portfolio);
                    let mut portfolio = self.portfolio.write().await;
                    portfolio.close_position(&pos_id, price);
                    portfolio.update_balance("USDT", -commission);
                } else {
                    // Direct sell from balance
                    let asset = order.pair.base_asset();
                    portfolio.update_balance(asset, -order.quantity);
                    portfolio.update_balance("USDT", notional - commission);
                }
            }
        }

        Ok(())
    }

    fn simulate_execution_price(&self, price: Decimal, side: Side) -> Decimal {
        let slippage_amount = price * slippage();
        match side {
            Side::Buy => price + slippage_amount,  // Pay slightly more when buying
            Side::Sell => price - slippage_amount, // Receive slightly less when selling
        }
    }

    async fn check_pending_orders(&self, pair: TradingPair, price: Decimal) {
        let mut pending = self.pending_orders.write().await;
        let mut executed_indices = Vec::new();

        for (i, order) in pending.iter_mut().enumerate() {
            if order.pair != pair {
                continue;
            }

            let should_execute = match order.order_type {
                OrderType::Limit => match order.side {
                    Side::Buy => price <= order.price.unwrap_or(Decimal::MAX),
                    Side::Sell => price >= order.price.unwrap_or(Decimal::ZERO),
                },
                OrderType::StopLoss | OrderType::StopLossLimit => {
                    let stop = order.stop_price.unwrap_or(price);
                    match order.side {
                        Side::Sell => price <= stop,
                        Side::Buy => price >= stop,
                    }
                }
                OrderType::TakeProfit | OrderType::TakeProfitLimit => {
                    let stop = order.stop_price.unwrap_or(price);
                    match order.side {
                        Side::Sell => price >= stop,
                        Side::Buy => price <= stop,
                    }
                }
                _ => false,
            };

            if should_execute {
                executed_indices.push(i);
            }
        }

        // Execute triggered orders
        for i in executed_indices.into_iter().rev() {
            let mut order = pending.remove(i);
            let exec_price = order.price.unwrap_or(price);
            if let Err(e) = self.execute_order(&mut order, exec_price).await {
                warn!("Failed to execute triggered order: {}", e);
            }
        }
    }

    pub async fn cancel_order(&self, order_id: &str) -> Result<()> {
        let mut pending = self.pending_orders.write().await;
        if let Some(idx) = pending.iter().position(|o| o.client_order_id == order_id) {
            let order = pending.remove(idx);
            info!("Order cancelled: {}", order.client_order_id);

            let mut portfolio = self.portfolio.write().await;
            portfolio.update_order(order_id, OrderStatus::Cancelled, Decimal::ZERO, None);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Order not found: {}", order_id))
        }
    }

    pub async fn get_portfolio(&self) -> Portfolio {
        self.portfolio.read().await.clone()
    }

    pub async fn get_portfolio_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, Portfolio> {
        self.portfolio.write().await
    }

    pub async fn portfolio_summary(&self) -> PortfolioSummary {
        let portfolio = self.portfolio.read().await;
        let prices = self.prices.read().await;

        PortfolioSummary {
            total_equity: portfolio.total_equity(&prices),
            available_balance: portfolio.available_usdt(),
            unrealized_pnl: portfolio.total_unrealized_pnl(),
            realized_pnl: portfolio.total_pnl,
            open_positions: portfolio.position_count(),
            total_trades: portfolio.total_trades,
            win_rate: portfolio.win_rate(),
            max_drawdown: portfolio.max_drawdown,
        }
    }

    pub fn portfolio_arc(&self) -> Arc<RwLock<Portfolio>> {
        Arc::clone(&self.portfolio)
    }

    pub fn prices_arc(&self) -> Arc<RwLock<HashMap<TradingPair, Decimal>>> {
        Arc::clone(&self.prices)
    }
}

#[derive(Debug, Clone)]
pub struct PortfolioSummary {
    pub total_equity: Decimal,
    pub available_balance: Decimal,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
    pub open_positions: usize,
    pub total_trades: u64,
    pub win_rate: Decimal,
    pub max_drawdown: Decimal,
}

impl std::fmt::Display for PortfolioSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Portfolio Summary ===")?;
        writeln!(f, "Total Equity:     ${:.2}", self.total_equity)?;
        writeln!(f, "Available:        ${:.2}", self.available_balance)?;
        writeln!(f, "Unrealized P&L:   ${:.2}", self.unrealized_pnl)?;
        writeln!(f, "Realized P&L:     ${:.2}", self.realized_pnl)?;
        writeln!(f, "Open Positions:   {}", self.open_positions)?;
        writeln!(f, "Total Trades:     {}", self.total_trades)?;
        writeln!(f, "Win Rate:         {:.1}%", self.win_rate)?;
        writeln!(f, "Max Drawdown:     {:.2}%", self.max_drawdown)?;
        Ok(())
    }
}
