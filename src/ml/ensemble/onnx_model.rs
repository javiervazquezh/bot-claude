use anyhow::Result;
use std::path::Path;

use crate::ml::TradeFeatures;

/// Single ONNX model wrapper for inference using ONNX Runtime
pub struct OnnxModel {
    session: ort::session::Session,
    name: String,
}

impl OnnxModel {
    /// Load an ONNX model from file
    pub fn load(path: impl AsRef<Path>, name: &str) -> Result<Self> {
        let session = ort::session::Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .commit_from_file(path.as_ref())?;

        Ok(Self {
            session,
            name: name.to_string(),
        })
    }

    /// Run inference, returning win probability
    pub fn predict(&mut self, features: &TradeFeatures) -> Result<f64> {
        let arr = features.to_array();
        let input: Vec<f32> = arr.iter().map(|&v| v as f32).collect();

        // Create input tensor from (shape, data) tuple
        let input_tensor = ort::value::Tensor::from_array(
            ([1usize, TradeFeatures::NUM_FEATURES], input.into_boxed_slice()),
        )?;

        let outputs = self.session.run(
            ort::inputs![input_tensor],
        )?;

        // ONNX classifiers from sklearn/xgboost output:
        //   output[0] = predicted label
        //   output[1] = probabilities [batch, n_classes]
        // We want probability of class 1 (win)
        if outputs.len() > 1 {
            let (shape, data) = outputs[1].try_extract_tensor::<f32>()?;
            let dims = &**shape;
            let win_prob = if dims.len() == 2 && dims[1] >= 2 {
                data[1] as f64  // [batch=0, class=1]
            } else {
                *data.last().unwrap_or(&0.5) as f64
            };
            Ok(win_prob)
        } else {
            let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;
            Ok(*data.first().unwrap_or(&0.5) as f64)
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onnx_model_missing_file() {
        let result = OnnxModel::load("/nonexistent/model.onnx", "test");
        assert!(result.is_err());
    }
}
