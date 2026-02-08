mod types;
mod indicators;
mod strategies;
mod exchange;
mod engine;
mod risk;
mod config;
mod web;
mod analytics;
mod database;
mod notifications;

use anyhow::{anyhow, Result};
use chrono::{NaiveDate, Utc};
use clap::{Parser, Subcommand};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

use config::{RuntimeConfig, RuntimeConfigManager};
use engine::{BacktestConfig, BacktestEngine, BotController, PaperTradingEngine, TradeExecutor};
use exchange::{BinanceClient, BinanceWebSocket, MarketEvent};
use risk::RiskManager;
use strategies::{create_strategies_for_pair, CombinedStrategy, Strategy};
use types::{TimeFrame, TradingPair, CandleBuffer, Signal, Side};
use web::{AppState, DashboardState, start_dashboard_server, SignalRecord, PortfolioState, PositionInfo, TradeRecord};

#[derive(Parser)]
#[command(name = "crypto-trading-bot")]
#[command(author = "Trading Bot")]
#[command(version = "0.1.0")]
#[command(about = "Automated cryptocurrency trading bot for Binance.US", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the trading bot in paper trading mode
    Paper {
        /// Initial capital in USDT
        #[arg(short, long, default_value = "2000")]
        capital: f64,

        /// Dashboard port (default: 3000)
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
    /// Run the trading bot in live mode (requires API keys)
    Live,
    /// Backtest strategies on historical data
    Backtest {
        /// Start date (YYYY-MM-DD)
        #[arg(short, long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(short, long)]
        end: String,
        /// Save trades to database for dashboard visualization
        #[arg(long)]
        save_to_db: bool,
        /// Run walk-forward validation with N windows
        #[arg(long)]
        walk_forward: Option<usize>,
    },
    /// Show current market prices
    Prices,
    /// Analyze current market conditions
    Analyze {
        /// Trading pair to analyze
        #[arg(short, long)]
        pair: Option<String>,
    },
    /// Show portfolio status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Crypto Trading Bot v0.1.0");

    match cli.command {
        Commands::Paper { capital, port } => {
            run_paper_trading(Decimal::try_from(capital)?, port).await?;
        }
        Commands::Live => {
            error!("Live trading not yet implemented. Use paper trading mode first.");
        }
        Commands::Backtest { start, end, save_to_db, walk_forward } => {
            if let Some(n_windows) = walk_forward {
                run_walk_forward_backtest(&start, &end, n_windows).await?;
            } else {
                run_backtest(&start, &end, save_to_db).await?;
            }
        }
        Commands::Prices => {
            show_prices().await?;
        }
        Commands::Analyze { pair } => {
            analyze_market(pair).await?;
        }
        Commands::Status => {
            info!("Status command - no active session");
        }
    }

    Ok(())
}

async fn run_paper_trading(initial_capital: Decimal, dashboard_port: u16) -> Result<()> {
    info!("Starting paper trading with ${:.2} capital", initial_capital);
    info!("Mode: Paper Trading");

    // Create runtime config and manager
    let runtime_config = RuntimeConfig::default();
    let config_manager = Arc::new(RuntimeConfigManager::new(runtime_config));

    // Initialize database first
    let db_path = "sqlite:./trading_bot.db";
    let db = Arc::new(database::Database::new(db_path).await?);
    info!("Trade database initialized");

    // Create bot controller (starts in Running state)
    let controller = Arc::new(BotController::new_running());

    // Create notification manager
    let notifications = Arc::new(notifications::NotificationManager::new(Some(Arc::clone(&db))));

    // Initialize dashboard state
    let dashboard = DashboardState::new();

    // Create app state for web server
    let app_state = AppState {
        dashboard: dashboard.clone(),
        controller: Arc::clone(&controller),
        config_manager: Arc::clone(&config_manager),
        database: Some(Arc::clone(&db)),
        notifications: Some(Arc::clone(&notifications)),
    };

    // Start dashboard server in background
    let app_state_clone = app_state.clone();
    tokio::spawn(async move {
        if let Err(e) = start_dashboard_server(app_state_clone, dashboard_port).await {
            error!("Dashboard server error: {}", e);
        }
    });

    info!("Dashboard available at http://localhost:{}", dashboard_port);

    // Initialize components
    let exchange = BinanceClient::public_only();
    let engine = Arc::new(PaperTradingEngine::new(initial_capital).with_exchange(exchange));

    // Create risk manager with shared config
    let risk_manager = Arc::new(RiskManager::new(config_manager.config_arc()));

    // Initialize engine (fetch initial data)
    engine.initialize().await?;

    // Send bot started notification
    notifications.notify(notifications::AlertType::BotStarted).await;

    // Create executor with controller and config
    let executor = Arc::new(TradeExecutor::new(
        Arc::clone(&engine),
        Arc::clone(&risk_manager),
        config_manager.config_arc(),
        Arc::clone(&controller),
        Arc::clone(&notifications),
    ));

    // Create strategies for each pair
    let mut strategies: Vec<CombinedStrategy> = vec![
        create_strategies_for_pair(TradingPair::BTCUSDT),
        create_strategies_for_pair(TradingPair::ETHUSDT),
        create_strategies_for_pair(TradingPair::SOLUSDT),
    ];

    // Connect to WebSocket for real-time data
    let mut ws = BinanceWebSocket::new();
    ws.subscribe_all_pairs(TimeFrame::M5);
    let mut event_rx = ws.connect().await?;

    info!("Connected to market data feed");
    info!("Monitoring BTC, ETH, SOL on 5-minute timeframe");
    info!("Press Ctrl+C to stop");

    // Update initial portfolio state
    update_dashboard_portfolio(&dashboard, &engine).await;

    // Display initial portfolio
    let summary = engine.portfolio_summary().await;
    println!("\n{}", summary);

    // Log startup
    dashboard.add_log("INFO".to_string(), "Bot initialized with historical candles - ready to trade!".to_string()).await;
    dashboard.add_log("INFO".to_string(), format!("Monitoring {} pairs on 5-minute timeframe", strategies.len())).await;

    // Main trading loop
    let mut candle_count = 0u64;
    let mut last_analysis = std::time::Instant::now();

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                match event {
                    MarketEvent::Candle(candle) => {
                        // Update dashboard price only when bot is running
                        if controller.is_running() && !controller.is_paused() {
                            dashboard.update_price(candle.pair, candle.close).await;
                        }

                        if candle.is_closed {
                            candle_count += 1;
                            engine.update_candle(candle.clone()).await;

                            // Run analysis every minute
                            if last_analysis.elapsed().as_secs() >= 60 {
                                last_analysis = std::time::Instant::now();

                                // Only process signals if controller allows
                                if controller.should_process_signals() {
                                    for strategy in &mut strategies {
                                        if strategy.pair() == candle.pair {
                                            if let Some(buffer) = engine.get_candles(candle.pair, TimeFrame::M5).await {
                                                if let Some(signal) = strategy.analyze(&buffer) {
                                                    let log_msg = format!(
                                                        "[{}] Signal: {:?} | Confidence: {:.0}% | {}",
                                                        signal.pair,
                                                        signal.signal,
                                                        signal.confidence * Decimal::from(100),
                                                        signal.reason
                                                    );
                                                    info!("{}", log_msg);
                                                    dashboard.add_log("INFO".to_string(), log_msg).await;

                                                    // Record signal in dashboard
                                                    let mut signal_record = SignalRecord {
                                                        timestamp: Utc::now(),
                                                        pair: signal.pair,
                                                        signal: format!("{:?}", signal.signal),
                                                        confidence: signal.confidence,
                                                        reason: signal.reason.clone(),
                                                        strategy: signal.strategy_name.clone(),
                                                        entry_price: signal.suggested_entry,
                                                        stop_loss: signal.suggested_stop_loss,
                                                        take_profit: signal.suggested_take_profit,
                                                        executed: false,
                                                    };

                                                    // Save signal to database
                                                    let signal_id = match db.insert_signal(&signal_record).await {
                                                        Ok(id) => Some(id),
                                                        Err(e) => {
                                                            warn!("Failed to save signal to database: {}", e);
                                                            None
                                                        }
                                                    };

                                                    // Process signal
                                                    if let Ok(Some(order_id)) = executor.process_signal(signal.clone()).await {
                                                        let order_msg = format!("Order placed: {}", order_id);
                                                        info!("{}", order_msg);
                                                        dashboard.add_log("INFO".to_string(), order_msg).await;
                                                        signal_record.executed = true;

                                                        // Update signal as executed in database
                                                        if let Some(sid) = signal_id {
                                                            if let Err(e) = db.update_signal_executed(sid, &order_id).await {
                                                                warn!("Failed to update signal as executed: {}", e);
                                                            }
                                                        }

                                                        // Record trade
                                                        let trade = TradeRecord {
                                                            id: order_id.clone(),
                                                            timestamp: Utc::now(),
                                                            pair: signal.pair,
                                                            side: if signal.signal.is_bullish() { Side::Buy } else { Side::Sell },
                                                            quantity: Decimal::ZERO, // Would get from actual order
                                                            entry_price: signal.suggested_entry.unwrap_or(candle.close),
                                                            exit_price: None,
                                                            pnl: Decimal::ZERO,
                                                            pnl_pct: Decimal::ZERO,
                                                            fees: Decimal::ZERO,
                                                            strategy: signal.strategy_name.clone(),
                                                            exit_reason: None,
                                                            status: "Open".to_string(),
                                                        };

                                                        // Save to database
                                                        if let Err(e) = db.insert_trade(&trade).await {
                                                            warn!("Failed to save trade to database: {}", e);
                                                        }

                                                        dashboard.add_trade(trade).await;
                                                    }

                                                    // Add signal to dashboard
                                                    dashboard.add_signal(signal_record).await;
                                                }
                                            }
                                        }
                                    }
                                }

                                // Check stop losses (always check even when paused to protect capital)
                                if controller.is_running() {
                                    if let Ok(closed) = executor.check_stop_losses().await {
                                        for id in closed {
                                            info!("Position closed by stop/take profit: {}", id);
                                        }
                                    }
                                }

                                // Update dashboard portfolio
                                update_dashboard_portfolio(&dashboard, &engine).await;

                                // Periodic console summary
                                if candle_count % 12 == 0 {
                                    let summary = engine.portfolio_summary().await;
                                    println!("\n{}", summary);
                                }
                            }
                        }
                    }
                    MarketEvent::BookTicker(ticker) => {
                        // Only update prices when bot is running and not paused
                        if controller.is_running() && !controller.is_paused() {
                            engine.update_price(ticker.pair, ticker.bid_price).await;
                            dashboard.update_price(ticker.pair, ticker.bid_price).await;
                        }
                    }
                    MarketEvent::Ticker(ticker) => {
                        // Only update prices when bot is running and not paused
                        if controller.is_running() && !controller.is_paused() {
                            engine.update_price(ticker.pair, ticker.price).await;
                            dashboard.update_price(ticker.pair, ticker.price).await;
                        }
                    }
                    MarketEvent::Disconnected => {
                        warn!("WebSocket disconnected, waiting for reconnection...");
                    }
                    MarketEvent::Error(e) => {
                        error!("WebSocket error: {}", e);
                    }
                    _ => {}
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Shutting down...");
                let _ = controller.stop().await;
                break;
            }
        }
    }

    // Final summary
    let summary = engine.portfolio_summary().await;
    println!("\n=== Final Results ===");
    println!("{}", summary);

    let portfolio = engine.get_portfolio().await;
    let pnl_pct = if !portfolio.initial_capital.is_zero() {
        (portfolio.total_pnl / portfolio.initial_capital) * Decimal::from(100)
    } else {
        Decimal::ZERO
    };

    println!("Total Return: {:.2}%", pnl_pct);
    println!("Profit Factor: {:.2}", portfolio.profit_factor());
    println!("Win Rate: {:.1}%", portfolio.win_rate());

    Ok(())
}

async fn update_dashboard_portfolio(dashboard: &DashboardState, engine: &Arc<PaperTradingEngine>) {
    let portfolio = engine.get_portfolio().await;
    let prices = engine.prices_arc();
    let prices_map = prices.read().await;

    let positions: Vec<PositionInfo> = portfolio.get_open_positions()
        .iter()
        .map(|p| {
            let duration = p.duration();
            let duration_hours = duration.num_hours();

            PositionInfo {
                pair: p.pair,
                side: format!("{:?}", p.side),
                quantity: p.quantity,
                entry_price: p.entry_price,
                current_price: p.current_price,
                pnl: p.unrealized_pnl,
                pnl_pct: p.pnl_percentage(),
                stop_loss: p.stop_loss,
                take_profit: p.take_profit,
                duration_hours,
            }
        })
        .collect();

    let state = PortfolioState {
        total_equity: portfolio.total_equity(&prices_map),
        available_balance: portfolio.available_usdt(),
        unrealized_pnl: portfolio.total_unrealized_pnl(),
        realized_pnl: portfolio.total_pnl,
        positions,
        max_drawdown: portfolio.max_drawdown,
    };

    dashboard.update_portfolio(state).await;
}

async fn show_prices() -> Result<()> {
    let client = BinanceClient::public_only();

    println!("\n=== Current Prices ===");

    for pair in TradingPair::all() {
        match client.get_ticker(pair).await {
            Ok(ticker) => {
                let change_symbol = if ticker.price_change_24h > Decimal::ZERO { "+" } else { "" };
                println!(
                    "{}: ${:.2} | 24h: {}${:.2} ({}{:.2}%) | Vol: ${:.0}",
                    pair,
                    ticker.price,
                    change_symbol,
                    ticker.price_change_24h,
                    change_symbol,
                    ticker.price_change_pct_24h,
                    ticker.volume_24h * ticker.price
                );
            }
            Err(e) => {
                error!("Failed to get price for {}: {}", pair, e);
            }
        }
    }

    Ok(())
}

async fn analyze_market(pair_str: Option<String>) -> Result<()> {
    let client = BinanceClient::public_only();

    let pairs = if let Some(p) = pair_str {
        match TradingPair::from_str(&p) {
            Some(pair) => vec![pair],
            None => {
                error!("Invalid pair: {}. Use BTCUSDT, ETHUSDT, or SOLUSDT", p);
                return Ok(());
            }
        }
    } else {
        TradingPair::all()
    };

    println!("\n=== Market Analysis ===");

    for pair in pairs {
        println!("\n--- {} ---", pair);

        // Fetch candles
        let candles = client.get_candles(pair, TimeFrame::H1, 100).await?;
        let mut buffer = CandleBuffer::new(100);
        for candle in candles {
            buffer.push(candle);
        }

        // Run analysis
        let mut strategy = create_strategies_for_pair(pair);
        if let Some(signal) = strategy.analyze(&buffer) {
            println!("Signal: {:?}", signal.signal);
            println!("Confidence: {:.0}%", signal.confidence * Decimal::from(100));
            println!("Analysis: {}", signal.reason);

            if let Some(entry) = signal.suggested_entry {
                println!("Entry: ${:.2}", entry);
            }
            if let Some(sl) = signal.suggested_stop_loss {
                println!("Stop Loss: ${:.2}", sl);
            }
            if let Some(tp) = signal.suggested_take_profit {
                println!("Take Profit: ${:.2}", tp);
            }
            if let Some(rr) = signal.risk_reward_ratio() {
                println!("Risk/Reward: {:.2}:1", rr);
            }
        } else {
            println!("Insufficient data for analysis");
        }
    }

    Ok(())
}

async fn run_backtest(start: &str, end: &str, save_to_db: bool) -> Result<()> {
    // Parse dates
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .map_err(|_| anyhow!("Invalid start date format. Use YYYY-MM-DD"))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .map_err(|_| anyhow!("Invalid end date format. Use YYYY-MM-DD"))?;

    if end_date <= start_date {
        return Err(anyhow!("End date must be after start date"));
    }

    // If save_to_db is set, run a simplified single scenario and save to database
    if save_to_db {
        info!("=== Running Backtest and Saving to Database ===");
        info!("Period: {} to {}", start_date, end_date);
        info!("Pairs: BTC, ETH, SOL");
        info!("Timeframe: 4-hour");
        info!("Initial Capital: $2,000");
        info!("Strategy: Conservative (5% risk, 60% allocation)");
        info!("This will take several minutes to fetch and process data...");
        println!();

        // Initialize database
        let db_path = "sqlite:./trading_bot.db";
        let db = database::Database::new(db_path).await?;
        info!("Database initialized");

        // Run backtest
        let config = BacktestConfig {
            start_date,
            end_date,
            initial_capital: Decimal::from(2000),
            timeframe: TimeFrame::H4,
            pairs: vec![
                TradingPair::BTCUSDT,
                TradingPair::ETHUSDT,
                TradingPair::SOLUSDT,
            ],
            fee_rate: dec!(0.001),
            slippage_rate: dec!(0.0005),
            min_confidence: dec!(0.65),
            min_risk_reward: dec!(2.0),
            risk_per_trade: dec!(0.05),
            max_allocation: dec!(0.60),
            max_correlated_positions: 2,
            walk_forward_windows: None,
            walk_forward_oos_pct: dec!(0.25),
        };

        let mut engine = BacktestEngine::new(config);
        let results = engine.run().await?;

        results.print_summary();

        // Save trades to database
        info!("\nInserting {} trades into database...", results.trades.len());

        for (i, backtest_trade) in results.trades.iter().enumerate() {
            // Convert from backtest TradeRecord to web TradeRecord
            let db_trade = TradeRecord {
                id: backtest_trade.id.clone(),
                timestamp: backtest_trade.exit_time,
                pair: backtest_trade.pair,
                side: backtest_trade.side,
                quantity: backtest_trade.quantity,
                entry_price: backtest_trade.entry_price,
                exit_price: Some(backtest_trade.exit_price),
                pnl: backtest_trade.pnl,
                pnl_pct: backtest_trade.pnl_pct,
                fees: backtest_trade.fees,
                strategy: backtest_trade.strategy.clone(),
                exit_reason: Some(format!("{:?}", backtest_trade.exit_reason)),
                status: "Closed".to_string(),
            };

            db.insert_trade(&db_trade).await?;

            if (i + 1) % 10 == 0 {
                info!("Inserted {}/{} trades", i + 1, results.trades.len());
            }
        }

        info!("\n✓ Successfully inserted {} trades into database", results.trades.len());
        info!("✓ Dashboard at http://localhost:3000 will now show historical trading data");
        info!("\nBacktest Summary:");
        info!("  Final Equity: ${:.2}", results.final_equity);
        info!("  Total Return: {:.2}%", results.total_return_pct);
        info!("  Total Trades: {}", results.total_trades);
        info!("  Win Rate: {:.1}%", results.win_rate_pct);
        info!("  Max Drawdown: {:.2}%", results.max_drawdown_pct);

        return Ok(());
    }

    // Standard backtest comparison (4 scenarios)
    info!("=== Starting Comprehensive Backtest Comparison ===");
    info!("Period: {} to {}", start_date, end_date);
    info!("Pairs: BTC, ETH, SOL");
    info!("Timeframe: 4-hour");
    info!("Initial Capital: $2,000");
    info!("Running 4 scenarios: Conservative (w/ & w/o PM), Aggressive (w/ & w/o PM)");
    println!();

    // Scenario 1: Conservative WITHOUT position management
    info!("\n{}", "=".repeat(80));
    info!("SCENARIO 1: Conservative 5-Year Profile WITHOUT Position Management");
    info!("{}", "=".repeat(80));
    let conservative_no_pm = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(2000),
        timeframe: TimeFrame::H4,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.65),  // Conservative 5-Year: 0.65 (2623% over 5 years)
        min_risk_reward: dec!(2.0),  // Conservative 5-Year: 2.0 R:R
        risk_per_trade: dec!(0.05),  // Conservative: 5% risk per trade
        max_allocation: dec!(0.60),  // Conservative: 60% max allocation per position
        max_correlated_positions: 2,
        walk_forward_windows: None,
        walk_forward_oos_pct: dec!(0.25),
    };
    let mut engine1 = BacktestEngine::new(conservative_no_pm);
    let results1 = engine1.run().await?;
    results1.print_summary();
    let json1 = serde_json::to_string_pretty(&results1)?;
    std::fs::write("backtest_conservative_no_pm.json", &json1)?;
    info!("Results saved to backtest_conservative_no_pm.json");

    // Scenario 2: Conservative WITH position management
    info!("\n{}", "=".repeat(80));
    info!("SCENARIO 2: Conservative 5-Year Profile WITH Position Management");
    info!("{}", "=".repeat(80));
    let conservative_with_pm = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(2000),
        timeframe: TimeFrame::H4,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.65),  // Conservative 5-Year: 0.65
        min_risk_reward: dec!(2.0),  // Conservative 5-Year: 2.0 R:R
        risk_per_trade: dec!(0.05),  // Conservative: 5% risk per trade
        max_allocation: dec!(0.60),  // Conservative: 60% max allocation per position
        max_correlated_positions: 2,
        walk_forward_windows: None,
        walk_forward_oos_pct: dec!(0.25),
    };
    let mut engine2 = BacktestEngine::new(conservative_with_pm);
    let results2 = engine2.run().await?;
    results2.print_summary();
    let json2 = serde_json::to_string_pretty(&results2)?;
    std::fs::write("backtest_conservative_with_pm.json", &json2)?;
    info!("Results saved to backtest_conservative_with_pm.json");

    // Scenario 3: Ultra Aggressive WITHOUT position management
    info!("\n{}", "=".repeat(80));
    info!("SCENARIO 3: Ultra Aggressive Profile WITHOUT Position Management");
    info!("{}", "=".repeat(80));
    let aggressive_no_pm = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(2000),
        timeframe: TimeFrame::H4,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.65),  // Ultra Aggressive: 0.65 (3965% over 5 years)
        min_risk_reward: dec!(2.0),  // Ultra Aggressive: 2.0 R:R
        risk_per_trade: dec!(0.12),  // Ultra Aggressive: 12% risk per trade
        max_allocation: dec!(0.90),  // Ultra Aggressive: 90% max allocation per position
        max_correlated_positions: 3,
        walk_forward_windows: None,
        walk_forward_oos_pct: dec!(0.25),
    };
    let mut engine3 = BacktestEngine::new(aggressive_no_pm);
    let results3 = engine3.run().await?;
    results3.print_summary();
    let json3 = serde_json::to_string_pretty(&results3)?;
    std::fs::write("backtest_aggressive_no_pm.json", &json3)?;
    info!("Results saved to backtest_aggressive_no_pm.json");

    // Scenario 4: Ultra Aggressive WITH position management
    info!("\n{}", "=".repeat(80));
    info!("SCENARIO 4: Ultra Aggressive Profile WITH Position Management");
    info!("{}", "=".repeat(80));
    let aggressive_with_pm = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(2000),
        timeframe: TimeFrame::H4,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.65),  // Ultra Aggressive: 0.65
        min_risk_reward: dec!(2.0),  // Ultra Aggressive: 2.0 R:R
        risk_per_trade: dec!(0.12),  // Ultra Aggressive: 12% risk per trade
        max_allocation: dec!(0.90),  // Ultra Aggressive: 90% max allocation per position
        max_correlated_positions: 3,
        walk_forward_windows: None,
        walk_forward_oos_pct: dec!(0.25),
    };
    let mut engine4 = BacktestEngine::new(aggressive_with_pm);
    let results4 = engine4.run().await?;
    results4.print_summary();
    let json4 = serde_json::to_string_pretty(&results4)?;
    std::fs::write("backtest_aggressive_with_pm.json", &json4)?;
    info!("Results saved to backtest_aggressive_with_pm.json");

    // Print comparison summary
    info!("\n\n{}", "=".repeat(80));
    info!("COMPARISON SUMMARY");
    info!("{}", "=".repeat(80));
    println!("\n{:<45} {:>12} {:>12} {:>10} {:>10} {:>10}", "Scenario", "Final Equity", "Return %", "Max DD %", "Sharpe", "Alpha %");
    println!("{}", "-".repeat(103));
    println!("{:<45} ${:>11.2} {:>11.2}% {:>9.2}% {:>10.2} {:>9.2}%",
        "Conservative 5-Year WITHOUT PM", results1.final_equity, results1.total_return_pct, results1.max_drawdown_pct, results1.sharpe_ratio, results1.alpha_pct);
    println!("{:<45} ${:>11.2} {:>11.2}% {:>9.2}% {:>10.2} {:>9.2}%",
        "Conservative 5-Year WITH PM", results2.final_equity, results2.total_return_pct, results2.max_drawdown_pct, results2.sharpe_ratio, results2.alpha_pct);
    println!("{:<45} ${:>11.2} {:>11.2}% {:>9.2}% {:>10.2} {:>9.2}%",
        "Ultra Aggressive WITHOUT PM", results3.final_equity, results3.total_return_pct, results3.max_drawdown_pct, results3.sharpe_ratio, results3.alpha_pct);
    println!("{:<45} ${:>11.2} {:>11.2}% {:>9.2}% {:>10.2} {:>9.2}%",
        "Ultra Aggressive WITH PM", results4.final_equity, results4.total_return_pct, results4.max_drawdown_pct, results4.sharpe_ratio, results4.alpha_pct);
    println!("{}", "=".repeat(103));

    // Calculate improvements
    let conservative_improvement = ((results2.total_return_pct - results1.total_return_pct) / results1.total_return_pct) * dec!(100);
    let aggressive_improvement = ((results4.total_return_pct - results3.total_return_pct) / results3.total_return_pct) * dec!(100);
    let conservative_dd_improvement = results1.max_drawdown_pct - results2.max_drawdown_pct;
    let aggressive_dd_improvement = results3.max_drawdown_pct - results4.max_drawdown_pct;

    println!("\nPosition Management Impact:");
    println!("  Conservative: Return improved by {:.2}%, Drawdown reduced by {:.2}%",
        conservative_improvement, conservative_dd_improvement);
    println!("  Aggressive: Return improved by {:.2}%, Drawdown reduced by {:.2}%",
        aggressive_improvement, aggressive_dd_improvement);

    Ok(())
}

async fn run_walk_forward_backtest(start: &str, end: &str, n_windows: usize) -> Result<()> {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .map_err(|_| anyhow!("Invalid start date format. Use YYYY-MM-DD"))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .map_err(|_| anyhow!("Invalid end date format. Use YYYY-MM-DD"))?;

    if end_date <= start_date {
        return Err(anyhow!("End date must be after start date"));
    }

    if n_windows < 2 {
        return Err(anyhow!("Walk-forward requires at least 2 windows"));
    }

    info!("=== Walk-Forward Validation ===");
    info!("Period: {} to {}", start_date, end_date);
    info!("Windows: {}", n_windows);
    info!("Pairs: BTC, ETH, SOL");
    info!("Timeframe: 4-hour");
    println!();

    let config = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(2000),
        timeframe: TimeFrame::H4,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.65),
        min_risk_reward: dec!(2.0),
        risk_per_trade: dec!(0.05),
        max_allocation: dec!(0.60),
        max_correlated_positions: 2,
        walk_forward_windows: Some(n_windows),
        walk_forward_oos_pct: dec!(0.25),
    };

    let result = BacktestEngine::run_walk_forward(config, n_windows).await?;
    result.print_summary();

    Ok(())
}
