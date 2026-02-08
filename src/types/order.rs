use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Side, TradingPair};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
    StopLoss,
    StopLossLimit,
    TakeProfit,
    TakeProfitLimit,
    OCO,
}

impl OrderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            OrderType::Market => "MARKET",
            OrderType::Limit => "LIMIT",
            OrderType::StopLoss => "STOP_LOSS",
            OrderType::StopLossLimit => "STOP_LOSS_LIMIT",
            OrderType::TakeProfit => "TAKE_PROFIT",
            OrderType::TakeProfitLimit => "TAKE_PROFIT_LIMIT",
            OrderType::OCO => "OCO",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

impl OrderStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, OrderStatus::Pending | OrderStatus::Open | OrderStatus::PartiallyFilled)
    }

    pub fn is_final(&self) -> bool {
        matches!(self, OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    GTC, // Good Till Cancel
    IOC, // Immediate or Cancel
    FOK, // Fill or Kill
}

impl TimeInForce {
    pub fn as_str(&self) -> &'static str {
        match self {
            TimeInForce::GTC => "GTC",
            TimeInForce::IOC => "IOC",
            TimeInForce::FOK => "FOK",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub client_order_id: String,
    pub pair: TradingPair,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub time_in_force: Option<TimeInForce>,
}

impl OrderRequest {
    pub fn market(pair: TradingPair, side: Side, quantity: Decimal) -> Self {
        Self {
            client_order_id: Uuid::new_v4().to_string(),
            pair,
            side,
            order_type: OrderType::Market,
            quantity,
            price: None,
            stop_price: None,
            time_in_force: None,
        }
    }

    pub fn limit(pair: TradingPair, side: Side, quantity: Decimal, price: Decimal) -> Self {
        Self {
            client_order_id: Uuid::new_v4().to_string(),
            pair,
            side,
            order_type: OrderType::Limit,
            quantity,
            price: Some(price),
            stop_price: None,
            time_in_force: Some(TimeInForce::GTC),
        }
    }

    pub fn stop_loss(pair: TradingPair, side: Side, quantity: Decimal, stop_price: Decimal) -> Self {
        Self {
            client_order_id: Uuid::new_v4().to_string(),
            pair,
            side,
            order_type: OrderType::StopLoss,
            quantity,
            price: None,
            stop_price: Some(stop_price),
            time_in_force: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub client_order_id: String,
    pub exchange_order_id: Option<String>,
    pub pair: TradingPair,
    pub side: Side,
    pub order_type: OrderType,
    pub status: OrderStatus,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub price: Option<Decimal>,
    pub average_fill_price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub time_in_force: Option<TimeInForce>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub strategy_id: Option<String>,
}

impl Order {
    pub fn from_request(request: &OrderRequest) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            client_order_id: request.client_order_id.clone(),
            exchange_order_id: None,
            pair: request.pair,
            side: request.side,
            order_type: request.order_type,
            status: OrderStatus::Pending,
            quantity: request.quantity,
            filled_quantity: Decimal::ZERO,
            price: request.price,
            average_fill_price: None,
            stop_price: request.stop_price,
            time_in_force: request.time_in_force,
            created_at: now,
            updated_at: now,
            strategy_id: None,
        }
    }

    pub fn remaining_quantity(&self) -> Decimal {
        self.quantity - self.filled_quantity
    }

    pub fn fill_percentage(&self) -> Decimal {
        if self.quantity.is_zero() {
            Decimal::ZERO
        } else {
            (self.filled_quantity / self.quantity) * Decimal::from(100)
        }
    }

    pub fn notional_value(&self) -> Option<Decimal> {
        self.average_fill_price.map(|p| p * self.filled_quantity)
    }
}

/// OCO (One-Cancels-Other) order request for exchange-side stop-loss + take-profit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OCOOrderRequest {
    pub list_client_order_id: String,
    pub pair: TradingPair,
    pub side: Side,
    pub quantity: Decimal,
    pub price: Decimal,            // Limit price (take profit)
    pub stop_price: Decimal,       // Stop trigger price
    pub stop_limit_price: Decimal, // Stop limit execution price
}

impl OCOOrderRequest {
    pub fn new(
        pair: TradingPair,
        side: Side,
        quantity: Decimal,
        take_profit_price: Decimal,
        stop_loss_price: Decimal,
    ) -> Self {
        // Stop limit price slightly below stop price for fill probability
        let stop_limit_price = stop_loss_price * (Decimal::ONE - Decimal::new(1, 3)); // 0.1% below
        Self {
            list_client_order_id: Uuid::new_v4().to_string(),
            pair,
            side,
            quantity,
            price: take_profit_price,
            stop_price: stop_loss_price,
            stop_limit_price,
        }
    }
}

/// Response from an OCO order placement
#[derive(Debug, Clone)]
pub struct OCOOrderResult {
    pub list_order_id: String,
    pub list_client_order_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: String,
    pub trade_id: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub commission: Decimal,
    pub commission_asset: String,
    pub timestamp: DateTime<Utc>,
}
