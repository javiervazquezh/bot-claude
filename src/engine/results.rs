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

        // Calculate Sharpe, Sortino, Calmar ratios
        let (sharpe_ratio, sortino_ratio) = Self::calculate_ratios(trades, initial_capital);

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
            pair_stats,
            equity_curve: equity_curve.to_vec(),
            trades: trades.to_vec(),
        }
    }

    /// Calculate Sharpe and Sortino ratios from trade returns
    fn calculate_ratios(trades: &[TradeRecord], initial_capital: Decimal) -> (Decimal, Decimal) {
        if trades.is_empty() {
            return (Decimal::ZERO, Decimal::ZERO);
        }

        // Calculate returns as percentage
        let returns: Vec<f64> = trades
            .iter()
            .map(|t| {
                let pnl_pct: f64 = t.pnl_pct.try_into().unwrap_or(0.0);
                pnl_pct / 100.0
            })
            .collect();

        let n = returns.len() as f64;
        let mean_return = returns.iter().sum::<f64>() / n;

        // Standard deviation
        let variance = returns.iter().map(|r| (r - mean_return).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        // Sharpe ratio (assuming risk-free rate of 0)
        let sharpe = if std_dev > 0.0 {
            (mean_return / std_dev) * (252_f64).sqrt() // Annualized
        } else {
            0.0
        };

        // Downside deviation (for Sortino)
        let negative_returns: Vec<f64> = returns.iter().filter(|&&r| r < 0.0).copied().collect();
        let downside_variance = if !negative_returns.is_empty() {
            negative_returns.iter().map(|r| r.powi(2)).sum::<f64>() / negative_returns.len() as f64
        } else {
            0.0
        };
        let downside_dev = downside_variance.sqrt();

        let sortino = if downside_dev > 0.0 {
            (mean_return / downside_dev) * (252_f64).sqrt()
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
