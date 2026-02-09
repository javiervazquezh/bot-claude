#![allow(dead_code)]
pub mod binance;
pub mod websocket;
pub mod orderbook;

pub use binance::*;
pub use websocket::*;

use async_trait::async_trait;
use rust_decimal::Decimal;
use crate::types::{Candle, Order, OrderRequest, Ticker, TimeFrame, TradingPair};

#[async_trait]
pub trait Exchange: Send + Sync {
    async fn get_ticker(&self, pair: TradingPair) -> anyhow::Result<Ticker>;
    async fn get_candles(&self, pair: TradingPair, timeframe: TimeFrame, limit: u32) -> anyhow::Result<Vec<Candle>>;
    async fn place_order(&self, request: OrderRequest) -> anyhow::Result<Order>;
    async fn cancel_order(&self, pair: TradingPair, order_id: &str) -> anyhow::Result<()>;
    async fn get_order(&self, pair: TradingPair, order_id: &str) -> anyhow::Result<Order>;
    async fn get_balance(&self, asset: &str) -> anyhow::Result<Decimal>;
    async fn get_server_time(&self) -> anyhow::Result<u64>;
}
