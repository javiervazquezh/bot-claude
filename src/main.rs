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
mod ml;

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
        /// Path to trained HMM model JSON file
        #[arg(long)]
        hmm: Option<String>,
        /// Path to ONNX ensemble model directory
        #[arg(long)]
        ensemble: Option<String>,
    },
    /// Export training data from backtest for Python ML training
    ExportTrainingData {
        /// Start date (YYYY-MM-DD)
        #[arg(short, long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(short, long)]
        end: String,
        /// Output CSV file path
        #[arg(short, long, default_value = "training_data.csv")]
        output: String,
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
    /// Train HMM regime detector from historical data
    TrainHmm {
        /// Trading pair to train on (e.g., BTCUSDT, ETHUSDT, SOLUSDT)
        #[arg(short, long, default_value = "BTCUSDT")]
        pair: String,
        /// Start date (YYYY-MM-DD)
        #[arg(short, long)]
        start: String,
        /// End date (YYYY-MM-DD)
        #[arg(short, long)]
        end: String,
        /// Timeframe (M5, M15, H1, H4, D1)
        #[arg(short = 'f', long, default_value = "H1")]
        timeframe: String,
        /// Output directory for trained model (defaults to ~/.claude/models/)
        #[arg(short, long)]
        output: Option<String>,
        /// Number of EM iterations
        #[arg(long, default_value = "50")]
        n_iter: usize,
        /// Convergence tolerance
        #[arg(long, default_value = "0.0001")]
        tolerance: f64,
    },
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
        Commands::Backtest { start, end, save_to_db, walk_forward, hmm, ensemble } => {
            if let Some(n_windows) = walk_forward {
                run_walk_forward_backtest(&start, &end, n_windows, hmm.as_deref()).await?;
            } else {
                run_backtest(&start, &end, save_to_db, hmm.as_deref(), ensemble.as_deref()).await?;
            }
        }
        Commands::ExportTrainingData { start, end, output } => {
            export_training_data(&start, &end, &output).await?;
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
        Commands::TrainHmm { pair, start, end, timeframe, output, n_iter, tolerance } => {
            train_hmm_model(&pair, &start, &end, &timeframe, output.as_deref(), n_iter, tolerance).await?;
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

    // Restore portfolio state from database if available
    if let Ok(Some((usdt_balance, _initial_capital, total_pnl, peak_equity, max_drawdown))) =
        db.load_portfolio_state().await
    {
        let mut portfolio = engine.get_portfolio_mut().await;
        portfolio.set_balance("USDT", usdt_balance);
        portfolio.total_pnl = total_pnl;
        portfolio.peak_equity = peak_equity;
        portfolio.max_drawdown = max_drawdown;
        info!("Restored portfolio state: balance=${:.2}, PnL=${:.2}, drawdown={:.2}%",
            usdt_balance, total_pnl, max_drawdown);
    }

    // Restore open positions from database
    if let Ok(positions) = db.get_open_positions().await {
        if !positions.is_empty() {
            let mut portfolio = engine.get_portfolio_mut().await;
            for position in positions {
                info!("Restored position: {} {} @ ${:.2}", position.pair, position.quantity, position.entry_price);
                portfolio.positions.insert(position.id.clone(), position);
            }
        }
    }

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
    ws.subscribe_all_pairs(TimeFrame::H1);
    let mut event_rx = ws.connect().await?;

    info!("Connected to market data feed");
    info!("Monitoring BTC, ETH, SOL on 1-hour timeframe");
    info!("Press Ctrl+C to stop");

    // Update initial portfolio state
    update_dashboard_portfolio(&dashboard, &engine).await;

    // Display initial portfolio
    let summary = engine.portfolio_summary().await;
    println!("\n{}", summary);

    // Log startup
    dashboard.add_log("INFO".to_string(), "Bot initialized with historical candles - ready to trade!".to_string()).await;
    dashboard.add_log("INFO".to_string(), format!("Monitoring {} pairs on 1-hour timeframe", strategies.len())).await;

    // Main trading loop
    let mut candle_count = 0u64;

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

                            // Run analysis on every closed candle (not gated by timer)
                            {
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

                                    // Update drawdown tracking (C2)
                                    {
                                        let prices = engine.prices_arc();
                                        let prices_map = prices.read().await;
                                        let mut portfolio = engine.get_portfolio_mut().await;
                                        portfolio.update_drawdown(&prices_map);
                                    }

                                    // Check emergency stop (C3)
                                    {
                                        let portfolio = engine.get_portfolio().await;
                                        if risk_manager.check_emergency_stop(&portfolio).await {
                                            warn!("EMERGENCY STOP triggered! Closing all positions.");
                                            dashboard.add_log("CRITICAL".to_string(), "Emergency stop triggered - closing all positions".to_string()).await;
                                            // Force-close all open positions
                                            for pos in portfolio.get_open_positions() {
                                                if let Some(_price) = engine.get_price(pos.pair).await {
                                                    let request = crate::types::OrderRequest::market(
                                                        pos.pair, Side::Sell, pos.quantity,
                                                    );
                                                    if let Err(e) = engine.place_order(request).await {
                                                        error!("Failed to emergency close {}: {}", pos.pair, e);
                                                    }
                                                }
                                            }
                                            let _ = controller.stop().await;
                                        }
                                    }
                                }

                                // Update dashboard portfolio
                                update_dashboard_portfolio(&dashboard, &engine).await;

                                // Periodic console summary and state persistence
                                if candle_count % 48 == 0 {
                                    let summary = engine.portfolio_summary().await;
                                    println!("\n{}", summary);

                                    // Save portfolio state to database
                                    let save_portfolio = engine.get_portfolio().await;
                                    if let Err(e) = db.save_portfolio_state(
                                        save_portfolio.available_usdt(),
                                        save_portfolio.initial_capital,
                                        save_portfolio.total_pnl,
                                        save_portfolio.peak_equity,
                                        save_portfolio.max_drawdown,
                                    ).await {
                                        warn!("Failed to save portfolio state: {}", e);
                                    }
                                    for pos in save_portfolio.get_open_positions() {
                                        if let Err(e) = db.upsert_position(pos).await {
                                            warn!("Failed to save position: {}", e);
                                        }
                                    }
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

    // Save final state to database
    let final_portfolio = engine.get_portfolio().await;
    if let Err(e) = db.save_portfolio_state(
        final_portfolio.available_usdt(),
        final_portfolio.initial_capital,
        final_portfolio.total_pnl,
        final_portfolio.peak_equity,
        final_portfolio.max_drawdown,
    ).await {
        warn!("Failed to save final portfolio state: {}", e);
    }
    for pos in final_portfolio.get_open_positions() {
        if let Err(e) = db.upsert_position(pos).await {
            warn!("Failed to save final position state: {}", e);
        }
    }
    info!("Portfolio state saved to database");

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

async fn run_backtest(start: &str, end: &str, save_to_db: bool, hmm_path: Option<&str>, ensemble_dir: Option<&str>) -> Result<()> {
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
        info!("Timeframe: 1-hour");
        info!("Initial Capital: $10,000");
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
            initial_capital: Decimal::from(10000),
            timeframe: TimeFrame::H1,
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
            max_drawdown_pct: dec!(15),
            walk_forward_windows: None,
            walk_forward_oos_pct: dec!(0.25),
            hmm_model_path: hmm_path.map(|s| s.to_string()),
            ensemble_model_dir: ensemble_dir.map(|s| s.to_string()),
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

    // Standard backtest comparison (3 scenarios with different risk profiles)
    info!("=== Starting Comprehensive Backtest Comparison ===");
    info!("Period: {} to {}", start_date, end_date);
    info!("Pairs: BTC, ETH, SOL");
    info!("Timeframe: 1-hour");
    info!("Initial Capital: $10,000");
    info!("Running 3 scenarios: Conservative, Moderate, Aggressive");
    println!();

    // Scenario 1: Conservative (high confidence, low risk, low allocation)
    info!("\n{}", "=".repeat(80));
    info!("SCENARIO 1: Conservative");
    info!("{}", "=".repeat(80));
    let conservative = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(10000),
        timeframe: TimeFrame::H1,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.68),   // High bar: only strong signals
        min_risk_reward: dec!(2.0),   // Require solid R:R
        risk_per_trade: dec!(0.03),   // 3% risk per trade
        max_allocation: dec!(0.35),   // 35% max allocation per position
        max_correlated_positions: 2,
        max_drawdown_pct: dec!(10),   // Tight emergency stop
        walk_forward_windows: None,
        walk_forward_oos_pct: dec!(0.25),
        hmm_model_path: hmm_path.map(|s| s.to_string()),
            ensemble_model_dir: ensemble_dir.map(|s| s.to_string()),
    };
    let mut engine1 = BacktestEngine::new(conservative);
    let results1 = engine1.run().await?;
    results1.print_summary();
    let json1 = serde_json::to_string_pretty(&results1)?;
    std::fs::write("backtest_conservative.json", &json1)?;
    info!("Results saved to backtest_conservative.json");

    // Scenario 2: Moderate (balanced confidence, moderate risk)
    info!("\n{}", "=".repeat(80));
    info!("SCENARIO 2: Moderate");
    info!("{}", "=".repeat(80));
    let moderate = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(10000),
        timeframe: TimeFrame::H1,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.65),   // Same as H4 moderate
        min_risk_reward: dec!(2.0),   // Same as H4
        risk_per_trade: dec!(0.05),   // 5% risk per trade (same as H4)
        max_allocation: dec!(0.60),   // 60% max allocation (same as H4)
        max_correlated_positions: 2,
        max_drawdown_pct: dec!(15),
        walk_forward_windows: None,
        walk_forward_oos_pct: dec!(0.25),
        hmm_model_path: hmm_path.map(|s| s.to_string()),
            ensemble_model_dir: ensemble_dir.map(|s| s.to_string()),
    };
    let mut engine2 = BacktestEngine::new(moderate);
    let results2 = engine2.run().await?;
    results2.print_summary();
    let json2 = serde_json::to_string_pretty(&results2)?;
    std::fs::write("backtest_moderate.json", &json2)?;
    info!("Results saved to backtest_moderate.json");

    // Scenario 3: Aggressive (lower confidence bar, high risk, high allocation)
    info!("\n{}", "=".repeat(80));
    info!("SCENARIO 3: Aggressive");
    info!("{}", "=".repeat(80));
    let aggressive = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(10000),
        timeframe: TimeFrame::H1,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.55),   // Lower bar: more trades
        min_risk_reward: dec!(1.5),   // Accept lower R:R
        risk_per_trade: dec!(0.12),   // 12% risk per trade
        max_allocation: dec!(0.90),   // 90% max allocation per position
        max_correlated_positions: 3,
        max_drawdown_pct: dec!(20),   // Wider emergency stop
        walk_forward_windows: None,
        walk_forward_oos_pct: dec!(0.25),
        hmm_model_path: hmm_path.map(|s| s.to_string()),
            ensemble_model_dir: ensemble_dir.map(|s| s.to_string()),
    };
    let mut engine3 = BacktestEngine::new(aggressive);
    let results3 = engine3.run().await?;
    results3.print_summary();
    let json3 = serde_json::to_string_pretty(&results3)?;
    std::fs::write("backtest_aggressive.json", &json3)?;
    info!("Results saved to backtest_aggressive.json");

    // Print comparison summary
    info!("\n\n{}", "=".repeat(80));
    info!("COMPARISON SUMMARY");
    info!("{}", "=".repeat(80));
    println!("\n{:<25} {:>12} {:>12} {:>10} {:>10} {:>10}", "Scenario", "Final Equity", "Return %", "Max DD %", "Sharpe", "Alpha %");
    println!("{}", "-".repeat(83));
    println!("{:<25} ${:>11.2} {:>11.2}% {:>9.2}% {:>10.2} {:>9.2}%",
        "Conservative", results1.final_equity, results1.total_return_pct, results1.max_drawdown_pct, results1.sharpe_ratio, results1.alpha_pct);
    println!("{:<25} ${:>11.2} {:>11.2}% {:>9.2}% {:>10.2} {:>9.2}%",
        "Moderate", results2.final_equity, results2.total_return_pct, results2.max_drawdown_pct, results2.sharpe_ratio, results2.alpha_pct);
    println!("{:<25} ${:>11.2} {:>11.2}% {:>9.2}% {:>10.2} {:>9.2}%",
        "Aggressive", results3.final_equity, results3.total_return_pct, results3.max_drawdown_pct, results3.sharpe_ratio, results3.alpha_pct);
    println!("{}", "=".repeat(83));

    Ok(())
}

async fn run_walk_forward_backtest(start: &str, end: &str, n_windows: usize, hmm_path: Option<&str>) -> Result<()> {
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

    info!("=== Rolling Window Backtest ===");
    info!("Period: {} to {}", start_date, end_date);
    info!("Windows: {}", n_windows);
    info!("Pairs: BTC, ETH, SOL");
    info!("Timeframe: 1-hour");
    println!();

    let config = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(10000),
        timeframe: TimeFrame::H1,
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
        max_drawdown_pct: dec!(15),
        walk_forward_windows: Some(n_windows),
        walk_forward_oos_pct: dec!(0.25),
        hmm_model_path: hmm_path.map(|s| s.to_string()),
        ensemble_model_dir: None,
    };

    let result = BacktestEngine::run_walk_forward(config, n_windows).await?;
    result.print_summary();

    Ok(())
}

async fn export_training_data(start: &str, end: &str, output: &str) -> Result<()> {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .map_err(|_| anyhow!("Invalid start date format. Use YYYY-MM-DD"))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .map_err(|_| anyhow!("Invalid end date format. Use YYYY-MM-DD"))?;

    info!("=== Exporting Training Data ===");
    info!("Period: {} to {}", start_date, end_date);
    info!("Output: {}", output);

    // Run backtest to collect trade data
    let config = BacktestConfig {
        start_date,
        end_date,
        initial_capital: Decimal::from(10000),
        timeframe: TimeFrame::H1,
        pairs: vec![
            TradingPair::BTCUSDT,
            TradingPair::ETHUSDT,
            TradingPair::SOLUSDT,
        ],
        fee_rate: dec!(0.001),
        slippage_rate: dec!(0.0005),
        min_confidence: dec!(0.55),
        min_risk_reward: dec!(1.5),
        risk_per_trade: dec!(0.12),
        max_allocation: dec!(0.90),
        max_correlated_positions: 3,
        max_drawdown_pct: dec!(25),
        walk_forward_windows: None,
        walk_forward_oos_pct: dec!(0.25),
        hmm_model_path: None,
        ensemble_model_dir: None,
    };

    let mut engine = BacktestEngine::new(config);
    let _results = engine.run().await?;

    // Get training data from outcome tracker
    let training_data = engine.get_training_data();

    if training_data.is_empty() {
        return Err(anyhow!("No training data collected. Run a longer backtest period."));
    }

    // Write CSV
    let mut file = std::fs::File::create(output)?;
    use std::io::Write;

    // Header
    writeln!(file, "signal_strength,confidence,risk_reward_ratio,rsi_14,atr_pct,ema_spread_pct,bb_position,price_vs_200ema,volume_ratio,volatility_regime,recent_win_rate,recent_avg_pnl_pct,streak,hour_of_day,day_of_week,pair_id,ob_spread_pct,ob_depth_imbalance,ob_mid_price_momentum,ob_spread_volatility,ob_book_pressure,ob_weighted_spread,ob_best_volume_ratio,ob_depth_ratio,win,pnl_pct")?;

    // Data rows
    for (features, is_win, pnl_pct) in &training_data {
        let arr = features.to_array();
        let row: Vec<String> = arr.iter().map(|v| format!("{:.6}", v)).collect();
        writeln!(file, "{},{},{:.6}", row.join(","), if *is_win { 1 } else { 0 }, pnl_pct)?;
    }

    info!("Exported {} trades to {}", training_data.len(), output);
    info!("  Wins: {}", training_data.iter().filter(|(_, w, _)| *w).count());
    info!("  Losses: {}", training_data.iter().filter(|(_, w, _)| !*w).count());

    Ok(())
}

async fn train_hmm_model(
    pair_str: &str,
    start_str: &str,
    end_str: &str,
    timeframe_str: &str,
    output_path: Option<&str>,
    n_iter: usize,
    tolerance: f64,
) -> Result<()> {
    use chrono::{NaiveDate, NaiveTime};
    use ml::hmm::{GaussianHMM, extract_regime_features_batch};
    use std::fs::File;
    use std::io::Write;

    info!("═══════════════════════════════════════════════════");
    info!("HMM Regime Detector Training");
    info!("═══════════════════════════════════════════════════");
    info!("Pair: {}", pair_str);
    info!("Period: {} to {}", start_str, end_str);
    info!("Timeframe: {}", timeframe_str);
    info!("EM iterations: {}", n_iter);
    info!("Tolerance: {}", tolerance);

    // Parse inputs
    let pair = match pair_str.to_uppercase().as_str() {
        "BTCUSDT" | "BTC" => TradingPair::BTCUSDT,
        "ETHUSDT" | "ETH" => TradingPair::ETHUSDT,
        "SOLUSDT" | "SOL" => TradingPair::SOLUSDT,
        "BNBUSDT" | "BNB" => TradingPair::BNBUSDT,
        "ADAUSDT" | "ADA" => TradingPair::ADAUSDT,
        "XRPUSDT" | "XRP" => TradingPair::XRPUSDT,
        _ => return Err(anyhow::anyhow!("Unknown trading pair: {}", pair_str)),
    };

    let timeframe = match timeframe_str.to_uppercase().as_str() {
        "M5" | "5M" => TimeFrame::M5,
        "M15" | "15M" => TimeFrame::M15,
        "H1" | "1H" => TimeFrame::H1,
        "H4" | "4H" => TimeFrame::H4,
        "D1" | "1D" => TimeFrame::D1,
        _ => return Err(anyhow::anyhow!("Unknown timeframe: {}", timeframe_str)),
    };

    let start_date = NaiveDate::parse_from_str(start_str, "%Y-%m-%d")?;
    let end_date = NaiveDate::parse_from_str(end_str, "%Y-%m-%d")?;

    let start = start_date
        .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
        .and_utc();
    let end = end_date
        .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
        .and_utc();

    // Step 1: Fetch historical data
    info!("━━━ Step 1: Fetching historical data ━━━");
    let exchange = BinanceClient::public_only();
    let candles = exchange
        .get_historical_candles(pair, timeframe, start, end)
        .await?;
    info!("✓ Fetched {} candles", candles.len());

    if candles.len() < 100 {
        return Err(anyhow::anyhow!(
            "Not enough data for training (need at least 100 candles, got {})",
            candles.len()
        ));
    }

    // Step 2: Extract regime features
    info!("━━━ Step 2: Extracting regime features ━━━");
    let mut candle_buffer = CandleBuffer::new(candles.len());
    for candle in &candles {
        candle_buffer.push(candle.clone());
    }

    // Extract features with a reasonable window size (500 is just for the check, actual sliding window is 30)
    let features = extract_regime_features_batch(&candle_buffer, 500)?;
    let (n_obs, n_features) = features.dim();
    info!("✓ Extracted {} observations with {} features", n_obs, n_features);
    info!("  Features: log returns, volatility, volume ratio, RSI, EMA spread, MACD, price momentum, volume momentum");

    if n_obs < 30 {
        return Err(anyhow::anyhow!(
            "Not enough observations for training (need at least 30, got {})",
            n_obs
        ));
    }

    // Step 3: Initialize and train HMM
    info!("━━━ Step 3: Training 3-state Gaussian HMM ━━━");
    let mut hmm = GaussianHMM::new(n_features);

    // Initialize with K-means clustering
    info!("  Initializing with K-means clustering...");
    hmm.init_with_kmeans(&features)?;

    // Train with Baum-Welch EM algorithm
    info!("  Training with Baum-Welch EM algorithm...");
    info!("  (This may take a few minutes for large datasets)");

    let start_time = std::time::Instant::now();
    let (final_log_likelihood, converged_at) = hmm.fit(&features, n_iter, tolerance)?;
    let training_time = start_time.elapsed();

    info!("✓ Training complete in {:.1}s", training_time.as_secs_f64());
    info!("  Final log-likelihood: {:.2}", final_log_likelihood);
    info!("  Converged at iteration: {}", converged_at);

    // Step 4: Validate model by predicting states
    info!("━━━ Step 4: Validating trained model ━━━");
    let predicted_states = hmm.predict(&features)?;

    // Count state distribution
    let mut state_counts = [0usize; 3];
    for &state in &predicted_states {
        if state < 3 {
            state_counts[state] += 1;
        }
    }

    let total = predicted_states.len() as f64;
    info!("  State distribution:");
    info!("    Bull (State 0):    {:6} candles ({:5.1}%)", state_counts[0], state_counts[0] as f64 / total * 100.0);
    info!("    Bear (State 1):    {:6} candles ({:5.1}%)", state_counts[1], state_counts[1] as f64 / total * 100.0);
    info!("    Neutral (State 2): {:6} candles ({:5.1}%)", state_counts[2], state_counts[2] as f64 / total * 100.0);

    // Check for degenerate model (one state dominates >95%)
    let max_state_pct = *state_counts.iter().max().unwrap_or(&0) as f64 / total * 100.0;
    if max_state_pct > 95.0 {
        warn!("⚠ Warning: Model may be degenerate (one state covers {:.1}% of data)", max_state_pct);
        warn!("  Consider using more training data or different initialization");
    }

    // Step 5: Save trained model
    info!("━━━ Step 5: Saving trained model ━━━");

    let output_dir = match output_path {
        Some(path) => path.to_string(),
        None => {
            let home = std::env::var("HOME")?;
            format!("{}/.claude/models", home)
        }
    };

    std::fs::create_dir_all(&output_dir)?;

    // Save model to JSON file
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("hmm_{}_{}_{}_{}.json",
        pair_str.to_lowercase(),
        timeframe_str.to_lowercase(),
        start_str.replace("-", ""),
        timestamp
    );
    let model_path = std::path::Path::new(&output_dir).join(&filename);

    // Serialize HMM to JSON
    let model_json = serde_json::json!({
        "model_type": "HMM",
        "n_states": 3,
        "n_features": n_features,
        "transition": hmm.transition.as_slice().unwrap().to_vec(),
        "start_prob": hmm.start_prob.to_vec(),
        "means": hmm.means.as_slice().unwrap().to_vec(),
        "means_shape": [3, n_features],
        "training_metadata": {
            "pair": format!("{:?}", pair),
            "timeframe": format!("{:?}", timeframe),
            "start_date": start_str,
            "end_date": end_str,
            "n_observations": n_obs,
            "n_candles": candles.len(),
            "final_log_likelihood": final_log_likelihood,
            "em_iterations": converged_at,
            "state_distribution": {
                "bull_pct": format!("{:.1}", state_counts[0] as f64 / total * 100.0),
                "bear_pct": format!("{:.1}", state_counts[1] as f64 / total * 100.0),
                "neutral_pct": format!("{:.1}", state_counts[2] as f64 / total * 100.0),
            }
        }
    });

    let mut file = File::create(&model_path)?;
    file.write_all(serde_json::to_string_pretty(&model_json)?.as_bytes())?;

    let model_path_str = model_path.to_string_lossy();
    info!("✓ Model saved to: {}", model_path_str);

    // Print summary
    info!("═══════════════════════════════════════════════════");
    info!("Training Summary:");
    info!("  Training data: {} candles from {} to {}", candles.len(), start_str, end_str);
    info!("  Observations: {} (after feature extraction)", n_obs);
    info!("  Final log-likelihood: {:.2}", final_log_likelihood);
    info!("  Model file: {}", model_path_str);
    info!("═══════════════════════════════════════════════════");
    info!("✓ HMM training complete!");
    info!("");
    info!("To use this model in your strategy:");
    info!("  1. Load the saved JSON and reconstruct the HMM");
    info!("  2. Create detector: let detector = RegimeDetector::new(8);");
    info!("  3. Load model: detector.load_model(hmm).await;");
    info!("  4. Enable in strategy: strategy.with_regime_detector(Arc::new(RwLock::new(detector)))");

    Ok(())
}
