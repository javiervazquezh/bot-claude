use anyhow::{anyhow, Result};
use ndarray::{Array1, Array2, Axis};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::TradeFeatures;

/// Training report after model fit
#[derive(Debug, Clone)]
pub struct TrainingReport {
    pub samples: usize,
    pub accuracy: f64,
    pub wins_in_data: usize,
    pub losses_in_data: usize,
}

/// Model weights for persistence (logistic regression coefficients)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelWeights {
    coefficients: Vec<f64>,
    intercept: f64,
    feature_means: Vec<f64>,
    feature_stds: Vec<f64>,
}

/// ML trade predictor using logistic regression
/// Predicts whether a trade signal will result in a win or loss
pub struct TradePredictor {
    weights: Option<ModelWeights>,
    win_probability_threshold: f64,
    min_training_samples: usize,
    is_trained: bool,
}

impl TradePredictor {
    pub fn new() -> Self {
        Self {
            weights: None,
            win_probability_threshold: 0.55,
            min_training_samples: 30,
            is_trained: false,
        }
    }

    /// Train the model from historical features + outcomes
    pub fn train(&mut self, data: &[(TradeFeatures, bool)]) -> Result<TrainingReport> {
        let n = data.len();
        if n < self.min_training_samples {
            return Err(anyhow!("Not enough training samples: {} < {}", n, self.min_training_samples));
        }

        let num_features = TradeFeatures::NUM_FEATURES;
        let mut features = Array2::<f64>::zeros((n, num_features));
        let mut labels = Vec::with_capacity(n);

        for (i, (feat, outcome)) in data.iter().enumerate() {
            let arr = feat.to_array();
            for (j, &val) in arr.iter().enumerate() {
                features[[i, j]] = val;
            }
            labels.push(if *outcome { 1.0 } else { 0.0 });
        }

        // Compute feature means and stds for normalization
        let means = features.mean_axis(Axis(0)).unwrap();
        let stds = features.std_axis(Axis(0), 1.0);

        // Normalize features (z-score)
        let mut normalized = features.clone();
        for j in 0..num_features {
            let std = stds[j];
            if std > 1e-10 {
                for i in 0..n {
                    normalized[[i, j]] = (features[[i, j]] - means[j]) / std;
                }
            } else {
                for i in 0..n {
                    normalized[[i, j]] = 0.0;
                }
            }
        }

        // Train logistic regression using gradient descent
        let (coefficients, intercept) = self.fit_logistic_regression(&normalized, &labels, 1000, 0.01)?;

        // Calculate training accuracy
        let mut correct = 0;
        for i in 0..n {
            let mut z = intercept;
            for j in 0..num_features {
                z += coefficients[j] * normalized[[i, j]];
            }
            let prob = sigmoid(z);
            let predicted = prob >= 0.5;
            let actual = labels[i] >= 0.5;
            if predicted == actual { correct += 1; }
        }
        let accuracy = correct as f64 / n as f64;

        let wins = data.iter().filter(|(_, o)| *o).count();
        let losses = n - wins;

        self.weights = Some(ModelWeights {
            coefficients,
            intercept,
            feature_means: means.to_vec(),
            feature_stds: stds.to_vec(),
        });
        self.is_trained = true;

        info!("ML model trained: {} samples, {:.1}% accuracy, {}/{} wins",
            n, accuracy * 100.0, wins, n);

        Ok(TrainingReport { samples: n, accuracy, wins_in_data: wins, losses_in_data: losses })
    }

    /// Fit logistic regression via gradient descent
    fn fit_logistic_regression(
        &self,
        features: &Array2<f64>,
        labels: &[f64],
        max_iter: usize,
        learning_rate: f64,
    ) -> Result<(Vec<f64>, f64)> {
        let n = features.nrows();
        let num_features = features.ncols();

        let mut coefficients = vec![0.0; num_features];
        let mut intercept = 0.0;

        for _iter in 0..max_iter {
            let mut grad_coef = vec![0.0; num_features];
            let mut grad_intercept = 0.0;

            for i in 0..n {
                let mut z = intercept;
                for j in 0..num_features {
                    z += coefficients[j] * features[[i, j]];
                }
                let pred = sigmoid(z);
                let error = pred - labels[i];

                grad_intercept += error;
                for j in 0..num_features {
                    grad_coef[j] += error * features[[i, j]];
                }
            }

            // Update with L2 regularization (lambda = 0.01)
            let lambda = 0.01;
            intercept -= learning_rate * grad_intercept / n as f64;
            for j in 0..num_features {
                coefficients[j] -= learning_rate * (grad_coef[j] / n as f64 + lambda * coefficients[j]);
            }
        }

        Ok((coefficients, intercept))
    }

    /// Predict win probability for a new signal
    pub fn predict_win_probability(&self, features: &TradeFeatures) -> Option<f64> {
        let weights = self.weights.as_ref()?;
        let arr = features.to_array();
        let num_features = TradeFeatures::NUM_FEATURES;

        // Normalize using stored means/stds
        let mut z = weights.intercept;
        for j in 0..num_features {
            let std = weights.feature_stds[j];
            let normalized = if std > 1e-10 {
                (arr[j] - weights.feature_means[j]) / std
            } else {
                0.0
            };
            z += weights.coefficients[j] * normalized;
        }

        Some(sigmoid(z))
    }

    /// Should we take this trade?
    pub fn should_trade(&self, features: &TradeFeatures) -> bool {
        if !self.is_trained {
            return true; // Pass-through until model is trained
        }
        match self.predict_win_probability(features) {
            Some(prob) => {
                debug!("ML prediction: win_prob={:.2}%, threshold={:.2}%",
                    prob * 100.0, self.win_probability_threshold * 100.0);
                prob >= self.win_probability_threshold
            }
            None => true,
        }
    }

    pub fn is_trained(&self) -> bool {
        self.is_trained
    }

    /// Serialize model to JSON string for persistence
    pub fn save_to_json(&self) -> Result<String> {
        match &self.weights {
            Some(w) => Ok(serde_json::to_string(w)?),
            None => Err(anyhow!("No model to save")),
        }
    }

    /// Load model from JSON string
    pub fn load_from_json(json: &str) -> Result<Self> {
        let weights: ModelWeights = serde_json::from_str(json)?;
        Ok(Self {
            weights: Some(weights),
            win_probability_threshold: 0.55,
            min_training_samples: 30,
            is_trained: true,
        })
    }
}

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}
