use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::{Row, Sqlite};
use std::path::Path;
use std::str::FromStr;
use tracing::info;

use crate::types::{Position, PositionStatus, Side, TradingPair};
use crate::web::state::TradeRecord;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Initialize database with schema
    pub async fn new(db_path: &str) -> Result<Self> {
        info!("Initializing SQLite database at: {}", db_path);

        // Create database file if it doesn't exist
        let options = SqliteConnectOptions::from_str(db_path)?
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let db = Self { pool };
        db.create_schema().await?;

        info!("Database initialized successfully");
        Ok(db)
    }

    /// Create database schema
    async fn create_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS trades (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                pair TEXT NOT NULL,
                side TEXT NOT NULL,
                entry_price TEXT NOT NULL,
                exit_price TEXT,
                quantity TEXT NOT NULL,
                pnl TEXT NOT NULL,
                pnl_pct TEXT NOT NULL,
                fees TEXT NOT NULL,
                strategy TEXT NOT NULL,
                exit_reason TEXT,
                status TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_trades_timestamp ON trades(timestamp)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_trades_pair ON trades(pair)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS equity_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                equity TEXT NOT NULL,
                unrealized_pnl TEXT NOT NULL,
                realized_pnl TEXT NOT NULL,
                max_drawdown TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_equity_timestamp ON equity_snapshots(timestamp)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                pair TEXT NOT NULL,
                signal TEXT NOT NULL,
                confidence TEXT NOT NULL,
                reason TEXT NOT NULL,
                strategy TEXT NOT NULL,
                entry_price TEXT,
                stop_loss TEXT,
                take_profit TEXT,
                executed INTEGER NOT NULL DEFAULT 0,
                trade_id TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_signals_timestamp ON signals(timestamp)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_signals_pair ON signals(pair)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_signals_executed ON signals(executed)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Add partial_exits table for tracking partial position closes
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS partial_exits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                position_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                quantity TEXT NOT NULL,
                exit_price TEXT NOT NULL,
                pnl TEXT NOT NULL,
                reason TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_partial_exits_position ON partial_exits(position_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_partial_exits_timestamp ON partial_exits(timestamp)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Add position_state column to trades table if it doesn't exist
        // This stores JSON serialized PositionState for advanced position management
        sqlx::query(
            r#"
            ALTER TABLE trades ADD COLUMN position_state TEXT
            "#,
        )
        .execute(&self.pool)
        .await
        .ok(); // Ignore error if column already exists

        // Add notifications table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS notifications (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                severity TEXT NOT NULL,
                alert_type TEXT NOT NULL,
                alert_data TEXT NOT NULL,
                acknowledged INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_notifications_timestamp ON notifications(timestamp DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_notifications_severity ON notifications(severity)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_notifications_acknowledged ON notifications(acknowledged)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Positions table for state persistence across restarts
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS positions (
                id TEXT PRIMARY KEY,
                pair TEXT NOT NULL,
                side TEXT NOT NULL,
                status TEXT NOT NULL,
                entry_price TEXT NOT NULL,
                current_price TEXT NOT NULL,
                quantity TEXT NOT NULL,
                stop_loss TEXT,
                take_profit TEXT,
                unrealized_pnl TEXT NOT NULL,
                realized_pnl TEXT NOT NULL,
                peak_pnl_pct TEXT NOT NULL DEFAULT '0',
                opened_at TEXT NOT NULL,
                closed_at TEXT,
                strategy_id TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_positions_status ON positions(status)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Portfolio state table (single row, upserted on each save)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS portfolio_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                usdt_balance TEXT NOT NULL,
                initial_capital TEXT NOT NULL,
                total_pnl TEXT NOT NULL,
                peak_equity TEXT NOT NULL,
                max_drawdown TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Save or update a position in the database
    pub async fn upsert_position(&self, position: &Position) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO positions (
                id, pair, side, status, entry_price, current_price, quantity,
                stop_loss, take_profit, unrealized_pnl, realized_pnl, peak_pnl_pct,
                opened_at, closed_at, strategy_id, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                current_price = excluded.current_price,
                stop_loss = excluded.stop_loss,
                take_profit = excluded.take_profit,
                unrealized_pnl = excluded.unrealized_pnl,
                realized_pnl = excluded.realized_pnl,
                peak_pnl_pct = excluded.peak_pnl_pct,
                closed_at = excluded.closed_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&position.id)
        .bind(position.pair.as_str())
        .bind(format!("{:?}", position.side))
        .bind(format!("{:?}", position.status))
        .bind(position.entry_price.to_string())
        .bind(position.current_price.to_string())
        .bind(position.quantity.to_string())
        .bind(position.stop_loss.map(|p| p.to_string()))
        .bind(position.take_profit.map(|p| p.to_string()))
        .bind(position.unrealized_pnl.to_string())
        .bind(position.realized_pnl.to_string())
        .bind(position.peak_pnl_pct.to_string())
        .bind(position.opened_at.to_rfc3339())
        .bind(position.closed_at.map(|t| t.to_rfc3339()))
        .bind(&position.strategy_id)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Load all open positions from the database
    pub async fn get_open_positions(&self) -> Result<Vec<Position>> {
        let rows = sqlx::query(
            r#"
            SELECT id, pair, side, status, entry_price, current_price, quantity,
                   stop_loss, take_profit, unrealized_pnl, realized_pnl, peak_pnl_pct,
                   opened_at, closed_at, strategy_id
            FROM positions
            WHERE status = 'Open'
            ORDER BY opened_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut positions = Vec::new();
        for row in rows {
            positions.push(Position {
                id: row.get("id"),
                pair: parse_trading_pair(row.get("pair"))?,
                side: parse_side(row.get("side"))?,
                status: PositionStatus::Open,
                entry_price: Decimal::from_str(row.get("entry_price"))?,
                current_price: Decimal::from_str(row.get("current_price"))?,
                quantity: Decimal::from_str(row.get("quantity"))?,
                stop_loss: row.get::<Option<String>, _>("stop_loss")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                take_profit: row.get::<Option<String>, _>("take_profit")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                unrealized_pnl: Decimal::from_str(row.get("unrealized_pnl"))?,
                realized_pnl: Decimal::from_str(row.get("realized_pnl"))?,
                peak_pnl_pct: row.get::<Option<String>, _>("peak_pnl_pct")
                    .and_then(|s| Decimal::from_str(&s).ok())
                    .unwrap_or(Decimal::ZERO),
                opened_at: DateTime::parse_from_rfc3339(row.get("opened_at"))?.with_timezone(&Utc),
                closed_at: None,
                strategy_id: row.get("strategy_id"),
                order_ids: Vec::new(),
                oco_order_id: None,
            });
        }

        Ok(positions)
    }

    /// Mark a position as closed in the database
    pub async fn close_position_in_db(&self, id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE positions
            SET status = 'Closed', closed_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(Utc::now().to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Save portfolio state (single-row upsert)
    pub async fn save_portfolio_state(
        &self,
        usdt_balance: Decimal,
        initial_capital: Decimal,
        total_pnl: Decimal,
        peak_equity: Decimal,
        max_drawdown: Decimal,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO portfolio_state (id, usdt_balance, initial_capital, total_pnl, peak_equity, max_drawdown, updated_at)
            VALUES (1, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                usdt_balance = excluded.usdt_balance,
                initial_capital = excluded.initial_capital,
                total_pnl = excluded.total_pnl,
                peak_equity = excluded.peak_equity,
                max_drawdown = excluded.max_drawdown,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(usdt_balance.to_string())
        .bind(initial_capital.to_string())
        .bind(total_pnl.to_string())
        .bind(peak_equity.to_string())
        .bind(max_drawdown.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Load portfolio state
    pub async fn load_portfolio_state(&self) -> Result<Option<(Decimal, Decimal, Decimal, Decimal, Decimal)>> {
        let row = sqlx::query(
            r#"
            SELECT usdt_balance, initial_capital, total_pnl, peak_equity, max_drawdown
            FROM portfolio_state
            WHERE id = 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some((
                Decimal::from_str(row.get("usdt_balance"))?,
                Decimal::from_str(row.get("initial_capital"))?,
                Decimal::from_str(row.get("total_pnl"))?,
                Decimal::from_str(row.get("peak_equity"))?,
                Decimal::from_str(row.get("max_drawdown"))?,
            ))),
            None => Ok(None),
        }
    }

    /// Insert a trade record
    pub async fn insert_trade(&self, trade: &TradeRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO trades (
                id, timestamp, pair, side, entry_price, exit_price,
                quantity, pnl, pnl_pct, fees, strategy, exit_reason, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&trade.id)
        .bind(trade.timestamp.to_rfc3339())
        .bind(format!("{:?}", trade.pair))
        .bind(format!("{:?}", trade.side))
        .bind(trade.entry_price.to_string())
        .bind(trade.exit_price.map(|p| p.to_string()))
        .bind(trade.quantity.to_string())
        .bind(trade.pnl.to_string())
        .bind(trade.pnl_pct.to_string())
        .bind(trade.fees.to_string())
        .bind(&trade.strategy)
        .bind(&trade.exit_reason)
        .bind(&trade.status)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all trades (for analytics)
    pub async fn get_all_trades(&self) -> Result<Vec<TradeRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, timestamp, pair, side, entry_price, exit_price,
                   quantity, pnl, pnl_pct, fees, strategy, exit_reason, status
            FROM trades
            ORDER BY timestamp DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut trades = Vec::new();
        for row in rows {
            trades.push(TradeRecord {
                id: row.get("id"),
                timestamp: DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc),
                pair: parse_trading_pair(row.get("pair"))?,
                side: parse_side(row.get("side"))?,
                entry_price: Decimal::from_str(row.get("entry_price"))?,
                exit_price: row.get::<Option<String>, _>("exit_price")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                quantity: Decimal::from_str(row.get("quantity"))?,
                pnl: Decimal::from_str(row.get("pnl"))?,
                pnl_pct: Decimal::from_str(row.get("pnl_pct"))?,
                fees: Decimal::from_str(row.get("fees"))?,
                strategy: row.get("strategy"),
                exit_reason: row.get("exit_reason"),
                status: row.get("status"),
            });
        }

        Ok(trades)
    }

    /// Get trades for a specific time range
    pub async fn get_trades_since(&self, since: DateTime<Utc>) -> Result<Vec<TradeRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, timestamp, pair, side, entry_price, exit_price,
                   quantity, pnl, pnl_pct, fees, strategy, exit_reason, status
            FROM trades
            WHERE timestamp >= ?
            ORDER BY timestamp DESC
            "#,
        )
        .bind(since.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;

        let mut trades = Vec::new();
        for row in rows {
            trades.push(TradeRecord {
                id: row.get("id"),
                timestamp: DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc),
                pair: parse_trading_pair(row.get("pair"))?,
                side: parse_side(row.get("side"))?,
                entry_price: Decimal::from_str(row.get("entry_price"))?,
                exit_price: row.get::<Option<String>, _>("exit_price")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                quantity: Decimal::from_str(row.get("quantity"))?,
                pnl: Decimal::from_str(row.get("pnl"))?,
                pnl_pct: Decimal::from_str(row.get("pnl_pct"))?,
                fees: Decimal::from_str(row.get("fees"))?,
                strategy: row.get("strategy"),
                exit_reason: row.get("exit_reason"),
                status: row.get("status"),
            });
        }

        Ok(trades)
    }

    /// Get recent N trades
    pub async fn get_recent_trades(&self, limit: u32) -> Result<Vec<TradeRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, timestamp, pair, side, entry_price, exit_price,
                   quantity, pnl, pnl_pct, fees, strategy, exit_reason, status
            FROM trades
            ORDER BY timestamp DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut trades = Vec::new();
        for row in rows {
            trades.push(TradeRecord {
                id: row.get("id"),
                timestamp: DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc),
                pair: parse_trading_pair(row.get("pair"))?,
                side: parse_side(row.get("side"))?,
                entry_price: Decimal::from_str(row.get("entry_price"))?,
                exit_price: row.get::<Option<String>, _>("exit_price")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                quantity: Decimal::from_str(row.get("quantity"))?,
                pnl: Decimal::from_str(row.get("pnl"))?,
                pnl_pct: Decimal::from_str(row.get("pnl_pct"))?,
                fees: Decimal::from_str(row.get("fees"))?,
                strategy: row.get("strategy"),
                exit_reason: row.get("exit_reason"),
                status: row.get("status"),
            });
        }

        Ok(trades)
    }

    /// Get trade count
    pub async fn get_trade_count(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM trades")
            .fetch_one(&self.pool)
            .await?;

        Ok(row.get("count"))
    }

    /// Insert equity snapshot
    pub async fn insert_equity_snapshot(
        &self,
        timestamp: DateTime<Utc>,
        equity: Decimal,
        unrealized_pnl: Decimal,
        realized_pnl: Decimal,
        max_drawdown: Decimal,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO equity_snapshots (timestamp, equity, unrealized_pnl, realized_pnl, max_drawdown)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(timestamp.to_rfc3339())
        .bind(equity.to_string())
        .bind(unrealized_pnl.to_string())
        .bind(realized_pnl.to_string())
        .bind(max_drawdown.to_string())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get equity snapshots for a time range
    pub async fn get_equity_snapshots_since(&self, since: DateTime<Utc>) -> Result<Vec<(DateTime<Utc>, Decimal)>> {
        let rows = sqlx::query(
            r#"
            SELECT timestamp, equity
            FROM equity_snapshots
            WHERE timestamp >= ?
            ORDER BY timestamp ASC
            "#,
        )
        .bind(since.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;

        let mut snapshots = Vec::new();
        for row in rows {
            let timestamp = DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc);
            let equity = Decimal::from_str(row.get("equity"))?;
            snapshots.push((timestamp, equity));
        }

        Ok(snapshots)
    }

    /// Insert a signal record
    pub async fn insert_signal(&self, signal: &crate::web::state::SignalRecord) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO signals (
                timestamp, pair, signal, confidence, reason, strategy,
                entry_price, stop_loss, take_profit, executed
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(signal.timestamp.to_rfc3339())
        .bind(format!("{:?}", signal.pair))
        .bind(&signal.signal)
        .bind(signal.confidence.to_string())
        .bind(&signal.reason)
        .bind(&signal.strategy)
        .bind(signal.entry_price.map(|p| p.to_string()))
        .bind(signal.stop_loss.map(|p| p.to_string()))
        .bind(signal.take_profit.map(|p| p.to_string()))
        .bind(if signal.executed { 1 } else { 0 })
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Update signal as executed and link to trade
    pub async fn update_signal_executed(&self, signal_id: i64, trade_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE signals
            SET executed = 1, trade_id = ?
            WHERE id = ?
            "#,
        )
        .bind(trade_id)
        .bind(signal_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all signals
    pub async fn get_all_signals(&self) -> Result<Vec<crate::web::state::SignalRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT timestamp, pair, signal, confidence, reason, strategy,
                   entry_price, stop_loss, take_profit, executed
            FROM signals
            ORDER BY timestamp DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut signals = Vec::new();
        for row in rows {
            signals.push(crate::web::state::SignalRecord {
                timestamp: DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc),
                pair: parse_trading_pair(row.get("pair"))?,
                signal: row.get("signal"),
                confidence: Decimal::from_str(row.get("confidence"))?,
                reason: row.get("reason"),
                strategy: row.get("strategy"),
                entry_price: row.get::<Option<String>, _>("entry_price")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                stop_loss: row.get::<Option<String>, _>("stop_loss")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                take_profit: row.get::<Option<String>, _>("take_profit")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                executed: row.get::<i32, _>("executed") == 1,
            });
        }

        Ok(signals)
    }

    /// Get signals for a specific time range
    pub async fn get_signals_since(&self, since: DateTime<Utc>) -> Result<Vec<crate::web::state::SignalRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT timestamp, pair, signal, confidence, reason, strategy,
                   entry_price, stop_loss, take_profit, executed
            FROM signals
            WHERE timestamp >= ?
            ORDER BY timestamp DESC
            "#,
        )
        .bind(since.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;

        let mut signals = Vec::new();
        for row in rows {
            signals.push(crate::web::state::SignalRecord {
                timestamp: DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc),
                pair: parse_trading_pair(row.get("pair"))?,
                signal: row.get("signal"),
                confidence: Decimal::from_str(row.get("confidence"))?,
                reason: row.get("reason"),
                strategy: row.get("strategy"),
                entry_price: row.get::<Option<String>, _>("entry_price")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                stop_loss: row.get::<Option<String>, _>("stop_loss")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                take_profit: row.get::<Option<String>, _>("take_profit")
                    .and_then(|s| Decimal::from_str(&s).ok()),
                executed: row.get::<i32, _>("executed") == 1,
            });
        }

        Ok(signals)
    }

    /// Get signal statistics
    pub async fn get_signal_stats(&self) -> Result<(i64, i64, i64)> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) as total,
                SUM(CASE WHEN executed = 1 THEN 1 ELSE 0 END) as executed,
                SUM(CASE WHEN executed = 0 THEN 1 ELSE 0 END) as not_executed
            FROM signals
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok((
            row.get("total"),
            row.get("executed"),
            row.get("not_executed"),
        ))
    }

    /// Insert a partial exit record
    pub async fn insert_partial_exit(
        &self,
        position_id: &str,
        timestamp: DateTime<Utc>,
        quantity: Decimal,
        exit_price: Decimal,
        pnl: Decimal,
        reason: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO partial_exits (position_id, timestamp, quantity, exit_price, pnl, reason)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(position_id)
        .bind(timestamp.to_rfc3339())
        .bind(quantity.to_string())
        .bind(exit_price.to_string())
        .bind(pnl.to_string())
        .bind(reason)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all partial exits for a specific position
    pub async fn get_partial_exits_for_position(&self, position_id: &str) -> Result<Vec<PartialExitRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT timestamp, quantity, exit_price, pnl, reason
            FROM partial_exits
            WHERE position_id = ?
            ORDER BY timestamp ASC
            "#,
        )
        .bind(position_id)
        .fetch_all(&self.pool)
        .await?;

        let mut exits = Vec::new();
        for row in rows {
            exits.push(PartialExitRecord {
                timestamp: DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc),
                quantity: Decimal::from_str(row.get("quantity"))?,
                exit_price: Decimal::from_str(row.get("exit_price"))?,
                pnl: Decimal::from_str(row.get("pnl"))?,
                reason: row.get("reason"),
            });
        }

        Ok(exits)
    }

    /// Update position state JSON for a trade
    pub async fn update_position_state(&self, position_id: &str, state_json: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE trades
            SET position_state = ?
            WHERE id = ?
            "#,
        )
        .bind(state_json)
        .bind(position_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert a notification
    pub async fn insert_notification(&self, notification: &crate::notifications::Notification) -> Result<()> {
        let severity_str = match notification.severity {
            crate::notifications::Severity::Info => "Info",
            crate::notifications::Severity::Warning => "Warning",
            crate::notifications::Severity::Critical => "Critical",
        };

        let alert_type_str = notification.alert_type.title();
        let alert_data_json = serde_json::to_string(&notification.alert_type)?;

        sqlx::query(
            r#"
            INSERT INTO notifications (id, timestamp, severity, alert_type, alert_data, acknowledged)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&notification.id)
        .bind(notification.timestamp.to_rfc3339())
        .bind(severity_str)
        .bind(alert_type_str)
        .bind(alert_data_json)
        .bind(if notification.acknowledged { 1 } else { 0 })
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get recent notifications
    pub async fn get_recent_notifications(&self, limit: usize) -> Result<Vec<crate::notifications::Notification>> {
        let rows = sqlx::query(
            r#"
            SELECT id, timestamp, severity, alert_data, acknowledged
            FROM notifications
            ORDER BY timestamp DESC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut notifications = Vec::new();
        for row in rows {
            let severity_str: String = row.get("severity");
            let severity = match severity_str.as_str() {
                "Info" => crate::notifications::Severity::Info,
                "Warning" => crate::notifications::Severity::Warning,
                "Critical" => crate::notifications::Severity::Critical,
                _ => crate::notifications::Severity::Info,
            };

            let alert_data_json: String = row.get("alert_data");
            let alert_type: crate::notifications::AlertType = serde_json::from_str(&alert_data_json)?;

            notifications.push(crate::notifications::Notification {
                id: row.get("id"),
                timestamp: DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc),
                severity,
                alert_type,
                acknowledged: row.get::<i32, _>("acknowledged") == 1,
            });
        }

        Ok(notifications)
    }

    /// Get unacknowledged critical notifications
    pub async fn get_unacknowledged_critical(&self) -> Result<Vec<crate::notifications::Notification>> {
        let rows = sqlx::query(
            r#"
            SELECT id, timestamp, severity, alert_data, acknowledged
            FROM notifications
            WHERE severity = 'Critical' AND acknowledged = 0
            ORDER BY timestamp DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut notifications = Vec::new();
        for row in rows {
            let alert_data_json: String = row.get("alert_data");
            let alert_type: crate::notifications::AlertType = serde_json::from_str(&alert_data_json)?;

            notifications.push(crate::notifications::Notification {
                id: row.get("id"),
                timestamp: DateTime::parse_from_rfc3339(row.get("timestamp"))?.with_timezone(&Utc),
                severity: crate::notifications::Severity::Critical,
                alert_type,
                acknowledged: false,
            });
        }

        Ok(notifications)
    }

    /// Acknowledge a notification
    pub async fn acknowledge_notification(&self, id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE notifications
            SET acknowledged = 1
            WHERE id = ?
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Clean up old data (optional - for long-running bots)
    pub async fn cleanup_old_data(&self, keep_days: i64) -> Result<()> {
        let cutoff = Utc::now() - chrono::Duration::days(keep_days);

        sqlx::query("DELETE FROM equity_snapshots WHERE timestamp < ?")
            .bind(cutoff.to_rfc3339())
            .execute(&self.pool)
            .await?;

        sqlx::query("DELETE FROM notifications WHERE timestamp < ? AND acknowledged = 1")
            .bind(cutoff.to_rfc3339())
            .execute(&self.pool)
            .await?;

        info!("Cleaned up data older than {} days", keep_days);
        Ok(())
    }
}

/// Record of a partial position exit
#[derive(Debug, Clone)]
pub struct PartialExitRecord {
    pub timestamp: DateTime<Utc>,
    pub quantity: Decimal,
    pub exit_price: Decimal,
    pub pnl: Decimal,
    pub reason: String,
}

fn parse_trading_pair(s: &str) -> Result<TradingPair> {
    match s {
        "BTCUSDT" => Ok(TradingPair::BTCUSDT),
        "ETHUSDT" => Ok(TradingPair::ETHUSDT),
        "SOLUSDT" => Ok(TradingPair::SOLUSDT),
        "BNBUSDT" => Ok(TradingPair::BNBUSDT),
        "ADAUSDT" => Ok(TradingPair::ADAUSDT),
        "XRPUSDT" => Ok(TradingPair::XRPUSDT),
        _ => Err(anyhow::anyhow!("Unknown trading pair: {}", s)),
    }
}

fn parse_side(s: &str) -> Result<Side> {
    match s {
        "Buy" => Ok(Side::Buy),
        "Sell" => Ok(Side::Sell),
        _ => Err(anyhow::anyhow!("Unknown side: {}", s)),
    }
}
