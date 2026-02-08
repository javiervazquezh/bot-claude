use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::RuntimeConfig;
use crate::engine::Portfolio;
use crate::types::{PositionStatus, TradingPair};

pub struct RiskManager {
    config: Arc<RwLock<RuntimeConfig>>,
    daily_loss: RwLock<Decimal>,
    daily_loss_reset: RwLock<chrono::NaiveDate>,
}

impl RiskManager {
    pub fn new(config: Arc<RwLock<RuntimeConfig>>) -> Self {
        Self {
            config,
            daily_loss: RwLock::new(Decimal::ZERO),
            daily_loss_reset: RwLock::new(chrono::Utc::now().date_naive()),
        }
    }

    async fn check_daily_reset(&self) {
        let today = chrono::Utc::now().date_naive();
        let mut reset_date = self.daily_loss_reset.write().await;
        if today != *reset_date {
            *self.daily_loss.write().await = Decimal::ZERO;
            *reset_date = today;
            info!("Daily loss counter reset");
        }
    }

    pub async fn record_loss(&self, loss: Decimal) {
        self.check_daily_reset().await;
        if loss < Decimal::ZERO {
            *self.daily_loss.write().await += loss.abs();
        }
    }

    pub async fn can_open_position(&self, portfolio: &Portfolio, pair: TradingPair) -> bool {
        let config = self.config.read().await;
        let limits = &config.risk;

        // Check max positions
        let open_positions = portfolio.position_count();
        if open_positions >= limits.max_positions {
            debug!("Max positions reached: {}/{}", open_positions, limits.max_positions);
            return false;
        }

        // Check if already have position in this pair
        if portfolio.has_open_position(pair) {
            debug!("Already have position in {}", pair);
            return false;
        }

        // Check correlation group limits (max 2 in same group)
        let group = pair.correlation_group();
        let correlated_count = portfolio.positions.values()
            .filter(|p| p.status == PositionStatus::Open && p.pair.correlation_group() == group)
            .count();
        if correlated_count >= 2 {
            debug!("Correlation limit: {} positions in '{}' group", correlated_count, group);
            return false;
        }

        // Check max drawdown
        if portfolio.max_drawdown > limits.max_drawdown_pct {
            warn!("Max drawdown exceeded: {:.2}% > {:.2}%",
                portfolio.max_drawdown, limits.max_drawdown_pct);
            return false;
        }

        // Check daily loss limit
        let daily_loss = *self.daily_loss.read().await;
        let daily_loss_pct = if !portfolio.initial_capital.is_zero() {
            (daily_loss / portfolio.initial_capital) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        if daily_loss_pct > limits.max_daily_loss_pct {
            warn!("Daily loss limit exceeded: {:.2}% > {:.2}%",
                daily_loss_pct, limits.max_daily_loss_pct);
            return false;
        }

        true
    }

    pub async fn calculate_position_size(
        &self,
        portfolio: &Portfolio,
        pair: TradingPair,
        entry_price: Decimal,
        stop_loss: Option<Decimal>,
    ) -> Decimal {
        let config = self.config.read().await;
        let limits = &config.risk;

        let available = portfolio.available_usdt();
        let total_equity = portfolio.initial_capital + portfolio.total_pnl;

        // Calculate maximum position value based on pair allocation
        let max_allocation = pair.max_position_pct();
        let max_position_value = total_equity * max_allocation;

        // Calculate risk-based position size if stop loss provided
        let risk_based_size = if let Some(sl) = stop_loss {
            let risk_per_trade = total_equity * limits.risk_per_trade_pct / Decimal::from(100);
            let risk_per_unit = (entry_price - sl).abs();

            if risk_per_unit.is_zero() {
                Decimal::ZERO
            } else {
                (risk_per_trade / risk_per_unit).min(max_position_value / entry_price)
            }
        } else {
            // Default to max allocation if no stop loss
            max_position_value / entry_price
        };

        // Apply minimum trade size
        let min_notional = pair.min_notional();
        let min_quantity = min_notional / entry_price;

        // Use the smaller of available capital and calculated size
        let max_from_available = available * limits.max_single_position_pct / Decimal::from(100);
        let position_value = max_from_available.min(max_position_value);
        let quantity = (position_value / entry_price).min(risk_based_size);

        // Round to pair's quantity precision
        let precision = pair.quantity_precision();
        let factor = Decimal::from(10u32.pow(precision));
        let rounded = (quantity * factor).floor() / factor;

        // Ensure minimum
        if rounded < min_quantity {
            debug!(
                "Calculated quantity {:.6} below minimum {:.6} for {}",
                rounded, min_quantity, pair
            );
            return Decimal::ZERO;
        }

        debug!(
            "Position size for {}: {:.6} (${:.2}) | Risk: ${:.2}",
            pair,
            rounded,
            rounded * entry_price,
            stop_loss.map(|sl| (entry_price - sl).abs() * rounded).unwrap_or(Decimal::ZERO)
        );

        rounded
    }

    pub async fn calculate_stop_loss(
        &self,
        entry_price: Decimal,
        is_long: bool,
        atr: Option<Decimal>,
    ) -> Decimal {
        let config = self.config.read().await;
        let risk_pct = config.risk.default_stop_loss_pct / Decimal::from(100);

        let stop_distance = if let Some(atr_value) = atr {
            // Use 1.5x ATR for stop loss
            atr_value * Decimal::new(15, 1)
        } else {
            // Default percentage-based stop
            entry_price * risk_pct
        };

        if is_long {
            entry_price - stop_distance
        } else {
            entry_price + stop_distance
        }
    }

    pub async fn calculate_take_profit(
        &self,
        entry_price: Decimal,
        stop_loss: Decimal,
        is_long: bool,
    ) -> Decimal {
        let config = self.config.read().await;
        let risk = (entry_price - stop_loss).abs();
        let reward = risk * config.risk.min_risk_reward_ratio;

        if is_long {
            entry_price + reward
        } else {
            entry_price - reward
        }
    }

    pub async fn should_close_position(
        &self,
        pnl_pct: Decimal,
        peak_pnl_pct: Decimal,
        holding_duration_hours: i64,
    ) -> Option<CloseReason> {
        let config = self.config.read().await;
        let limits = &config.risk;

        // Check stop loss
        if pnl_pct < -limits.default_stop_loss_pct {
            return Some(CloseReason::StopLoss);
        }

        // Check take profit
        if pnl_pct > limits.default_take_profit_pct {
            return Some(CloseReason::TakeProfit);
        }

        // Check max holding time (for day trading focus)
        if holding_duration_hours > limits.max_holding_hours as i64 {
            return Some(CloseReason::TimeLimit);
        }

        // Trailing stop: once peak PnL exceeded 12%, trail by 5% from peak
        if peak_pnl_pct > Decimal::from(12) {
            let trailing_stop_level = peak_pnl_pct - Decimal::from(5);
            if pnl_pct < trailing_stop_level {
                return Some(CloseReason::TrailingStop);
            }
        }

        None
    }

    pub async fn assess_portfolio_risk(&self, portfolio: &Portfolio) -> RiskAssessment {
        let config = self.config.read().await;
        let daily_loss = *self.daily_loss.read().await;

        let position_count = portfolio.position_count();
        let unrealized_pnl_pct = if !portfolio.initial_capital.is_zero() {
            (portfolio.total_unrealized_pnl() / portfolio.initial_capital) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        let risk_level = if portfolio.max_drawdown > Decimal::from(15) {
            RiskLevel::Critical
        } else if portfolio.max_drawdown > Decimal::from(10) {
            RiskLevel::High
        } else if portfolio.max_drawdown > Decimal::from(5) {
            RiskLevel::Moderate
        } else {
            RiskLevel::Low
        };

        let exposure_pct = if !portfolio.initial_capital.is_zero() {
            let total_position_value: Decimal = portfolio
                .positions
                .values()
                .filter(|p| p.status == PositionStatus::Open)
                .map(|p| p.notional_value())
                .sum();
            (total_position_value / portfolio.initial_capital) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        RiskAssessment {
            risk_level,
            position_count,
            exposure_pct,
            unrealized_pnl_pct,
            max_drawdown: portfolio.max_drawdown,
            daily_loss,
            can_trade: matches!(risk_level, RiskLevel::Low | RiskLevel::Moderate),
        }
    }

    /// Check if portfolio drawdown exceeds limits and all positions should be force-closed
    pub async fn check_emergency_stop(&self, portfolio: &Portfolio) -> bool {
        let config = self.config.read().await;
        let limits = &config.risk;

        if portfolio.max_drawdown > limits.max_drawdown_pct {
            warn!(
                "EMERGENCY STOP: Drawdown {:.2}% exceeds maximum {:.2}%. Force-closing all positions.",
                portfolio.max_drawdown, limits.max_drawdown_pct
            );
            return true;
        }

        false
    }

    pub fn config_arc(&self) -> Arc<RwLock<RuntimeConfig>> {
        Arc::clone(&self.config)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    StopLoss,
    TakeProfit,
    TrailingStop,
    TimeLimit,
    Manual,
    Signal,
    EmergencyStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Moderate,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct RiskAssessment {
    pub risk_level: RiskLevel,
    pub position_count: usize,
    pub exposure_pct: Decimal,
    pub unrealized_pnl_pct: Decimal,
    pub max_drawdown: Decimal,
    pub daily_loss: Decimal,
    pub can_trade: bool,
}

impl std::fmt::Display for RiskAssessment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Risk Assessment ===")?;
        writeln!(f, "Risk Level:       {:?}", self.risk_level)?;
        writeln!(f, "Positions:        {}", self.position_count)?;
        writeln!(f, "Exposure:         {:.1}%", self.exposure_pct)?;
        writeln!(f, "Unrealized P&L:   {:.2}%", self.unrealized_pnl_pct)?;
        writeln!(f, "Max Drawdown:     {:.2}%", self.max_drawdown)?;
        writeln!(f, "Daily Loss:       ${:.2}", self.daily_loss)?;
        writeln!(f, "Can Trade:        {}", if self.can_trade { "Yes" } else { "No" })?;
        Ok(())
    }
}
