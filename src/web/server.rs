use axum::{
    routing::{get, post, put},
    Router,
    response::Html,
};
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use super::{api, AppState};

pub async fn start_dashboard_server(state: AppState, port: u16) -> anyhow::Result<()> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Dashboard page
        .route("/", get(serve_dashboard))
        // API endpoints
        .route("/api/health", get(api::health_check))
        .route("/api/data", get(api::get_dashboard_data))
        .route("/api/portfolio", get(api::get_portfolio))
        .route("/api/signals", get(api::get_signals))
        .route("/api/trades", get(api::get_trades))
        .route("/api/stats", get(api::get_stats))
        .route("/api/prices", get(api::get_prices))
        .route("/api/analytics", get(api::get_analytics))
        .route("/api/signal-stats", get(api::get_signal_stats))
        // Control endpoints
        .route("/api/control/start", post(api::post_start))
        .route("/api/control/stop", post(api::post_stop))
        .route("/api/control/pause", post(api::post_pause))
        .route("/api/control/resume", post(api::post_resume))
        .route("/api/control/status", get(api::get_status))
        // Config endpoints
        .route("/api/config", get(api::get_config))
        .route("/api/config/risk", put(api::put_risk_settings))
        .route("/api/config/executor", put(api::put_executor_settings))
        .route("/api/config/strategies", put(api::put_strategy_settings))
        .route("/api/config/general", put(api::put_general_settings))
        // Profile endpoints
        .route("/api/profiles", get(api::get_profiles))
        .route("/api/profile/current", get(api::get_current_profile))
        .route("/api/profile/select", post(api::post_select_profile))
        // Notification endpoints
        .route("/api/notifications", get(api::get_notifications))
        .route("/api/notifications/critical", get(api::get_critical_notifications))
        .route("/api/notifications/acknowledge", post(api::post_acknowledge_notification))
        // WebSocket
        .route("/ws", get(api::websocket_handler))
        .layer(cors)
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Dashboard server starting on http://localhost:{}", port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn serve_dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

const DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Crypto Trading Bot Dashboard</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/chartjs-adapter-date-fns"></script>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            background: #0f1419;
            color: #e7e9ea;
            min-height: 100vh;
        }
        .header {
            background: #16202a;
            padding: 1rem 2rem;
            border-bottom: 1px solid #2f3336;
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 2rem;
            flex-wrap: wrap;
        }
        .header h1 { font-size: 1.5rem; color: #1da1f2; }
        .header-left { display: flex; align-items: center; gap: 2rem; }
        .header-right { display: flex; align-items: center; gap: 2rem; flex-wrap: wrap; }
        .status { display: flex; align-items: center; gap: 0.5rem; }
        .status-dot {
            width: 10px; height: 10px; border-radius: 50%;
            background: #00ba7c; animation: pulse 2s infinite;
        }
        .status-dot.disconnected { background: #f4212e; animation: none; }
        @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.5; } }

        .container { padding: 1.5rem; max-width: 1600px; margin: 0 auto; }

        .grid { display: grid; gap: 1.5rem; }
        .grid-4 { grid-template-columns: repeat(4, 1fr); }
        .grid-3 { grid-template-columns: repeat(3, 1fr); }
        .grid-2 { grid-template-columns: repeat(2, 1fr); }

        @media (max-width: 1200px) { .grid-4, .grid-3 { grid-template-columns: repeat(2, 1fr); } }
        @media (max-width: 768px) { .grid-4, .grid-3, .grid-2 { grid-template-columns: 1fr; } }

        .card {
            background: #16202a;
            border-radius: 12px;
            padding: 1.5rem;
            border: 1px solid #2f3336;
        }
        .card-title {
            font-size: 0.875rem;
            color: #71767b;
            text-transform: uppercase;
            letter-spacing: 0.5px;
            margin-bottom: 0.75rem;
        }
        .card-value {
            font-size: 2rem;
            font-weight: 700;
        }
        .card-subtitle { font-size: 0.875rem; color: #71767b; margin-top: 0.25rem; }

        .positive { color: #00ba7c; }
        .negative { color: #f4212e; }
        .neutral { color: #71767b; }

        .chart-container { height: 300px; position: relative; }
        .chart-container.large { height: 400px; }

        .table-container { overflow-x: auto; max-height: 400px; }
        table { width: 100%; border-collapse: collapse; }
        th, td { padding: 0.75rem; text-align: left; border-bottom: 1px solid #2f3336; }
        th { color: #71767b; font-weight: 500; font-size: 0.75rem; text-transform: uppercase; position: sticky; top: 0; background: #16202a; }
        td { font-size: 0.875rem; }
        tr:hover { background: #1c2732; }

        .signal-badge {
            display: inline-block;
            padding: 0.25rem 0.5rem;
            border-radius: 4px;
            font-size: 0.75rem;
            font-weight: 600;
        }
        .signal-buy { background: rgba(0, 186, 124, 0.2); color: #00ba7c; }
        .signal-sell { background: rgba(244, 33, 46, 0.2); color: #f4212e; }
        .signal-neutral { background: rgba(113, 118, 123, 0.2); color: #71767b; }

        .price-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; }
        .price-card {
            background: #1c2732;
            padding: 1rem;
            border-radius: 8px;
            text-align: center;
        }
        .price-symbol { font-weight: 600; color: #1da1f2; margin-bottom: 0.5rem; }
        .price-value { font-size: 1.5rem; font-weight: 700; }

        .section-title {
            font-size: 1.25rem;
            font-weight: 600;
            margin-bottom: 1rem;
            color: #e7e9ea;
        }

        .mt-1 { margin-top: 1.5rem; }

        /* View Navigation Styles */
        .view-nav {
            background: #16202a;
            border-bottom: 1px solid #2f3336;
            padding: 0 2rem;
            display: flex;
            gap: 0.5rem;
        }
        .view-tab {
            padding: 1rem 1.5rem;
            background: none;
            border: none;
            color: #71767b;
            cursor: pointer;
            font-size: 0.875rem;
            font-weight: 600;
            border-bottom: 2px solid transparent;
            transition: all 0.2s;
        }
        .view-tab:hover { color: #e7e9ea; }
        .view-tab.active { color: #1da1f2; border-bottom-color: #1da1f2; }

        .view-content { display: none; }
        .view-content.active { display: block; }

        /* Control Panel Styles */
        .control-panel {
            display: flex;
            flex-wrap: wrap;
            gap: 1rem;
            align-items: center;
        }
        .control-buttons { display: flex; gap: 0.5rem; flex-wrap: wrap; }
        .header .control-buttons { gap: 0.25rem; }
        .header .control-buttons .btn {
            padding: 0.4rem 0.75rem;
            font-size: 0.8rem;
        }
        .btn {
            padding: 0.5rem 1rem;
            border: none;
            border-radius: 6px;
            font-size: 0.875rem;
            font-weight: 600;
            cursor: pointer;
            transition: all 0.2s;
        }
        .btn:disabled { opacity: 0.5; cursor: not-allowed; }
        .btn-success { background: #00ba7c; color: white; }
        .btn-success:hover:not(:disabled) { background: #00a36c; }
        .btn-warning { background: #ffad1f; color: #0f1419; }
        .btn-warning:hover:not(:disabled) { background: #e09d1c; }
        .btn-danger { background: #f4212e; color: white; }
        .btn-danger:hover:not(:disabled) { background: #dc1e28; }
        .btn-primary { background: #1da1f2; color: white; }
        .btn-primary:hover:not(:disabled) { background: #1a91da; }

        .bot-status {
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }
        .bot-status-label {
            font-size: 0.75rem;
            color: #71767b;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .status-badge {
            padding: 0.25rem 0.75rem;
            border-radius: 20px;
            font-size: 0.75rem;
            font-weight: 600;
            text-transform: uppercase;
        }
        .status-running { background: rgba(0, 186, 124, 0.2); color: #00ba7c; }
        .status-paused { background: rgba(255, 173, 31, 0.2); color: #ffad1f; }
        .status-stopped { background: rgba(244, 33, 46, 0.2); color: #f4212e; }

        .control-info {
            display: flex;
            gap: 1.5rem;
            font-size: 0.875rem;
            color: #71767b;
        }
        .header .control-info {
            gap: 1rem;
            font-size: 0.8rem;
        }

        /* Config Panel Styles */
        .config-panel { margin-top: 1.5rem; }
        .config-tabs {
            display: flex;
            gap: 0.5rem;
            border-bottom: 1px solid #2f3336;
            margin-bottom: 1rem;
        }
        .tab {
            padding: 0.75rem 1rem;
            background: none;
            border: none;
            color: #71767b;
            cursor: pointer;
            font-size: 0.875rem;
            border-bottom: 2px solid transparent;
            transition: all 0.2s;
        }
        .tab:hover { color: #e7e9ea; }
        .tab.active { color: #1da1f2; border-bottom-color: #1da1f2; }

        .tab-content { display: none; }
        .tab-content.active { display: block; }

        .form-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 1rem;
        }
        .form-group { margin-bottom: 0.5rem; }
        .form-group label {
            display: block;
            font-size: 0.75rem;
            color: #71767b;
            margin-bottom: 0.25rem;
            text-transform: uppercase;
        }
        .form-group input, .form-group select {
            width: 100%;
            padding: 0.5rem;
            background: #1c2732;
            border: 1px solid #2f3336;
            border-radius: 4px;
            color: #e7e9ea;
            font-size: 0.875rem;
        }
        .form-group input:focus, .form-group select:focus {
            outline: none;
            border-color: #1da1f2;
        }
        .checkbox-group {
            display: flex;
            gap: 1rem;
            flex-wrap: wrap;
        }
        .checkbox-group label {
            display: flex;
            align-items: center;
            gap: 0.5rem;
            text-transform: none;
            font-size: 0.875rem;
            color: #e7e9ea;
        }
        .strategy-section { margin-top: 1.5rem; padding-top: 1rem; border-top: 1px solid #2f3336; }
        .strategy-section h4 { font-size: 0.875rem; color: #1da1f2; margin-bottom: 1rem; }

        .toast {
            position: fixed;
            bottom: 20px;
            right: 20px;
            padding: 1rem 1.5rem;
            border-radius: 8px;
            font-size: 0.875rem;
            z-index: 1000;
            animation: slideIn 0.3s ease;
        }
        .toast-success { background: #00ba7c; color: white; }
        .toast-error { background: #f4212e; color: white; }
        @keyframes slideIn { from { transform: translateX(100%); opacity: 0; } to { transform: translateX(0); opacity: 1; } }

        /* Notification Styles */
        .notification-bell {
            position: relative;
            cursor: pointer;
            padding: 0.5rem;
            border-radius: 50%;
            background: #1c2732;
            display: flex;
            align-items: center;
            justify-content: center;
            transition: background 0.2s;
        }
        .notification-bell:hover { background: #2f3336; }
        .notification-bell-icon { font-size: 1.25rem; }
        .notification-badge {
            position: absolute;
            top: 0;
            right: 0;
            background: #f4212e;
            color: white;
            font-size: 0.65rem;
            font-weight: 700;
            padding: 0.15rem 0.4rem;
            border-radius: 10px;
            min-width: 18px;
            text-align: center;
        }
        .notification-panel {
            position: fixed;
            top: 70px;
            right: 20px;
            width: 400px;
            max-height: 600px;
            background: #16202a;
            border: 1px solid #2f3336;
            border-radius: 12px;
            box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
            z-index: 999;
            display: none;
            flex-direction: column;
        }
        .notification-panel.visible { display: flex; }
        .notification-header {
            padding: 1rem 1.5rem;
            border-bottom: 1px solid #2f3336;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        .notification-header h3 { font-size: 1rem; margin: 0; }
        .notification-close {
            background: none;
            border: none;
            color: #71767b;
            font-size: 1.5rem;
            cursor: pointer;
            padding: 0;
            line-height: 1;
        }
        .notification-close:hover { color: #e7e9ea; }
        .notification-list {
            overflow-y: auto;
            flex: 1;
        }
        .notification-item {
            padding: 1rem 1.5rem;
            border-bottom: 1px solid #2f3336;
            cursor: pointer;
            transition: background 0.2s;
        }
        .notification-item:hover { background: #1c2732; }
        .notification-item.unread { background: rgba(29, 161, 242, 0.05); }
        .notification-item-header {
            display: flex;
            justify-content: space-between;
            align-items: flex-start;
            margin-bottom: 0.5rem;
        }
        .notification-title {
            font-size: 0.875rem;
            font-weight: 600;
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }
        .notification-time {
            font-size: 0.75rem;
            color: #71767b;
        }
        .notification-content {
            font-size: 0.8rem;
            color: #8b98a5;
            line-height: 1.4;
        }
        .notification-severity {
            display: inline-block;
            width: 8px;
            height: 8px;
            border-radius: 50%;
            margin-right: 0.5rem;
        }
        .notification-severity.Info { background: #1da1f2; }
        .notification-severity.Warning { background: #ffad1f; }
        .notification-severity.Critical { background: #f4212e; animation: pulse 2s infinite; }
        .notification-empty {
            padding: 3rem 1.5rem;
            text-align: center;
            color: #71767b;
        }
        .clear-all-btn {
            padding: 0.75rem 1.5rem;
            border: none;
            background: #1c2732;
            color: #e7e9ea;
            border-top: 1px solid #2f3336;
            cursor: pointer;
            font-size: 0.875rem;
            transition: background 0.2s;
        }
        .clear-all-btn:hover { background: #2f3336; }

        /* Strategy Profile Styles */
        .profile-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 1rem;
        }
        .profile-card {
            background: #1c2732;
            border: 2px solid #2f3336;
            border-radius: 8px;
            padding: 1rem;
            cursor: pointer;
            transition: all 0.2s;
            position: relative;
        }
        .profile-card:hover {
            border-color: #1da1f2;
            background: #253342;
        }
        .profile-card.active {
            border-color: #00ba7c;
            background: rgba(0, 186, 124, 0.1);
        }
        .profile-card.active::before {
            content: "✓ ACTIVE";
            position: absolute;
            top: 0.5rem;
            right: 0.5rem;
            font-size: 0.7rem;
            font-weight: 700;
            color: #00ba7c;
            background: rgba(0, 186, 124, 0.2);
            padding: 0.25rem 0.5rem;
            border-radius: 4px;
        }
        .profile-name {
            font-size: 1.1rem;
            font-weight: 600;
            color: #1da1f2;
            margin-bottom: 0.5rem;
        }
        .profile-desc {
            font-size: 0.875rem;
            color: #71767b;
            margin-bottom: 0.75rem;
            line-height: 1.4;
        }
        .profile-stats {
            display: flex;
            justify-content: space-between;
            gap: 1rem;
            margin-top: 0.75rem;
            padding-top: 0.75rem;
            border-top: 1px solid #2f3336;
        }
        .profile-stat {
            text-align: center;
        }
        .profile-stat-label {
            font-size: 0.7rem;
            color: #71767b;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .profile-stat-value {
            font-size: 0.95rem;
            font-weight: 600;
            margin-top: 0.25rem;
        }
        .profile-stat-value.target { color: #00ba7c; }
        .profile-stat-value.risk-high { color: #ffad1f; }
        .profile-stat-value.risk-very-high { color: #f4212e; }
        .profile-stat-value.risk-extreme { color: #8b0000; font-weight: 700; }
        .profile-stat-value.risk-moderate { color: #17bf63; }
        .profile-stat-value.risk-medium { color: #1da1f2; }
    </style>
</head>
<body>
    <div class="header">
        <div class="header-left">
            <h1>Crypto Trading Bot</h1>
            <div class="status">
                <div class="status-dot" id="status-dot"></div>
                <span id="status-text">Connecting...</span>
            </div>
        </div>
        <div class="header-right">
            <div class="bot-status">
                <span class="bot-status-label">Bot Status</span>
                <span class="status-badge status-stopped" id="bot-status">Stopped</span>
            </div>
            <div class="control-buttons">
                <button class="btn btn-success" id="btn-start" onclick="controlBot('start')">Start</button>
                <button class="btn btn-warning" id="btn-pause" onclick="controlBot('pause')" disabled>Pause</button>
                <button class="btn btn-primary" id="btn-resume" onclick="controlBot('resume')" disabled>Resume</button>
                <button class="btn btn-danger" id="btn-stop" onclick="controlBot('stop')" disabled>Stop</button>
            </div>
            <div class="control-info">
                <span>Uptime: <strong id="uptime">--</strong></span>
                <span>Trades: <strong id="trades-count">0</strong></span>
            </div>
            <div class="notification-bell" onclick="toggleNotificationPanel()">
                <span class="notification-bell-icon">🔔</span>
                <span class="notification-badge" id="notification-badge" style="display: none;">0</span>
            </div>
        </div>
    </div>

    <!-- Notification Panel -->
    <div class="notification-panel" id="notification-panel">
        <div class="notification-header">
            <h3>Notifications</h3>
            <button class="notification-close" onclick="toggleNotificationPanel()">×</button>
        </div>
        <div class="notification-list" id="notification-list">
            <div class="notification-empty">No notifications</div>
        </div>
        <button class="clear-all-btn" onclick="clearAllNotifications()">Clear All</button>
    </div>

    <!-- View Navigation -->
    <div class="view-nav">
        <button class="view-tab active" onclick="switchView('dashboard')">Dashboard</button>
        <button class="view-tab" onclick="switchView('analytics')">Analytics</button>
        <button class="view-tab" onclick="switchView('logs')">Logs</button>
        <button class="view-tab" onclick="switchView('configuration')">Configuration</button>
    </div>

    <!-- Dashboard View -->
    <div id="view-dashboard" class="view-content active">
    <div class="container">
        <!-- Portfolio Overview -->
        <div class="grid grid-4 mt-1">
            <div class="card">
                <div class="card-title">Total Equity</div>
                <div class="card-value" id="total-equity">$0.00</div>
                <div class="card-subtitle" id="equity-change">--</div>
            </div>
            <div class="card">
                <div class="card-title">Unrealized P&L</div>
                <div class="card-value" id="unrealized-pnl">$0.00</div>
            </div>
            <div class="card">
                <div class="card-title">Realized P&L</div>
                <div class="card-value" id="realized-pnl">$0.00</div>
            </div>
            <div class="card">
                <div class="card-title">Max Drawdown</div>
                <div class="card-value" id="max-drawdown">0.00%</div>
            </div>
        </div>

        <!-- Prices -->
        <div class="card mt-1">
            <div class="card-title">Live Prices</div>
            <div class="price-grid">
                <div class="price-card">
                    <div class="price-symbol">BTC/USDT</div>
                    <div class="price-value" id="price-btc">$0.00</div>
                </div>
                <div class="price-card">
                    <div class="price-symbol">ETH/USDT</div>
                    <div class="price-value" id="price-eth">$0.00</div>
                </div>
                <div class="price-card">
                    <div class="price-symbol">SOL/USDT</div>
                    <div class="price-value" id="price-sol">$0.00</div>
                </div>
            </div>
        </div>

        <!-- Charts -->
        <div class="grid grid-2 mt-1">
            <div class="card">
                <div class="card-title">Equity Curve</div>
                <div class="chart-container">
                    <canvas id="equity-chart"></canvas>
                </div>
            </div>
            <div class="card">
                <div class="card-title">Price Chart (% Change)</div>
                <div class="chart-container">
                    <canvas id="price-chart"></canvas>
                </div>
            </div>
        </div>

        <!-- Open Positions -->
        <div class="card mt-1">
            <div class="section-title">Open Positions</div>
            <div class="table-container">
                <table>
                    <thead>
                        <tr>
                            <th>Pair</th>
                            <th>Side</th>
                            <th>Qty</th>
                            <th>Entry</th>
                            <th>Current</th>
                            <th>P&L</th>
                            <th>P&L %</th>
                            <th>Layers</th>
                            <th>Stops</th>
                            <th>Duration</th>
                        </tr>
                    </thead>
                    <tbody id="positions-table"></tbody>
                </table>
            </div>
            <div id="no-positions" style="text-align: center; padding: 2rem; color: #71767b; display: none;">
                No open positions
            </div>
        </div>

        <!-- Statistics -->
        <div class="grid grid-4 mt-1">
            <div class="card">
                <div class="card-title">Total Trades</div>
                <div class="card-value" id="total-trades">0</div>
            </div>
            <div class="card">
                <div class="card-title">Win Rate</div>
                <div class="card-value" id="win-rate">0.0%</div>
                <div class="card-subtitle"><span id="winners">0</span> W / <span id="losers">0</span> L</div>
            </div>
            <div class="card">
                <div class="card-title">Profit Factor</div>
                <div class="card-value" id="profit-factor">0.00</div>
            </div>
            <div class="card">
                <div class="card-title">Avg Win / Loss</div>
                <div class="card-value"><span id="avg-win" class="positive">$0</span> / <span id="avg-loss" class="negative">$0</span></div>
            </div>
        </div>

        <!-- Signals & Trades -->
        <div class="grid grid-2 mt-1">
            <div class="card">
                <div class="section-title">Recent Signals</div>
                <div class="table-container">
                    <table>
                        <thead>
                            <tr>
                                <th>Time</th>
                                <th>Pair</th>
                                <th>Signal</th>
                                <th>Confidence</th>
                                <th>Status</th>
                            </tr>
                        </thead>
                        <tbody id="signals-table"></tbody>
                    </table>
                </div>
            </div>
            <div class="card">
                <div class="section-title">Recent Trades</div>
                <div class="table-container">
                    <table>
                        <thead>
                            <tr>
                                <th>Time</th>
                                <th>Pair</th>
                                <th>Side</th>
                                <th>Price</th>
                                <th>P&L</th>
                            </tr>
                        </thead>
                        <tbody id="trades-table"></tbody>
                    </table>
                </div>
            </div>
        </div>
    </div>
    </div>

    <!-- Logs View -->
    <div id="view-logs" class="view-content">
    <div class="container">
        <div class="card" style="margin-top: 1.5rem;">
            <div class="section-title">Live Logs</div>
            <div id="log-container" style="background: #0f1419; border-radius: 8px; padding: 1rem; height: 600px; overflow-y: auto; font-family: 'Monaco', 'Courier New', monospace; font-size: 0.875rem; line-height: 1.6;">
                <div id="log-entries">
                    <!-- Logs will appear here -->
                    <div style="color: #71767b;">Waiting for logs...</div>
                </div>
            </div>
        </div>
    </div>
    </div>

    <!-- Analytics View -->
    <div id="view-analytics" class="view-content">
    <div class="container">
        <!-- Overall Performance Metrics -->
        <div class="card" style="margin-top: 1.5rem;">
            <div class="section-title">Overall Performance</div>
            <div class="grid grid-4 mt-1">
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Total Trades</div>
                    <div style="font-size: 1.5rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-total-trades">0</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Win Rate</div>
                    <div style="font-size: 1.5rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-win-rate">0%</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Profit Factor</div>
                    <div style="font-size: 1.5rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-profit-factor">0.00</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Sharpe Ratio</div>
                    <div style="font-size: 1.5rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-sharpe">0.00</div>
                </div>
            </div>
        </div>

        <!-- Risk Metrics -->
        <div class="card" style="margin-top: 1.5rem;">
            <div class="section-title">Risk Metrics</div>
            <div class="grid grid-3 mt-1">
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Max Drawdown</div>
                    <div style="font-size: 1.5rem; font-weight: 600; margin-top: 0.5rem; color: #f4212e;" id="analytics-max-dd">0%</div>
                    <div style="font-size: 0.75rem; color: #71767b; margin-top: 0.25rem;" id="analytics-max-dd-date">--</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Sortino Ratio</div>
                    <div style="font-size: 1.5rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-sortino">0.00</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Calmar Ratio</div>
                    <div style="font-size: 1.5rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-calmar">0.00</div>
                </div>
            </div>
        </div>

        <!-- Rolling Returns -->
        <div class="card" style="margin-top: 1.5rem;">
            <div class="section-title">Rolling Returns</div>
            <div class="grid grid-5 mt-1">
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">7 Days</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-return-7d">0%</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">30 Days</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-return-30d">0%</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">90 Days</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-return-90d">0%</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">YTD</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-return-ytd">0%</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">All Time</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-return-all">0%</div>
                </div>
            </div>
        </div>

        <!-- Win/Loss Streaks -->
        <div class="card" style="margin-top: 1.5rem;">
            <div class="section-title">Streaks</div>
            <div class="grid grid-4 mt-1">
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Current Streak</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-current-streak">0</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Longest Win Streak</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem; color: #00ba7c;" id="analytics-max-win-streak">0</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Longest Loss Streak</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem; color: #f4212e;" id="analytics-max-loss-streak">0</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Avg Win Streak</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem;" id="analytics-avg-win-streak">0.0</div>
                </div>
            </div>
        </div>

        <!-- Performance by Pair -->
        <div class="card" style="margin-top: 1.5rem;">
            <div class="section-title">Performance by Trading Pair</div>
            <div style="overflow-x: auto; margin-top: 1rem;">
                <table>
                    <thead>
                        <tr>
                            <th>Pair</th>
                            <th>Trades</th>
                            <th>Win Rate</th>
                            <th>Total P&L</th>
                            <th>Avg Win</th>
                            <th>Avg Loss</th>
                            <th>Profit Factor</th>
                        </tr>
                    </thead>
                    <tbody id="analytics-by-pair"></tbody>
                </table>
            </div>
        </div>

        <!-- Performance by Strategy -->
        <div class="card" style="margin-top: 1.5rem;">
            <div class="section-title">Performance by Strategy</div>
            <div style="overflow-x: auto; margin-top: 1rem;">
                <table>
                    <thead>
                        <tr>
                            <th>Strategy</th>
                            <th>Trades</th>
                            <th>Win Rate</th>
                            <th>Total P&L</th>
                            <th>Avg Win</th>
                            <th>Avg Loss</th>
                            <th>Profit Factor</th>
                        </tr>
                    </thead>
                    <tbody id="analytics-by-strategy"></tbody>
                </table>
            </div>
        </div>

        <!-- Trade Distribution -->
        <div class="card" style="margin-top: 1.5rem;">
            <div class="section-title">Trade Distribution</div>
            <div class="grid grid-4 mt-1">
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Avg Win</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem; color: #00ba7c;" id="analytics-avg-win">$0.00</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Avg Loss</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem; color: #f4212e;" id="analytics-avg-loss">$0.00</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Largest Win</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem; color: #00ba7c;" id="analytics-largest-win">$0.00</div>
                </div>
                <div style="padding: 1rem; background: #16202a; border-radius: 8px;">
                    <div style="font-size: 0.875rem; color: #71767b;">Largest Loss</div>
                    <div style="font-size: 1.25rem; font-weight: 600; margin-top: 0.5rem; color: #f4212e;" id="analytics-largest-loss">$0.00</div>
                </div>
            </div>
        </div>
    </div>
    </div>

    <!-- Configuration View -->
    <div id="view-configuration" class="view-content">
    <div class="container">
        <!-- Strategy Profile Selection -->
        <div class="card">
            <div class="section-title">Strategy Profile</div>
            <div class="profile-grid" id="profile-grid-config">
                <!-- Profiles will be loaded here -->
                <div class="profile-card" style="text-align: center; padding: 2rem;">
                    <div style="color: #71767b;">Loading profiles...</div>
                </div>
            </div>
        </div>

        <!-- Configuration Panel -->
        <div class="card config-panel mt-1">
            <div class="section-title">Advanced Settings</div>
            <div class="config-tabs">
                <button class="tab active" data-tab="risk">Risk Settings</button>
                <button class="tab" data-tab="executor">Executor</button>
                <button class="tab" data-tab="strategies">Strategies</button>
                <button class="tab" data-tab="general">General</button>
            </div>

            <!-- Risk Settings Tab -->
            <div id="tab-risk" class="tab-content active">
                <form id="risk-form" onsubmit="saveRiskSettings(event)">
                    <div class="form-grid">
                        <div class="form-group">
                            <label>Max Positions</label>
                            <input type="number" id="max-positions" min="1" max="10" value="3">
                        </div>
                        <div class="form-group">
                            <label>Max Single Position %</label>
                            <input type="number" id="max-single-position" min="5" max="50" step="1" value="25">
                        </div>
                        <div class="form-group">
                            <label>Max Total Exposure %</label>
                            <input type="number" id="max-total-exposure" min="10" max="100" step="5" value="60">
                        </div>
                        <div class="form-group">
                            <label>Risk Per Trade %</label>
                            <input type="number" id="risk-per-trade" min="0.5" max="5" step="0.5" value="1.5">
                        </div>
                        <div class="form-group">
                            <label>Default Stop Loss %</label>
                            <input type="number" id="default-stop-loss" min="1" max="10" step="0.5" value="2">
                        </div>
                        <div class="form-group">
                            <label>Default Take Profit %</label>
                            <input type="number" id="default-take-profit" min="1" max="20" step="0.5" value="6">
                        </div>
                        <div class="form-group">
                            <label>Min Risk/Reward Ratio</label>
                            <input type="number" id="min-rr-ratio" min="1" max="5" step="0.5" value="1.5">
                        </div>
                        <div class="form-group">
                            <label>Max Drawdown %</label>
                            <input type="number" id="max-drawdown-config" min="5" max="30" step="1" value="15">
                        </div>
                        <div class="form-group">
                            <label>Max Daily Loss %</label>
                            <input type="number" id="max-daily-loss" min="1" max="10" step="0.5" value="5">
                        </div>
                        <div class="form-group">
                            <label>Max Holding Hours</label>
                            <input type="number" id="max-holding-hours" min="1" max="720" value="72">
                        </div>
                    </div>
                    <button type="submit" class="btn btn-primary" style="margin-top: 1rem;">Save Risk Settings</button>
                </form>
            </div>

            <!-- Executor Settings Tab -->
            <div id="tab-executor" class="tab-content">
                <form id="executor-form" onsubmit="saveExecutorSettings(event)">
                    <div class="form-grid">
                        <div class="form-group">
                            <label>Min Confidence %</label>
                            <input type="number" id="min-confidence" min="30" max="95" step="5" value="60">
                        </div>
                        <div class="form-group">
                            <label>Min Risk/Reward</label>
                            <input type="number" id="executor-min-rr" min="1" max="5" step="0.5" value="1.5">
                        </div>
                    </div>
                    <button type="submit" class="btn btn-primary" style="margin-top: 1rem;">Save Executor Settings</button>
                </form>
            </div>

            <!-- Strategies Settings Tab -->
            <div id="tab-strategies" class="tab-content">
                <form id="strategies-form" onsubmit="saveStrategySettings(event)">
                    <div class="strategy-section" style="margin-top: 0; border-top: none; padding-top: 0;">
                        <h4>Trend Strategy</h4>
                        <div class="form-grid">
                            <div class="form-group">
                                <label>Fast EMA Period</label>
                                <input type="number" id="trend-ema-fast" min="3" max="50" value="9">
                            </div>
                            <div class="form-group">
                                <label>Slow EMA Period</label>
                                <input type="number" id="trend-ema-slow" min="10" max="100" value="21">
                            </div>
                            <div class="form-group">
                                <label>ATR Period</label>
                                <input type="number" id="trend-atr" min="5" max="30" value="14">
                            </div>
                            <div class="form-group">
                                <label>ATR SL Multiplier</label>
                                <input type="number" id="trend-atr-sl" min="0.5" max="5" step="0.5" value="1.5">
                            </div>
                            <div class="form-group">
                                <label>ATR TP Multiplier</label>
                                <input type="number" id="trend-atr-tp" min="1" max="10" step="0.5" value="3">
                            </div>
                        </div>
                    </div>

                    <div class="strategy-section">
                        <h4>Momentum Strategy</h4>
                        <div class="form-grid">
                            <div class="form-group">
                                <label>RSI Period</label>
                                <input type="number" id="momentum-rsi" min="5" max="30" value="14">
                            </div>
                            <div class="form-group">
                                <label>RSI Overbought</label>
                                <input type="number" id="momentum-ob" min="60" max="90" value="70">
                            </div>
                            <div class="form-group">
                                <label>RSI Oversold</label>
                                <input type="number" id="momentum-os" min="10" max="40" value="30">
                            </div>
                            <div class="form-group">
                                <label>Volume Threshold</label>
                                <input type="number" id="momentum-vol" min="1" max="5" step="0.5" value="1.5">
                            </div>
                        </div>
                    </div>

                    <div class="strategy-section">
                        <h4>Mean Reversion Strategy</h4>
                        <div class="form-grid">
                            <div class="form-group">
                                <label>Bollinger Period</label>
                                <input type="number" id="mr-bb-period" min="10" max="50" value="20">
                            </div>
                            <div class="form-group">
                                <label>Bollinger Std Dev</label>
                                <input type="number" id="mr-bb-std" min="1" max="4" step="0.5" value="2">
                            </div>
                            <div class="form-group">
                                <label>RSI Overbought</label>
                                <input type="number" id="mr-rsi-ob" min="60" max="90" value="75">
                            </div>
                            <div class="form-group">
                                <label>RSI Oversold</label>
                                <input type="number" id="mr-rsi-os" min="10" max="40" value="25">
                            </div>
                        </div>
                    </div>

                    <div class="strategy-section">
                        <h4>Breakout Strategy</h4>
                        <div class="form-grid">
                            <div class="form-group">
                                <label>Lookback Period</label>
                                <input type="number" id="breakout-lookback" min="5" max="50" value="20">
                            </div>
                            <div class="form-group">
                                <label>Breakout Threshold</label>
                                <input type="number" id="breakout-threshold" min="0.5" max="5" step="0.5" value="1.5">
                            </div>
                        </div>
                    </div>

                    <button type="submit" class="btn btn-primary" style="margin-top: 1rem;">Save Strategy Settings</button>
                </form>
            </div>

            <!-- General Settings Tab -->
            <div id="tab-general" class="tab-content">
                <form id="general-form" onsubmit="saveGeneralSettings(event)">
                    <div class="form-grid">
                        <div class="form-group">
                            <label>Enabled Trading Pairs</label>
                            <div class="checkbox-group">
                                <label><input type="checkbox" id="pair-btc" value="BTCUSDT" checked> BTC/USDT</label>
                                <label><input type="checkbox" id="pair-eth" value="ETHUSDT" checked> ETH/USDT</label>
                                <label><input type="checkbox" id="pair-sol" value="SOLUSDT" checked> SOL/USDT</label>
                            </div>
                        </div>
                        <div class="form-group">
                            <label>Timeframe</label>
                            <select id="timeframe">
                                <option value="M5">5 Minutes</option>
                                <option value="M15">15 Minutes</option>
                                <option value="H1">1 Hour</option>
                                <option value="H4">4 Hours</option>
                            </select>
                        </div>
                    </div>
                    <button type="submit" class="btn btn-primary" style="margin-top: 1rem;">Save General Settings</button>
                </form>
            </div>
        </div>
    </div>
    </div>

    <script>
        let ws;
        let equityChart, priceChart;
        let reconnectTimeout;
        let currentConfig = {};

        // Initialize charts
        function initCharts() {
            const chartOptions = {
                responsive: true,
                maintainAspectRatio: false,
                plugins: { legend: { display: false } },
                scales: {
                    x: {
                        type: 'time',
                        grid: { color: '#2f3336' },
                        ticks: { color: '#71767b' }
                    },
                    y: {
                        grid: { color: '#2f3336' },
                        ticks: { color: '#71767b' }
                    }
                }
            };

            equityChart = new Chart(document.getElementById('equity-chart'), {
                type: 'line',
                data: {
                    datasets: [{
                        label: 'Equity',
                        data: [],
                        borderColor: '#1da1f2',
                        backgroundColor: 'rgba(29, 161, 242, 0.1)',
                        fill: true,
                        tension: 0.4
                    }]
                },
                options: chartOptions
            });

            priceChart = new Chart(document.getElementById('price-chart'), {
                type: 'line',
                data: {
                    datasets: [
                        { label: 'BTC', data: [], borderColor: '#f7931a', tension: 0.4 },
                        { label: 'ETH', data: [], borderColor: '#627eea', tension: 0.4 },
                        { label: 'SOL', data: [], borderColor: '#00ffa3', tension: 0.4 }
                    ]
                },
                options: {
                    ...chartOptions,
                    plugins: { legend: { display: true, labels: { color: '#e7e9ea' } } },
                    scales: {
                        ...chartOptions.scales,
                        y: { ...chartOptions.scales.y, display: true, title: { display: true, text: '% Change', color: '#71767b' } }
                    }
                }
            });
        }

        // Tab switching
        document.querySelectorAll('.config-tabs .tab').forEach(tab => {
            tab.addEventListener('click', () => {
                document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
                document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
                tab.classList.add('active');
                document.getElementById('tab-' + tab.dataset.tab).classList.add('active');
            });
        });

        // Bot Control
        async function controlBot(action) {
            try {
                const response = await fetch('/api/control/' + action, { method: 'POST' });
                const result = await response.json();
                if (!response.ok) {
                    showToast('error', result.error || 'Action failed');
                } else {
                    showToast('success', 'Bot ' + action + 'ed successfully');
                    if (result.state) updateBotStatus(result.state);
                }
            } catch (e) {
                showToast('error', 'Failed: ' + e.message);
            }
        }

        function updateBotStatus(state) {
            const badge = document.getElementById('bot-status');
            badge.textContent = state.status;
            badge.className = 'status-badge status-' + state.status.toLowerCase();

            document.getElementById('btn-start').disabled = state.status !== 'Stopped';
            document.getElementById('btn-pause').disabled = state.status !== 'Running';
            document.getElementById('btn-resume').disabled = state.status !== 'Paused';
            document.getElementById('btn-stop').disabled = state.status === 'Stopped';

            document.getElementById('uptime').textContent = formatUptime(state.uptime_seconds);
            document.getElementById('trades-count').textContent = state.trades_count;
        }

        function formatUptime(seconds) {
            if (!seconds) return '--';
            const h = Math.floor(seconds / 3600);
            const m = Math.floor((seconds % 3600) / 60);
            return h > 0 ? h + 'h ' + m + 'm' : m + 'm';
        }

        // Configuration
        function populateConfig(config) {
            if (!config) return;
            currentConfig = config;

            // Risk
            if (config.risk) {
                document.getElementById('max-positions').value = config.risk.max_positions;
                document.getElementById('max-single-position').value = parseFloat(config.risk.max_single_position_pct);
                document.getElementById('max-total-exposure').value = parseFloat(config.risk.max_total_exposure_pct);
                document.getElementById('risk-per-trade').value = parseFloat(config.risk.risk_per_trade_pct);
                document.getElementById('default-stop-loss').value = parseFloat(config.risk.default_stop_loss_pct);
                document.getElementById('default-take-profit').value = parseFloat(config.risk.default_take_profit_pct);
                document.getElementById('min-rr-ratio').value = parseFloat(config.risk.min_risk_reward_ratio);
                document.getElementById('max-drawdown-config').value = parseFloat(config.risk.max_drawdown_pct);
                document.getElementById('max-daily-loss').value = parseFloat(config.risk.max_daily_loss_pct);
                document.getElementById('max-holding-hours').value = config.risk.max_holding_hours;
            }

            // Executor
            if (config.executor) {
                document.getElementById('min-confidence').value = parseFloat(config.executor.min_confidence) * 100;
                document.getElementById('executor-min-rr').value = parseFloat(config.executor.min_risk_reward);
            }

            // Strategies
            if (config.strategies) {
                const s = config.strategies;
                if (s.trend) {
                    document.getElementById('trend-ema-fast').value = s.trend.ema_fast_period;
                    document.getElementById('trend-ema-slow').value = s.trend.ema_slow_period;
                    document.getElementById('trend-atr').value = s.trend.atr_period;
                    document.getElementById('trend-atr-sl').value = parseFloat(s.trend.atr_multiplier_sl);
                    document.getElementById('trend-atr-tp').value = parseFloat(s.trend.atr_multiplier_tp);
                }
                if (s.momentum) {
                    document.getElementById('momentum-rsi').value = s.momentum.rsi_period;
                    document.getElementById('momentum-ob').value = parseFloat(s.momentum.rsi_overbought);
                    document.getElementById('momentum-os').value = parseFloat(s.momentum.rsi_oversold);
                    document.getElementById('momentum-vol').value = parseFloat(s.momentum.volume_threshold);
                }
                if (s.mean_reversion) {
                    document.getElementById('mr-bb-period').value = s.mean_reversion.bollinger_period;
                    document.getElementById('mr-bb-std').value = parseFloat(s.mean_reversion.bollinger_std_dev);
                    document.getElementById('mr-rsi-ob').value = parseFloat(s.mean_reversion.rsi_overbought);
                    document.getElementById('mr-rsi-os').value = parseFloat(s.mean_reversion.rsi_oversold);
                }
                if (s.breakout) {
                    document.getElementById('breakout-lookback').value = s.breakout.lookback_period;
                    document.getElementById('breakout-threshold').value = parseFloat(s.breakout.breakout_threshold);
                }
            }

            // General
            if (config.general) {
                document.getElementById('pair-btc').checked = config.general.enabled_pairs.includes('BTCUSDT');
                document.getElementById('pair-eth').checked = config.general.enabled_pairs.includes('ETHUSDT');
                document.getElementById('pair-sol').checked = config.general.enabled_pairs.includes('SOLUSDT');
                document.getElementById('timeframe').value = config.general.timeframe;
            }
        }

        async function saveRiskSettings(event) {
            event.preventDefault();
            const settings = {
                max_positions: parseInt(document.getElementById('max-positions').value),
                max_single_position_pct: document.getElementById('max-single-position').value,
                max_total_exposure_pct: document.getElementById('max-total-exposure').value,
                risk_per_trade_pct: document.getElementById('risk-per-trade').value,
                default_stop_loss_pct: document.getElementById('default-stop-loss').value,
                default_take_profit_pct: document.getElementById('default-take-profit').value,
                min_risk_reward_ratio: document.getElementById('min-rr-ratio').value,
                max_drawdown_pct: document.getElementById('max-drawdown-config').value,
                max_daily_loss_pct: document.getElementById('max-daily-loss').value,
                max_holding_hours: parseInt(document.getElementById('max-holding-hours').value)
            };
            await saveConfig('/api/config/risk', settings);
        }

        async function saveExecutorSettings(event) {
            event.preventDefault();
            const settings = {
                min_confidence: (parseFloat(document.getElementById('min-confidence').value) / 100).toString(),
                min_risk_reward: document.getElementById('executor-min-rr').value
            };
            await saveConfig('/api/config/executor', settings);
        }

        async function saveStrategySettings(event) {
            event.preventDefault();
            const settings = {
                trend: {
                    ema_fast_period: parseInt(document.getElementById('trend-ema-fast').value),
                    ema_slow_period: parseInt(document.getElementById('trend-ema-slow').value),
                    atr_period: parseInt(document.getElementById('trend-atr').value),
                    min_trend_strength: "0.5",
                    atr_multiplier_sl: document.getElementById('trend-atr-sl').value,
                    atr_multiplier_tp: document.getElementById('trend-atr-tp').value
                },
                momentum: {
                    rsi_period: parseInt(document.getElementById('momentum-rsi').value),
                    ema_fast_period: 8,
                    ema_slow_period: 21,
                    rsi_overbought: document.getElementById('momentum-ob').value,
                    rsi_oversold: document.getElementById('momentum-os').value,
                    volume_threshold: document.getElementById('momentum-vol').value
                },
                mean_reversion: {
                    bollinger_period: parseInt(document.getElementById('mr-bb-period').value),
                    bollinger_std_dev: document.getElementById('mr-bb-std').value,
                    rsi_period: 14,
                    rsi_oversold: document.getElementById('mr-rsi-os').value,
                    rsi_overbought: document.getElementById('mr-rsi-ob').value
                },
                breakout: {
                    lookback_period: parseInt(document.getElementById('breakout-lookback').value),
                    breakout_threshold: document.getElementById('breakout-threshold').value
                }
            };
            await saveConfig('/api/config/strategies', settings);
        }

        async function saveGeneralSettings(event) {
            event.preventDefault();
            const pairs = [];
            if (document.getElementById('pair-btc').checked) pairs.push('BTCUSDT');
            if (document.getElementById('pair-eth').checked) pairs.push('ETHUSDT');
            if (document.getElementById('pair-sol').checked) pairs.push('SOLUSDT');

            const settings = {
                enabled_pairs: pairs,
                timeframe: document.getElementById('timeframe').value
            };
            await saveConfig('/api/config/general', settings);
        }

        async function saveConfig(endpoint, settings) {
            try {
                const response = await fetch(endpoint, {
                    method: 'PUT',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(settings)
                });
                const result = await response.json();
                if (!response.ok) {
                    showToast('error', result.error || 'Failed to save');
                } else {
                    showToast('success', 'Settings saved');
                }
            } catch (e) {
                showToast('error', 'Failed: ' + e.message);
            }
        }

        function showToast(type, message) {
            const toast = document.createElement('div');
            toast.className = 'toast toast-' + type;
            toast.textContent = message;
            document.body.appendChild(toast);
            setTimeout(() => toast.remove(), 3000);
        }

        function connectWebSocket() {
            const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
            ws = new WebSocket(protocol + '//' + window.location.host + '/ws');

            ws.onopen = () => {
                document.getElementById('status-dot').classList.remove('disconnected');
                document.getElementById('status-text').textContent = 'Connected';
            };

            ws.onclose = () => {
                document.getElementById('status-dot').classList.add('disconnected');
                document.getElementById('status-text').textContent = 'Disconnected';
                reconnectTimeout = setTimeout(connectWebSocket, 3000);
            };

            ws.onmessage = (event) => {
                const msg = JSON.parse(event.data);

                if (msg.type === 'initial') {
                    updateDashboard(msg.data);
                    if (msg.status) updateBotStatus(msg.status);
                    if (msg.config) populateConfig(msg.config);
                } else if (msg.type === 'PriceUpdate') {
                    updatePrice(msg.pair, msg.price);
                } else if (msg.type === 'NewSignal') {
                    addSignal(msg);
                } else if (msg.type === 'NewTrade') {
                    addTrade(msg);
                } else if (msg.type === 'PortfolioUpdate') {
                    updatePortfolio(msg);
                } else if (msg.type === 'StatusChange') {
                    if (msg.status) updateBotStatus(msg.status);
                } else if (msg.type === 'ConfigChange') {
                    fetch('/api/config').then(r => r.json()).then(populateConfig);
                } else if (msg.type === 'NewLog') {
                    addLog(msg);
                }
            };
        }

        // Log Management with virtual scrolling (memory efficient)
        const MAX_RENDERED_LOGS = 200; // Only keep last 200 logs in DOM
        let logCount = 0;

        function addLog(log) {
            const container = document.getElementById('log-entries');

            // Remove placeholder
            if (container.children.length === 1 && container.children[0].textContent === 'Waiting for logs...') {
                container.innerHTML = '';
            }

            const logEntry = document.createElement('div');
            logEntry.style.marginBottom = '2px';
            logEntry.style.whiteSpace = 'pre-wrap';
            logEntry.style.wordBreak = 'break-word';

            const timestamp = new Date(log.timestamp).toLocaleTimeString();
            const levelColor = log.level === 'INFO' ? '#00ba7c' :
                              log.level === 'WARN' ? '#ffad1f' :
                              log.level === 'ERROR' ? '#f4212e' : '#71767b';

            logEntry.innerHTML = `<span style="color: #71767b;">[${timestamp}]</span> <span style="color: ${levelColor}; font-weight: 600;">${log.level}</span> <span style="color: #e7e9ea;">${escapeHtml(log.message)}</span>`;

            container.insertBefore(logEntry, container.firstChild);
            logCount++;

            // Memory management: Remove old logs beyond MAX_RENDERED_LOGS
            if (logCount > MAX_RENDERED_LOGS) {
                container.removeChild(container.lastChild);
                logCount--;
            }
        }

        function escapeHtml(text) {
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }

        function updateDashboard(data) {
            if (data.portfolio) updatePortfolio(data.portfolio);
            if (data.stats) updateStats(data.stats);
            if (data.current_prices) updatePrices(data.current_prices);
            if (data.recent_signals) updateSignalsTable(data.recent_signals);
            if (data.recent_trades) updateTradesTable(data.recent_trades);
            updateCharts(data);
        }

        function updatePortfolio(p) {
            document.getElementById('total-equity').textContent = formatCurrency(p.total_equity);
            document.getElementById('unrealized-pnl').textContent = formatCurrency(p.unrealized_pnl);
            document.getElementById('unrealized-pnl').className = 'card-value ' + getPnlClass(p.unrealized_pnl);
            document.getElementById('realized-pnl').textContent = formatCurrency(p.realized_pnl);
            document.getElementById('realized-pnl').className = 'card-value ' + getPnlClass(p.realized_pnl);
            document.getElementById('max-drawdown').textContent = formatPercent(p.max_drawdown);

            // Update positions table
            updatePositionsTable(p.positions || []);
        }

        function updatePositionsTable(positions) {
            const tbody = document.getElementById('positions-table');
            const noPositions = document.getElementById('no-positions');

            if (!positions || positions.length === 0) {
                tbody.innerHTML = '';
                noPositions.style.display = 'block';
                return;
            }

            noPositions.style.display = 'none';
            tbody.innerHTML = positions.map(pos => {
                const stopsInfo = [];
                if (pos.stop_loss) {
                    stopsInfo.push('SL: ' + formatCurrency(pos.stop_loss));
                }
                if (pos.take_profit) {
                    stopsInfo.push('TP: ' + formatCurrency(pos.take_profit));
                }
                if (pos.trailing_stop_active) {
                    stopsInfo.push('🔄 Trail: ' + formatCurrency(pos.trailing_stop_level));
                }
                if (pos.breakeven_stop_set) {
                    stopsInfo.push('⚖️ BE');
                }

                const layerInfo = pos.layers > 1 ? pos.layers + ' layers' : '1 layer';
                if (pos.partial_exits_count > 0) {
                    layerInfo += ' (' + pos.partial_exits_count + ' exits)';
                }

                const durationText = pos.duration_hours < 24 ?
                    pos.duration_hours + 'h' :
                    (pos.duration_hours / 24).toFixed(1) + 'd';

                return '<tr>' +
                    '<td><strong>' + pos.pair + '</strong></td>' +
                    '<td class="' + (pos.side === 'Buy' ? 'positive' : 'negative') + '">' + pos.side + '</td>' +
                    '<td>' + parseFloat(pos.quantity).toFixed(4) + '</td>' +
                    '<td>' + formatCurrency(pos.entry_price) + '</td>' +
                    '<td>' + formatCurrency(pos.current_price) + '</td>' +
                    '<td class="' + getPnlClass(pos.pnl) + '">' + formatCurrency(pos.pnl) + '</td>' +
                    '<td class="' + getPnlClass(pos.pnl) + '">' + parseFloat(pos.pnl_pct).toFixed(2) + '%</td>' +
                    '<td style="font-size: 0.8rem;">' + layerInfo + '</td>' +
                    '<td style="font-size: 0.75rem; max-width: 150px; word-wrap: break-word;">' +
                        (stopsInfo.length > 0 ? stopsInfo.join('<br>') : '-') + '</td>' +
                    '<td>' + durationText + '</td>' +
                    '</tr>';
            }).join('');
        }

        function updateStats(s) {
            document.getElementById('total-trades').textContent = s.total_trades;
            const winRate = s.total_trades > 0 ? (s.winning_trades / s.total_trades * 100) : 0;
            document.getElementById('win-rate').textContent = winRate.toFixed(1) + '%';
            document.getElementById('winners').textContent = s.winning_trades;
            document.getElementById('losers').textContent = s.losing_trades;

            const profitFactor = Math.abs(parseFloat(s.total_loss)) > 0 ?
                (parseFloat(s.total_profit) / Math.abs(parseFloat(s.total_loss))).toFixed(2) : '0.00';
            document.getElementById('profit-factor').textContent = profitFactor;

            const avgWin = s.winning_trades > 0 ? parseFloat(s.total_profit) / s.winning_trades : 0;
            const avgLoss = s.losing_trades > 0 ? parseFloat(s.total_loss) / s.losing_trades : 0;
            document.getElementById('avg-win').textContent = '$' + avgWin.toFixed(0);
            document.getElementById('avg-loss').textContent = '$' + avgLoss.toFixed(0);
        }

        function updatePrices(prices) {
            if (prices.BTCUSDT) document.getElementById('price-btc').textContent = formatCurrency(prices.BTCUSDT);
            if (prices.ETHUSDT) document.getElementById('price-eth').textContent = formatCurrency(prices.ETHUSDT);
            if (prices.SOLUSDT) document.getElementById('price-sol').textContent = formatCurrency(prices.SOLUSDT);
        }

        function updatePrice(pair, price) {
            const el = document.getElementById('price-' + pair.replace('USDT', '').toLowerCase());
            if (el) el.textContent = formatCurrency(price);
        }

        function updateSignalsTable(signals) {
            const tbody = document.getElementById('signals-table');
            tbody.innerHTML = signals.map(s => '<tr>' +
                '<td>' + formatTime(s.timestamp) + '</td>' +
                '<td>' + s.pair + '</td>' +
                '<td><span class="signal-badge signal-' + getSignalClass(s.signal) + '">' + s.signal + '</span></td>' +
                '<td>' + (parseFloat(s.confidence) * 100).toFixed(0) + '%</td>' +
                '<td>' + (s.executed ? 'Executed' : 'Pending') + '</td>' +
                '</tr>').join('');
        }

        function updateTradesTable(trades) {
            const tbody = document.getElementById('trades-table');
            tbody.innerHTML = trades.map(t => '<tr>' +
                '<td>' + formatTime(t.timestamp) + '</td>' +
                '<td>' + t.pair + '</td>' +
                '<td class="' + (t.side === 'Buy' ? 'positive' : 'negative') + '">' + t.side + '</td>' +
                '<td>' + formatCurrency(t.entry_price) + '</td>' +
                '<td class="' + getPnlClass(t.pnl) + '">' + formatCurrency(t.pnl) + '</td>' +
                '</tr>').join('');
        }

        function addSignal(signal) {
            const tbody = document.getElementById('signals-table');
            const row = document.createElement('tr');
            row.innerHTML = '<td>' + formatTime(signal.timestamp) + '</td>' +
                '<td>' + signal.pair + '</td>' +
                '<td><span class="signal-badge signal-' + getSignalClass(signal.signal) + '">' + signal.signal + '</span></td>' +
                '<td>' + (parseFloat(signal.confidence) * 100).toFixed(0) + '%</td>' +
                '<td>' + (signal.executed ? 'Executed' : 'Pending') + '</td>';
            tbody.insertBefore(row, tbody.firstChild);
            if (tbody.children.length > 20) tbody.removeChild(tbody.lastChild);
        }

        function addTrade(trade) {
            const tbody = document.getElementById('trades-table');
            const row = document.createElement('tr');
            row.innerHTML = '<td>' + formatTime(trade.timestamp) + '</td>' +
                '<td>' + trade.pair + '</td>' +
                '<td class="' + (trade.side === 'Buy' ? 'positive' : 'negative') + '">' + trade.side + '</td>' +
                '<td>' + formatCurrency(trade.entry_price) + '</td>' +
                '<td class="' + getPnlClass(trade.pnl) + '">' + formatCurrency(trade.pnl) + '</td>';
            tbody.insertBefore(row, tbody.firstChild);
            if (tbody.children.length > 20) tbody.removeChild(tbody.lastChild);
        }

        function updateCharts(data) {
            if (data.equity_history && data.equity_history.length > 0) {
                equityChart.data.datasets[0].data = data.equity_history.map(p => ({
                    x: new Date(p.timestamp),
                    y: parseFloat(p.equity)
                }));
                equityChart.update('none');
            }

            if (data.price_history) {
                const pairs = ['BTCUSDT', 'ETHUSDT', 'SOLUSDT'];
                pairs.forEach((pair, i) => {
                    const history = data.price_history[pair];
                    if (history && history.length > 0) {
                        const firstPrice = parseFloat(history[0].price);
                        priceChart.data.datasets[i].data = history.map(p => ({
                            x: new Date(p.timestamp),
                            y: (parseFloat(p.price) / firstPrice - 1) * 100
                        }));
                    }
                });
                priceChart.update('none');
            }
        }

        function formatCurrency(value) {
            const num = parseFloat(value);
            if (isNaN(num)) return '$0.00';
            return '$' + num.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
        }

        function formatPercent(value) {
            return parseFloat(value).toFixed(2) + '%';
        }

        function formatDecimal(value, decimals = 2) {
            const num = parseFloat(value);
            if (isNaN(num)) return '0.00';
            return num.toFixed(decimals);
        }

        function formatTime(timestamp) {
            return new Date(timestamp).toLocaleTimeString();
        }

        function getPnlClass(value) {
            const num = parseFloat(value);
            if (num > 0) return 'positive';
            if (num < 0) return 'negative';
            return 'neutral';
        }

        function getSignalClass(signal) {
            if (signal.includes('Buy')) return 'buy';
            if (signal.includes('Sell')) return 'sell';
            return 'neutral';
        }

        // Strategy Profile Functions
        let currentProfileName = null;

        async function loadProfiles() {
            try {
                const [profilesResp, currentResp] = await Promise.all([
                    fetch('/api/profiles'),
                    fetch('/api/profile/current')
                ]);
                const profiles = await profilesResp.json();
                const current = await currentResp.json();

                currentProfileName = current.profile;
                displayProfiles(profiles, currentProfileName);
            } catch (e) {
                console.error('Failed to load profiles:', e);
                document.getElementById('profile-grid').innerHTML =
                    '<div style="text-align: center; padding: 2rem; color: #f4212e;">Failed to load profiles</div>';
            }
        }

        function displayProfiles(profiles, currentProfile) {
            const grid = document.getElementById('profile-grid-config');

            grid.innerHTML = profiles.map(p => {
                const isActive = p.profile === currentProfile;
                const riskClass = p.risk_level === 'Extreme' ? 'risk-extreme' :
                                 p.risk_level === 'Very High' ? 'risk-very-high' :
                                 p.risk_level === 'High' ? 'risk-high' :
                                 p.risk_level === 'Moderate' ? 'risk-moderate' : 'risk-medium';

                return `
                    <div class="profile-card ${isActive ? 'active' : ''}"
                         onclick="selectProfile('${p.profile}')"
                         data-profile="${p.profile}">
                        <div class="profile-name">${p.name}</div>
                        <div class="profile-desc">${p.description}</div>
                        <div class="profile-stats">
                            <div class="profile-stat">
                                <div class="profile-stat-label">Target</div>
                                <div class="profile-stat-value target">${p.target_return}</div>
                            </div>
                            <div class="profile-stat">
                                <div class="profile-stat-label">Risk</div>
                                <div class="profile-stat-value ${riskClass}">${p.risk_level}</div>
                            </div>
                        </div>
                    </div>
                `;
            }).join('');
        }

        async function selectProfile(profile) {
            if (profile === 'Custom') {
                showToast('error', 'Custom profile requires manual configuration');
                return;
            }

            if (profile === currentProfileName) {
                return; // Already selected
            }

            try {
                const response = await fetch('/api/profile/select', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ profile })
                });
                const result = await response.json();

                if (!response.ok) {
                    showToast('error', result.error || 'Failed to switch profile');
                } else {
                    showToast('success', result.message);
                    currentProfileName = profile;

                    // Update UI to show new active profile
                    document.querySelectorAll('.profile-card').forEach(card => {
                        if (card.dataset.profile === profile) {
                            card.classList.add('active');
                        } else {
                            card.classList.remove('active');
                        }
                    });

                    // Reload configuration
                    fetch('/api/config')
                        .then(r => r.json())
                        .then(populateConfig)
                        .catch(console.error);
                }
            } catch (e) {
                showToast('error', 'Failed to switch profile: ' + e.message);
            }
        }

        // Analytics Functions
        function loadAnalytics() {
            fetch('/api/analytics')
                .then(r => r.json())
                .then(updateAnalytics)
                .catch(e => {
                    console.error('Failed to load analytics:', e);
                    // Show empty state
                    document.querySelectorAll('[id^="analytics-"]').forEach(el => {
                        if (el.tagName === 'TBODY') {
                            el.innerHTML = '<tr><td colspan="7" style="text-align: center; color: #71767b;">No data available</td></tr>';
                        }
                    });
                });
        }

        function updateAnalytics(analytics) {
            // Overall metrics
            document.getElementById('analytics-total-trades').textContent = analytics.overall.total_trades || 0;
            document.getElementById('analytics-win-rate').textContent = formatPercent(analytics.overall.win_rate);
            document.getElementById('analytics-profit-factor').textContent = formatDecimal(analytics.overall.profit_factor, 2);
            document.getElementById('analytics-sharpe').textContent = formatDecimal(analytics.risk_metrics.sharpe_ratio, 2);

            // Risk metrics
            document.getElementById('analytics-max-dd').textContent = formatPercent(analytics.drawdown_analysis.max_drawdown_pct);
            if (analytics.drawdown_analysis.max_drawdown_date) {
                document.getElementById('analytics-max-dd-date').textContent =
                    new Date(analytics.drawdown_analysis.max_drawdown_date).toLocaleDateString();
            }
            document.getElementById('analytics-sortino').textContent = formatDecimal(analytics.risk_metrics.sortino_ratio, 2);
            document.getElementById('analytics-calmar').textContent = formatDecimal(analytics.risk_metrics.calmar_ratio, 2);

            // Rolling returns
            document.getElementById('analytics-return-7d').textContent = formatPercent(analytics.rolling_returns.return_7d);
            document.getElementById('analytics-return-30d').textContent = formatPercent(analytics.rolling_returns.return_30d);
            document.getElementById('analytics-return-90d').textContent = formatPercent(analytics.rolling_returns.return_90d);
            document.getElementById('analytics-return-ytd').textContent = formatPercent(analytics.rolling_returns.return_ytd);
            document.getElementById('analytics-return-all').textContent = formatPercent(analytics.rolling_returns.return_all_time);

            // Streaks
            const currentStreak = analytics.win_loss_streaks.current_streak;
            const currentStreakEl = document.getElementById('analytics-current-streak');
            currentStreakEl.textContent = currentStreak;
            currentStreakEl.style.color = currentStreak > 0 ? '#00ba7c' : currentStreak < 0 ? '#f4212e' : '#e7e9ea';

            document.getElementById('analytics-max-win-streak').textContent = analytics.win_loss_streaks.max_win_streak;
            document.getElementById('analytics-max-loss-streak').textContent = analytics.win_loss_streaks.max_loss_streak;
            document.getElementById('analytics-avg-win-streak').textContent = formatDecimal(analytics.win_loss_streaks.avg_win_streak, 1);

            // Trade distribution
            document.getElementById('analytics-avg-win').textContent = formatCurrency(analytics.overall.avg_win);
            document.getElementById('analytics-avg-loss').textContent = formatCurrency(analytics.overall.avg_loss);
            document.getElementById('analytics-largest-win').textContent = formatCurrency(analytics.overall.largest_win);
            document.getElementById('analytics-largest-loss').textContent = formatCurrency(analytics.overall.largest_loss);

            // Performance by pair
            const pairTable = document.getElementById('analytics-by-pair');
            pairTable.innerHTML = '';
            if (Object.keys(analytics.by_pair).length === 0) {
                pairTable.innerHTML = '<tr><td colspan="7" style="text-align: center; color: #71767b;">No data available</td></tr>';
            } else {
                Object.entries(analytics.by_pair).forEach(([pair, metrics]) => {
                    const row = document.createElement('tr');
                    const pnlColor = parseFloat(metrics.total_pnl) >= 0 ? '#00ba7c' : '#f4212e';
                    row.innerHTML = `
                        <td style="font-weight: 600;">${pair}</td>
                        <td>${metrics.trades}</td>
                        <td>${formatPercent(metrics.win_rate)}</td>
                        <td style="color: ${pnlColor};">${formatCurrency(metrics.total_pnl)}</td>
                        <td style="color: #00ba7c;">${formatCurrency(metrics.avg_win)}</td>
                        <td style="color: #f4212e;">${formatCurrency(metrics.avg_loss)}</td>
                        <td>${formatDecimal(metrics.profit_factor, 2)}</td>
                    `;
                    pairTable.appendChild(row);
                });
            }

            // Performance by strategy
            const strategyTable = document.getElementById('analytics-by-strategy');
            strategyTable.innerHTML = '';
            if (Object.keys(analytics.by_strategy).length === 0) {
                strategyTable.innerHTML = '<tr><td colspan="7" style="text-align: center; color: #71767b;">No data available</td></tr>';
            } else {
                Object.entries(analytics.by_strategy).forEach(([strategy, metrics]) => {
                    const row = document.createElement('tr');
                    const pnlColor = parseFloat(metrics.total_pnl) >= 0 ? '#00ba7c' : '#f4212e';
                    row.innerHTML = `
                        <td style="font-weight: 600;">${strategy}</td>
                        <td>${metrics.trades}</td>
                        <td>${formatPercent(metrics.win_rate)}</td>
                        <td style="color: ${pnlColor};">${formatCurrency(metrics.total_pnl)}</td>
                        <td style="color: #00ba7c;">${formatCurrency(metrics.avg_win)}</td>
                        <td style="color: #f4212e;">${formatCurrency(metrics.avg_loss)}</td>
                        <td>${formatDecimal(metrics.profit_factor, 2)}</td>
                    `;
                    strategyTable.appendChild(row);
                });
            }
        }

        // Notification Functions
        let notificationPanel = null;
        let notifications = [];

        function toggleNotificationPanel() {
            if (!notificationPanel) {
                notificationPanel = document.getElementById('notification-panel');
            }

            const isVisible = notificationPanel.style.display === 'flex';
            notificationPanel.style.display = isVisible ? 'none' : 'flex';

            if (!isVisible) {
                loadNotifications();
            }
        }

        async function loadNotifications() {
            try {
                const response = await fetch('/api/notifications');
                const data = await response.json();
                notifications = data.notifications || [];
                renderNotifications();
                updateNotificationBadge();
            } catch (e) {
                console.error('Failed to load notifications:', e);
            }
        }

        function renderNotifications() {
            const list = document.getElementById('notification-list');

            if (notifications.length === 0) {
                list.innerHTML = '<div class="notification-empty">No notifications</div>';
                return;
            }

            list.innerHTML = notifications.map(notif => {
                const time = new Date(notif.timestamp).toLocaleTimeString();
                const message = formatNotificationMessage(notif);
                const acknowledgedClass = notif.acknowledged ? 'acknowledged' : '';

                return `
                    <div class="notification-item ${acknowledgedClass}"
                         onclick="acknowledgeNotification('${notif.id}')">
                        <div class="notification-severity ${notif.severity}">${notif.severity}</div>
                        <div class="notification-content">
                            <div class="notification-message">${message}</div>
                            <div class="notification-time">${time}</div>
                        </div>
                    </div>
                `;
            }).join('');
        }

        function formatNotificationMessage(notif) {
            const type = notif.alert_type.type;
            const data = notif.alert_type.data || {};

            switch (type) {
                case 'PositionOpened':
                    return `🟢 ${data.side} ${data.quantity} ${data.pair} @ $${data.entry_price}`;
                case 'PositionClosed':
                    return `🔴 Closed ${data.pair} | P&L: ${data.pnl} (${data.pnl_pct}%) | ${data.reason}`;
                case 'StopLossTriggered':
                    return `⛔ Stop Loss: ${data.pair} @ $${data.price} | Loss: ${data.loss}`;
                case 'TakeProfitTriggered':
                    return `✅ Take Profit: ${data.pair} @ $${data.price} | Profit: ${data.profit}`;
                case 'PartialExitExecuted':
                    return `📊 Partial Exit: ${data.quantity} ${data.pair} | P&L: ${data.pnl} | ${data.reason}`;
                case 'TrailingStopActivated':
                    return `🔄 Trailing Stop Active: ${data.pair} @ $${data.activation_price}`;
                case 'BreakEvenStopSet':
                    return `⚖️ Break-Even Stop Set: ${data.pair} @ $${data.entry_price}`;
                case 'PositionScaled':
                    return `📈 Position Scaled: +${data.added_quantity} ${data.pair} | Avg: $${data.new_avg_entry}`;
                case 'MaxDrawdownApproached':
                    return `⚠️ Drawdown Warning: ${data.current_drawdown}% (Max: ${data.max_allowed}%)`;
                case 'MaxDrawdownExceeded':
                    return `🚨 CRITICAL: Max Drawdown Exceeded! ${data.current_drawdown}% > ${data.max_allowed}%`;
                case 'DailyLossLimitApproached':
                    return `⚠️ Daily Loss Warning: ${data.current_loss} (Limit: ${data.limit})`;
                case 'DailyLossLimitExceeded':
                    return `🚨 CRITICAL: Daily Loss Limit Exceeded! ${data.current_loss} > ${data.limit}`;
                case 'MaxPositionsReached':
                    return `⚠️ Max Positions Reached: ${data.current}/${data.max}`;
                case 'LowBalance':
                    return `⚠️ Low Balance: ${data.available} (Required: ${data.required})`;
                case 'LargePosition':
                    return `⚠️ Large Position: ${data.pair} ${data.size_pct}% (Max: ${data.max_allowed}%)`;
                case 'WinRateChanged':
                    return `📊 Win Rate: ${data.old_rate}% → ${data.new_rate}%`;
                case 'ProfitMilestone':
                    return `🎉 Profit Milestone: ${data.milestone} | Total: ${data.total}`;
                case 'LossMilestone':
                    return `📉 Loss Alert: ${data.milestone} | Total: ${data.total}`;
                case 'BotStarted':
                    return `▶️ Trading Bot Started`;
                case 'BotStopped':
                    return `⏸️ Trading Bot Stopped: ${data.reason || 'Manual stop'}`;
                case 'BotPaused':
                    return `⏸️ Trading Bot Paused`;
                case 'ConnectionLost':
                    return `📡 Connection Lost: ${data.exchange}`;
                case 'ConnectionRestored':
                    return `📡 Connection Restored: ${data.exchange}`;
                case 'ConfigurationChanged':
                    return `⚙️ Config Updated: ${data.section}`;
                case 'ErrorOccurred':
                    return `❌ Error: ${data.error} | ${data.context}`;
                default:
                    return `📢 ${type}`;
            }
        }

        async function acknowledgeNotification(id) {
            try {
                await fetch('/api/notifications/acknowledge', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ id })
                });

                // Update local state
                const notif = notifications.find(n => n.id === id);
                if (notif) {
                    notif.acknowledged = true;
                }

                renderNotifications();
                updateNotificationBadge();
            } catch (e) {
                console.error('Failed to acknowledge notification:', e);
            }
        }

        async function clearAllNotifications() {
            const unacknowledged = notifications.filter(n => !n.acknowledged);

            for (const notif of unacknowledged) {
                await acknowledgeNotification(notif.id);
            }

            showToast('success', 'All notifications cleared');
        }

        function updateNotificationBadge() {
            const badge = document.getElementById('notification-badge');
            const unacknowledgedCount = notifications.filter(n => !n.acknowledged).length;

            if (unacknowledgedCount > 0) {
                badge.textContent = unacknowledgedCount;
                badge.style.display = 'block';
            } else {
                badge.style.display = 'none';
            }
        }

        // Poll for new notifications every 10 seconds
        setInterval(async () => {
            try {
                const response = await fetch('/api/notifications/critical');
                const data = await response.json();
                const criticalNotifications = data.notifications || [];

                // Check for new critical notifications
                const newCritical = criticalNotifications.filter(cn =>
                    !notifications.some(n => n.id === cn.id)
                );

                if (newCritical.length > 0) {
                    // Add to notifications list
                    notifications.unshift(...newCritical);
                    updateNotificationBadge();

                    // Show toast for critical alerts
                    newCritical.forEach(notif => {
                        const message = formatNotificationMessage(notif);
                        showToast('error', message);
                    });
                }
            } catch (e) {
                console.error('Failed to poll notifications:', e);
            }
        }, 10000);

        // View Switching
        function switchView(viewName) {
            // Update tab buttons
            document.querySelectorAll('.view-tab').forEach(tab => {
                tab.classList.remove('active');
            });
            event.target.classList.add('active');

            // Update view content
            document.querySelectorAll('.view-content').forEach(view => {
                view.classList.remove('active');
            });
            document.getElementById('view-' + viewName).classList.add('active');

            // Load analytics when switching to analytics view
            if (viewName === 'analytics') {
                loadAnalytics();
            }
        }

        // Initialize
        initCharts();
        loadProfiles();
        loadNotifications();
        connectWebSocket();

        // Fetch initial data via REST as backup
        fetch('/api/data')
            .then(r => r.json())
            .then(d => {
                if (d.data) updateDashboard(d.data);
                if (d.status) updateBotStatus(d.status);
                if (d.config) populateConfig(d.config);
            })
            .catch(console.error);
    </script>
</body>
</html>
"##;
