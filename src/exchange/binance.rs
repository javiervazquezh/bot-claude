#![allow(dead_code)]
use anyhow::{anyhow, Result};
use chrono::{DateTime, TimeZone, Utc};
use hmac::{Hmac, Mac};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::str::FromStr;
use tracing::{debug, info};

use crate::types::{
    Candle, OCOOrderRequest, OCOOrderResult, Order, OrderRequest, OrderStatus, OrderType, Side,
    Ticker, TimeFrame, TimeInForce, TradingPair,
};

const BINANCE_US_API: &str = "https://api.binance.us";
const BINANCE_US_TESTNET: &str = "https://testnet.binance.vision";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct BinanceClient {
    client: Client,
    api_key: String,
    secret_key: String,
    base_url: String,
    use_testnet: bool,
}

impl BinanceClient {
    pub fn new(api_key: String, secret_key: String, use_testnet: bool) -> Self {
        let base_url = if use_testnet {
            BINANCE_US_TESTNET.to_string()
        } else {
            BINANCE_US_API.to_string()
        };

        Self {
            client: Client::new(),
            api_key,
            secret_key,
            base_url,
            use_testnet,
        }
    }

    pub fn public_only() -> Self {
        Self {
            client: Client::new(),
            api_key: String::new(),
            secret_key: String::new(),
            base_url: BINANCE_US_API.to_string(),
            use_testnet: false,
        }
    }

    fn sign(&self, query: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn build_signed_query(&self, params: &HashMap<&str, String>) -> String {
        let timestamp = Utc::now().timestamp_millis();
        let mut query_parts: Vec<String> = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        query_parts.push(format!("timestamp={}", timestamp));
        query_parts.push(format!("recvWindow=5000"));
        let query = query_parts.join("&");
        let signature = self.sign(&query);
        format!("{}&signature={}", query, signature)
    }

    pub async fn get_server_time(&self) -> Result<u64> {
        let url = format!("{}/api/v3/time", self.base_url);
        let resp: ServerTimeResponse = self.client.get(&url).send().await?.json().await?;
        Ok(resp.server_time)
    }

    pub async fn get_ticker(&self, pair: TradingPair) -> Result<Ticker> {
        let url = format!(
            "{}/api/v3/ticker/24hr?symbol={}",
            self.base_url,
            pair.as_str()
        );
        let resp: TickerResponse = self.client.get(&url).send().await?.json().await?;

        Ok(Ticker {
            pair,
            price: Decimal::from_str(&resp.last_price)?,
            bid: Decimal::from_str(&resp.bid_price)?,
            ask: Decimal::from_str(&resp.ask_price)?,
            volume_24h: Decimal::from_str(&resp.volume)?,
            price_change_24h: Decimal::from_str(&resp.price_change)?,
            price_change_pct_24h: Decimal::from_str(&resp.price_change_percent)?,
            high_24h: Decimal::from_str(&resp.high_price)?,
            low_24h: Decimal::from_str(&resp.low_price)?,
            timestamp: Utc::now(),
        })
    }

    pub async fn get_candles(
        &self,
        pair: TradingPair,
        timeframe: TimeFrame,
        limit: u32,
    ) -> Result<Vec<Candle>> {
        let url = format!(
            "{}/api/v3/klines?symbol={}&interval={}&limit={}",
            self.base_url,
            pair.as_str(),
            timeframe.as_str(),
            limit
        );

        let resp: Vec<Vec<serde_json::Value>> = self.client.get(&url).send().await?.json().await?;

        let candles: Result<Vec<Candle>> = resp
            .into_iter()
            .map(|k| {
                let open_time = k.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
                let open = k.get(1).and_then(|v| v.as_str()).unwrap_or("0");
                let high = k.get(2).and_then(|v| v.as_str()).unwrap_or("0");
                let low = k.get(3).and_then(|v| v.as_str()).unwrap_or("0");
                let close = k.get(4).and_then(|v| v.as_str()).unwrap_or("0");
                let volume = k.get(5).and_then(|v| v.as_str()).unwrap_or("0");
                let close_time = k.get(6).and_then(|v| v.as_i64()).unwrap_or(0);
                let quote_volume = k.get(7).and_then(|v| v.as_str()).unwrap_or("0");
                let trades = k.get(8).and_then(|v| v.as_u64()).unwrap_or(0);

                Ok(Candle {
                    pair,
                    timeframe,
                    open_time: Utc.timestamp_millis_opt(open_time).unwrap(),
                    close_time: Utc.timestamp_millis_opt(close_time).unwrap(),
                    open: Decimal::from_str(open)?,
                    high: Decimal::from_str(high)?,
                    low: Decimal::from_str(low)?,
                    close: Decimal::from_str(close)?,
                    volume: Decimal::from_str(volume)?,
                    quote_volume: Decimal::from_str(quote_volume)?,
                    trades,
                    is_closed: true,
                })
            })
            .collect();

        candles
    }

    /// Fetches historical candles between two dates with automatic pagination.
    /// Binance limits to 1000 candles per request, so this handles pagination.
    pub async fn get_historical_candles(
        &self,
        pair: TradingPair,
        timeframe: TimeFrame,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<Candle>> {
        let mut all_candles = Vec::new();
        let mut current_start = start_time.timestamp_millis();
        let end_millis = end_time.timestamp_millis();

        info!(
            "Fetching historical candles for {} from {} to {}",
            pair, start_time, end_time
        );

        loop {
            let url = format!(
                "{}/api/v3/klines?symbol={}&interval={}&startTime={}&endTime={}&limit=1000",
                self.base_url,
                pair.as_str(),
                timeframe.as_str(),
                current_start,
                end_millis
            );

            let resp: Vec<Vec<serde_json::Value>> = self.client.get(&url).send().await?.json().await?;

            if resp.is_empty() {
                break;
            }

            let batch_len = resp.len();

            let batch: Result<Vec<Candle>> = resp
                .into_iter()
                .map(|k| {
                    let open_time = k.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
                    let open = k.get(1).and_then(|v| v.as_str()).unwrap_or("0");
                    let high = k.get(2).and_then(|v| v.as_str()).unwrap_or("0");
                    let low = k.get(3).and_then(|v| v.as_str()).unwrap_or("0");
                    let close = k.get(4).and_then(|v| v.as_str()).unwrap_or("0");
                    let volume = k.get(5).and_then(|v| v.as_str()).unwrap_or("0");
                    let close_time = k.get(6).and_then(|v| v.as_i64()).unwrap_or(0);
                    let quote_volume = k.get(7).and_then(|v| v.as_str()).unwrap_or("0");
                    let trades = k.get(8).and_then(|v| v.as_u64()).unwrap_or(0);

                    Ok(Candle {
                        pair,
                        timeframe,
                        open_time: Utc.timestamp_millis_opt(open_time).unwrap(),
                        close_time: Utc.timestamp_millis_opt(close_time).unwrap(),
                        open: Decimal::from_str(open)?,
                        high: Decimal::from_str(high)?,
                        low: Decimal::from_str(low)?,
                        close: Decimal::from_str(close)?,
                        volume: Decimal::from_str(volume)?,
                        quote_volume: Decimal::from_str(quote_volume)?,
                        trades,
                        is_closed: true,
                    })
                })
                .collect();

            let candles = batch?;

            if let Some(last) = candles.last() {
                current_start = last.close_time.timestamp_millis() + 1;
            }

            all_candles.extend(candles);

            // If we got fewer than 1000 candles, we've reached the end
            if batch_len < 1000 || current_start >= end_millis {
                break;
            }

            // Rate limiting - avoid hitting API limits
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        info!("Fetched {} candles for {}", all_candles.len(), pair);
        Ok(all_candles)
    }

    pub async fn get_order_book(&self, pair: TradingPair, limit: u32) -> Result<OrderBook> {
        let url = format!(
            "{}/api/v3/depth?symbol={}&limit={}",
            self.base_url,
            pair.as_str(),
            limit
        );

        let resp: OrderBookResponse = self.client.get(&url).send().await?.json().await?;

        let bids: Result<Vec<(Decimal, Decimal)>> = resp
            .bids
            .into_iter()
            .map(|(price, qty)| Ok((Decimal::from_str(&price)?, Decimal::from_str(&qty)?)))
            .collect();

        let asks: Result<Vec<(Decimal, Decimal)>> = resp
            .asks
            .into_iter()
            .map(|(price, qty)| Ok((Decimal::from_str(&price)?, Decimal::from_str(&qty)?)))
            .collect();

        Ok(OrderBook {
            pair,
            bids: bids?,
            asks: asks?,
            last_update_id: resp.last_update_id,
        })
    }

    pub async fn place_order(&self, request: &OrderRequest) -> Result<Order> {
        let url = format!("{}/api/v3/order", self.base_url);

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("symbol", request.pair.as_str().to_string());
        params.insert("side", request.side.as_str().to_string());
        params.insert("type", request.order_type.as_str().to_string());
        params.insert("quantity", request.quantity.to_string());
        params.insert("newClientOrderId", request.client_order_id.clone());

        if let Some(price) = request.price {
            params.insert("price", price.to_string());
        }

        if let Some(stop_price) = request.stop_price {
            params.insert("stopPrice", stop_price.to_string());
        }

        if let Some(tif) = request.time_in_force {
            params.insert("timeInForce", tif.as_str().to_string());
        }

        let query = self.build_signed_query(&params);
        let full_url = format!("{}?{}", url, query);

        debug!("Placing order: {:?}", request);

        let resp = self
            .client
            .post(&full_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await?;
            return Err(anyhow!("Order placement failed: {}", error_text));
        }

        let order_resp: OrderResponse = resp.json().await?;
        self.convert_order_response(order_resp, request.pair)
    }

    /// Place an OCO (One-Cancels-Other) order for exchange-side stop-loss + take-profit
    pub async fn place_oco_order(&self, request: &OCOOrderRequest) -> Result<OCOOrderResult> {
        let url = format!("{}/api/v3/orderList/oco", self.base_url);

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("symbol", request.pair.as_str().to_string());
        params.insert("side", request.side.as_str().to_string());
        params.insert("quantity", request.quantity.to_string());
        params.insert("price", request.price.to_string());
        params.insert("stopPrice", request.stop_price.to_string());
        params.insert("stopLimitPrice", request.stop_limit_price.to_string());
        params.insert("stopLimitTimeInForce", "GTC".to_string());
        params.insert("listClientOrderId", request.list_client_order_id.clone());

        let query = self.build_signed_query(&params);
        let full_url = format!("{}?{}", url, query);

        debug!("Placing OCO order: {:?}", request);

        let resp = self
            .client
            .post(&full_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await?;
            return Err(anyhow!("OCO order placement failed: {}", error_text));
        }

        let oco_resp: serde_json::Value = resp.json().await?;
        let list_order_id = oco_resp["orderListId"]
            .as_u64()
            .map(|id| id.to_string())
            .unwrap_or_default();

        info!(
            "OCO order placed: SL={}, TP={}, list_id={}",
            request.stop_price, request.price, list_order_id
        );

        Ok(OCOOrderResult {
            list_order_id,
            list_client_order_id: request.list_client_order_id.clone(),
        })
    }

    /// Cancel an OCO order list
    pub async fn cancel_oco_order(&self, pair: TradingPair, list_client_order_id: &str) -> Result<()> {
        let url = format!("{}/api/v3/orderList", self.base_url);

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("symbol", pair.as_str().to_string());
        params.insert("listClientOrderId", list_client_order_id.to_string());

        let query = self.build_signed_query(&params);
        let full_url = format!("{}?{}", url, query);

        let resp = self
            .client
            .delete(&full_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await?;
            return Err(anyhow!("OCO cancellation failed: {}", error_text));
        }

        info!("OCO order {} cancelled", list_client_order_id);
        Ok(())
    }

    pub async fn cancel_order(&self, pair: TradingPair, order_id: &str) -> Result<()> {
        let url = format!("{}/api/v3/order", self.base_url);

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("symbol", pair.as_str().to_string());
        params.insert("origClientOrderId", order_id.to_string());

        let query = self.build_signed_query(&params);
        let full_url = format!("{}?{}", url, query);

        let resp = self
            .client
            .delete(&full_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await?;
            return Err(anyhow!("Order cancellation failed: {}", error_text));
        }

        info!("Order {} cancelled", order_id);
        Ok(())
    }

    pub async fn get_order(&self, pair: TradingPair, order_id: &str) -> Result<Order> {
        let url = format!("{}/api/v3/order", self.base_url);

        let mut params: HashMap<&str, String> = HashMap::new();
        params.insert("symbol", pair.as_str().to_string());
        params.insert("origClientOrderId", order_id.to_string());

        let query = self.build_signed_query(&params);
        let full_url = format!("{}?{}", url, query);

        let resp = self
            .client
            .get(&full_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await?;
            return Err(anyhow!("Get order failed: {}", error_text));
        }

        let order_resp: OrderResponse = resp.json().await?;
        self.convert_order_response(order_resp, pair)
    }

    pub async fn get_balance(&self, asset: &str) -> Result<Decimal> {
        let url = format!("{}/api/v3/account", self.base_url);

        let params: HashMap<&str, String> = HashMap::new();
        let query = self.build_signed_query(&params);
        let full_url = format!("{}?{}", url, query);

        let resp = self
            .client
            .get(&full_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await?;
            return Err(anyhow!("Get balance failed: {}", error_text));
        }

        let account: AccountResponse = resp.json().await?;

        for balance in account.balances {
            if balance.asset == asset {
                return Ok(Decimal::from_str(&balance.free)?);
            }
        }

        Ok(Decimal::ZERO)
    }

    pub async fn get_all_balances(&self) -> Result<HashMap<String, (Decimal, Decimal)>> {
        let url = format!("{}/api/v3/account", self.base_url);

        let params: HashMap<&str, String> = HashMap::new();
        let query = self.build_signed_query(&params);
        let full_url = format!("{}?{}", url, query);

        let resp = self
            .client
            .get(&full_url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let error_text = resp.text().await?;
            return Err(anyhow!("Get balances failed: {}", error_text));
        }

        let account: AccountResponse = resp.json().await?;

        let mut balances = HashMap::new();
        for balance in account.balances {
            let free = Decimal::from_str(&balance.free).unwrap_or(Decimal::ZERO);
            let locked = Decimal::from_str(&balance.locked).unwrap_or(Decimal::ZERO);
            if !free.is_zero() || !locked.is_zero() {
                balances.insert(balance.asset, (free, locked));
            }
        }

        Ok(balances)
    }

    fn convert_order_response(&self, resp: OrderResponse, pair: TradingPair) -> Result<Order> {
        let status = match resp.status.as_str() {
            "NEW" => OrderStatus::Open,
            "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
            "FILLED" => OrderStatus::Filled,
            "CANCELED" => OrderStatus::Cancelled,
            "REJECTED" => OrderStatus::Rejected,
            "EXPIRED" => OrderStatus::Expired,
            _ => OrderStatus::Pending,
        };

        let order_type = match resp.order_type.as_str() {
            "MARKET" => OrderType::Market,
            "LIMIT" => OrderType::Limit,
            "STOP_LOSS" => OrderType::StopLoss,
            "STOP_LOSS_LIMIT" => OrderType::StopLossLimit,
            "TAKE_PROFIT" => OrderType::TakeProfit,
            "TAKE_PROFIT_LIMIT" => OrderType::TakeProfitLimit,
            _ => OrderType::Market,
        };

        let side = match resp.side.as_str() {
            "BUY" => Side::Buy,
            _ => Side::Sell,
        };

        let time_in_force = resp.time_in_force.as_ref().and_then(|tif| match tif.as_str() {
            "GTC" => Some(TimeInForce::GTC),
            "IOC" => Some(TimeInForce::IOC),
            "FOK" => Some(TimeInForce::FOK),
            _ => None,
        });

        Ok(Order {
            id: resp.order_id.to_string(),
            client_order_id: resp.client_order_id,
            exchange_order_id: Some(resp.order_id.to_string()),
            pair,
            side,
            order_type,
            status,
            quantity: Decimal::from_str(&resp.orig_qty)?,
            filled_quantity: Decimal::from_str(&resp.executed_qty)?,
            price: resp.price.as_ref().and_then(|p| Decimal::from_str(p).ok()),
            average_fill_price: resp
                .avg_price
                .as_ref()
                .and_then(|p| Decimal::from_str(p).ok())
                .or_else(|| {
                    resp.cummulative_quote_qty
                        .as_ref()
                        .and_then(|q| Decimal::from_str(q).ok())
                        .and_then(|quote| {
                            let qty = Decimal::from_str(&resp.executed_qty).ok()?;
                            if qty.is_zero() {
                                None
                            } else {
                                Some(quote / qty)
                            }
                        })
                }),
            stop_price: resp
                .stop_price
                .as_ref()
                .and_then(|p| Decimal::from_str(p).ok()),
            time_in_force,
            created_at: Utc.timestamp_millis_opt(resp.transact_time.unwrap_or(0)).unwrap(),
            updated_at: Utc::now(),
            strategy_id: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct OrderBook {
    pub pair: TradingPair,
    pub bids: Vec<(Decimal, Decimal)>,
    pub asks: Vec<(Decimal, Decimal)>,
    pub last_update_id: u64,
}

impl OrderBook {
    pub fn best_bid(&self) -> Option<(Decimal, Decimal)> {
        self.bids.first().copied()
    }

    pub fn best_ask(&self) -> Option<(Decimal, Decimal)> {
        self.asks.first().copied()
    }

    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some((bid, _)), Some((ask, _))) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some((bid, _)), Some((ask, _))) => Some((bid + ask) / Decimal::from(2)),
            _ => None,
        }
    }
}

// API Response Types
#[derive(Debug, Deserialize)]
struct ServerTimeResponse {
    #[serde(rename = "serverTime")]
    server_time: u64,
}

#[derive(Debug, Deserialize)]
struct TickerResponse {
    #[serde(rename = "lastPrice")]
    last_price: String,
    #[serde(rename = "bidPrice")]
    bid_price: String,
    #[serde(rename = "askPrice")]
    ask_price: String,
    volume: String,
    #[serde(rename = "priceChange")]
    price_change: String,
    #[serde(rename = "priceChangePercent")]
    price_change_percent: String,
    #[serde(rename = "highPrice")]
    high_price: String,
    #[serde(rename = "lowPrice")]
    low_price: String,
}

#[derive(Debug, Deserialize)]
struct KlineResponse {
    #[serde(rename = "0")]
    open_time: i64,
    #[serde(rename = "1")]
    open: String,
    #[serde(rename = "2")]
    high: String,
    #[serde(rename = "3")]
    low: String,
    #[serde(rename = "4")]
    close: String,
    #[serde(rename = "5")]
    volume: String,
    #[serde(rename = "6")]
    close_time: i64,
    #[serde(rename = "7")]
    quote_volume: String,
    #[serde(rename = "8")]
    trades: u64,
}

#[derive(Debug, Deserialize)]
struct OrderBookResponse {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<(String, String)>,
    asks: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
struct OrderResponse {
    #[serde(rename = "orderId")]
    order_id: u64,
    #[serde(rename = "clientOrderId")]
    client_order_id: String,
    status: String,
    #[serde(rename = "type")]
    order_type: String,
    side: String,
    #[serde(rename = "origQty")]
    orig_qty: String,
    #[serde(rename = "executedQty")]
    executed_qty: String,
    price: Option<String>,
    #[serde(rename = "avgPrice")]
    avg_price: Option<String>,
    #[serde(rename = "cummulativeQuoteQty")]
    cummulative_quote_qty: Option<String>,
    #[serde(rename = "stopPrice")]
    stop_price: Option<String>,
    #[serde(rename = "timeInForce")]
    time_in_force: Option<String>,
    #[serde(rename = "transactTime")]
    transact_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AccountResponse {
    balances: Vec<BalanceResponse>,
}

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    asset: String,
    free: String,
    locked: String,
}
