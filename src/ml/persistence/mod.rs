use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing::{info, warn};

/// Model type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelType {
    LogisticRegression,
    XGBoost,
    RandomForest,
    LSTM,
    SAC,
}

impl ModelType {
    pub fn as_str(&self) -> &str {
        match self {
            ModelType::LogisticRegression => "logistic_regression",
            ModelType::XGBoost => "xgboost",
            ModelType::RandomForest => "random_forest",
            ModelType::LSTM => "lstm",
            ModelType::SAC => "sac",
        }
    }

    pub fn file_extension(&self) -> &str {
        match self {
            ModelType::LogisticRegression => "bin",
            ModelType::XGBoost => "json",
            ModelType::RandomForest => "bin",
            ModelType::LSTM => "onnx",
            ModelType::SAC => "onnx",
        }
    }
}

impl FromStr for ModelType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "logistic_regression" => Ok(ModelType::LogisticRegression),
            "xgboost" => Ok(ModelType::XGBoost),
            "random_forest" => Ok(ModelType::RandomForest),
            "lstm" => Ok(ModelType::LSTM),
            "sac" => Ok(ModelType::SAC),
            _ => Err(anyhow!("Unknown model type: {}", s)),
        }
    }
}

/// Semantic version for models
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ModelVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }

    pub fn initial() -> Self {
        Self::new(1, 0, 0)
    }

    pub fn bump_major(&self) -> Self {
        Self::new(self.major + 1, 0, 0)
    }

    pub fn bump_minor(&self) -> Self {
        Self::new(self.major, self.minor + 1, 0)
    }

    pub fn bump_patch(&self) -> Self {
        Self::new(self.major, self.minor, self.patch + 1)
    }

    pub fn as_string(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for ModelVersion {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid version format: {}", s));
        }

        Ok(Self {
            major: parts[0].parse()?,
            minor: parts[1].parse()?,
            patch: parts[2].parse()?,
        })
    }
}

impl std::fmt::Display for ModelVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Model metadata and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetrics {
    pub accuracy: Option<f64>,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub f1_score: Option<f64>,
    pub auc_roc: Option<f64>,
    pub train_samples: usize,
    pub test_samples: usize,
    pub walk_forward_folds: Option<usize>,
    pub overfitting_ratio: Option<f64>,
}

impl ModelMetrics {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

/// Model record from database
#[derive(Debug, Clone)]
pub struct ModelRecord {
    pub id: i64,
    pub model_type: ModelType,
    pub version: ModelVersion,
    pub trained_at: DateTime<Utc>,
    pub metrics: ModelMetrics,
    pub model_path: PathBuf,
    pub is_active: bool,
}

/// Model persistence manager
pub struct ModelPersistence {
    pool: SqlitePool,
    models_dir: PathBuf,
}

impl ModelPersistence {
    pub fn new(pool: SqlitePool, models_dir: impl Into<PathBuf>) -> Self {
        let models_dir = models_dir.into();
        Self { pool, models_dir }
    }

    /// Ensure models directory exists
    pub fn ensure_models_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.models_dir)?;
        Ok(())
    }

    /// Generate model file path
    fn model_path(&self, model_type: ModelType, version: &ModelVersion) -> PathBuf {
        let filename = format!(
            "{}_{}.{}",
            model_type.as_str(),
            version.as_string(),
            model_type.file_extension()
        );
        self.models_dir.join(filename)
    }

    /// Save a model to disk and database
    pub async fn save_model(
        &self,
        model_type: ModelType,
        version: ModelVersion,
        model_data: &[u8],
        metrics: ModelMetrics,
    ) -> Result<i64> {
        self.ensure_models_dir()?;

        let model_path = self.model_path(model_type, &version);

        // Write model file
        std::fs::write(&model_path, model_data)?;
        info!(
            "Saved {} model v{} to {}",
            model_type.as_str(),
            version,
            model_path.display()
        );

        // Insert into database
        let metrics_json = metrics.to_json()?;
        let result = sqlx::query(
            r#"
            INSERT INTO ml_models (model_type, version, trained_at, metrics_json, model_path, is_active)
            VALUES (?, ?, ?, ?, ?, 0)
            "#,
        )
        .bind(model_type.as_str())
        .bind(version.as_string())
        .bind(Utc::now().to_rfc3339())
        .bind(metrics_json)
        .bind(model_path.to_string_lossy().to_string())
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Load model data from disk
    pub async fn load_model(&self, model_type: ModelType, version: &ModelVersion) -> Result<Vec<u8>> {
        let model_path = self.model_path(model_type, version);

        if !model_path.exists() {
            return Err(anyhow!(
                "Model file not found: {}",
                model_path.display()
            ));
        }

        let data = std::fs::read(&model_path)?;
        info!(
            "Loaded {} model v{} from {}",
            model_type.as_str(),
            version,
            model_path.display()
        );

        Ok(data)
    }

    /// Load the active model for a given type
    pub async fn load_active_model(&self, model_type: ModelType) -> Result<Vec<u8>> {
        let record = self.get_active_model(model_type).await?
            .ok_or_else(|| anyhow!("No active model found for {}", model_type.as_str()))?;

        std::fs::read(&record.model_path)
            .map_err(|e| anyhow!("Failed to read model file: {}", e))
    }

    /// Get active model record
    pub async fn get_active_model(&self, model_type: ModelType) -> Result<Option<ModelRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, model_type, version, trained_at, metrics_json, model_path, is_active
            FROM ml_models
            WHERE model_type = ? AND is_active = 1
            ORDER BY trained_at DESC
            LIMIT 1
            "#,
        )
        .bind(model_type.as_str())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let version_str: String = row.get("version");
                let metrics_json: String = row.get("metrics_json");
                let trained_at_str: String = row.get("trained_at");

                Ok(Some(ModelRecord {
                    id: row.get("id"),
                    model_type: ModelType::from_str(row.get("model_type"))?,
                    version: ModelVersion::from_str(&version_str)?,
                    trained_at: DateTime::parse_from_rfc3339(&trained_at_str)?
                        .with_timezone(&Utc),
                    metrics: ModelMetrics::from_json(&metrics_json)?,
                    model_path: PathBuf::from(row.get::<String, _>("model_path")),
                    is_active: row.get::<i32, _>("is_active") == 1,
                }))
            }
            None => Ok(None),
        }
    }

    /// List all models of a given type
    pub async fn list_models(&self, model_type: ModelType) -> Result<Vec<ModelRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, model_type, version, trained_at, metrics_json, model_path, is_active
            FROM ml_models
            WHERE model_type = ?
            ORDER BY trained_at DESC
            "#,
        )
        .bind(model_type.as_str())
        .fetch_all(&self.pool)
        .await?;

        let mut models = Vec::new();
        for row in rows {
            let version_str: String = row.get("version");
            let metrics_json: String = row.get("metrics_json");
            let trained_at_str: String = row.get("trained_at");

            models.push(ModelRecord {
                id: row.get("id"),
                model_type: ModelType::from_str(row.get("model_type"))?,
                version: ModelVersion::from_str(&version_str)?,
                trained_at: DateTime::parse_from_rfc3339(&trained_at_str)?
                    .with_timezone(&Utc),
                metrics: ModelMetrics::from_json(&metrics_json)?,
                model_path: PathBuf::from(row.get::<String, _>("model_path")),
                is_active: row.get::<i32, _>("is_active") == 1,
            });
        }

        Ok(models)
    }

    /// Activate a model version (deactivates all others of same type)
    pub async fn activate_model(&self, model_id: i64) -> Result<()> {
        // Get model type
        let row = sqlx::query("SELECT model_type FROM ml_models WHERE id = ?")
            .bind(model_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| anyhow!("Model not found: {}", model_id))?;

        let model_type: String = row.get("model_type");

        // Deactivate all models of this type
        sqlx::query("UPDATE ml_models SET is_active = 0 WHERE model_type = ?")
            .bind(&model_type)
            .execute(&self.pool)
            .await?;

        // Activate the specified model
        sqlx::query("UPDATE ml_models SET is_active = 1 WHERE id = ?")
            .bind(model_id)
            .execute(&self.pool)
            .await?;

        info!(
            "Activated {} model ID {}",
            model_type, model_id
        );

        Ok(())
    }

    /// Get the latest version for a model type
    pub async fn get_latest_version(&self, model_type: ModelType) -> Result<Option<ModelVersion>> {
        let row = sqlx::query(
            r#"
            SELECT version FROM ml_models
            WHERE model_type = ?
            ORDER BY trained_at DESC
            LIMIT 1
            "#,
        )
        .bind(model_type.as_str())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let version_str: String = row.get("version");
                Ok(Some(ModelVersion::from_str(&version_str)?))
            }
            None => Ok(None),
        }
    }

    /// Suggest next version (defaults to patch bump)
    pub async fn suggest_next_version(&self, model_type: ModelType) -> Result<ModelVersion> {
        match self.get_latest_version(model_type).await? {
            Some(latest) => Ok(latest.bump_patch()),
            None => Ok(ModelVersion::initial()),
        }
    }

    /// Delete a model (file and database record)
    pub async fn delete_model(&self, model_id: i64) -> Result<()> {
        // Get model path first
        let row = sqlx::query("SELECT model_path, is_active FROM ml_models WHERE id = ?")
            .bind(model_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| anyhow!("Model not found: {}", model_id))?;

        let model_path: String = row.get("model_path");
        let is_active: i32 = row.get("is_active");

        if is_active == 1 {
            warn!("Attempting to delete active model ID {}", model_id);
            return Err(anyhow!("Cannot delete active model. Deactivate it first."));
        }

        // Delete file
        let path = PathBuf::from(model_path);
        if path.exists() {
            std::fs::remove_file(&path)?;
            info!("Deleted model file: {}", path.display());
        }

        // Delete database record
        sqlx::query("DELETE FROM ml_models WHERE id = ?")
            .bind(model_id)
            .execute(&self.pool)
            .await?;

        info!("Deleted model ID {} from database", model_id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_version_parsing() {
        let v = ModelVersion::from_str("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.as_string(), "1.2.3");
    }

    #[test]
    fn test_version_bumps() {
        let v = ModelVersion::new(1, 2, 3);
        assert_eq!(v.bump_major(), ModelVersion::new(2, 0, 0));
        assert_eq!(v.bump_minor(), ModelVersion::new(1, 3, 0));
        assert_eq!(v.bump_patch(), ModelVersion::new(1, 2, 4));
    }

    #[test]
    fn test_model_type_from_str() {
        assert_eq!(ModelType::from_str("xgboost").unwrap(), ModelType::XGBoost);
        assert_eq!(ModelType::from_str("lstm").unwrap(), ModelType::LSTM);
        assert!(ModelType::from_str("unknown").is_err());
    }

    #[test]
    fn test_model_type_extensions() {
        assert_eq!(ModelType::XGBoost.file_extension(), "json");
        assert_eq!(ModelType::LSTM.file_extension(), "onnx");
        assert_eq!(ModelType::RandomForest.file_extension(), "bin");
    }
}
