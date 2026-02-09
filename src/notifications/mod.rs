#![allow(dead_code)]
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::types::TradingPair;

/// Notification severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

/// Types of notifications
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AlertType {
    // Trade alerts
    PositionOpened {
        pair: TradingPair,
        side: String,
        quantity: String,
        entry_price: String,
    },
    PositionClosed {
        pair: TradingPair,
        pnl: String,
        pnl_pct: String,
        reason: String,
    },
    StopLossTriggered {
        pair: TradingPair,
        price: String,
        loss: String,
    },
    TakeProfitTriggered {
        pair: TradingPair,
        price: String,
        profit: String,
    },
    PartialExitExecuted {
        pair: TradingPair,
        quantity: String,
        pnl: String,
        reason: String,
    },
    TrailingStopActivated {
        pair: TradingPair,
        activation_price: String,
        trail_level: String,
    },
    BreakEvenStopSet {
        pair: TradingPair,
        entry_price: String,
    },
    PositionScaled {
        pair: TradingPair,
        added_quantity: String,
        new_avg_entry: String,
    },

    // Risk alerts
    MaxDrawdownApproached {
        current_drawdown: String,
        max_allowed: String,
    },
    MaxDrawdownExceeded {
        current_drawdown: String,
        max_allowed: String,
    },
    DailyLossLimitApproached {
        current_loss: String,
        limit: String,
    },
    DailyLossLimitExceeded {
        current_loss: String,
        limit: String,
    },
    MaxPositionsReached {
        current: usize,
        max: usize,
    },
    LowBalance {
        available: String,
        required: String,
    },
    LargePosition {
        pair: TradingPair,
        size_pct: String,
        max_allowed: String,
    },

    // Performance alerts
    WinRateChanged {
        old_rate: String,
        new_rate: String,
        trades_count: u64,
    },
    ProfitMilestone {
        total_profit: String,
        milestone: String,
    },
    LossMilestone {
        total_loss: String,
        milestone: String,
    },
    ProfitFactorChanged {
        old_pf: String,
        new_pf: String,
    },

    // System alerts
    ConnectionLost {
        service: String,
    },
    ConnectionRestored {
        service: String,
    },
    BotStarted,
    BotStopped,
    BotPaused,
    BotResumed,
    ConfigurationChanged {
        setting: String,
        old_value: String,
        new_value: String,
    },
    Error {
        component: String,
        message: String,
    },
}

impl AlertType {
    /// Get default severity for this alert type
    pub fn default_severity(&self) -> Severity {
        match self {
            // Critical alerts
            AlertType::MaxDrawdownExceeded { .. } => Severity::Critical,
            AlertType::DailyLossLimitExceeded { .. } => Severity::Critical,
            AlertType::ConnectionLost { .. } => Severity::Critical,
            AlertType::Error { .. } => Severity::Critical,

            // Warning alerts
            AlertType::StopLossTriggered { .. } => Severity::Warning,
            AlertType::MaxDrawdownApproached { .. } => Severity::Warning,
            AlertType::DailyLossLimitApproached { .. } => Severity::Warning,
            AlertType::MaxPositionsReached { .. } => Severity::Warning,
            AlertType::LowBalance { .. } => Severity::Warning,
            AlertType::LargePosition { .. } => Severity::Warning,
            AlertType::LossMilestone { .. } => Severity::Warning,

            // Info alerts (everything else)
            _ => Severity::Info,
        }
    }

    /// Get a human-readable title for this alert
    pub fn title(&self) -> String {
        match self {
            AlertType::PositionOpened { pair, .. } => format!("Position Opened: {}", pair),
            AlertType::PositionClosed { pair, .. } => format!("Position Closed: {}", pair),
            AlertType::StopLossTriggered { pair, .. } => format!("Stop Loss Hit: {}", pair),
            AlertType::TakeProfitTriggered { pair, .. } => format!("Take Profit Hit: {}", pair),
            AlertType::PartialExitExecuted { pair, .. } => format!("Partial Exit: {}", pair),
            AlertType::TrailingStopActivated { pair, .. } => format!("Trailing Stop Activated: {}", pair),
            AlertType::BreakEvenStopSet { pair, .. } => format!("Break-Even Stop Set: {}", pair),
            AlertType::PositionScaled { pair, .. } => format!("Position Scaled: {}", pair),

            AlertType::MaxDrawdownApproached { .. } => "Max Drawdown Warning".to_string(),
            AlertType::MaxDrawdownExceeded { .. } => "MAX DRAWDOWN EXCEEDED".to_string(),
            AlertType::DailyLossLimitApproached { .. } => "Daily Loss Limit Warning".to_string(),
            AlertType::DailyLossLimitExceeded { .. } => "DAILY LOSS LIMIT EXCEEDED".to_string(),
            AlertType::MaxPositionsReached { .. } => "Max Positions Reached".to_string(),
            AlertType::LowBalance { .. } => "Low Balance Warning".to_string(),
            AlertType::LargePosition { .. } => "Large Position Warning".to_string(),

            AlertType::WinRateChanged { .. } => "Win Rate Update".to_string(),
            AlertType::ProfitMilestone { .. } => "Profit Milestone Reached".to_string(),
            AlertType::LossMilestone { .. } => "Loss Milestone".to_string(),
            AlertType::ProfitFactorChanged { .. } => "Profit Factor Update".to_string(),

            AlertType::ConnectionLost { service } => format!("Connection Lost: {}", service),
            AlertType::ConnectionRestored { service } => format!("Connection Restored: {}", service),
            AlertType::BotStarted => "Bot Started".to_string(),
            AlertType::BotStopped => "Bot Stopped".to_string(),
            AlertType::BotPaused => "Bot Paused".to_string(),
            AlertType::BotResumed => "Bot Resumed".to_string(),
            AlertType::ConfigurationChanged { setting, .. } => format!("Config Changed: {}", setting),
            AlertType::Error { component, .. } => format!("Error in {}", component),
        }
    }
}

/// A notification/alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub severity: Severity,
    pub alert_type: AlertType,
    pub acknowledged: bool,
}

impl Notification {
    pub fn new(alert_type: AlertType) -> Self {
        let severity = alert_type.default_severity();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            severity,
            alert_type,
            acknowledged: false,
        }
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
}

/// Notification manager
pub struct NotificationManager {
    notifications: Arc<RwLock<Vec<Notification>>>,
    database: Option<Arc<crate::database::Database>>,
    max_notifications: usize,
}

impl NotificationManager {
    pub fn new(database: Option<Arc<crate::database::Database>>) -> Self {
        Self {
            notifications: Arc::new(RwLock::new(Vec::new())),
            database,
            max_notifications: 500,
        }
    }

    /// Send a notification
    pub async fn notify(&self, alert_type: AlertType) {
        self.notify_with_severity(alert_type, None).await;
    }

    /// Send a notification with custom severity
    pub async fn notify_with_severity(&self, alert_type: AlertType, severity: Option<Severity>) {
        let mut notification = Notification::new(alert_type);
        if let Some(sev) = severity {
            notification.severity = sev;
        }

        // Log to console based on severity
        let title = notification.alert_type.title();
        match notification.severity {
            Severity::Critical => error!("🚨 {} - {:?}", title, notification.alert_type),
            Severity::Warning => warn!("⚠️  {} - {:?}", title, notification.alert_type),
            Severity::Info => info!("ℹ️  {} - {:?}", title, notification.alert_type),
        }

        // Store in memory
        let mut notifications = self.notifications.write().await;
        notifications.insert(0, notification.clone());

        // Limit size
        if notifications.len() > self.max_notifications {
            notifications.truncate(self.max_notifications);
        }
        drop(notifications);

        // Store in database
        if let Some(db) = &self.database {
            if let Err(e) = db.insert_notification(&notification).await {
                error!("Failed to save notification to database: {}", e);
            }
        }
    }

    /// Get all notifications
    pub async fn get_all(&self) -> Vec<Notification> {
        self.notifications.read().await.clone()
    }

    /// Get recent notifications
    pub async fn get_recent(&self, limit: usize) -> Vec<Notification> {
        let notifications = self.notifications.read().await;
        notifications.iter().take(limit).cloned().collect()
    }

    /// Get unacknowledged critical notifications
    pub async fn get_critical_unacknowledged(&self) -> Vec<Notification> {
        let notifications = self.notifications.read().await;
        notifications
            .iter()
            .filter(|n| n.severity == Severity::Critical && !n.acknowledged)
            .cloned()
            .collect()
    }

    /// Acknowledge a notification
    pub async fn acknowledge(&self, id: &str) {
        let mut notifications = self.notifications.write().await;
        if let Some(notification) = notifications.iter_mut().find(|n| n.id == id) {
            notification.acknowledged = true;
        }
    }

    /// Clear old notifications
    pub async fn clear_old(&self, keep_count: usize) {
        let mut notifications = self.notifications.write().await;
        if notifications.len() > keep_count {
            notifications.truncate(keep_count);
        }
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new(None)
    }
}

// Helper functions for common notifications

/// Create a position opened notification
pub fn position_opened(pair: TradingPair, side: String, quantity: Decimal, entry_price: Decimal) -> AlertType {
    AlertType::PositionOpened {
        pair,
        side,
        quantity: quantity.to_string(),
        entry_price: entry_price.to_string(),
    }
}

/// Create a position closed notification
pub fn position_closed(pair: TradingPair, pnl: Decimal, pnl_pct: Decimal, reason: String) -> AlertType {
    AlertType::PositionClosed {
        pair,
        pnl: pnl.to_string(),
        pnl_pct: pnl_pct.to_string(),
        reason,
    }
}

/// Create a stop loss triggered notification
pub fn stop_loss_triggered(pair: TradingPair, price: Decimal, loss: Decimal) -> AlertType {
    AlertType::StopLossTriggered {
        pair,
        price: price.to_string(),
        loss: loss.to_string(),
    }
}

/// Create a take profit triggered notification
pub fn take_profit_triggered(pair: TradingPair, price: Decimal, profit: Decimal) -> AlertType {
    AlertType::TakeProfitTriggered {
        pair,
        price: price.to_string(),
        profit: profit.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_notification_creation() {
        let alert = position_opened(TradingPair::BTCUSDT, "Buy".to_string(), dec!(1.0), dec!(50000));
        let notification = Notification::new(alert);

        assert_eq!(notification.severity, Severity::Info);
        assert!(!notification.acknowledged);
    }

    #[test]
    fn test_severity_defaults() {
        let critical = AlertType::MaxDrawdownExceeded {
            current_drawdown: "20%".to_string(),
            max_allowed: "15%".to_string(),
        };
        assert_eq!(critical.default_severity(), Severity::Critical);

        let warning = AlertType::StopLossTriggered {
            pair: TradingPair::BTCUSDT,
            price: "48000".to_string(),
            loss: "-500".to_string(),
        };
        assert_eq!(warning.default_severity(), Severity::Warning);

        let info = AlertType::BotStarted;
        assert_eq!(info.default_severity(), Severity::Info);
    }

    #[tokio::test]
    async fn test_notification_manager() {
        let manager = NotificationManager::new(None);

        manager.notify(AlertType::BotStarted).await;
        manager.notify(position_opened(
            TradingPair::ETHUSDT,
            "Buy".to_string(),
            dec!(10.0),
            dec!(3000),
        )).await;

        let notifications = manager.get_all().await;
        assert_eq!(notifications.len(), 2);

        let recent = manager.get_recent(1).await;
        assert_eq!(recent.len(), 1);
    }
}
