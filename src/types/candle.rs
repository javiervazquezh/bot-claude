use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::{TimeFrame, TradingPair};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub pair: TradingPair,
    pub timeframe: TimeFrame,
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub quote_volume: Decimal,
    pub trades: u64,
    pub is_closed: bool,
}

impl Candle {
    pub fn body_size(&self) -> Decimal {
        (self.close - self.open).abs()
    }

    pub fn range(&self) -> Decimal {
        self.high - self.low
    }

    pub fn upper_wick(&self) -> Decimal {
        self.high - self.close.max(self.open)
    }

    pub fn lower_wick(&self) -> Decimal {
        self.close.min(self.open) - self.low
    }

    pub fn is_bullish(&self) -> bool {
        self.close > self.open
    }

    pub fn is_bearish(&self) -> bool {
        self.close < self.open
    }

    pub fn is_doji(&self) -> bool {
        let body = self.body_size();
        let range = self.range();
        if range.is_zero() {
            return true;
        }
        body / range < Decimal::new(1, 1) // Body less than 10% of range
    }

    pub fn body_percentage(&self) -> Decimal {
        let range = self.range();
        if range.is_zero() {
            return Decimal::ZERO;
        }
        (self.body_size() / range) * Decimal::from(100)
    }

    pub fn typical_price(&self) -> Decimal {
        (self.high + self.low + self.close) / Decimal::from(3)
    }

    pub fn hlc3(&self) -> Decimal {
        self.typical_price()
    }

    pub fn ohlc4(&self) -> Decimal {
        (self.open + self.high + self.low + self.close) / Decimal::from(4)
    }

    pub fn change(&self) -> Decimal {
        self.close - self.open
    }

    pub fn change_percentage(&self) -> Decimal {
        if self.open.is_zero() {
            return Decimal::ZERO;
        }
        ((self.close - self.open) / self.open) * Decimal::from(100)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CandleBuffer {
    pub candles: Vec<Candle>,
    pub max_size: usize,
}

impl CandleBuffer {
    pub fn new(max_size: usize) -> Self {
        Self {
            candles: Vec::with_capacity(max_size),
            max_size,
        }
    }

    pub fn push(&mut self, candle: Candle) {
        if self.candles.len() >= self.max_size {
            self.candles.remove(0);
        }
        self.candles.push(candle);
    }

    pub fn len(&self) -> usize {
        self.candles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candles.is_empty()
    }

    pub fn last(&self) -> Option<&Candle> {
        self.candles.last()
    }

    pub fn last_n(&self, n: usize) -> &[Candle] {
        let len = self.candles.len();
        if n >= len {
            &self.candles[..]
        } else {
            &self.candles[len - n..]
        }
    }

    pub fn closes(&self) -> Vec<Decimal> {
        self.candles.iter().map(|c| c.close).collect()
    }

    pub fn highs(&self) -> Vec<Decimal> {
        self.candles.iter().map(|c| c.high).collect()
    }

    pub fn lows(&self) -> Vec<Decimal> {
        self.candles.iter().map(|c| c.low).collect()
    }

    pub fn volumes(&self) -> Vec<Decimal> {
        self.candles.iter().map(|c| c.volume).collect()
    }

    pub fn typical_prices(&self) -> Vec<Decimal> {
        self.candles.iter().map(|c| c.typical_price()).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker {
    pub pair: TradingPair,
    pub price: Decimal,
    pub bid: Decimal,
    pub ask: Decimal,
    pub volume_24h: Decimal,
    pub price_change_24h: Decimal,
    pub price_change_pct_24h: Decimal,
    pub high_24h: Decimal,
    pub low_24h: Decimal,
    pub timestamp: DateTime<Utc>,
}

impl Ticker {
    pub fn spread(&self) -> Decimal {
        self.ask - self.bid
    }

    pub fn spread_percentage(&self) -> Decimal {
        if self.bid.is_zero() {
            return Decimal::ZERO;
        }
        (self.spread() / self.bid) * Decimal::from(100)
    }

    pub fn mid_price(&self) -> Decimal {
        (self.bid + self.ask) / Decimal::from(2)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub pair: TradingPair,
    pub price: Decimal,
    pub quantity: Decimal,
    pub is_buyer_maker: bool,
    pub timestamp: DateTime<Utc>,
}
