use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use crate::types::{Candle, Side, Ticker, TimeFrame, Trade, TradingPair};

const BINANCE_US_WS: &str = "wss://stream.binance.us:9443/ws";
const BINANCE_US_STREAM: &str = "wss://stream.binance.us:9443/stream";

#[derive(Debug, Clone)]
pub enum MarketEvent {
    Ticker(Ticker),
    Candle(Candle),
    Trade(Trade),
    BookTicker(BookTicker),
    Disconnected,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookTicker {
    pub pair: TradingPair,
    pub bid_price: Decimal,
    pub bid_qty: Decimal,
    pub ask_price: Decimal,
    pub ask_qty: Decimal,
}

pub struct BinanceWebSocket {
    streams: Vec<String>,
}

impl BinanceWebSocket {
    pub fn new() -> Self {
        Self { streams: Vec::new() }
    }

    pub fn subscribe_ticker(&mut self, pair: TradingPair) -> &mut Self {
        let stream = format!("{}@ticker", pair.as_str().to_lowercase());
        self.streams.push(stream);
        self
    }

    pub fn subscribe_kline(&mut self, pair: TradingPair, timeframe: TimeFrame) -> &mut Self {
        let stream = format!(
            "{}@kline_{}",
            pair.as_str().to_lowercase(),
            timeframe.as_str()
        );
        self.streams.push(stream);
        self
    }

    pub fn subscribe_trade(&mut self, pair: TradingPair) -> &mut Self {
        let stream = format!("{}@trade", pair.as_str().to_lowercase());
        self.streams.push(stream);
        self
    }

    pub fn subscribe_book_ticker(&mut self, pair: TradingPair) -> &mut Self {
        let stream = format!("{}@bookTicker", pair.as_str().to_lowercase());
        self.streams.push(stream);
        self
    }

    pub fn subscribe_all_pairs(&mut self, timeframe: TimeFrame) -> &mut Self {
        for pair in TradingPair::all() {
            self.subscribe_kline(pair, timeframe);
            self.subscribe_book_ticker(pair);
        }
        self
    }

    pub async fn connect(self) -> Result<mpsc::Receiver<MarketEvent>> {
        let (tx, rx) = mpsc::channel(1000);

        if self.streams.is_empty() {
            return Err(anyhow!("No streams subscribed"));
        }

        let url = if self.streams.len() == 1 {
            format!("{}/{}", BINANCE_US_WS, self.streams[0])
        } else {
            format!("{}?streams={}", BINANCE_US_STREAM, self.streams.join("/"))
        };

        info!("Connecting to WebSocket: {}", url);

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            loop {
                match Self::run_connection(&url, tx_clone.clone()).await {
                    Ok(_) => {
                        warn!("WebSocket connection closed, reconnecting...");
                    }
                    Err(e) => {
                        error!("WebSocket error: {}, reconnecting...", e);
                        let _ = tx_clone.send(MarketEvent::Error(e.to_string())).await;
                    }
                }

                let _ = tx_clone.send(MarketEvent::Disconnected).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        });

        Ok(rx)
    }

    async fn run_connection(url: &str, tx: mpsc::Sender<MarketEvent>) -> Result<()> {
        let (ws_stream, _) = connect_async(url).await?;
        let (mut write, mut read) = ws_stream.split();

        info!("WebSocket connected");

        // Send ping every 30 seconds
        let ping_tx = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                debug!("Sending ping");
            }
        });

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Some(event) = Self::parse_message(&text) {
                        if tx.send(event).await.is_err() {
                            break;
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received ping, sending pong");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn parse_message(text: &str) -> Option<MarketEvent> {
        // Try to parse as combined stream message first
        if let Ok(combined) = serde_json::from_str::<CombinedStreamMessage>(text) {
            return Self::parse_stream_data(&combined.stream, &combined.data);
        }

        // Try individual message types
        if let Ok(ticker) = serde_json::from_str::<WsTickerMessage>(text) {
            if ticker.event_type == "24hrTicker" {
                return Self::parse_ticker(&ticker);
            }
        }

        if let Ok(kline) = serde_json::from_str::<WsKlineMessage>(text) {
            if kline.event_type == "kline" {
                return Self::parse_kline(&kline);
            }
        }

        if let Ok(trade) = serde_json::from_str::<WsTradeMessage>(text) {
            if trade.event_type == "trade" {
                return Self::parse_trade(&trade);
            }
        }

        if let Ok(book) = serde_json::from_str::<WsBookTickerMessage>(text) {
            return Self::parse_book_ticker(&book);
        }

        debug!("Unknown message type: {}", text);
        None
    }

    fn parse_stream_data(stream: &str, data: &serde_json::Value) -> Option<MarketEvent> {
        if stream.contains("@ticker") {
            let ticker: WsTickerMessage = serde_json::from_value(data.clone()).ok()?;
            return Self::parse_ticker(&ticker);
        }

        if stream.contains("@kline") {
            let kline: WsKlineMessage = serde_json::from_value(data.clone()).ok()?;
            return Self::parse_kline(&kline);
        }

        if stream.contains("@trade") {
            let trade: WsTradeMessage = serde_json::from_value(data.clone()).ok()?;
            return Self::parse_trade(&trade);
        }

        if stream.contains("@bookTicker") {
            let book: WsBookTickerMessage = serde_json::from_value(data.clone()).ok()?;
            return Self::parse_book_ticker(&book);
        }

        None
    }

    fn parse_ticker(msg: &WsTickerMessage) -> Option<MarketEvent> {
        let pair = TradingPair::from_str(&msg.symbol)?;
        Some(MarketEvent::Ticker(Ticker {
            pair,
            price: Decimal::from_str(&msg.last_price).ok()?,
            bid: Decimal::from_str(&msg.best_bid).ok()?,
            ask: Decimal::from_str(&msg.best_ask).ok()?,
            volume_24h: Decimal::from_str(&msg.volume).ok()?,
            price_change_24h: Decimal::from_str(&msg.price_change).ok()?,
            price_change_pct_24h: Decimal::from_str(&msg.price_change_percent).ok()?,
            high_24h: Decimal::from_str(&msg.high_price).ok()?,
            low_24h: Decimal::from_str(&msg.low_price).ok()?,
            timestamp: Utc::now(),
        }))
    }

    fn parse_kline(msg: &WsKlineMessage) -> Option<MarketEvent> {
        let pair = TradingPair::from_str(&msg.symbol)?;
        let k = &msg.kline;
        let timeframe = match k.interval.as_str() {
            "1m" => TimeFrame::M1,
            "5m" => TimeFrame::M5,
            "15m" => TimeFrame::M15,
            "1h" => TimeFrame::H1,
            "4h" => TimeFrame::H4,
            "1d" => TimeFrame::D1,
            _ => return None,
        };

        Some(MarketEvent::Candle(Candle {
            pair,
            timeframe,
            open_time: Utc.timestamp_millis_opt(k.start_time).unwrap(),
            close_time: Utc.timestamp_millis_opt(k.close_time).unwrap(),
            open: Decimal::from_str(&k.open).ok()?,
            high: Decimal::from_str(&k.high).ok()?,
            low: Decimal::from_str(&k.low).ok()?,
            close: Decimal::from_str(&k.close).ok()?,
            volume: Decimal::from_str(&k.volume).ok()?,
            quote_volume: Decimal::from_str(&k.quote_volume).ok()?,
            trades: k.trades,
            is_closed: k.is_closed,
        }))
    }

    fn parse_trade(msg: &WsTradeMessage) -> Option<MarketEvent> {
        let pair = TradingPair::from_str(&msg.symbol)?;
        Some(MarketEvent::Trade(Trade {
            id: msg.trade_id.to_string(),
            pair,
            price: Decimal::from_str(&msg.price).ok()?,
            quantity: Decimal::from_str(&msg.quantity).ok()?,
            is_buyer_maker: msg.is_buyer_maker,
            timestamp: Utc.timestamp_millis_opt(msg.trade_time).unwrap(),
        }))
    }

    fn parse_book_ticker(msg: &WsBookTickerMessage) -> Option<MarketEvent> {
        let pair = TradingPair::from_str(&msg.symbol)?;
        Some(MarketEvent::BookTicker(BookTicker {
            pair,
            bid_price: Decimal::from_str(&msg.bid_price).ok()?,
            bid_qty: Decimal::from_str(&msg.bid_qty).ok()?,
            ask_price: Decimal::from_str(&msg.ask_price).ok()?,
            ask_qty: Decimal::from_str(&msg.ask_qty).ok()?,
        }))
    }
}

impl Default for BinanceWebSocket {
    fn default() -> Self {
        Self::new()
    }
}

// WebSocket Message Types
#[derive(Debug, Deserialize)]
struct CombinedStreamMessage {
    stream: String,
    data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct WsTickerMessage {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "c")]
    last_price: String,
    #[serde(rename = "b")]
    best_bid: String,
    #[serde(rename = "a")]
    best_ask: String,
    #[serde(rename = "v")]
    volume: String,
    #[serde(rename = "p")]
    price_change: String,
    #[serde(rename = "P")]
    price_change_percent: String,
    #[serde(rename = "h")]
    high_price: String,
    #[serde(rename = "l")]
    low_price: String,
}

#[derive(Debug, Deserialize)]
struct WsKlineMessage {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "k")]
    kline: WsKlineData,
}

#[derive(Debug, Deserialize)]
struct WsKlineData {
    #[serde(rename = "t")]
    start_time: i64,
    #[serde(rename = "T")]
    close_time: i64,
    #[serde(rename = "i")]
    interval: String,
    #[serde(rename = "o")]
    open: String,
    #[serde(rename = "h")]
    high: String,
    #[serde(rename = "l")]
    low: String,
    #[serde(rename = "c")]
    close: String,
    #[serde(rename = "v")]
    volume: String,
    #[serde(rename = "q")]
    quote_volume: String,
    #[serde(rename = "n")]
    trades: u64,
    #[serde(rename = "x")]
    is_closed: bool,
}

#[derive(Debug, Deserialize)]
struct WsTradeMessage {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "t")]
    trade_id: u64,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    quantity: String,
    #[serde(rename = "T")]
    trade_time: i64,
    #[serde(rename = "m")]
    is_buyer_maker: bool,
}

#[derive(Debug, Deserialize)]
struct WsBookTickerMessage {
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "b")]
    bid_price: String,
    #[serde(rename = "B")]
    bid_qty: String,
    #[serde(rename = "a")]
    ask_price: String,
    #[serde(rename = "A")]
    ask_qty: String,
}
