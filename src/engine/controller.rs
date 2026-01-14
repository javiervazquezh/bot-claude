use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BotStatus {
    Running,
    Paused,
    Stopped,
}

impl std::fmt::Display for BotStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BotStatus::Running => write!(f, "Running"),
            BotStatus::Paused => write!(f, "Paused"),
            BotStatus::Stopped => write!(f, "Stopped"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BotState {
    pub status: BotStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub paused_at: Option<DateTime<Utc>>,
    pub uptime_seconds: u64,
    pub trades_count: u64,
}

pub struct BotController {
    is_running: AtomicBool,
    is_paused: AtomicBool,
    started_at: RwLock<Option<DateTime<Utc>>>,
    paused_at: RwLock<Option<DateTime<Utc>>>,
    trades_count: AtomicU64,
    status_tx: broadcast::Sender<BotStatus>,
}

impl BotController {
    pub fn new() -> Self {
        let (status_tx, _) = broadcast::channel(32);
        Self {
            is_running: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            started_at: RwLock::new(None),
            paused_at: RwLock::new(None),
            trades_count: AtomicU64::new(0),
            status_tx,
        }
    }

    pub fn new_running() -> Self {
        let (status_tx, _) = broadcast::channel(32);
        Self {
            is_running: AtomicBool::new(true),
            is_paused: AtomicBool::new(false),
            started_at: RwLock::new(Some(Utc::now())),
            paused_at: RwLock::new(None),
            trades_count: AtomicU64::new(0),
            status_tx,
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        if self.is_running.load(Ordering::Acquire) && !self.is_paused.load(Ordering::Acquire) {
            return Err("Bot is already running".to_string());
        }

        self.is_running.store(true, Ordering::Release);
        self.is_paused.store(false, Ordering::Release);
        *self.started_at.write().await = Some(Utc::now());
        *self.paused_at.write().await = None;

        info!("Bot started");
        let _ = self.status_tx.send(BotStatus::Running);
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), String> {
        if !self.is_running.load(Ordering::Acquire) {
            return Err("Bot is not running".to_string());
        }

        self.is_running.store(false, Ordering::Release);
        self.is_paused.store(false, Ordering::Release);
        *self.paused_at.write().await = None;

        info!("Bot stopped");
        let _ = self.status_tx.send(BotStatus::Stopped);
        Ok(())
    }

    pub async fn pause(&self) -> Result<(), String> {
        if !self.is_running.load(Ordering::Acquire) {
            return Err("Bot is not running".to_string());
        }
        if self.is_paused.load(Ordering::Acquire) {
            return Err("Bot is already paused".to_string());
        }

        self.is_paused.store(true, Ordering::Release);
        *self.paused_at.write().await = Some(Utc::now());

        info!("Bot paused");
        let _ = self.status_tx.send(BotStatus::Paused);
        Ok(())
    }

    pub async fn resume(&self) -> Result<(), String> {
        if !self.is_running.load(Ordering::Acquire) {
            return Err("Bot is not running".to_string());
        }
        if !self.is_paused.load(Ordering::Acquire) {
            return Err("Bot is not paused".to_string());
        }

        self.is_paused.store(false, Ordering::Release);
        *self.paused_at.write().await = None;

        info!("Bot resumed");
        let _ = self.status_tx.send(BotStatus::Running);
        Ok(())
    }

    pub fn should_process_signals(&self) -> bool {
        self.is_running.load(Ordering::Acquire) && !self.is_paused.load(Ordering::Acquire)
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Acquire)
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Acquire)
    }

    pub fn increment_trades(&self) {
        self.trades_count.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn get_state(&self) -> BotState {
        let started_at = *self.started_at.read().await;
        let paused_at = *self.paused_at.read().await;

        let status = if !self.is_running.load(Ordering::Acquire) {
            BotStatus::Stopped
        } else if self.is_paused.load(Ordering::Acquire) {
            BotStatus::Paused
        } else {
            BotStatus::Running
        };

        let uptime_seconds = started_at
            .map(|start| (Utc::now() - start).num_seconds().max(0) as u64)
            .unwrap_or(0);

        BotState {
            status,
            started_at,
            paused_at,
            uptime_seconds,
            trades_count: self.trades_count.load(Ordering::Relaxed),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BotStatus> {
        self.status_tx.subscribe()
    }
}

impl Default for BotController {
    fn default() -> Self {
        Self::new()
    }
}
