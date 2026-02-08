use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Side, TradingPair};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionStatus {
    Open,
    Closed,
    Liquidated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: String,
    pub pair: TradingPair,
    pub side: Side,
    pub status: PositionStatus,
    pub entry_price: Decimal,
    pub current_price: Decimal,
    pub quantity: Decimal,
    pub stop_loss: Option<Decimal>,
    pub take_profit: Option<Decimal>,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
    #[serde(default)]
    pub peak_pnl_pct: Decimal,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub strategy_id: String,
    pub order_ids: Vec<String>,
    #[serde(default)]
    pub oco_order_id: Option<String>,
}

impl Position {
    pub fn new(
        pair: TradingPair,
        side: Side,
        entry_price: Decimal,
        quantity: Decimal,
        strategy_id: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            pair,
            side,
            status: PositionStatus::Open,
            entry_price,
            current_price: entry_price,
            quantity,
            stop_loss: None,
            take_profit: None,
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            peak_pnl_pct: Decimal::ZERO,
            opened_at: Utc::now(),
            closed_at: None,
            strategy_id,
            order_ids: Vec::new(),
            oco_order_id: None,
        }
    }

    pub fn update_price(&mut self, price: Decimal) {
        self.current_price = price;
        self.unrealized_pnl = self.calculate_pnl(price);
        let pnl_pct = self.pnl_percentage();
        if pnl_pct > self.peak_pnl_pct {
            self.peak_pnl_pct = pnl_pct;
        }
    }

    pub fn calculate_pnl(&self, price: Decimal) -> Decimal {
        let price_diff = price - self.entry_price;
        match self.side {
            Side::Buy => price_diff * self.quantity,
            Side::Sell => -price_diff * self.quantity,
        }
    }

    pub fn pnl_percentage(&self) -> Decimal {
        if self.entry_price.is_zero() {
            return Decimal::ZERO;
        }
        let entry_value = self.entry_price * self.quantity;
        if entry_value.is_zero() {
            return Decimal::ZERO;
        }
        (self.unrealized_pnl / entry_value) * Decimal::from(100)
    }

    pub fn notional_value(&self) -> Decimal {
        self.current_price * self.quantity
    }

    pub fn entry_value(&self) -> Decimal {
        self.entry_price * self.quantity
    }

    pub fn should_stop_loss(&self) -> bool {
        if let Some(sl) = self.stop_loss {
            match self.side {
                Side::Buy => self.current_price <= sl,
                Side::Sell => self.current_price >= sl,
            }
        } else {
            false
        }
    }

    pub fn should_take_profit(&self) -> bool {
        if let Some(tp) = self.take_profit {
            match self.side {
                Side::Buy => self.current_price >= tp,
                Side::Sell => self.current_price <= tp,
            }
        } else {
            false
        }
    }

    pub fn close(&mut self, exit_price: Decimal) {
        self.current_price = exit_price;
        self.realized_pnl = self.calculate_pnl(exit_price);
        self.unrealized_pnl = Decimal::ZERO;
        self.peak_pnl_pct = Decimal::ZERO;
        self.status = PositionStatus::Closed;
        self.closed_at = Some(Utc::now());
    }

    pub fn with_stop_loss(mut self, stop_loss: Decimal) -> Self {
        self.stop_loss = Some(stop_loss);
        self
    }

    pub fn with_take_profit(mut self, take_profit: Decimal) -> Self {
        self.take_profit = Some(take_profit);
        self
    }

    pub fn duration(&self) -> chrono::Duration {
        let end = self.closed_at.unwrap_or_else(Utc::now);
        end - self.opened_at
    }

    pub fn is_profitable(&self) -> bool {
        self.unrealized_pnl > Decimal::ZERO || self.realized_pnl > Decimal::ZERO
    }

}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub timestamp: DateTime<Utc>,
    pub total_equity: Decimal,
    pub available_balance: Decimal,
    pub total_unrealized_pnl: Decimal,
    pub total_realized_pnl: Decimal,
    pub positions: Vec<Position>,
    pub daily_pnl: Decimal,
    pub daily_pnl_percentage: Decimal,
}

impl PortfolioSnapshot {
    pub fn position_count(&self) -> usize {
        self.positions.iter().filter(|p| p.status == PositionStatus::Open).count()
    }

    pub fn total_position_value(&self) -> Decimal {
        self.positions
            .iter()
            .filter(|p| p.status == PositionStatus::Open)
            .map(|p| p.notional_value())
            .sum()
    }
}
