use rust_decimal::Decimal;
use tracing::debug;

use super::features::RecentTrade;
use super::TradeFeatures;

/// Tracks trade features and outcomes for ML training
/// In-memory version for backtesting (no async DB dependency)
pub struct OutcomeTracker {
    /// Completed trades with features and outcomes
    completed: Vec<(TradeFeatures, bool, f64)>, // (features, is_win, pnl_pct)
    /// Pending trades (opened but not yet closed)
    pending: Vec<(String, TradeFeatures)>, // (trade_id, features)
}

impl OutcomeTracker {
    pub fn new() -> Self {
        Self {
            completed: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// Record features when a trade is opened
    pub fn record_entry(&mut self, trade_id: &str, features: TradeFeatures) {
        self.pending.push((trade_id.to_string(), features));
    }

    /// Record outcome when a trade is closed
    pub fn record_exit(&mut self, trade_id: &str, pnl_pct: Decimal) {
        if let Some(idx) = self.pending.iter().position(|(id, _)| id == trade_id) {
            let (_, features) = self.pending.remove(idx);
            let pnl_f64: f64 = pnl_pct.try_into().unwrap_or(0.0);
            let is_win = pnl_f64 > 0.0;
            self.completed.push((features, is_win, pnl_f64));
            debug!("ML tracker: recorded outcome for {} (win={})", trade_id, is_win);
        }
    }

    /// Get training data (features + win/loss label)
    pub fn get_training_data(&self) -> Vec<(TradeFeatures, bool)> {
        self.completed.iter()
            .map(|(f, w, _)| (f.clone(), *w))
            .collect()
    }

    /// Get training data with pnl (features + win/loss + pnl_pct)
    pub fn get_training_data_with_pnl(&self) -> Vec<(TradeFeatures, bool, f64)> {
        self.completed.clone()
    }

    /// Get recent trades for performance feature computation
    pub fn recent_trades(&self, n: usize) -> Vec<RecentTrade> {
        self.completed.iter()
            .rev()
            .take(n)
            .map(|(_, is_win, pnl_pct)| RecentTrade {
                is_win: *is_win,
                pnl_pct: *pnl_pct,
            })
            .collect()
    }

    /// Number of completed trade records
    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }
}
