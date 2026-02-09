#![allow(dead_code)]
pub mod gaussian_hmm;
pub mod features;
pub mod regime_detector;

pub use gaussian_hmm::GaussianHMM;
pub use features::{extract_regime_features, extract_regime_features_batch};
pub use regime_detector::RegimeDetector;

/// Market regime states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegimeState {
    Bull = 0,
    Bear = 1,
    Neutral = 2,
}

impl RegimeState {
    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(RegimeState::Bull),
            1 => Some(RegimeState::Bear),
            2 => Some(RegimeState::Neutral),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            RegimeState::Bull => "Bull",
            RegimeState::Bear => "Bear",
            RegimeState::Neutral => "Neutral",
        }
    }
}
