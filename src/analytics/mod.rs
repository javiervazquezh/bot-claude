use chrono::{DateTime, Duration, Utc, Datelike, Timelike};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::TradingPair;
use crate::web::state::TradeRecord;

/// Comprehensive performance analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceAnalytics {
    pub overall: OverallMetrics,
    pub by_pair: HashMap<TradingPair, PairMetrics>,
    pub by_strategy: HashMap<String, StrategyMetrics>,
    pub rolling_returns: RollingReturns,
    pub drawdown_analysis: DrawdownAnalysis,
    pub trade_distribution: TradeDistribution,
    pub risk_metrics: RiskMetrics,
    pub win_loss_streaks: WinLossStreaks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverallMetrics {
    pub total_trades: u64,
    pub winning_trades: u64,
    pub losing_trades: u64,
    pub win_rate: Decimal,
    pub total_pnl: Decimal,
    pub total_pnl_pct: Decimal,
    pub avg_win: Decimal,
    pub avg_loss: Decimal,
    pub avg_win_pct: Decimal,
    pub avg_loss_pct: Decimal,
    pub largest_win: Decimal,
    pub largest_loss: Decimal,
    pub profit_factor: Decimal,
    pub expectancy: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairMetrics {
    pub pair: TradingPair,
    pub trades: u64,
    pub win_rate: Decimal,
    pub total_pnl: Decimal,
    pub avg_pnl: Decimal,
    pub contribution_pct: Decimal,
    pub avg_win: Decimal,
    pub avg_loss: Decimal,
    pub profit_factor: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyMetrics {
    pub strategy: String,
    pub trades: u64,
    pub win_rate: Decimal,
    pub total_pnl: Decimal,
    pub avg_pnl: Decimal,
    pub sharpe_ratio: Option<Decimal>,
    pub avg_win: Decimal,
    pub avg_loss: Decimal,
    pub profit_factor: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingReturns {
    pub return_7d: Decimal,
    pub return_30d: Decimal,
    pub return_90d: Decimal,
    pub return_ytd: Decimal,
    pub return_all_time: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawdownAnalysis {
    pub current_drawdown: Decimal,
    pub max_drawdown: Decimal,
    pub max_drawdown_pct: Decimal, // For dashboard compatibility
    pub max_drawdown_date: Option<DateTime<Utc>>,
    pub max_drawdown_duration_hours: u64,
    pub avg_drawdown: Decimal,
    pub recovery_time_hours: Option<u64>,
    pub drawdown_periods: Vec<DrawdownPeriod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawdownPeriod {
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub peak_equity: Decimal,
    pub trough_equity: Decimal,
    pub drawdown_pct: Decimal,
    pub duration_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeDistribution {
    pub pnl_histogram: Vec<HistogramBucket>,
    pub duration_histogram: Vec<HistogramBucket>,
    pub hourly_performance: Vec<HourlyStats>,
    pub daily_performance: Vec<DailyStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramBucket {
    pub range_start: Decimal,
    pub range_end: Decimal,
    pub count: u64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyStats {
    pub hour: u32,
    pub trades: u64,
    pub win_rate: Decimal,
    pub avg_pnl: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStats {
    pub day: String, // "Monday", "Tuesday", etc.
    pub trades: u64,
    pub win_rate: Decimal,
    pub avg_pnl: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    pub sharpe_ratio: Decimal,
    pub sortino_ratio: Decimal,
    pub calmar_ratio: Decimal,
    pub max_drawdown: Decimal,
    pub avg_drawdown: Decimal,
    pub volatility: Decimal,
    pub downside_deviation: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WinLossStreaks {
    pub current_streak: i32, // Positive for wins, negative for losses
    pub max_win_streak: u32,
    pub max_loss_streak: u32,
    pub avg_win_streak: Decimal,
    pub avg_loss_streak: Decimal,
}

/// Calculate comprehensive analytics from trade history
pub struct AnalyticsCalculator;

impl AnalyticsCalculator {
    pub fn calculate(
        trades: &[TradeRecord],
        initial_capital: Decimal,
        current_equity: Decimal,
    ) -> PerformanceAnalytics {
        // CRITICAL: Sort trades by timestamp for correct equity curve and drawdown analysis
        let mut sorted_trades = trades.to_vec();
        sorted_trades.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        let overall = Self::calculate_overall_metrics(&sorted_trades, initial_capital, current_equity);
        let by_pair = Self::calculate_pair_metrics(&sorted_trades);
        let by_strategy = Self::calculate_strategy_metrics(&sorted_trades);
        let rolling_returns = Self::calculate_rolling_returns(&sorted_trades, initial_capital);
        let drawdown_analysis = Self::calculate_drawdown_analysis(&sorted_trades, initial_capital);
        let trade_distribution = Self::calculate_trade_distribution(&sorted_trades);
        let risk_metrics = Self::calculate_risk_metrics(&sorted_trades, initial_capital, current_equity);
        let win_loss_streaks = Self::calculate_win_loss_streaks(&sorted_trades);

        PerformanceAnalytics {
            overall,
            by_pair,
            by_strategy,
            rolling_returns,
            drawdown_analysis,
            trade_distribution,
            risk_metrics,
            win_loss_streaks,
        }
    }

    fn calculate_overall_metrics(
        trades: &[TradeRecord],
        initial_capital: Decimal,
        current_equity: Decimal,
    ) -> OverallMetrics {
        if trades.is_empty() {
            return OverallMetrics {
                total_trades: 0,
                winning_trades: 0,
                losing_trades: 0,
                win_rate: Decimal::ZERO,
                total_pnl: Decimal::ZERO,
                total_pnl_pct: Decimal::ZERO,
                avg_win: Decimal::ZERO,
                avg_loss: Decimal::ZERO,
                avg_win_pct: Decimal::ZERO,
                avg_loss_pct: Decimal::ZERO,
                largest_win: Decimal::ZERO,
                largest_loss: Decimal::ZERO,
                profit_factor: Decimal::ZERO,
                expectancy: Decimal::ZERO,
            };
        }

        let total_trades = trades.len() as u64;
        let winning_trades = trades.iter().filter(|t| t.pnl > Decimal::ZERO).count() as u64;
        let losing_trades = trades.iter().filter(|t| t.pnl < Decimal::ZERO).count() as u64;

        let win_rate = if total_trades > 0 {
            Decimal::from(winning_trades) / Decimal::from(total_trades) * dec!(100)
        } else {
            Decimal::ZERO
        };

        let total_pnl: Decimal = trades.iter().map(|t| t.pnl).sum();
        let total_pnl_pct = if initial_capital > Decimal::ZERO {
            (current_equity - initial_capital) / initial_capital * dec!(100)
        } else {
            Decimal::ZERO
        };

        let wins: Vec<_> = trades.iter().filter(|t| t.pnl > Decimal::ZERO).collect();
        let losses: Vec<_> = trades.iter().filter(|t| t.pnl < Decimal::ZERO).collect();

        let avg_win = if !wins.is_empty() {
            wins.iter().map(|t| t.pnl).sum::<Decimal>() / Decimal::from(wins.len())
        } else {
            Decimal::ZERO
        };

        let avg_loss = if !losses.is_empty() {
            losses.iter().map(|t| t.pnl).sum::<Decimal>() / Decimal::from(losses.len())
        } else {
            Decimal::ZERO
        };

        let avg_win_pct = if !wins.is_empty() {
            wins.iter().map(|t| t.pnl_pct).sum::<Decimal>() / Decimal::from(wins.len())
        } else {
            Decimal::ZERO
        };

        let avg_loss_pct = if !losses.is_empty() {
            losses.iter().map(|t| t.pnl_pct).sum::<Decimal>() / Decimal::from(losses.len())
        } else {
            Decimal::ZERO
        };

        let largest_win = wins.iter().map(|t| t.pnl).max().unwrap_or(Decimal::ZERO);
        let largest_loss = losses.iter().map(|t| t.pnl).min().unwrap_or(Decimal::ZERO);

        let gross_profit: Decimal = wins.iter().map(|t| t.pnl).sum();
        let gross_loss: Decimal = losses.iter().map(|t| t.pnl.abs()).sum();

        let profit_factor = if gross_loss > Decimal::ZERO {
            gross_profit / gross_loss
        } else if gross_profit > Decimal::ZERO {
            dec!(999.99) // Infinite profit factor
        } else {
            Decimal::ZERO
        };

        let expectancy = if total_trades > 0 {
            total_pnl / Decimal::from(total_trades)
        } else {
            Decimal::ZERO
        };

        OverallMetrics {
            total_trades,
            winning_trades,
            losing_trades,
            win_rate,
            total_pnl,
            total_pnl_pct,
            avg_win,
            avg_loss,
            avg_win_pct,
            avg_loss_pct,
            largest_win,
            largest_loss,
            profit_factor,
            expectancy,
        }
    }

    fn calculate_pair_metrics(trades: &[TradeRecord]) -> HashMap<TradingPair, PairMetrics> {
        let mut metrics: HashMap<TradingPair, PairMetrics> = HashMap::new();

        let total_pnl: Decimal = trades.iter().map(|t| t.pnl).sum();

        for pair in [
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
            TradingPair::BNBUSDT,
            TradingPair::ADAUSDT,
            TradingPair::XRPUSDT,
        ] {
            let pair_trades: Vec<_> = trades.iter().filter(|t| t.pair == pair).collect();

            if pair_trades.is_empty() {
                continue;
            }

            let trade_count = pair_trades.len() as u64;
            let wins = pair_trades.iter().filter(|t| t.pnl > Decimal::ZERO).count() as u64;
            let win_rate = if trade_count > 0 {
                Decimal::from(wins) / Decimal::from(trade_count) * dec!(100)
            } else {
                Decimal::ZERO
            };

            let pair_pnl: Decimal = pair_trades.iter().map(|t| t.pnl).sum();
            let avg_pnl = pair_pnl / Decimal::from(trade_count);

            let contribution_pct = if total_pnl != Decimal::ZERO {
                pair_pnl / total_pnl * dec!(100)
            } else {
                Decimal::ZERO
            };

            // Calculate avg_win, avg_loss, and profit_factor
            let winning_trades: Vec<_> = pair_trades.iter().filter(|t| t.pnl > Decimal::ZERO).collect();
            let losing_trades: Vec<_> = pair_trades.iter().filter(|t| t.pnl < Decimal::ZERO).collect();

            let avg_win = if !winning_trades.is_empty() {
                winning_trades.iter().map(|t| t.pnl).sum::<Decimal>() / Decimal::from(winning_trades.len())
            } else {
                Decimal::ZERO
            };

            let avg_loss = if !losing_trades.is_empty() {
                losing_trades.iter().map(|t| t.pnl).sum::<Decimal>() / Decimal::from(losing_trades.len())
            } else {
                Decimal::ZERO
            };

            let total_wins: Decimal = winning_trades.iter().map(|t| t.pnl).sum();
            let total_losses: Decimal = losing_trades.iter().map(|t| t.pnl).sum();
            let profit_factor = if total_losses.abs() > Decimal::ZERO {
                total_wins / total_losses.abs()
            } else {
                Decimal::ZERO
            };

            metrics.insert(
                pair,
                PairMetrics {
                    pair,
                    trades: trade_count,
                    win_rate,
                    total_pnl: pair_pnl,
                    avg_pnl,
                    contribution_pct,
                    avg_win,
                    avg_loss,
                    profit_factor,
                },
            );
        }

        metrics
    }

    fn calculate_strategy_metrics(trades: &[TradeRecord]) -> HashMap<String, StrategyMetrics> {
        let mut metrics: HashMap<String, StrategyMetrics> = HashMap::new();

        // Group by strategy
        let mut strategy_groups: HashMap<String, Vec<&TradeRecord>> = HashMap::new();
        for trade in trades {
            strategy_groups
                .entry(trade.strategy.clone())
                .or_insert_with(Vec::new)
                .push(trade);
        }

        for (strategy, strat_trades) in strategy_groups {
            let trade_count = strat_trades.len() as u64;
            let wins = strat_trades.iter().filter(|t| t.pnl > Decimal::ZERO).count() as u64;
            let win_rate = if trade_count > 0 {
                Decimal::from(wins) / Decimal::from(trade_count) * dec!(100)
            } else {
                Decimal::ZERO
            };

            let strat_pnl: Decimal = strat_trades.iter().map(|t| t.pnl).sum();
            let avg_pnl = strat_pnl / Decimal::from(trade_count);

            // Calculate avg_win, avg_loss, and profit_factor
            let winning_trades: Vec<_> = strat_trades.iter().filter(|t| t.pnl > Decimal::ZERO).collect();
            let losing_trades: Vec<_> = strat_trades.iter().filter(|t| t.pnl < Decimal::ZERO).collect();

            let avg_win = if !winning_trades.is_empty() {
                winning_trades.iter().map(|t| t.pnl).sum::<Decimal>() / Decimal::from(winning_trades.len())
            } else {
                Decimal::ZERO
            };

            let avg_loss = if !losing_trades.is_empty() {
                losing_trades.iter().map(|t| t.pnl).sum::<Decimal>() / Decimal::from(losing_trades.len())
            } else {
                Decimal::ZERO
            };

            let total_wins: Decimal = winning_trades.iter().map(|t| t.pnl).sum();
            let total_losses: Decimal = losing_trades.iter().map(|t| t.pnl).sum();
            let profit_factor = if total_losses.abs() > Decimal::ZERO {
                total_wins / total_losses.abs()
            } else {
                Decimal::ZERO
            };

            metrics.insert(
                strategy.clone(),
                StrategyMetrics {
                    strategy,
                    trades: trade_count,
                    win_rate,
                    total_pnl: strat_pnl,
                    avg_pnl,
                    sharpe_ratio: None, // TODO: Calculate per-strategy Sharpe
                    avg_win,
                    avg_loss,
                    profit_factor,
                },
            );
        }

        metrics
    }

    fn calculate_rolling_returns(trades: &[TradeRecord], initial_capital: Decimal) -> RollingReturns {
        if trades.is_empty() {
            return RollingReturns {
                return_7d: Decimal::ZERO,
                return_30d: Decimal::ZERO,
                return_90d: Decimal::ZERO,
                return_ytd: Decimal::ZERO,
                return_all_time: Decimal::ZERO,
            };
        }

        // Build equity curve to find equity at various points in time
        let mut equity_at_time: Vec<(DateTime<Utc>, Decimal)> = vec![(trades[0].timestamp, initial_capital)];
        let mut running_equity = initial_capital;

        for trade in trades {
            running_equity += trade.pnl;
            equity_at_time.push((trade.timestamp, running_equity));
        }

        // Use the last trade date as reference point
        let reference_date = trades.last().map(|t| t.timestamp).unwrap_or_else(Utc::now);
        let final_equity = running_equity;

        // Find equity at start of each period
        let date_7d_ago = reference_date - Duration::days(7);
        let date_30d_ago = reference_date - Duration::days(30);
        let date_90d_ago = reference_date - Duration::days(90);

        // Helper function to find equity at a specific date
        let find_equity_at = |target_date: DateTime<Utc>| -> Decimal {
            equity_at_time
                .iter()
                .filter(|(date, _)| *date <= target_date)
                .last()
                .map(|(_, equity)| *equity)
                .unwrap_or(initial_capital)
        };

        let equity_7d_ago = find_equity_at(date_7d_ago);
        let equity_30d_ago = find_equity_at(date_30d_ago);
        let equity_90d_ago = find_equity_at(date_90d_ago);

        // Get start of reference year
        let year_start = reference_date.date_naive()
            .with_month(1).unwrap()
            .with_day(1).unwrap()
            .and_hms_opt(0, 0, 0).unwrap()
            .and_utc();
        let equity_ytd_start = find_equity_at(year_start);

        // Calculate returns as percentage change from starting equity
        let calc_return = |start_equity: Decimal, end_equity: Decimal| -> Decimal {
            if start_equity > Decimal::ZERO {
                (end_equity - start_equity) / start_equity * dec!(100)
            } else {
                Decimal::ZERO
            }
        };

        RollingReturns {
            return_7d: calc_return(equity_7d_ago, final_equity),
            return_30d: calc_return(equity_30d_ago, final_equity),
            return_90d: calc_return(equity_90d_ago, final_equity),
            return_ytd: calc_return(equity_ytd_start, final_equity),
            return_all_time: calc_return(initial_capital, final_equity),
        }
    }

    fn calculate_drawdown_analysis(trades: &[TradeRecord], initial_capital: Decimal) -> DrawdownAnalysis {
        if trades.is_empty() {
            return DrawdownAnalysis {
                current_drawdown: Decimal::ZERO,
                max_drawdown: Decimal::ZERO,
                max_drawdown_pct: Decimal::ZERO,
                max_drawdown_date: None,
                max_drawdown_duration_hours: 0,
                avg_drawdown: Decimal::ZERO,
                recovery_time_hours: None,
                drawdown_periods: vec![],
            };
        }

        // Calculate equity curve
        let mut equity = initial_capital;
        let mut peak = initial_capital;
        let mut max_dd = Decimal::ZERO;
        let mut max_dd_date: Option<DateTime<Utc>> = None;
        let mut current_dd = Decimal::ZERO;
        let mut drawdown_periods = Vec::new();
        let mut in_drawdown = false;
        let mut dd_start = trades[0].timestamp;
        let mut dd_peak_equity = initial_capital;

        for trade in trades {
            equity += trade.pnl;

            if equity > peak {
                // New peak - end any drawdown
                if in_drawdown {
                    let duration = trade.timestamp.signed_duration_since(dd_start);
                    drawdown_periods.push(DrawdownPeriod {
                        start_time: dd_start,
                        end_time: Some(trade.timestamp),
                        peak_equity: dd_peak_equity,
                        trough_equity: equity,
                        drawdown_pct: current_dd,
                        duration_hours: duration.num_hours() as u64,
                    });
                    in_drawdown = false;
                }
                peak = equity;
                current_dd = Decimal::ZERO;
            } else {
                // In drawdown
                if !in_drawdown {
                    in_drawdown = true;
                    dd_start = trade.timestamp;
                    dd_peak_equity = peak;
                }

                current_dd = (peak - equity) / peak * dec!(100);
                if current_dd > max_dd {
                    max_dd = current_dd;
                    max_dd_date = Some(trade.timestamp);
                }
            }
        }

        // Handle ongoing drawdown
        if in_drawdown {
            drawdown_periods.push(DrawdownPeriod {
                start_time: dd_start,
                end_time: None,
                peak_equity: dd_peak_equity,
                trough_equity: equity,
                drawdown_pct: current_dd,
                duration_hours: Utc::now().signed_duration_since(dd_start).num_hours() as u64,
            });
        }

        let avg_drawdown = if !drawdown_periods.is_empty() {
            drawdown_periods.iter().map(|d| d.drawdown_pct).sum::<Decimal>()
                / Decimal::from(drawdown_periods.len())
        } else {
            Decimal::ZERO
        };

        let max_dd_duration = drawdown_periods
            .iter()
            .filter(|d| d.end_time.is_some())
            .map(|d| d.duration_hours)
            .max()
            .unwrap_or(0);

        DrawdownAnalysis {
            current_drawdown: current_dd,
            max_drawdown: max_dd,
            max_drawdown_pct: max_dd, // Same value, for dashboard compatibility
            max_drawdown_date: max_dd_date,
            max_drawdown_duration_hours: max_dd_duration,
            avg_drawdown,
            recovery_time_hours: if in_drawdown { None } else { Some(0) },
            drawdown_periods,
        }
    }

    fn calculate_trade_distribution(trades: &[TradeRecord]) -> TradeDistribution {
        // P&L Histogram
        let mut pnl_histogram = vec![
            HistogramBucket { range_start: dec!(-1000), range_end: dec!(-100), count: 0, label: "< -$100".to_string() },
            HistogramBucket { range_start: dec!(-100), range_end: dec!(-50), count: 0, label: "-$100 to -$50".to_string() },
            HistogramBucket { range_start: dec!(-50), range_end: dec!(0), count: 0, label: "-$50 to $0".to_string() },
            HistogramBucket { range_start: dec!(0), range_end: dec!(50), count: 0, label: "$0 to $50".to_string() },
            HistogramBucket { range_start: dec!(50), range_end: dec!(100), count: 0, label: "$50 to $100".to_string() },
            HistogramBucket { range_start: dec!(100), range_end: dec!(1000), count: 0, label: "> $100".to_string() },
        ];

        for trade in trades {
            for bucket in &mut pnl_histogram {
                if trade.pnl >= bucket.range_start && trade.pnl < bucket.range_end {
                    bucket.count += 1;
                    break;
                }
            }
        }

        // Hourly performance
        let mut hourly_stats: Vec<HourlyStats> = (0..24)
            .map(|hour| HourlyStats {
                hour,
                trades: 0,
                win_rate: Decimal::ZERO,
                avg_pnl: Decimal::ZERO,
            })
            .collect();

        for trade in trades {
            let hour = trade.timestamp.hour();
            if let Some(stats) = hourly_stats.get_mut(hour as usize) {
                stats.trades += 1;
            }
        }

        for stats in &mut hourly_stats {
            let hour_trades: Vec<_> = trades
                .iter()
                .filter(|t| t.timestamp.hour() == stats.hour)
                .collect();

            if !hour_trades.is_empty() {
                let wins = hour_trades.iter().filter(|t| t.pnl > Decimal::ZERO).count();
                stats.win_rate = Decimal::from(wins) / Decimal::from(hour_trades.len()) * dec!(100);
                stats.avg_pnl = hour_trades.iter().map(|t| t.pnl).sum::<Decimal>()
                    / Decimal::from(hour_trades.len());
            }
        }

        // Daily performance
        let days = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
        let mut daily_stats: Vec<DailyStats> = days
            .iter()
            .map(|&day| DailyStats {
                day: day.to_string(),
                trades: 0,
                win_rate: Decimal::ZERO,
                avg_pnl: Decimal::ZERO,
            })
            .collect();

        for trade in trades {
            let weekday = trade.timestamp.weekday().num_days_from_monday() as usize;
            if let Some(stats) = daily_stats.get_mut(weekday) {
                stats.trades += 1;
            }
        }

        for (idx, stats) in daily_stats.iter_mut().enumerate() {
            let day_trades: Vec<_> = trades
                .iter()
                .filter(|t| t.timestamp.weekday().num_days_from_monday() as usize == idx)
                .collect();

            if !day_trades.is_empty() {
                let wins = day_trades.iter().filter(|t| t.pnl > Decimal::ZERO).count();
                stats.win_rate = Decimal::from(wins) / Decimal::from(day_trades.len()) * dec!(100);
                stats.avg_pnl = day_trades.iter().map(|t| t.pnl).sum::<Decimal>()
                    / Decimal::from(day_trades.len());
            }
        }

        TradeDistribution {
            pnl_histogram,
            duration_histogram: vec![], // TODO: Implement
            hourly_performance: hourly_stats,
            daily_performance: daily_stats,
        }
    }

    fn calculate_risk_metrics(
        trades: &[TradeRecord],
        initial_capital: Decimal,
        current_equity: Decimal,
    ) -> RiskMetrics {
        if trades.is_empty() {
            return RiskMetrics {
                sharpe_ratio: Decimal::ZERO,
                sortino_ratio: Decimal::ZERO,
                calmar_ratio: Decimal::ZERO,
                max_drawdown: Decimal::ZERO,
                avg_drawdown: Decimal::ZERO,
                volatility: Decimal::ZERO,
                downside_deviation: Decimal::ZERO,
            };
        }

        // Calculate returns as fractions (pnl_pct is already in percent, so divide by 100)
        let returns: Vec<f64> = trades
            .iter()
            .map(|t| t.pnl_pct.to_f64().unwrap_or(0.0) / 100.0)
            .collect();
        let n = returns.len() as f64;
        let mean_return = returns.iter().sum::<f64>() / n;

        // Volatility (standard deviation of returns)
        let variance = returns
            .iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>()
            / n;
        let std_dev = variance.sqrt();

        // Annualization factor for Sharpe/Sortino (sqrt(252) for 252 trading days)
        let annualization_factor = 252_f64.sqrt();

        // Sharpe Ratio (assuming 0% risk-free rate, annualized)
        let sharpe_ratio = if std_dev > 0.0 {
            (mean_return / std_dev) * annualization_factor
        } else {
            0.0
        };

        // Downside deviation (only negative returns)
        let negative_returns: Vec<f64> = returns
            .iter()
            .filter(|&&r| r < 0.0)
            .copied()
            .collect();

        let downside_deviation = if !negative_returns.is_empty() {
            let downside_variance = negative_returns
                .iter()
                .map(|r| r.powi(2))
                .sum::<f64>()
                / negative_returns.len() as f64;
            downside_variance.sqrt()
        } else {
            0.0
        };

        // Sortino Ratio (annualized)
        let sortino_ratio = if downside_deviation > 0.0 {
            (mean_return / downside_deviation) * annualization_factor
        } else if mean_return > 0.0 {
            999.99 // Very high
        } else {
            0.0
        };

        // Store volatility as annualized percentage
        let volatility = Decimal::from_f64_retain(std_dev * 100.0 * annualization_factor)
            .unwrap_or(Decimal::ZERO);

        // Max drawdown
        let dd_analysis = Self::calculate_drawdown_analysis(trades, initial_capital);

        // Calmar Ratio (return / max drawdown)
        let total_return = (current_equity - initial_capital) / initial_capital * dec!(100);
        let calmar_ratio = if dd_analysis.max_drawdown > Decimal::ZERO {
            total_return / dd_analysis.max_drawdown
        } else if total_return > Decimal::ZERO {
            dec!(999.99)
        } else {
            Decimal::ZERO
        };

        RiskMetrics {
            sharpe_ratio: Decimal::from_f64_retain(sharpe_ratio).unwrap_or(Decimal::ZERO),
            sortino_ratio: Decimal::from_f64_retain(sortino_ratio).unwrap_or(Decimal::ZERO),
            calmar_ratio,
            max_drawdown: dd_analysis.max_drawdown,
            avg_drawdown: dd_analysis.avg_drawdown,
            volatility,
            downside_deviation: Decimal::from_f64_retain(downside_deviation * 100.0 * annualization_factor)
                .unwrap_or(Decimal::ZERO),
        }
    }

    fn calculate_win_loss_streaks(trades: &[TradeRecord]) -> WinLossStreaks {
        if trades.is_empty() {
            return WinLossStreaks {
                current_streak: 0,
                max_win_streak: 0,
                max_loss_streak: 0,
                avg_win_streak: Decimal::ZERO,
                avg_loss_streak: Decimal::ZERO,
            };
        }

        let mut current_streak = 0i32;
        let mut max_win_streak = 0u32;
        let mut max_loss_streak = 0u32;
        let mut current_win_streak = 0u32;
        let mut current_loss_streak = 0u32;

        let mut win_streaks = Vec::new();
        let mut loss_streaks = Vec::new();

        for trade in trades {
            if trade.pnl > Decimal::ZERO {
                // Win
                if current_streak >= 0 {
                    current_streak += 1;
                    current_win_streak += 1;
                } else {
                    // Streak broken
                    if current_loss_streak > 0 {
                        loss_streaks.push(current_loss_streak);
                    }
                    current_streak = 1;
                    current_win_streak = 1;
                    current_loss_streak = 0;
                }
                max_win_streak = max_win_streak.max(current_win_streak);
            } else if trade.pnl < Decimal::ZERO {
                // Loss
                if current_streak <= 0 {
                    current_streak -= 1;
                    current_loss_streak += 1;
                } else {
                    // Streak broken
                    if current_win_streak > 0 {
                        win_streaks.push(current_win_streak);
                    }
                    current_streak = -1;
                    current_loss_streak = 1;
                    current_win_streak = 0;
                }
                max_loss_streak = max_loss_streak.max(current_loss_streak);
            }
        }

        // Add final streak
        if current_win_streak > 0 {
            win_streaks.push(current_win_streak);
        }
        if current_loss_streak > 0 {
            loss_streaks.push(current_loss_streak);
        }

        let avg_win_streak = if !win_streaks.is_empty() {
            Decimal::from(win_streaks.iter().sum::<u32>()) / Decimal::from(win_streaks.len())
        } else {
            Decimal::ZERO
        };

        let avg_loss_streak = if !loss_streaks.is_empty() {
            Decimal::from(loss_streaks.iter().sum::<u32>()) / Decimal::from(loss_streaks.len())
        } else {
            Decimal::ZERO
        };

        WinLossStreaks {
            current_streak,
            max_win_streak,
            max_loss_streak,
            avg_win_streak,
            avg_loss_streak,
        }
    }
}
