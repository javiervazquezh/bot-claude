use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::{Side, TradingPair};

/// Comprehensive backtest results with all metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResults {
    // Configuration
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub initial_capital: Decimal,
    pub final_equity: Decimal,

    // Overall Performance
    pub total_return: Decimal,
    pub total_return_pct: Decimal,
    pub annualized_return_pct: Decimal,

    // Risk Metrics
    pub max_drawdown_pct: Decimal,
    pub sharpe_ratio: Decimal,
    pub sortino_ratio: Decimal,
    pub calmar_ratio: Decimal,

    // Trade Statistics
    pub total_trades: u64,
    pub winning_trades: u64,
    pub losing_trades: u64,
    pub win_rate_pct: Decimal,
    pub profit_factor: Decimal,
    pub average_win: Decimal,
    pub average_loss: Decimal,
    pub largest_win: Decimal,
    pub largest_loss: Decimal,
    pub average_trade_pnl: Decimal,

    // Profit/Loss Breakdown
    pub gross_profit: Decimal,
    pub gross_loss: Decimal,
    pub net_profit: Decimal,
    pub total_fees: Decimal,

    // Per-Pair Statistics
    pub pair_stats: HashMap<TradingPair, PairStats>,

    // Benchmark (Buy & Hold)
    pub benchmark_return_pct: Decimal,
    pub benchmark_final_equity: Decimal,
    pub benchmark_max_drawdown_pct: Decimal,
    pub alpha_pct: Decimal,

    // Time Series Data
    pub equity_curve: Vec<EquityPoint>,
    pub trades: Vec<TradeRecord>,
}

impl BacktestResults {
    /// Pretty print results to console
    pub fn print_summary(&self) {
        println!("\n{}", "=".repeat(60));
        println!("                    BACKTEST RESULTS");
        println!("{}", "=".repeat(60));
        println!("Period:             {} to {}", self.start_date, self.end_date);
        println!("Initial Capital:    ${:.2}", self.initial_capital);
        println!("Final Equity:       ${:.2}", self.final_equity);
        println!("{}", "-".repeat(60));
        println!("PERFORMANCE");
        println!("  Total Return:       ${:.2} ({:.2}%)", self.net_profit, self.total_return_pct);
        println!("  Annualized Return:  {:.2}%", self.annualized_return_pct);
        println!("  Max Drawdown:       {:.2}%", self.max_drawdown_pct);
        println!("  Sharpe Ratio:       {:.2}", self.sharpe_ratio);
        println!("  Sortino Ratio:      {:.2}", self.sortino_ratio);
        println!("  Calmar Ratio:       {:.2}", self.calmar_ratio);
        println!("{}", "-".repeat(60));
        println!("TRADES");
        println!("  Total Trades:       {}", self.total_trades);
        println!("  Winning Trades:     {} ({:.1}%)", self.winning_trades, self.win_rate_pct);
        println!("  Losing Trades:      {}", self.losing_trades);
        println!("  Profit Factor:      {:.2}", self.profit_factor);
        println!("  Average Win:        ${:.2}", self.average_win);
        println!("  Average Loss:       ${:.2}", self.average_loss);
        println!("  Largest Win:        ${:.2}", self.largest_win);
        println!("  Largest Loss:       ${:.2}", self.largest_loss);
        println!("  Total Fees:         ${:.2}", self.total_fees);
        println!("{}", "-".repeat(60));
        println!("BENCHMARK (Buy & Hold)");
        println!("  B&H Return:         {:.2}%", self.benchmark_return_pct);
        println!("  B&H Final Equity:   ${:.2}", self.benchmark_final_equity);
        println!("  Strategy Alpha:     {:.2}%", self.alpha_pct);
        println!("{}", "-".repeat(60));
        println!("BY PAIR");
        for (pair, stats) in &self.pair_stats {
            println!(
                "  {}: {} trades, {:.1}% win rate, ${:.2} net P&L",
                pair, stats.trades, stats.win_rate_pct, stats.net_pnl
            );
        }
        println!("{}", "=".repeat(60));
    }
}

/// Per-pair trading statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStats {
    pub pair: TradingPair,
    pub trades: u64,
    pub wins: u64,
    pub losses: u64,
    pub net_pnl: Decimal,
    pub win_rate_pct: Decimal,
    pub profit_factor: Decimal,
    pub gross_profit: Decimal,
    pub gross_loss: Decimal,
}

impl PairStats {
    pub fn new(pair: TradingPair) -> Self {
        Self {
            pair,
            trades: 0,
            wins: 0,
            losses: 0,
            net_pnl: Decimal::ZERO,
            win_rate_pct: Decimal::ZERO,
            profit_factor: Decimal::ZERO,
            gross_profit: Decimal::ZERO,
            gross_loss: Decimal::ZERO,
        }
    }

    pub fn add_trade(&mut self, pnl: Decimal) {
        self.trades += 1;
        self.net_pnl += pnl;

        if pnl > Decimal::ZERO {
            self.wins += 1;
            self.gross_profit += pnl;
        } else if pnl < Decimal::ZERO {
            self.losses += 1;
            self.gross_loss += pnl.abs();
        }

        // Update derived metrics
        if self.trades > 0 {
            self.win_rate_pct = Decimal::from(self.wins) / Decimal::from(self.trades) * dec!(100);
        }
        if !self.gross_loss.is_zero() {
            self.profit_factor = self.gross_profit / self.gross_loss;
        } else if self.gross_profit > Decimal::ZERO {
            self.profit_factor = dec!(100);
        }
    }
}

/// Point on the equity curve
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: DateTime<Utc>,
    pub equity: Decimal,
    pub drawdown_pct: Decimal,
}

/// Record of a completed trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub id: String,
    pub pair: TradingPair,
    pub side: Side,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub quantity: Decimal,
    pub pnl: Decimal,
    pub pnl_pct: Decimal,
    pub fees: Decimal,
    pub strategy: String,
    pub exit_reason: ExitReason,
}

/// Reason for exiting a trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExitReason {
    Signal,
    StopLoss,
    TakeProfit,
    EndOfBacktest,
    PartialExit,
    TimeLimit,
}

impl std::fmt::Display for ExitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitReason::Signal => write!(f, "Signal"),
            ExitReason::StopLoss => write!(f, "Stop Loss"),
            ExitReason::TakeProfit => write!(f, "Take Profit"),
            ExitReason::PartialExit => write!(f, "Partial Exit"),
            ExitReason::TimeLimit => write!(f, "Time Limit"),
            ExitReason::EndOfBacktest => write!(f, "End of Backtest"),
        }
    }
}

/// Walk-forward validation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardResult {
    pub windows: Vec<WindowResult>,
    pub aggregate_oos_return_pct: Decimal,
    pub aggregate_oos_sharpe: Decimal,
    pub aggregate_is_return_pct: Decimal,
    pub aggregate_is_sharpe: Decimal,
    pub overfitting_ratio: Decimal,
}

impl WalkForwardResult {
    pub fn print_summary(&self) {
        println!("\n{}", "=".repeat(80));
        println!("              ROLLING WINDOW BACKTEST RESULTS");
        println!("  (Note: uses same strategy params per window, not re-optimized)");
        println!("{}", "=".repeat(80));
        println!("{:<8} {:>14} {:>14} {:>14} {:>14}", "Window", "IS Return %", "IS Sharpe", "OOS Return %", "OOS Sharpe");
        println!("{}", "-".repeat(80));
        for w in &self.windows {
            println!("{:<8} {:>13.2}% {:>14.2} {:>13.2}% {:>14.2}",
                format!("#{}", w.window_num),
                w.is_results.total_return_pct, w.is_results.sharpe_ratio,
                w.oos_results.total_return_pct, w.oos_results.sharpe_ratio);
        }
        println!("{}", "-".repeat(80));
        println!("{:<8} {:>13.2}% {:>14.2} {:>13.2}% {:>14.2}",
            "Avg", self.aggregate_is_return_pct, self.aggregate_is_sharpe,
            self.aggregate_oos_return_pct, self.aggregate_oos_sharpe);
        println!("{}", "-".repeat(80));
        println!("Overfitting Ratio (IS/OOS Sharpe): {:.2} (closer to 1.0 = less overfit)", self.overfitting_ratio);
        println!("{}", "=".repeat(80));
    }
}

/// Single window in walk-forward validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowResult {
    pub window_num: usize,
    pub is_start: NaiveDate,
    pub is_end: NaiveDate,
    pub oos_start: NaiveDate,
    pub oos_end: NaiveDate,
    pub is_results: BacktestResults,
    pub oos_results: BacktestResults,
}

/// Calculator for backtest metrics
pub struct MetricsCalculator;

impl MetricsCalculator {
    /// Calculate all metrics from trades and equity curve
    pub fn calculate(
        start_date: NaiveDate,
        end_date: NaiveDate,
        initial_capital: Decimal,
        final_equity: Decimal,
        trades: &[TradeRecord],
        equity_curve: &[EquityPoint],
        benchmark_final_equity: Decimal,
    ) -> BacktestResults {
        let total_trades = trades.len() as u64;
        let winning_trades: Vec<_> = trades.iter().filter(|t| t.pnl > Decimal::ZERO).collect();
        let losing_trades: Vec<_> = trades.iter().filter(|t| t.pnl < Decimal::ZERO).collect();

        let wins = winning_trades.len() as u64;
        let losses = losing_trades.len() as u64;

        let gross_profit: Decimal = winning_trades.iter().map(|t| t.pnl).sum();
        let gross_loss: Decimal = losing_trades.iter().map(|t| t.pnl.abs()).sum();
        let total_fees: Decimal = trades.iter().map(|t| t.fees).sum();

        let net_profit = final_equity - initial_capital;
        let total_return = net_profit;
        let total_return_pct = if !initial_capital.is_zero() {
            (net_profit / initial_capital) * dec!(100)
        } else {
            Decimal::ZERO
        };

        // Calculate days for annualization
        let days = (end_date - start_date).num_days().max(1) as f64;
        let years = days / 365.0;
        let annualized_return_pct = if years > 0.0 {
            let return_factor = Decimal::ONE + (total_return_pct / dec!(100));
            let return_f64: f64 = return_factor.try_into().unwrap_or(1.0);
            let annual_factor = return_f64.powf(1.0 / years) - 1.0;
            Decimal::try_from(annual_factor * 100.0).unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        // Win rate
        let win_rate_pct = if total_trades > 0 {
            Decimal::from(wins) / Decimal::from(total_trades) * dec!(100)
        } else {
            Decimal::ZERO
        };

        // Profit factor
        let profit_factor = if !gross_loss.is_zero() {
            gross_profit / gross_loss
        } else if gross_profit > Decimal::ZERO {
            dec!(100)
        } else {
            Decimal::ONE
        };

        // Average win/loss
        let average_win = if wins > 0 {
            gross_profit / Decimal::from(wins)
        } else {
            Decimal::ZERO
        };

        let average_loss = if losses > 0 {
            gross_loss / Decimal::from(losses)
        } else {
            Decimal::ZERO
        };

        // Largest win/loss
        let largest_win = winning_trades
            .iter()
            .map(|t| t.pnl)
            .max()
            .unwrap_or(Decimal::ZERO);

        let largest_loss = losing_trades
            .iter()
            .map(|t| t.pnl.abs())
            .max()
            .unwrap_or(Decimal::ZERO);

        // Average trade P&L
        let average_trade_pnl = if total_trades > 0 {
            net_profit / Decimal::from(total_trades)
        } else {
            Decimal::ZERO
        };

        // Max drawdown from equity curve
        let max_drawdown_pct = equity_curve
            .iter()
            .map(|e| e.drawdown_pct)
            .max()
            .unwrap_or(Decimal::ZERO);

        // Calculate Sharpe, Sortino, Calmar ratios (from daily equity returns)
        let (sharpe_ratio, sortino_ratio) = Self::calculate_ratios(trades, initial_capital, equity_curve);

        let calmar_ratio = if !max_drawdown_pct.is_zero() {
            annualized_return_pct / max_drawdown_pct
        } else if annualized_return_pct > Decimal::ZERO {
            dec!(100)
        } else {
            Decimal::ZERO
        };

        // Per-pair statistics
        let mut pair_stats: HashMap<TradingPair, PairStats> = HashMap::new();
        for trade in trades {
            pair_stats
                .entry(trade.pair)
                .or_insert_with(|| PairStats::new(trade.pair))
                .add_trade(trade.pnl);
        }

        // Benchmark (Buy & Hold) metrics
        let benchmark_return_pct = if !initial_capital.is_zero() {
            ((benchmark_final_equity - initial_capital) / initial_capital) * dec!(100)
        } else {
            Decimal::ZERO
        };
        let alpha_pct = total_return_pct - benchmark_return_pct;
        // Benchmark max drawdown is computed by caller; use 0 as placeholder here
        let benchmark_max_drawdown_pct = Decimal::ZERO;

        BacktestResults {
            start_date,
            end_date,
            initial_capital,
            final_equity,
            total_return,
            total_return_pct,
            annualized_return_pct,
            max_drawdown_pct,
            sharpe_ratio,
            sortino_ratio,
            calmar_ratio,
            total_trades,
            winning_trades: wins,
            losing_trades: losses,
            win_rate_pct,
            profit_factor,
            average_win,
            average_loss,
            largest_win,
            largest_loss,
            average_trade_pnl,
            gross_profit,
            gross_loss,
            net_profit,
            total_fees,
            benchmark_return_pct,
            benchmark_final_equity,
            benchmark_max_drawdown_pct,
            alpha_pct,
            pair_stats,
            equity_curve: equity_curve.to_vec(),
            trades: trades.to_vec(),
        }
    }

    /// Calculate Sharpe and Sortino ratios from daily equity returns
    fn calculate_ratios(trades: &[TradeRecord], initial_capital: Decimal, equity_curve: &[EquityPoint]) -> (Decimal, Decimal) {
        // Prefer daily equity returns (correct methodology)
        // Fall back to per-trade returns only if equity curve has < 2 days
        let returns = Self::daily_returns_from_equity(initial_capital, equity_curve);

        if returns.len() < 2 {
            // Fallback: per-trade returns (less accurate but better than nothing)
            let trade_returns: Vec<f64> = trades
                .iter()
                .map(|t| {
                    let pnl_pct: f64 = t.pnl_pct.try_into().unwrap_or(0.0);
                    pnl_pct / 100.0
                })
                .collect();
            if trade_returns.is_empty() {
                return (Decimal::ZERO, Decimal::ZERO);
            }
            return Self::compute_sharpe_sortino(&trade_returns, 365.0);
        }

        Self::compute_sharpe_sortino(&returns, 365.0)
    }

    /// Extract daily returns from equity curve points
    fn daily_returns_from_equity(initial_capital: Decimal, equity_curve: &[EquityPoint]) -> Vec<f64> {
        if equity_curve.is_empty() {
            return Vec::new();
        }

        // Group equity points by calendar date, take last per day
        let mut daily_equity: Vec<f64> = Vec::new();
        let mut last_date = None;

        // Start with initial capital as day 0
        let init_cap: f64 = initial_capital.try_into().unwrap_or(1.0);
        daily_equity.push(init_cap);

        for point in equity_curve {
            let date = point.timestamp.date_naive();
            let eq: f64 = point.equity.try_into().unwrap_or(0.0);

            if last_date.map_or(true, |d| d != date) {
                daily_equity.push(eq);
                last_date = Some(date);
            } else {
                // Same date: update to latest value
                if let Some(last) = daily_equity.last_mut() {
                    *last = eq;
                }
            }
        }

        // Compute daily returns: r_t = (E_t - E_{t-1}) / E_{t-1}
        daily_equity.windows(2)
            .map(|w| {
                if w[0] > 0.0 {
                    (w[1] - w[0]) / w[0]
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// Compute Sharpe and Sortino from a returns series
    fn compute_sharpe_sortino(returns: &[f64], annualization_days: f64) -> (Decimal, Decimal) {
        if returns.is_empty() {
            return (Decimal::ZERO, Decimal::ZERO);
        }

        let n = returns.len() as f64;
        let mean_return = returns.iter().sum::<f64>() / n;

        // Sample standard deviation (Bessel's correction: n-1)
        let variance = if n > 1.0 {
            returns.iter().map(|r| (r - mean_return).powi(2)).sum::<f64>() / (n - 1.0)
        } else {
            0.0
        };
        let std_dev = variance.sqrt();

        // Sharpe ratio (risk-free rate = 0, annualized with sqrt(365) for crypto)
        let sharpe = if std_dev > 0.0 {
            (mean_return / std_dev) * annualization_days.sqrt()
        } else {
            0.0
        };

        // Downside deviation (for Sortino) — sample formula
        let downside_variance = if n > 1.0 {
            returns.iter()
                .map(|&r| if r < 0.0 { r.powi(2) } else { 0.0 })
                .sum::<f64>() / (n - 1.0)
        } else {
            0.0
        };
        let downside_dev = downside_variance.sqrt();

        let sortino = if downside_dev > 0.0 {
            (mean_return / downside_dev) * annualization_days.sqrt()
        } else if mean_return > 0.0 {
            100.0
        } else {
            0.0
        };

        (
            Decimal::try_from(sharpe).unwrap_or(Decimal::ZERO),
            Decimal::try_from(sortino).unwrap_or(Decimal::ZERO),
        )
    }
}
