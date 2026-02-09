#![allow(dead_code)]
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TradingPair {
    BTCUSDT,
    ETHUSDT,
    SOLUSDT,
    BNBUSDT,
    ADAUSDT,
    XRPUSDT,
}

impl TradingPair {
    pub fn as_str(&self) -> &'static str {
        match self {
            TradingPair::BTCUSDT => "BTCUSDT",
            TradingPair::ETHUSDT => "ETHUSDT",
            TradingPair::SOLUSDT => "SOLUSDT",
            TradingPair::BNBUSDT => "BNBUSDT",
            TradingPair::ADAUSDT => "ADAUSDT",
            TradingPair::XRPUSDT => "XRPUSDT",
        }
    }

    pub fn base_asset(&self) -> &'static str {
        match self {
            TradingPair::BTCUSDT => "BTC",
            TradingPair::ETHUSDT => "ETH",
            TradingPair::SOLUSDT => "SOL",
            TradingPair::BNBUSDT => "BNB",
            TradingPair::ADAUSDT => "ADA",
            TradingPair::XRPUSDT => "XRP",
        }
    }

    pub fn quote_asset(&self) -> &'static str {
        "USDT"
    }

    pub fn max_position_pct(&self) -> Decimal {
        match self {
            TradingPair::BTCUSDT => Decimal::new(40, 2), // 40%
            TradingPair::ETHUSDT => Decimal::new(30, 2), // 30%
            TradingPair::SOLUSDT => Decimal::new(25, 2), // 25%
            TradingPair::BNBUSDT => Decimal::new(20, 2), // 20%
            TradingPair::ADAUSDT => Decimal::new(15, 2), // 15%
            TradingPair::XRPUSDT => Decimal::new(15, 2), // 15%
        }
    }

    pub fn all() -> Vec<TradingPair> {
        vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ]
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "BTCUSDT" => Some(TradingPair::BTCUSDT),
            "ETHUSDT" => Some(TradingPair::ETHUSDT),
            "SOLUSDT" => Some(TradingPair::SOLUSDT),
            "BNBUSDT" => Some(TradingPair::BNBUSDT),
            "ADAUSDT" => Some(TradingPair::ADAUSDT),
            "XRPUSDT" => Some(TradingPair::XRPUSDT),
            _ => None,
        }
    }

    pub fn correlation_group(&self) -> &'static str {
        match self {
            TradingPair::BTCUSDT => "btc",
            TradingPair::ETHUSDT | TradingPair::SOLUSDT => "alt_major",
            _ => "alt_minor",
        }
    }

    pub fn min_notional(&self) -> Decimal {
        Decimal::new(10, 0) // $10 minimum for Binance.US
    }

    pub fn price_precision(&self) -> u32 {
        match self {
            TradingPair::BTCUSDT => 2,
            TradingPair::ETHUSDT => 2,
            TradingPair::SOLUSDT => 2,
            TradingPair::BNBUSDT => 2,
            TradingPair::ADAUSDT => 4,
            TradingPair::XRPUSDT => 4,
        }
    }

    pub fn quantity_precision(&self) -> u32 {
        match self {
            TradingPair::BTCUSDT => 5,
            TradingPair::ETHUSDT => 4,
            TradingPair::SOLUSDT => 2,
            TradingPair::BNBUSDT => 2,
            TradingPair::ADAUSDT => 1,
            TradingPair::XRPUSDT => 1,
        }
    }
}

impl fmt::Display for TradingPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(&self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeFrame {
    M1,   // 1 minute
    M5,   // 5 minutes
    M15,  // 15 minutes
    H1,   // 1 hour
    H4,   // 4 hours
    D1,   // 1 day
}

impl TimeFrame {
    pub fn as_str(&self) -> &'static str {
        match self {
            TimeFrame::M1 => "1m",
            TimeFrame::M5 => "5m",
            TimeFrame::M15 => "15m",
            TimeFrame::H1 => "1h",
            TimeFrame::H4 => "4h",
            TimeFrame::D1 => "1d",
        }
    }

    pub fn to_minutes(&self) -> u64 {
        match self {
            TimeFrame::M1 => 1,
            TimeFrame::M5 => 5,
            TimeFrame::M15 => 15,
            TimeFrame::H1 => 60,
            TimeFrame::H4 => 240,
            TimeFrame::D1 => 1440,
        }
    }

    pub fn to_milliseconds(&self) -> u64 {
        self.to_minutes() * 60 * 1000
    }
}

impl fmt::Display for TimeFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradingMode {
    Paper,
    Live,
}

impl fmt::Display for TradingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TradingMode::Paper => write!(f, "Paper"),
            TradingMode::Live => write!(f, "Live"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    StrongBuy,
    Buy,
    Neutral,
    Sell,
    StrongSell,
}

impl Signal {
    pub fn strength(&self) -> i8 {
        match self {
            Signal::StrongBuy => 2,
            Signal::Buy => 1,
            Signal::Neutral => 0,
            Signal::Sell => -1,
            Signal::StrongSell => -2,
        }
    }

    pub fn is_bullish(&self) -> bool {
        matches!(self, Signal::StrongBuy | Signal::Buy)
    }

    pub fn is_bearish(&self) -> bool {
        matches!(self, Signal::StrongSell | Signal::Sell)
    }
}
