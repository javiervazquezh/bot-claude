#![allow(dead_code)]
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::config::{ConfigChangeEvent, RuntimeConfigManager};
use crate::engine::{BotController, BotState};
use crate::types::{Side, TradingPair};

const MAX_SIGNALS: usize = 100;
const MAX_PRICE_HISTORY: usize = 500;
const MAX_TRADES: usize = 200;
const MAX_LOGS: usize = 500; // Limit logs for memory management

#[derive(Clone)]
pub struct DashboardState {
    inner: Arc<RwLock<DashboardData>>,
    pub tx: broadcast::Sender<DashboardEvent>,
}

impl DashboardState {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            inner: Arc::new(RwLock::new(DashboardData::default())),
            tx,
        }
    }

    pub async fn update_price(&self, pair: TradingPair, price: Decimal) {
        let mut data = self.inner.write().await;
        let history = data.price_history.entry(pair).or_insert_with(VecDeque::new);

        history.push_back(PricePoint {
            timestamp: Utc::now(),
            price,
        });

        if history.len() > MAX_PRICE_HISTORY {
            history.pop_front();
        }

        data.current_prices.insert(pair, price);

        let _ = self.tx.send(DashboardEvent::PriceUpdate { pair, price });
    }

    pub async fn add_signal(&self, signal: SignalRecord) {
        let mut data = self.inner.write().await;
        data.signals.push_front(signal.clone());
        if data.signals.len() > MAX_SIGNALS {
            data.signals.pop_back();
        }

        let _ = self.tx.send(DashboardEvent::NewSignal(signal));
    }

    pub async fn add_trade(&self, trade: TradeRecord) {
        let mut data = self.inner.write().await;

        // Update statistics
        data.stats.total_trades += 1;
        if trade.pnl > Decimal::ZERO {
            data.stats.winning_trades += 1;
            data.stats.total_profit += trade.pnl;
        } else if trade.pnl < Decimal::ZERO {
            data.stats.losing_trades += 1;
            data.stats.total_loss += trade.pnl.abs();
        }

        data.stats.total_pnl += trade.pnl;

        data.trades.push_front(trade.clone());
        if data.trades.len() > MAX_TRADES {
            data.trades.pop_back();
        }

        let _ = self.tx.send(DashboardEvent::NewTrade(trade));
    }

    pub async fn update_portfolio(&self, portfolio: PortfolioState) {
        let mut data = self.inner.write().await;

        // Track equity history
        data.equity_history.push_back(EquityPoint {
            timestamp: Utc::now(),
            equity: portfolio.total_equity,
            pnl: portfolio.unrealized_pnl + portfolio.realized_pnl,
        });

        if data.equity_history.len() > MAX_PRICE_HISTORY {
            data.equity_history.pop_front();
        }

        data.portfolio = portfolio.clone();

        let _ = self.tx.send(DashboardEvent::PortfolioUpdate(portfolio));
    }

    pub async fn add_log(&self, level: String, message: String) {
        let log = LogRecord {
            timestamp: Utc::now(),
            level,
            message,
        };

        let mut data = self.inner.write().await;
        data.logs.push_front(log.clone());

        // Keep only last MAX_LOGS entries for memory management
        if data.logs.len() > MAX_LOGS {
            data.logs.pop_back();
        }

        let _ = self.tx.send(DashboardEvent::NewLog(log));
    }

    pub async fn get_data(&self) -> DashboardData {
        self.inner.read().await.clone()
    }

    pub async fn get_api_response(&self) -> ApiResponse {
        let data = self.inner.read().await;
        ApiResponse {
            portfolio: data.portfolio.clone(),
            stats: data.stats.clone(),
            current_prices: data.current_prices.clone(),
            recent_signals: data.signals.iter().take(20).cloned().collect(),
            recent_trades: data.trades.iter().take(20).cloned().collect(),
            price_history: data.price_history.iter()
                .map(|(k, v)| (*k, v.iter().cloned().collect()))
                .collect(),
            equity_history: data.equity_history.iter().cloned().collect(),
        }
    }
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DashboardData {
    pub portfolio: PortfolioState,
    pub stats: TradingStats,
    pub current_prices: std::collections::HashMap<TradingPair, Decimal>,
    pub price_history: std::collections::HashMap<TradingPair, VecDeque<PricePoint>>,
    pub equity_history: VecDeque<EquityPoint>,
    pub signals: VecDeque<SignalRecord>,
    pub trades: VecDeque<TradeRecord>,
    pub logs: VecDeque<LogRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PortfolioState {
    pub total_equity: Decimal,
    pub available_balance: Decimal,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
    pub positions: Vec<PositionInfo>,
    pub max_drawdown: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionInfo {
    pub pair: TradingPair,
    pub side: String,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub current_price: Decimal,
    pub pnl: Decimal,
    pub pnl_pct: Decimal,
    pub stop_loss: Option<Decimal>,
    pub take_profit: Option<Decimal>,
    pub duration_hours: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TradingStats {
    pub total_trades: u64,
    pub winning_trades: u64,
    pub losing_trades: u64,
    pub total_pnl: Decimal,
    pub total_profit: Decimal,
    pub total_loss: Decimal,
}

impl TradingStats {
    pub fn win_rate(&self) -> Decimal {
        if self.total_trades == 0 {
            Decimal::ZERO
        } else {
            Decimal::from(self.winning_trades) / Decimal::from(self.total_trades) * Decimal::from(100)
        }
    }

    pub fn profit_factor(&self) -> Decimal {
        if self.total_loss.is_zero() {
            if self.total_profit > Decimal::ZERO {
                Decimal::from(100)
            } else {
                Decimal::ONE
            }
        } else {
            self.total_profit / self.total_loss
        }
    }

    pub fn avg_win(&self) -> Decimal {
        if self.winning_trades == 0 {
            Decimal::ZERO
        } else {
            self.total_profit / Decimal::from(self.winning_trades)
        }
    }

    pub fn avg_loss(&self) -> Decimal {
        if self.losing_trades == 0 {
            Decimal::ZERO
        } else {
            self.total_loss / Decimal::from(self.losing_trades)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub timestamp: DateTime<Utc>,
    pub price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: DateTime<Utc>,
    pub equity: Decimal,
    pub pnl: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalRecord {
    pub timestamp: DateTime<Utc>,
    pub pair: TradingPair,
    pub signal: String,
    pub confidence: Decimal,
    pub reason: String,
    pub strategy: String,
    pub entry_price: Option<Decimal>,
    pub stop_loss: Option<Decimal>,
    pub take_profit: Option<Decimal>,
    pub executed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub pair: TradingPair,
    pub side: Side,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub exit_price: Option<Decimal>,
    pub pnl: Decimal,
    pub pnl_pct: Decimal,
    pub fees: Decimal,
    pub strategy: String,
    pub exit_reason: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogRecord {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum DashboardEvent {
    PriceUpdate { pair: TradingPair, price: Decimal },
    NewSignal(SignalRecord),
    NewTrade(TradeRecord),
    PortfolioUpdate(PortfolioState),
    StatusChange { status: BotState },
    ConfigChange { change: ConfigChangeEvent },
    NewLog(LogRecord),
}

/// Combined application state for the web server
#[derive(Clone)]
pub struct AppState {
    pub dashboard: DashboardState,
    pub controller: Arc<BotController>,
    pub config_manager: Arc<RuntimeConfigManager>,
    pub database: Option<Arc<crate::database::Database>>,
    pub notifications: Option<Arc<crate::notifications::NotificationManager>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse {
    pub portfolio: PortfolioState,
    pub stats: TradingStats,
    pub current_prices: std::collections::HashMap<TradingPair, Decimal>,
    pub recent_signals: Vec<SignalRecord>,
    pub recent_trades: Vec<TradeRecord>,
    pub price_history: std::collections::HashMap<TradingPair, Vec<PricePoint>>,
    pub equity_history: Vec<EquityPoint>,
}
