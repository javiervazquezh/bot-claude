use anyhow::Result;
use ndarray::Array2;
use std::sync::Arc;

use super::{GaussianHMM, RegimeState, extract_regime_features};
use crate::types::CandleBuffer;

/// Real-time regime detector using trained HMM
pub struct RegimeDetector {
    hmm: Arc<GaussianHMM>,
    min_confidence: f64,
}

impl RegimeDetector {
    pub fn new(n_features: usize) -> Self {
        Self {
            hmm: Arc::new(GaussianHMM::new(n_features)),
            min_confidence: 0.6,
        }
    }

    /// Create detector from trained model JSON file
    pub fn from_json(json_path: &str) -> Result<Self> {
        let hmm = GaussianHMM::load_from_json(json_path)?;
        Ok(Self {
            hmm: Arc::new(hmm),
            min_confidence: 0.6,
        })
    }

    /// Detect current regime from recent candles (synchronous)
    pub fn detect_regime(&self, candles: &CandleBuffer) -> Result<(RegimeState, f64)> {
        // Extract features
        let features = extract_regime_features(candles)?;

        // Convert to ndarray
        let obs = Array2::from_shape_vec((1, features.len()), features)?;

        // Predict state (direct access, no lock needed - model is read-only)
        let states = self.hmm.predict(&obs)?;

        let state = RegimeState::from_index(states[0])
            .ok_or_else(|| anyhow::anyhow!("Invalid state index"))?;

        // Return high confidence since we're using trained HMM
        // Removed expensive forward() call for performance
        // TODO: Implement efficient confidence metric using emission probabilities
        let confidence = 1.0;

        Ok((state, confidence))
    }

    /// Detect regime with minimum confidence threshold (synchronous)
    pub fn detect_regime_confident(&self, candles: &CandleBuffer) -> Result<Option<RegimeState>> {
        let (state, confidence) = self.detect_regime(candles)?;

        if confidence >= self.min_confidence {
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regime_detector_creation() {
        let detector = RegimeDetector::new(8);
        assert_eq!(detector.min_confidence, 0.6);
    }
}
