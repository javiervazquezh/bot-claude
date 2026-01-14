use axum::{
    extract::{State, WebSocketUpgrade, ws::{Message, WebSocket}},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, error, info};

use crate::config::{
    RuntimeConfig, RiskSettings, ExecutorSettings, StrategySettings, GeneralSettings,
    StrategyProfile, StrategyConfig,
};
use super::{AppState, DashboardState, DashboardEvent};

// === Dashboard Data Endpoints ===

pub async fn get_dashboard_data(
    State(state): State<AppState>,
) -> impl IntoResponse {
    use rust_decimal::Decimal;

    let mut data = state.dashboard.get_api_response().await;
    let bot_state = state.controller.get_state().await;
    let config = state.config_manager.get_config().await;

    // Update with database data if available
    if let Some(db) = &state.database {
        if let Ok(trades) = db.get_all_trades().await {
            // Update portfolio
            let realized_pnl: Decimal = trades.iter().map(|t| t.pnl).sum();
            let initial_capital = Decimal::from(2000);
            let total_equity = initial_capital + realized_pnl;

            // Calculate max drawdown
            let mut sorted_trades = trades.clone();
            sorted_trades.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

            let mut equity = initial_capital;
            let mut peak = initial_capital;
            let mut max_dd = Decimal::ZERO;

            for trade in &sorted_trades {
                equity += trade.pnl;
                if equity > peak {
                    peak = equity;
                } else {
                    let dd = (peak - equity) / peak * Decimal::from(100);
                    if dd > max_dd {
                        max_dd = dd;
                    }
                }
            }

            data.portfolio.realized_pnl = realized_pnl;
            data.portfolio.total_equity = total_equity;
            data.portfolio.available_balance = total_equity;
            data.portfolio.unrealized_pnl = Decimal::ZERO;
            data.portfolio.max_drawdown = max_dd;

            // Update stats
            let total_trades = trades.len() as u64;
            let winning_trades = trades.iter().filter(|t| t.pnl > Decimal::ZERO).count() as u64;
            let losing_trades = total_trades - winning_trades;
            let total_profit: Decimal = trades.iter().filter(|t| t.pnl > Decimal::ZERO).map(|t| t.pnl).sum();
            let total_loss: Decimal = trades.iter().filter(|t| t.pnl < Decimal::ZERO).map(|t| t.pnl).sum();

            data.stats.total_trades = total_trades;
            data.stats.winning_trades = winning_trades;
            data.stats.losing_trades = losing_trades;
            data.stats.total_pnl = realized_pnl;
            data.stats.total_profit = total_profit;
            data.stats.total_loss = total_loss;

            // Update trades list
            data.recent_trades = trades.into_iter().collect();
        }
    }

    Json(json!({
        "data": data,
        "status": bot_state,
        "config": config
    }))
}

pub async fn get_portfolio(
    State(state): State<AppState>,
) -> impl IntoResponse {
    use rust_decimal::Decimal;

    let mut portfolio = state.dashboard.get_data().await.portfolio;

    // Update portfolio with database trade stats if available
    if let Some(db) = &state.database {
        if let Ok(trades) = db.get_all_trades().await {
            // Calculate realized P&L from closed trades
            let realized_pnl: Decimal = trades.iter().map(|t| t.pnl).sum();

            // Calculate total equity (initial capital + realized P&L)
            let initial_capital = Decimal::from(2000);
            let total_equity = initial_capital + realized_pnl;

            // Calculate max drawdown from trade history
            let mut sorted_trades = trades.clone();
            sorted_trades.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

            let mut equity = initial_capital;
            let mut peak = initial_capital;
            let mut max_dd = Decimal::ZERO;

            for trade in &sorted_trades {
                equity += trade.pnl;
                if equity > peak {
                    peak = equity;
                } else {
                    let dd = (peak - equity) / peak * Decimal::from(100);
                    if dd > max_dd {
                        max_dd = dd;
                    }
                }
            }

            portfolio.realized_pnl = realized_pnl;
            portfolio.total_equity = total_equity;
            portfolio.available_balance = total_equity; // All capital available in backtest data
            portfolio.unrealized_pnl = Decimal::ZERO; // No open positions in historical data
            portfolio.max_drawdown = max_dd;
        }
    }

    Json(portfolio)
}

pub async fn get_signals(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let data = state.dashboard.get_data().await;
    let signals: Vec<_> = data.signals.into_iter().collect();
    Json(signals)
}

pub async fn get_trades(
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Get trades from database if available, otherwise use in-memory
    let trades = if let Some(db) = &state.database {
        match db.get_all_trades().await {
            Ok(trades) => trades,
            Err(e) => {
                error!("Failed to fetch trades from database: {}", e);
                let data = state.dashboard.get_data().await;
                data.trades.into_iter().collect()
            }
        }
    } else {
        let data = state.dashboard.get_data().await;
        data.trades.into_iter().collect()
    };
    Json(trades)
}

pub async fn get_stats(
    State(state): State<AppState>,
) -> impl IntoResponse {
    use rust_decimal::Decimal;

    // Calculate stats from database trades if available
    let stats = if let Some(db) = &state.database {
        match db.get_all_trades().await {
            Ok(trades) => {
                let total_trades = trades.len() as u64;
                let winning_trades = trades.iter().filter(|t| t.pnl > Decimal::ZERO).count() as u64;
                let losing_trades = total_trades - winning_trades;
                let total_pnl: Decimal = trades.iter().map(|t| t.pnl).sum();
                let total_profit: Decimal = trades.iter().filter(|t| t.pnl > Decimal::ZERO).map(|t| t.pnl).sum();
                let total_loss: Decimal = trades.iter().filter(|t| t.pnl < Decimal::ZERO).map(|t| t.pnl).sum();

                json!({
                    "total_trades": total_trades,
                    "winning_trades": winning_trades,
                    "losing_trades": losing_trades,
                    "total_pnl": total_pnl.to_string(),
                    "total_profit": total_profit.to_string(),
                    "total_loss": total_loss.to_string(),
                })
            }
            Err(e) => {
                error!("Failed to fetch trades for stats: {}", e);
                let data = state.dashboard.get_data().await;
                json!(data.stats)
            }
        }
    } else {
        let data = state.dashboard.get_data().await;
        json!(data.stats)
    };
    Json(stats)
}

pub async fn get_prices(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let data = state.dashboard.get_data().await;
    Json(data.current_prices)
}

pub async fn get_analytics(
    State(state): State<AppState>,
) -> impl IntoResponse {
    use rust_decimal::Decimal;

    // Get trades from database if available, otherwise use in-memory
    let trades = if let Some(db) = &state.database {
        match db.get_all_trades().await {
            Ok(trades) => trades,
            Err(e) => {
                error!("Failed to fetch trades from database: {}", e);
                // Fall back to in-memory trades
                let data = state.dashboard.get_data().await;
                data.trades.into_iter().collect()
            }
        }
    } else {
        // Use in-memory trades
        let data = state.dashboard.get_data().await;
        data.trades.into_iter().collect()
    };

    // Get portfolio data for equity information
    let portfolio = state.dashboard.get_data().await.portfolio;

    // Determine initial capital and current equity correctly
    let (initial_capital, current_equity) = if !trades.is_empty() {
        // Calculate from trades: initial capital + total PnL = current equity
        let total_pnl: Decimal = trades.iter().map(|t| t.pnl).sum();

        // Use standard $2000 initial capital for backtest data
        let initial = Decimal::from(2000);
        let current = initial + total_pnl;

        (initial, current)
    } else {
        // No trades yet, use portfolio values (live bot just started)
        (Decimal::from(2000), portfolio.total_equity)
    };

    // Calculate analytics
    let analytics = crate::analytics::AnalyticsCalculator::calculate(
        &trades,
        initial_capital,
        current_equity,
    );
    Json(analytics)
}

pub async fn get_signal_stats(
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(db) = &state.database {
        match db.get_signal_stats().await {
            Ok((total, executed, not_executed)) => {
                let conversion_rate = if total > 0 {
                    (executed as f64 / total as f64) * 100.0
                } else {
                    0.0
                };

                Json(json!({
                    "total_signals": total,
                    "executed_signals": executed,
                    "not_executed_signals": not_executed,
                    "conversion_rate_pct": format!("{:.2}", conversion_rate)
                }))
            }
            Err(e) => {
                error!("Failed to fetch signal stats: {}", e);
                Json(json!({
                    "total_signals": 0,
                    "executed_signals": 0,
                    "not_executed_signals": 0,
                    "conversion_rate_pct": "0.00"
                }))
            }
        }
    } else {
        // Use in-memory data
        let data = state.dashboard.get_data().await;
        let total = data.signals.len() as i64;
        let executed = data.signals.iter().filter(|s| s.executed).count() as i64;
        let not_executed = total - executed;
        let conversion_rate = if total > 0 {
            (executed as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        Json(json!({
            "total_signals": total,
            "executed_signals": executed,
            "not_executed_signals": not_executed,
            "conversion_rate_pct": format!("{:.2}", conversion_rate)
        }))
    }
}

// === Bot Control Endpoints ===

pub async fn post_start(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.controller.start().await {
        Ok(()) => {
            let bot_state = state.controller.get_state().await;
            let _ = state.dashboard.tx.send(DashboardEvent::StatusChange { status: bot_state.clone() });
            (StatusCode::OK, Json(json!({"status": "ok", "state": bot_state}))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
        }
    }
}

pub async fn post_stop(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.controller.stop().await {
        Ok(()) => {
            let bot_state = state.controller.get_state().await;
            let _ = state.dashboard.tx.send(DashboardEvent::StatusChange { status: bot_state.clone() });
            (StatusCode::OK, Json(json!({"status": "ok", "state": bot_state}))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
        }
    }
}

pub async fn post_pause(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.controller.pause().await {
        Ok(()) => {
            let bot_state = state.controller.get_state().await;
            let _ = state.dashboard.tx.send(DashboardEvent::StatusChange { status: bot_state.clone() });
            (StatusCode::OK, Json(json!({"status": "ok", "state": bot_state}))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
        }
    }
}

pub async fn post_resume(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.controller.resume().await {
        Ok(()) => {
            let bot_state = state.controller.get_state().await;
            let _ = state.dashboard.tx.send(DashboardEvent::StatusChange { status: bot_state.clone() });
            (StatusCode::OK, Json(json!({"status": "ok", "state": bot_state}))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
        }
    }
}

pub async fn get_status(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let bot_state = state.controller.get_state().await;
    Json(bot_state)
}

// === Configuration Endpoints ===

pub async fn get_config(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.config_manager.get_config().await;
    Json(config)
}

pub async fn put_risk_settings(
    State(state): State<AppState>,
    Json(settings): Json<RiskSettings>,
) -> impl IntoResponse {
    match state.config_manager.update_risk(settings).await {
        Ok(()) => {
            (StatusCode::OK, Json(json!({"status": "ok", "message": "Risk settings updated"}))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
        }
    }
}

pub async fn put_executor_settings(
    State(state): State<AppState>,
    Json(settings): Json<ExecutorSettings>,
) -> impl IntoResponse {
    match state.config_manager.update_executor(settings).await {
        Ok(()) => {
            (StatusCode::OK, Json(json!({"status": "ok", "message": "Executor settings updated"}))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
        }
    }
}

pub async fn put_strategy_settings(
    State(state): State<AppState>,
    Json(settings): Json<StrategySettings>,
) -> impl IntoResponse {
    match state.config_manager.update_strategies(settings).await {
        Ok(()) => {
            (StatusCode::OK, Json(json!({"status": "ok", "message": "Strategy settings updated"}))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
        }
    }
}

pub async fn put_general_settings(
    State(state): State<AppState>,
    Json(settings): Json<GeneralSettings>,
) -> impl IntoResponse {
    match state.config_manager.update_general(settings).await {
        Ok(()) => {
            (StatusCode::OK, Json(json!({"status": "ok", "message": "General settings updated"}))).into_response()
        }
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response()
        }
    }
}

// === WebSocket Handler ===

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

async fn handle_websocket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.dashboard.tx.subscribe();

    info!("WebSocket client connected");

    // Send initial data including status and config
    let initial_data = state.dashboard.get_api_response().await;
    let bot_state = state.controller.get_state().await;
    let config = state.config_manager.get_config().await;

    let initial = json!({
        "type": "initial",
        "data": initial_data,
        "status": bot_state,
        "config": config
    });

    if let Ok(json_str) = serde_json::to_string(&initial) {
        let _ = sender.send(Message::Text(json_str)).await;
    }

    // Spawn task to forward events to client
    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming messages (for ping/pong)
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Ping(data)) => {
                debug!("Received ping");
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket client disconnected");
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    send_task.abort();
}

// === Strategy Profile Endpoints ===

#[derive(Serialize)]
pub struct ProfileInfo {
    pub profile: StrategyProfile,
    pub name: String,
    pub description: String,
    pub target_return: String,
    pub risk_level: String,
}

pub async fn get_profiles() -> impl IntoResponse {
    let profiles = vec![
        StrategyProfile::Conservative5Year,
        StrategyProfile::UltraAggressive,
    ];

    let profile_info: Vec<ProfileInfo> = profiles.iter().map(|p| {
        ProfileInfo {
            profile: *p,
            name: p.name().to_string(),
            description: p.description().to_string(),
            target_return: p.target_return().to_string(),
            risk_level: p.risk_level().to_string(),
        }
    }).collect();

    Json(profile_info)
}

pub async fn get_current_profile(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let config = state.config_manager.get_config().await;
    Json(json!({
        "profile": config.strategy_profile,
        "name": config.strategy_profile.name(),
        "description": config.strategy_profile.description(),
        "target_return": config.strategy_profile.target_return(),
        "risk_level": config.strategy_profile.risk_level(),
    }))
}

#[derive(Deserialize)]
pub struct SelectProfileRequest {
    pub profile: StrategyProfile,
}

pub async fn post_select_profile(
    State(state): State<AppState>,
    Json(req): Json<SelectProfileRequest>,
) -> impl IntoResponse {
    // Apply the profile configuration
    let profile_config = match req.profile {
        StrategyProfile::UltraAggressive => StrategyConfig::ultra_aggressive(),
        StrategyProfile::Conservative5Year => StrategyConfig::conservative_5year(),
        StrategyProfile::Custom => {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": "Custom profile requires manual configuration"
            }))).into_response();
        }
    };

    // Update the strategy profile in config
    let mut config = state.config_manager.get_config().await;
    config.strategy_profile = req.profile;

    // Note: In a real implementation, you would also update risk settings
    // based on the profile_config values. For now, we just update the profile marker.

    info!("Strategy profile changed to: {}", req.profile.name());

    (StatusCode::OK, Json(json!({
        "status": "ok",
        "message": format!("Strategy profile changed to {}", req.profile.name()),
        "profile": req.profile,
    }))).into_response()
}

// === Notifications ===

pub async fn get_notifications(
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(notifications) = &state.notifications {
        let recent = notifications.get_recent(50).await;
        Json(json!({
            "notifications": recent
        }))
    } else {
        Json(json!({
            "notifications": []
        }))
    }
}

pub async fn get_critical_notifications(
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(notifications) = &state.notifications {
        let critical = notifications.get_critical_unacknowledged().await;
        Json(json!({
            "notifications": critical
        }))
    } else {
        Json(json!({
            "notifications": []
        }))
    }
}

#[derive(Deserialize)]
pub struct AcknowledgeRequest {
    pub id: String,
}

pub async fn post_acknowledge_notification(
    State(state): State<AppState>,
    Json(req): Json<AcknowledgeRequest>,
) -> impl IntoResponse {
    if let Some(notifications) = &state.notifications {
        notifications.acknowledge(&req.id).await;

        if let Some(db) = &state.database {
            if let Err(e) = db.acknowledge_notification(&req.id).await {
                error!("Failed to acknowledge notification in database: {}", e);
            }
        }

        (StatusCode::OK, Json(json!({
            "status": "ok"
        }))).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({
            "error": "Notifications not available"
        }))).into_response()
    }
}

// === Health Check ===

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

pub async fn health_check() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: "0.1.0",
    })
}
