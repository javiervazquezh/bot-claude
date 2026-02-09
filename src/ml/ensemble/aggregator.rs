use anyhow::Result;
use tracing::{debug, info, warn};

use crate::ml::TradeFeatures;
use super::OnnxModel;

/// Model with its ensemble weight
struct WeightedModel {
    model: OnnxModel,
    weight: f64,
}

/// Ensemble predictor combining multiple ONNX models
pub struct EnsemblePredictor {
    models: Vec<WeightedModel>,
    threshold: f64,
}

impl EnsemblePredictor {
    pub fn new(threshold: f64) -> Self {
        Self {
            models: Vec::new(),
            threshold,
        }
    }

    /// Add a model with weight
    pub fn add_model(&mut self, model: OnnxModel, weight: f64) {
        info!("Ensemble: added model '{}' with weight {:.2}", model.name(), weight);
        self.models.push(WeightedModel { model, weight });
    }

    /// Load XGBoost + RF ensemble from directory
    pub fn load_from_dir(dir: &str, threshold: f64) -> Result<Self> {
        let mut ensemble = Self::new(threshold);

        let xgb_path = format!("{}/xgboost.onnx", dir);
        let rf_path = format!("{}/random_forest.onnx", dir);

        if std::path::Path::new(&xgb_path).exists() {
            let xgb = OnnxModel::load(&xgb_path, "xgboost")?;
            ensemble.add_model(xgb, 0.6);
        }

        if std::path::Path::new(&rf_path).exists() {
            let rf = OnnxModel::load(&rf_path, "random_forest")?;
            ensemble.add_model(rf, 0.4);
        }

        if ensemble.models.is_empty() {
            warn!("No ONNX models found in {}", dir);
        }

        Ok(ensemble)
    }

    /// Predict win probability (weighted average)
    pub fn predict_win_probability(&mut self, features: &TradeFeatures) -> Option<f64> {
        if self.models.is_empty() {
            return None;
        }

        let mut total_weight = 0.0;
        let mut weighted_sum = 0.0;

        for wm in &mut self.models {
            match wm.model.predict(features) {
                Ok(prob) => {
                    weighted_sum += prob * wm.weight;
                    total_weight += wm.weight;
                }
                Err(e) => {
                    debug!("Model '{}' prediction failed: {}", wm.model.name(), e);
                }
            }
        }

        if total_weight > 0.0 {
            Some(weighted_sum / total_weight)
        } else {
            None
        }
    }

    /// Should we take this trade?
    pub fn should_trade(&mut self, features: &TradeFeatures) -> bool {
        if self.models.is_empty() {
            return true; // Pass-through when no models loaded
        }

        match self.predict_win_probability(features) {
            Some(prob) => {
                debug!(
                    "Ensemble prediction: win_prob={:.1}%, threshold={:.1}%",
                    prob * 100.0,
                    self.threshold * 100.0
                );
                prob >= self.threshold
            }
            None => true, // Pass-through on prediction failure
        }
    }

    pub fn is_loaded(&self) -> bool {
        !self.models.is_empty()
    }

    pub fn model_count(&self) -> usize {
        self.models.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_ensemble_passes_through() {
        let mut ensemble = EnsemblePredictor::new(0.55);
        assert!(ensemble.should_trade(&test_features()));
        assert!(!ensemble.is_loaded());
        assert_eq!(ensemble.model_count(), 0);
    }

    #[test]
    fn test_predict_returns_none_when_empty() {
        let mut ensemble = EnsemblePredictor::new(0.55);
        assert!(ensemble.predict_win_probability(&test_features()).is_none());
    }

    fn test_features() -> TradeFeatures {
        TradeFeatures {
            signal_strength: 1.0,
            confidence: 0.7,
            rsi_14: 50.0,
            atr_pct: 1.5,
            ema_spread_pct: 0.5,
            bb_position: 0.5,
            volume_ratio: 1.0,
            volatility_regime: 1.0,
            recent_win_rate: 0.5,
            recent_avg_pnl_pct: 0.0,
            streak: 0.0,
            hour_of_day: 12.0,
            day_of_week: 3.0,
            pair_id: 0.0,
            macd_line: 0.0,
            macd_histogram: 0.0,
            stochastic_rsi_k: 50.0,
            mfi_14: 50.0,
            roc_10: 0.0,
            bb_width_pct: 2.0,
            atr_normalized_return: 0.0,
        }
    }
}
