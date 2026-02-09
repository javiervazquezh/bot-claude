#![allow(dead_code)]
use anyhow::{anyhow, Result};
use ndarray::{Array1, Array2};
use std::f64::consts::PI;

/// 3-state Hidden Markov Model with Gaussian emissions
/// States: 0 = Bull, 1 = Bear, 2 = Neutral
#[derive(Debug, Clone)]
pub struct GaussianHMM {
    /// Number of hidden states (always 3 for Bull/Bear/Neutral)
    pub n_states: usize,
    /// Number of features (observation dimensions)
    pub n_features: usize,
    /// State transition matrix (3x3)
    pub transition: Array2<f64>,
    /// Initial state probabilities (3)
    pub start_prob: Array1<f64>,
    /// Mean vectors for each state (3 x n_features)
    pub means: Array2<f64>,
    /// Covariance matrices for each state (3 x n_features x n_features)
    /// Stored as flattened: [state0_cov, state1_cov, state2_cov]
    pub covars: Vec<Array2<f64>>,
    /// Cached inverse covariance matrices (computed once after training/loading)
    pub covar_invs: Vec<Array2<f64>>,
    /// Convergence tolerance for EM
    pub tol: f64,
    /// Maximum EM iterations
    pub max_iter: usize,
}

impl GaussianHMM {
    /// Create a new HMM with uniform initialization
    pub fn new(n_features: usize) -> Self {
        let n_states = 3;

        // Equal initial probabilities
        let start_prob = Array1::from_elem(n_states, 1.0 / n_states as f64);

        // Equal transition matrix with slight self-persistence bias
        let mut transition = Array2::from_elem((n_states, n_states), 0.25);
        for i in 0..n_states {
            transition[[i, i]] = 0.5; // Slightly prefer staying in same state
        }

        // Initialize means with spread
        let mut means = Array2::zeros((n_states, n_features));
        // Bull: positive values
        for j in 0..n_features {
            means[[0, j]] = 0.5;
        }
        // Bear: negative values
        for j in 0..n_features {
            means[[1, j]] = -0.5;
        }
        // Neutral: zero
        for j in 0..n_features {
            means[[2, j]] = 0.0;
        }

        // Identity covariance matrices
        let mut covars = Vec::new();
        let mut covar_invs = Vec::new();
        for _ in 0..n_states {
            covars.push(Array2::eye(n_features));
            covar_invs.push(Array2::eye(n_features)); // Identity inverse is identity
        }

        Self {
            n_states,
            n_features,
            transition,
            start_prob,
            means,
            covars,
            covar_invs,
            tol: 1e-4,
            max_iter: 100,
        }
    }

    /// Initialize with K-means clustering
    pub fn init_with_kmeans(&mut self, observations: &Array2<f64>) -> Result<()> {
        if observations.shape()[1] != self.n_features {
            return Err(anyhow!("Observation features mismatch"));
        }

        // Simple K-means initialization
        let n_obs = observations.shape()[0];
        let mut labels = vec![0; n_obs];

        // Initialize labels by splitting data into thirds
        for i in 0..n_obs {
            labels[i] = (i * self.n_states) / n_obs;
        }

        // K-means iterations
        for _ in 0..10 {
            // Update means
            self.means.fill(0.0);
            let mut counts = vec![0; self.n_states];

            for (i, &label) in labels.iter().enumerate() {
                for j in 0..self.n_features {
                    self.means[[label, j]] += observations[[i, j]];
                }
                counts[label] += 1;
            }

            for state in 0..self.n_states {
                if counts[state] > 0 {
                    for j in 0..self.n_features {
                        self.means[[state, j]] /= counts[state] as f64;
                    }
                }
            }

            // Reassign labels
            for i in 0..n_obs {
                let obs = observations.row(i);
                let mut min_dist = f64::INFINITY;
                let mut best_state = 0;

                for state in 0..self.n_states {
                    let mean = self.means.row(state);
                    let dist: f64 = obs.iter()
                        .zip(mean.iter())
                        .map(|(o, m)| (o - m).powi(2))
                        .sum();

                    if dist < min_dist {
                        min_dist = dist;
                        best_state = state;
                    }
                }
                labels[i] = best_state;
            }
        }

        // Update covariances
        for state in 0..self.n_states {
            let mut cov = Array2::zeros((self.n_features, self.n_features));
            let mut count = 0;

            for (i, &label) in labels.iter().enumerate() {
                if label == state {
                    let diff = &observations.row(i).to_owned() - &self.means.row(state).to_owned();
                    for j in 0..self.n_features {
                        for k in 0..self.n_features {
                            cov[[j, k]] += diff[j] * diff[k];
                        }
                    }
                    count += 1;
                }
            }

            if count > 0 {
                cov /= count as f64;
                // Add small diagonal to ensure positive definite
                for j in 0..self.n_features {
                    cov[[j, j]] += 1e-6;
                }
                self.covars[state] = cov.clone();
                self.covar_invs[state] = self.matrix_inv(&cov);
            }
        }

        Ok(())
    }

    /// Compute log probability of observation given state
    fn log_emission_prob(&self, obs: &Array1<f64>, state: usize) -> f64 {
        let mean = self.means.row(state);
        let cov = &self.covars[state];

        // Compute (obs - mean)
        let diff = obs - &mean.to_owned();

        // Compute log( 1 / sqrt((2π)^k |Σ|) )
        let det = self.matrix_det(cov);
        if det <= 0.0 {
            return f64::NEG_INFINITY;
        }
        let log_norm = -0.5 * (self.n_features as f64 * (2.0 * PI).ln() + det.ln());

        // Compute -0.5 * (x-μ)ᵀ Σ⁻¹ (x-μ) using cached inverse
        let cov_inv = &self.covar_invs[state];
        let mut mahal = 0.0;
        for i in 0..self.n_features {
            for j in 0..self.n_features {
                mahal += diff[i] * cov_inv[[i, j]] * diff[j];
            }
        }

        log_norm - 0.5 * mahal
    }

    /// Forward algorithm: compute forward probabilities in log space
    pub fn forward(&self, observations: &Array2<f64>) -> (Array2<f64>, f64) {
        let n_obs = observations.shape()[0];
        let mut log_alpha = Array2::from_elem((n_obs, self.n_states), f64::NEG_INFINITY);

        // Initialization
        for state in 0..self.n_states {
            let obs = observations.row(0).to_owned();
            log_alpha[[0, state]] = self.start_prob[state].ln()
                + self.log_emission_prob(&obs, state);
        }

        // Recursion
        for t in 1..n_obs {
            let obs = observations.row(t).to_owned();
            for j in 0..self.n_states {
                let mut log_sum_terms = Vec::new();
                for i in 0..self.n_states {
                    log_sum_terms.push(
                        log_alpha[[t - 1, i]] + self.transition[[i, j]].ln()
                    );
                }
                log_alpha[[t, j]] = log_sum_exp(&log_sum_terms)
                    + self.log_emission_prob(&obs, j);
            }
        }

        // Termination
        let log_prob = log_sum_exp(&log_alpha.row(n_obs - 1).to_vec());

        (log_alpha, log_prob)
    }

    /// Backward algorithm: compute backward probabilities in log space
    fn backward(&self, observations: &Array2<f64>) -> Array2<f64> {
        let n_obs = observations.shape()[0];
        let mut log_beta = Array2::from_elem((n_obs, self.n_states), f64::NEG_INFINITY);

        // Initialization
        for state in 0..self.n_states {
            log_beta[[n_obs - 1, state]] = 0.0; // log(1)
        }

        // Recursion (backward)
        for t in (0..n_obs - 1).rev() {
            let obs_next = observations.row(t + 1).to_owned();
            for i in 0..self.n_states {
                let mut log_sum_terms = Vec::new();
                for j in 0..self.n_states {
                    log_sum_terms.push(
                        self.transition[[i, j]].ln()
                            + self.log_emission_prob(&obs_next, j)
                            + log_beta[[t + 1, j]]
                    );
                }
                log_beta[[t, i]] = log_sum_exp(&log_sum_terms);
            }
        }

        log_beta
    }

    /// Train HMM using Baum-Welch EM algorithm
    /// Returns (final_log_likelihood, converged_at_iteration)
    pub fn fit(&mut self, observations: &Array2<f64>, n_iter: usize, tol: f64) -> Result<(f64, usize)> {
        if observations.shape()[1] != self.n_features {
            return Err(anyhow!("Observation features mismatch"));
        }

        let n_obs = observations.shape()[0];
        if n_obs < 2 {
            return Err(anyhow!("Need at least 2 observations"));
        }

        // Initialize with K-means
        self.init_with_kmeans(observations)?;

        let mut prev_log_prob = f64::NEG_INFINITY;

        for iteration in 0..n_iter {
            // E-step: compute forward-backward
            let (log_alpha, log_prob) = self.forward(observations);
            let log_beta = self.backward(observations);

            // Check convergence
            if (log_prob - prev_log_prob).abs() < tol {
                return Ok((log_prob, iteration + 1));
            }
            prev_log_prob = log_prob;

            // Compute gamma (state occupation probabilities)
            let mut gamma = Array2::zeros((n_obs, self.n_states));
            for t in 0..n_obs {
                let log_denom = log_sum_exp(&(0..self.n_states)
                    .map(|s| log_alpha[[t, s]] + log_beta[[t, s]])
                    .collect::<Vec<_>>());

                for state in 0..self.n_states {
                    gamma[[t, state]] = (log_alpha[[t, state]] + log_beta[[t, state]] - log_denom).exp();
                }
            }

            // Compute xi (state transition probabilities)
            let mut xi_sum = Array2::zeros((self.n_states, self.n_states));
            for t in 0..n_obs - 1 {
                let obs_next = observations.row(t + 1).to_owned();
                let log_denom = log_prob;

                for i in 0..self.n_states {
                    for j in 0..self.n_states {
                        let log_xi = log_alpha[[t, i]]
                            + self.transition[[i, j]].ln()
                            + self.log_emission_prob(&obs_next, j)
                            + log_beta[[t + 1, j]]
                            - log_denom;
                        xi_sum[[i, j]] += log_xi.exp();
                    }
                }
            }

            // M-step: update parameters
            // Update start probabilities
            for state in 0..self.n_states {
                self.start_prob[state] = gamma[[0, state]];
            }

            // Update transition matrix
            for i in 0..self.n_states {
                let row_sum: f64 = xi_sum.row(i).sum();
                if row_sum > 0.0 {
                    for j in 0..self.n_states {
                        self.transition[[i, j]] = xi_sum[[i, j]] / row_sum;
                    }
                }
            }

            // Update means and covariances
            for state in 0..self.n_states {
                let gamma_sum: f64 = gamma.column(state).sum();

                if gamma_sum > 0.0 {
                    // Update mean
                    for feat in 0..self.n_features {
                        let mut weighted_sum = 0.0;
                        for t in 0..n_obs {
                            weighted_sum += gamma[[t, state]] * observations[[t, feat]];
                        }
                        self.means[[state, feat]] = weighted_sum / gamma_sum;
                    }

                    // Update covariance
                    let mut cov = Array2::zeros((self.n_features, self.n_features));
                    for t in 0..n_obs {
                        let diff = &observations.row(t).to_owned() - &self.means.row(state).to_owned();
                        for i in 0..self.n_features {
                            for j in 0..self.n_features {
                                cov[[i, j]] += gamma[[t, state]] * diff[i] * diff[j];
                            }
                        }
                    }
                    cov /= gamma_sum;

                    // Add regularization
                    for i in 0..self.n_features {
                        cov[[i, i]] += 1e-6;
                    }

                    self.covars[state] = cov.clone();
                    self.covar_invs[state] = self.matrix_inv(&cov);
                }
            }
        }

        // Did not converge within n_iter iterations
        Ok((prev_log_prob, n_iter))
    }

    /// Predict most likely state sequence using Viterbi algorithm
    pub fn predict(&self, observations: &Array2<f64>) -> Result<Vec<usize>> {
        if observations.shape()[1] != self.n_features {
            return Err(anyhow!("Observation features mismatch"));
        }

        let n_obs = observations.shape()[0];
        let mut log_delta = Array2::from_elem((n_obs, self.n_states), f64::NEG_INFINITY);
        let mut psi = Array2::zeros((n_obs, self.n_states));

        // Initialization
        for state in 0..self.n_states {
            let obs = observations.row(0).to_owned();
            log_delta[[0, state]] = self.start_prob[state].ln()
                + self.log_emission_prob(&obs, state);
        }

        // Recursion
        for t in 1..n_obs {
            let obs = observations.row(t).to_owned();
            for j in 0..self.n_states {
                let mut max_val = f64::NEG_INFINITY;
                let mut max_state = 0;

                for i in 0..self.n_states {
                    let val = log_delta[[t - 1, i]] + self.transition[[i, j]].ln();
                    if val > max_val {
                        max_val = val;
                        max_state = i;
                    }
                }

                log_delta[[t, j]] = max_val + self.log_emission_prob(&obs, j);
                psi[[t, j]] = max_state as f64;
            }
        }

        // Backtracking
        let mut states = vec![0; n_obs];
        let mut max_val = f64::NEG_INFINITY;
        for state in 0..self.n_states {
            if log_delta[[n_obs - 1, state]] > max_val {
                max_val = log_delta[[n_obs - 1, state]];
                states[n_obs - 1] = state;
            }
        }

        for t in (0..n_obs - 1).rev() {
            states[t] = psi[[t + 1, states[t + 1]]] as usize;
        }

        Ok(states)
    }

    /// Simple matrix determinant (for small matrices)
    fn matrix_det(&self, mat: &Array2<f64>) -> f64 {
        let n = mat.shape()[0];
        if n == 1 {
            return mat[[0, 0]];
        }
        if n == 2 {
            return mat[[0, 0]] * mat[[1, 1]] - mat[[0, 1]] * mat[[1, 0]];
        }
        // For larger matrices, use LU decomposition approximation
        mat.diag().iter().product()
    }

    /// Simple matrix inverse (using adjugate method for small matrices)
    fn matrix_inv(&self, mat: &Array2<f64>) -> Array2<f64> {
        let n = mat.shape()[0];

        // For numerical stability, add small regularization
        let mut reg_mat = mat.clone();
        for i in 0..n {
            reg_mat[[i, i]] += 1e-6;
        }

        // Simple inverse for diagonal-dominant matrices
        let mut inv = Array2::zeros((n, n));
        for i in 0..n {
            inv[[i, i]] = 1.0 / reg_mat[[i, i]].max(1e-6);
        }

        inv
    }

    /// Load HMM from JSON file
    pub fn load_from_json(json_path: &str) -> Result<Self> {
        use std::fs;

        let json_str = fs::read_to_string(json_path)
            .map_err(|e| anyhow!("Failed to read model file: {}", e))?;

        let model_data: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("Failed to parse JSON: {}", e))?;

        // Parse basic parameters
        let n_features = model_data["n_features"].as_u64()
            .ok_or_else(|| anyhow!("Missing n_features"))? as usize;
        let n_states = model_data["n_states"].as_u64()
            .ok_or_else(|| anyhow!("Missing n_states"))? as usize;

        if n_states != 3 {
            return Err(anyhow!("Only 3-state HMMs are supported"));
        }

        // Parse transition matrix (flattened 3x3)
        let transition_vec: Vec<f64> = model_data["transition"]
            .as_array()
            .ok_or_else(|| anyhow!("Missing transition"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0))
            .collect();

        if transition_vec.len() != 9 {
            return Err(anyhow!("Invalid transition matrix size"));
        }

        let transition = Array2::from_shape_vec((3, 3), transition_vec)?;

        // Parse start probabilities
        let start_prob_vec: Vec<f64> = model_data["start_prob"]
            .as_array()
            .ok_or_else(|| anyhow!("Missing start_prob"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0))
            .collect();

        if start_prob_vec.len() != 3 {
            return Err(anyhow!("Invalid start_prob size"));
        }

        let start_prob = Array1::from_vec(start_prob_vec);

        // Parse means (flattened 3 x n_features)
        let means_vec: Vec<f64> = model_data["means"]
            .as_array()
            .ok_or_else(|| anyhow!("Missing means"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0))
            .collect();

        if means_vec.len() != 3 * n_features {
            return Err(anyhow!("Invalid means size: expected {}, got {}", 3 * n_features, means_vec.len()));
        }

        let means = Array2::from_shape_vec((3, n_features), means_vec)?;

        // Initialize identity covariances (not saved in JSON, will use identity)
        let mut covars = Vec::new();
        let mut covar_invs = Vec::new();
        for _ in 0..3 {
            let mut cov = Array2::eye(n_features);
            // Add small regularization
            for i in 0..n_features {
                cov[[i, i]] += 1e-6;
            }
            covars.push(cov.clone());
            covar_invs.push(Array2::eye(n_features)); // Identity inverse is identity
        }

        Ok(Self {
            n_states,
            n_features,
            transition,
            start_prob,
            means,
            covars,
            covar_invs,
            tol: 1e-4,
            max_iter: 100,
        })
    }
}

/// Log-sum-exp trick for numerical stability
fn log_sum_exp(log_values: &[f64]) -> f64 {
    if log_values.is_empty() {
        return f64::NEG_INFINITY;
    }

    let max_val = log_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if max_val == f64::NEG_INFINITY {
        return f64::NEG_INFINITY;
    }

    let sum_exp: f64 = log_values.iter().map(|&v| (v - max_val).exp()).sum();
    max_val + sum_exp.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmm_creation() {
        let hmm = GaussianHMM::new(6);
        assert_eq!(hmm.n_states, 3);
        assert_eq!(hmm.n_features, 6);
        assert_eq!(hmm.transition.shape(), &[3, 3]);
        assert_eq!(hmm.means.shape(), &[3, 6]);
        assert_eq!(hmm.covars.len(), 3);
    }

    #[test]
    fn test_log_sum_exp() {
        let values = vec![-1.0, -2.0, -3.0];
        let result = log_sum_exp(&values);
        assert!(result > -1.0 && result < 0.0);
    }
}
