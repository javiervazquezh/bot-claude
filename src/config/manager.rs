#![allow(dead_code)]
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use serde::Serialize;
use tracing::info;

use super::runtime::{
    RuntimeConfig, RiskSettings, ExecutorSettings, StrategySettings, GeneralSettings,
};

#[derive(Debug, Clone, Serialize)]
pub enum ConfigChangeEvent {
    RiskUpdated(RiskSettings),
    ExecutorUpdated(ExecutorSettings),
    StrategyUpdated(StrategySettings),
    GeneralUpdated(GeneralSettings),
    FullConfigUpdated,
}

pub struct RuntimeConfigManager {
    config: Arc<RwLock<RuntimeConfig>>,
    change_tx: broadcast::Sender<ConfigChangeEvent>,
}

impl RuntimeConfigManager {
    pub fn new(initial: RuntimeConfig) -> Self {
        let (change_tx, _) = broadcast::channel(32);
        Self {
            config: Arc::new(RwLock::new(initial)),
            change_tx,
        }
    }

    pub async fn get_config(&self) -> RuntimeConfig {
        self.config.read().await.clone()
    }

    pub async fn update_risk(&self, settings: RiskSettings) -> Result<(), String> {
        let mut config = self.config.write().await;
        let old_risk = config.risk.clone();
        config.risk = settings.clone();

        if let Err(errors) = config.validate() {
            config.risk = old_risk;
            return Err(errors.join(", "));
        }

        info!("Risk settings updated: max_positions={}, risk_per_trade={}%",
              settings.max_positions, settings.risk_per_trade_pct);
        let _ = self.change_tx.send(ConfigChangeEvent::RiskUpdated(settings));
        Ok(())
    }

    pub async fn update_executor(&self, settings: ExecutorSettings) -> Result<(), String> {
        let mut config = self.config.write().await;
        let old_executor = config.executor.clone();
        config.executor = settings.clone();

        if let Err(errors) = config.validate() {
            config.executor = old_executor;
            return Err(errors.join(", "));
        }

        info!("Executor settings updated: min_confidence={}%, min_risk_reward={}",
              settings.min_confidence * rust_decimal::Decimal::from(100), settings.min_risk_reward);
        let _ = self.change_tx.send(ConfigChangeEvent::ExecutorUpdated(settings));
        Ok(())
    }

    pub async fn update_strategies(&self, settings: StrategySettings) -> Result<(), String> {
        let mut config = self.config.write().await;
        let old_strategies = config.strategies.clone();
        config.strategies = settings.clone();

        if let Err(errors) = config.validate() {
            config.strategies = old_strategies;
            return Err(errors.join(", "));
        }

        info!("Strategy settings updated");
        let _ = self.change_tx.send(ConfigChangeEvent::StrategyUpdated(settings));
        Ok(())
    }

    pub async fn update_general(&self, settings: GeneralSettings) -> Result<(), String> {
        let mut config = self.config.write().await;
        config.general = settings.clone();

        info!("General settings updated: pairs={:?}, timeframe={}",
              settings.enabled_pairs, settings.timeframe);
        let _ = self.change_tx.send(ConfigChangeEvent::GeneralUpdated(settings));
        Ok(())
    }

    pub async fn update_full(&self, new_config: RuntimeConfig) -> Result<(), String> {
        if let Err(errors) = new_config.validate() {
            return Err(errors.join(", "));
        }

        let mut config = self.config.write().await;
        *config = new_config;

        info!("Full configuration updated");
        let _ = self.change_tx.send(ConfigChangeEvent::FullConfigUpdated);
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConfigChangeEvent> {
        self.change_tx.subscribe()
    }

    pub fn config_arc(&self) -> Arc<RwLock<RuntimeConfig>> {
        Arc::clone(&self.config)
    }
}

impl Clone for RuntimeConfigManager {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            change_tx: self.change_tx.clone(),
        }
    }
}
