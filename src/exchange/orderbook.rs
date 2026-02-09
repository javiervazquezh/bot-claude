use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

use crate::types::TradingPair;

/// Order book level (bid or ask)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

/// Full order book snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub pair: TradingPair,
    pub timestamp: DateTime<Utc>,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
}

impl OrderBookSnapshot {
    /// Calculate mid price
    pub fn mid_price(&self) -> Option<Decimal> {
        let best_bid = self.bids.first()?.price;
        let best_ask = self.asks.first()?.price;
        Some((best_bid + best_ask) / dec!(2))
    }

    /// Calculate bid-ask spread percentage
    pub fn spread_pct(&self) -> Option<Decimal> {
        let best_bid = self.bids.first()?.price;
        let best_ask = self.asks.first()?.price;
        let spread = best_ask - best_bid;
        let mid = (best_bid + best_ask) / dec!(2);
        Some((spread / mid) * dec!(100))
    }

    /// Calculate depth imbalance across top N levels
    /// Returns value in [-1, 1]: positive = bid pressure, negative = ask pressure
    pub fn depth_imbalance(&self, levels: usize) -> Option<Decimal> {
        let bid_volume: Decimal = self.bids.iter()
            .take(levels)
            .map(|l| l.quantity)
            .sum();

        let ask_volume: Decimal = self.asks.iter()
            .take(levels)
            .map(|l| l.quantity)
            .sum();

        if bid_volume + ask_volume == dec!(0) {
            return Some(dec!(0));
        }

        Some((bid_volume - ask_volume) / (bid_volume + ask_volume))
    }

    /// Calculate book pressure (weighted by distance from mid)
    /// Closer levels get more weight
    pub fn book_pressure(&self) -> Option<Decimal> {
        let mid = self.mid_price()?;
        let mut bid_pressure = dec!(0);
        let mut ask_pressure = dec!(0);

        // Weight levels by inverse distance from mid
        for (i, bid) in self.bids.iter().take(5).enumerate() {
            let distance = mid - bid.price;
            if distance > dec!(0) {
                let weight = dec!(1) / (distance + dec!(0.0001));
                bid_pressure += bid.quantity * weight;
            }
        }

        for (i, ask) in self.asks.iter().take(5).enumerate() {
            let distance = ask.price - mid;
            if distance > dec!(0) {
                let weight = dec!(1) / (distance + dec!(0.0001));
                ask_pressure += ask.quantity * weight;
            }
        }

        let total = bid_pressure + ask_pressure;
        if total == dec!(0) {
            return Some(dec!(0));
        }

        Some((bid_pressure - ask_pressure) / total)
    }

    /// Calculate weighted spread (volume-weighted bid-ask spread)
    pub fn weighted_spread(&self) -> Option<Decimal> {
        let best_bid = self.bids.first()?;
        let best_ask = self.asks.first()?;

        let total_volume = best_bid.quantity + best_ask.quantity;
        if total_volume == dec!(0) {
            return self.spread_pct();
        }

        let spread = best_ask.price - best_bid.price;
        let mid = (best_bid.price + best_ask.price) / dec!(2);

        Some((spread / mid) * dec!(100))
    }

    /// Calculate ratio of volume at best bid/ask
    pub fn best_volume_ratio(&self) -> Option<Decimal> {
        let best_bid_qty = self.bids.first()?.quantity;
        let best_ask_qty = self.asks.first()?.quantity;

        if best_ask_qty == dec!(0) {
            return Some(dec!(10)); // Cap at 10 if no ask volume
        }

        let ratio = best_bid_qty / best_ask_qty;
        Some(ratio.min(dec!(10))) // Cap at 10
    }

    /// Calculate depth ratio (top 5 levels vs total depth)
    pub fn depth_ratio(&self) -> Option<Decimal> {
        let top5_bid: Decimal = self.bids.iter().take(5).map(|l| l.quantity).sum();
        let top5_ask: Decimal = self.asks.iter().take(5).map(|l| l.quantity).sum();
        let top5_total = top5_bid + top5_ask;

        let total_bid: Decimal = self.bids.iter().map(|l| l.quantity).sum();
        let total_ask: Decimal = self.asks.iter().map(|l| l.quantity).sum();
        let total = total_bid + total_ask;

        if total == dec!(0) {
            return Some(dec!(0));
        }

        Some(top5_total / total)
    }
}

/// Order book manager that maintains recent snapshots
pub struct OrderBookManager {
    snapshots: HashMap<TradingPair, VecDeque<OrderBookSnapshot>>,
    max_snapshots: usize,
}

impl OrderBookManager {
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: HashMap::new(),
            max_snapshots,
        }
    }

    /// Update order book with new snapshot
    pub fn update(&mut self, snapshot: OrderBookSnapshot) {
        let pair = snapshot.pair;
        let queue = self.snapshots.entry(pair).or_insert_with(VecDeque::new);

        queue.push_back(snapshot);

        // Keep only last N snapshots
        while queue.len() > self.max_snapshots {
            queue.pop_front();
        }
    }

    /// Get latest snapshot for a pair
    pub fn latest(&self, pair: TradingPair) -> Option<&OrderBookSnapshot> {
        self.snapshots.get(&pair)?.back()
    }

    /// Get all snapshots for a pair (up to max_snapshots)
    pub fn history(&self, pair: TradingPair) -> Option<&VecDeque<OrderBookSnapshot>> {
        self.snapshots.get(&pair)
    }

    /// Calculate mid-price momentum (% change over last N snapshots)
    pub fn mid_price_momentum(&self, pair: TradingPair, lookback: usize) -> Option<Decimal> {
        let history = self.history(pair)?;
        if history.len() < 2 {
            return Some(dec!(0));
        }

        let current = history.back()?.mid_price()?;
        let past = history.iter()
            .rev()
            .nth(lookback.min(history.len() - 1))?
            .mid_price()?;

        if past == dec!(0) {
            return Some(dec!(0));
        }

        Some(((current - past) / past) * dec!(100))
    }

    /// Calculate spread volatility (std dev of spreads over last N snapshots)
    pub fn spread_volatility(&self, pair: TradingPair) -> Option<Decimal> {
        let history = self.history(pair)?;
        if history.len() < 2 {
            return Some(dec!(0));
        }

        let spreads: Vec<Decimal> = history.iter()
            .filter_map(|s| s.spread_pct())
            .collect();

        if spreads.is_empty() {
            return Some(dec!(0));
        }

        let mean: Decimal = spreads.iter().sum::<Decimal>() / Decimal::from(spreads.len());

        let variance: Decimal = spreads.iter()
            .map(|s| {
                let diff = *s - mean;
                diff * diff
            })
            .sum::<Decimal>() / Decimal::from(spreads.len());

        // Approximate sqrt using Newton's method (since Decimal doesn't have sqrt)
        Some(Self::decimal_sqrt(variance))
    }

    /// Approximate square root for Decimal
    fn decimal_sqrt(value: Decimal) -> Decimal {
        if value <= dec!(0) {
            return dec!(0);
        }

        let mut x = value;
        let mut x_prev = dec!(0);

        // Newton's method: x_new = (x + value/x) / 2
        for _ in 0..10 {
            if (x - x_prev).abs() < dec!(0.0001) {
                break;
            }
            x_prev = x;
            x = (x + value / x) / dec!(2);
        }

        x
    }
}

/// Extract all 8 order book features from current state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookFeatures {
    pub spread_pct: Decimal,
    pub depth_imbalance: Decimal,
    pub mid_price_momentum: Decimal,
    pub spread_volatility: Decimal,
    pub book_pressure: Decimal,
    pub weighted_spread: Decimal,
    pub best_volume_ratio: Decimal,
    pub depth_ratio: Decimal,
}

impl OrderBookFeatures {
    pub fn extract(manager: &OrderBookManager, pair: TradingPair) -> Option<Self> {
        let latest = manager.latest(pair)?;

        Some(Self {
            spread_pct: latest.spread_pct().unwrap_or(dec!(0)),
            depth_imbalance: latest.depth_imbalance(5).unwrap_or(dec!(0)),
            mid_price_momentum: manager.mid_price_momentum(pair, 10).unwrap_or(dec!(0)),
            spread_volatility: manager.spread_volatility(pair).unwrap_or(dec!(0)),
            book_pressure: latest.book_pressure().unwrap_or(dec!(0)),
            weighted_spread: latest.weighted_spread().unwrap_or(dec!(0)),
            best_volume_ratio: latest.best_volume_ratio().unwrap_or(dec!(1)),
            depth_ratio: latest.depth_ratio().unwrap_or(dec!(1)),
        })
    }

    /// Convert to vector for ML features (preserves order)
    pub fn to_vec(&self) -> Vec<Decimal> {
        vec![
            self.spread_pct,
            self.depth_imbalance,
            self.mid_price_momentum,
            self.spread_volatility,
            self.book_pressure,
            self.weighted_spread,
            self.best_volume_ratio,
            self.depth_ratio,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_snapshot() -> OrderBookSnapshot {
        OrderBookSnapshot {
            pair: TradingPair::BTCUSD,
            timestamp: Utc::now(),
            bids: vec![
                OrderBookLevel { price: dec!(50000), quantity: dec!(1.0) },
                OrderBookLevel { price: dec!(49990), quantity: dec!(0.5) },
                OrderBookLevel { price: dec!(49980), quantity: dec!(0.3) },
            ],
            asks: vec![
                OrderBookLevel { price: dec!(50010), quantity: dec!(0.8) },
                OrderBookLevel { price: dec!(50020), quantity: dec!(0.4) },
                OrderBookLevel { price: dec!(50030), quantity: dec!(0.2) },
            ],
        }
    }

    #[test]
    fn test_mid_price() {
        let snapshot = mock_snapshot();
        let mid = snapshot.mid_price().unwrap();
        assert_eq!(mid, dec!(50005)); // (50000 + 50010) / 2
    }

    #[test]
    fn test_spread_pct() {
        let snapshot = mock_snapshot();
        let spread = snapshot.spread_pct().unwrap();
        // (50010 - 50000) / 50005 * 100 ≈ 0.02%
        assert!(spread > dec!(0.019) && spread < dec!(0.021));
    }

    #[test]
    fn test_depth_imbalance() {
        let snapshot = mock_snapshot();
        let imbalance = snapshot.depth_imbalance(3).unwrap();
        // Bid volume: 1.0 + 0.5 + 0.3 = 1.8
        // Ask volume: 0.8 + 0.4 + 0.2 = 1.4
        // Imbalance: (1.8 - 1.4) / (1.8 + 1.4) = 0.4 / 3.2 = 0.125
        assert!(imbalance > dec!(0.12) && imbalance < dec!(0.13));
    }

    #[test]
    fn test_order_book_manager() {
        let mut manager = OrderBookManager::new(50);
        let snapshot = mock_snapshot();

        manager.update(snapshot.clone());

        let latest = manager.latest(TradingPair::BTCUSD).unwrap();
        assert_eq!(latest.pair, TradingPair::BTCUSD);
    }

    #[test]
    fn test_feature_extraction() {
        let mut manager = OrderBookManager::new(50);
        let snapshot = mock_snapshot();

        manager.update(snapshot);

        let features = OrderBookFeatures::extract(&manager, TradingPair::BTCUSD).unwrap();
        assert!(features.spread_pct > dec!(0));
        assert!(features.depth_imbalance.abs() <= dec!(1));
    }
}
