#!/usr/bin/env python3
"""
Train XGBoost + Random Forest ensemble for trade win probability prediction.
Exports models as ONNX for inference in Rust via tract.

Usage:
    python train_ensemble.py --data training_data.csv --output ../models/
"""

import argparse
import os
import sys

import numpy as np
import pandas as pd
from sklearn.ensemble import RandomForestClassifier
from sklearn.metrics import (
    accuracy_score,
    classification_report,
    roc_auc_score,
)
from sklearn.model_selection import TimeSeriesSplit
from sklearn.preprocessing import StandardScaler
import xgboost as xgb

# ONNX export
import onnxmltools
from onnxmltools.convert.common.data_types import FloatTensorType
from skl2onnx import convert_sklearn

FEATURE_COLUMNS = [
    "signal_strength",
    "confidence",
    "risk_reward_ratio",
    "rsi_14",
    "atr_pct",
    "ema_spread_pct",
    "bb_position",
    "price_vs_200ema",
    "volume_ratio",
    "volatility_regime",
    "recent_win_rate",
    "recent_avg_pnl_pct",
    "streak",
    "hour_of_day",
    "day_of_week",
    "pair_id",
    "ob_spread_pct",
    "ob_depth_imbalance",
    "ob_mid_price_momentum",
    "ob_spread_volatility",
    "ob_book_pressure",
    "ob_weighted_spread",
    "ob_best_volume_ratio",
    "ob_depth_ratio",
]

TARGET_COLUMN = "win"


def load_data(path: str) -> pd.DataFrame:
    """Load training data CSV exported from Rust backtest."""
    df = pd.read_csv(path)
    print(f"Loaded {len(df)} trades from {path}")
    print(f"  Wins:   {df[TARGET_COLUMN].sum()} ({df[TARGET_COLUMN].mean()*100:.1f}%)")
    print(f"  Losses: {(1 - df[TARGET_COLUMN]).sum()} ({(1 - df[TARGET_COLUMN]).mean()*100:.1f}%)")
    return df


def walk_forward_evaluate(X, y, model_fn, n_splits=5):
    """Walk-forward cross-validation for time-series data."""
    tscv = TimeSeriesSplit(n_splits=n_splits)
    metrics = {"accuracy": [], "auc": [], "train_acc": []}

    for fold, (train_idx, test_idx) in enumerate(tscv.split(X)):
        X_train, X_test = X[train_idx], X[test_idx]
        y_train, y_test = y[train_idx], y[test_idx]

        model = model_fn()
        model.fit(X_train, y_train)

        # Training metrics
        train_pred = model.predict(X_train)
        train_acc = accuracy_score(y_train, train_pred)

        # Test metrics
        test_pred = model.predict(X_test)
        test_acc = accuracy_score(y_test, test_pred)

        try:
            test_proba = model.predict_proba(X_test)[:, 1]
            test_auc = roc_auc_score(y_test, test_proba)
        except Exception:
            test_auc = 0.5

        metrics["train_acc"].append(train_acc)
        metrics["accuracy"].append(test_acc)
        metrics["auc"].append(test_auc)

        print(
            f"  Fold {fold+1}: train_acc={train_acc:.3f}, "
            f"test_acc={test_acc:.3f}, test_auc={test_auc:.3f}"
        )

    avg_train = np.mean(metrics["train_acc"])
    avg_test = np.mean(metrics["accuracy"])
    avg_auc = np.mean(metrics["auc"])
    overfit_ratio = avg_train / avg_test if avg_test > 0 else float("inf")

    print(f"  Average: train_acc={avg_train:.3f}, test_acc={avg_test:.3f}, auc={avg_auc:.3f}")
    print(f"  Overfitting ratio: {overfit_ratio:.3f} (ideal ~1.0, concern >1.15)")

    return metrics, overfit_ratio


def train_xgboost(X_train, y_train, X_test, y_test):
    """Train XGBoost classifier with regularization."""
    print("\n=== Training XGBoost ===")

    model = xgb.XGBClassifier(
        max_depth=4,
        n_estimators=100,
        learning_rate=0.05,
        reg_alpha=0.1,       # L1 regularization
        reg_lambda=1.0,      # L2 regularization
        min_child_weight=5,
        subsample=0.8,
        colsample_bytree=0.8,
        use_label_encoder=False,
        eval_metric="logloss",
        random_state=42,
    )

    model.fit(
        X_train,
        y_train,
        eval_set=[(X_test, y_test)],
        verbose=False,
    )

    # Evaluate
    pred = model.predict(X_test)
    proba = model.predict_proba(X_test)[:, 1]
    acc = accuracy_score(y_test, pred)
    auc = roc_auc_score(y_test, proba)

    print(f"  Test accuracy: {acc:.3f}")
    print(f"  Test AUC: {auc:.3f}")
    print(f"  Feature importance (top 5):")
    importances = model.feature_importances_
    top_idx = np.argsort(importances)[::-1][:5]
    for idx in top_idx:
        print(f"    {FEATURE_COLUMNS[idx]}: {importances[idx]:.3f}")

    return model, {"accuracy": acc, "auc": auc}


def train_random_forest(X_train, y_train, X_test, y_test):
    """Train Random Forest classifier with balanced classes."""
    print("\n=== Training Random Forest ===")

    model = RandomForestClassifier(
        n_estimators=100,
        max_depth=6,
        min_samples_split=10,
        min_samples_leaf=5,
        class_weight="balanced",
        random_state=42,
    )

    model.fit(X_train, y_train)

    # Evaluate
    pred = model.predict(X_test)
    proba = model.predict_proba(X_test)[:, 1]
    acc = accuracy_score(y_test, pred)
    auc = roc_auc_score(y_test, proba)

    print(f"  Test accuracy: {acc:.3f}")
    print(f"  Test AUC: {auc:.3f}")
    print(f"  Feature importance (top 5):")
    importances = model.feature_importances_
    top_idx = np.argsort(importances)[::-1][:5]
    for idx in top_idx:
        print(f"    {FEATURE_COLUMNS[idx]}: {importances[idx]:.3f}")

    return model, {"accuracy": acc, "auc": auc}


def export_xgboost_onnx(model, output_path, n_features):
    """Export XGBoost model to ONNX."""
    print(f"\n  Exporting XGBoost to {output_path}")
    initial_type = [("float_input", FloatTensorType([None, n_features]))]
    onnx_model = onnxmltools.convert_xgboost(model, initial_types=initial_type)
    onnxmltools.utils.save_model(onnx_model, output_path)
    size_kb = os.path.getsize(output_path) / 1024
    print(f"  Saved: {size_kb:.1f} KB")


def export_rf_onnx(model, output_path, n_features):
    """Export Random Forest model to ONNX."""
    print(f"  Exporting Random Forest to {output_path}")
    initial_type = [("float_input", FloatTensorType([None, n_features]))]
    onnx_model = convert_sklearn(model, initial_types=initial_type)
    with open(output_path, "wb") as f:
        f.write(onnx_model.SerializeToString())
    size_kb = os.path.getsize(output_path) / 1024
    print(f"  Saved: {size_kb:.1f} KB")


def verify_onnx(model_path, X_sample, expected_proba, model_name):
    """Verify ONNX model produces same predictions as original."""
    import onnxruntime as ort

    sess = ort.InferenceSession(model_path)
    input_name = sess.get_inputs()[0].name
    X_float = X_sample[:5].astype(np.float32)
    result = sess.run(None, {input_name: X_float})

    # Probabilities are typically in result[1]
    if len(result) > 1:
        onnx_proba = result[1]
        if isinstance(onnx_proba, list):
            # sklearn exports as list of dicts
            onnx_proba = np.array([[d[0], d[1]] for d in onnx_proba])
        onnx_win_prob = onnx_proba[:, 1] if onnx_proba.ndim == 2 else onnx_proba
    else:
        onnx_win_prob = result[0].flatten()

    orig_win_prob = expected_proba[:5]
    max_diff = np.max(np.abs(onnx_win_prob - orig_win_prob))
    print(f"  {model_name} ONNX verification: max_diff={max_diff:.6f} {'OK' if max_diff < 0.01 else 'MISMATCH!'}")


def main():
    parser = argparse.ArgumentParser(description="Train ML ensemble for trade prediction")
    parser.add_argument("--data", required=True, help="Path to training_data.csv")
    parser.add_argument("--output", required=True, help="Output directory for ONNX models")
    parser.add_argument("--folds", type=int, default=5, help="Walk-forward CV folds")
    args = parser.parse_args()

    os.makedirs(args.output, exist_ok=True)

    # Load data
    df = load_data(args.data)

    if len(df) < 50:
        print(f"\nWARNING: Only {len(df)} trades. Results may be unreliable.")
        print("Consider using a longer backtest period.")

    X = df[FEATURE_COLUMNS].values.astype(np.float32)
    y = df[TARGET_COLUMN].values.astype(int)
    n_features = len(FEATURE_COLUMNS)

    # Replace NaN/inf with 0
    X = np.nan_to_num(X, nan=0.0, posinf=0.0, neginf=0.0)

    # Walk-forward validation
    print("\n=== Walk-Forward Validation: XGBoost ===")
    xgb_fn = lambda: xgb.XGBClassifier(
        max_depth=4, n_estimators=100, learning_rate=0.05,
        reg_alpha=0.1, reg_lambda=1.0, min_child_weight=5,
        subsample=0.8, colsample_bytree=0.8,
        use_label_encoder=False, eval_metric="logloss", random_state=42,
    )
    xgb_metrics, xgb_overfit = walk_forward_evaluate(X, y, xgb_fn, args.folds)

    print("\n=== Walk-Forward Validation: Random Forest ===")
    rf_fn = lambda: RandomForestClassifier(
        n_estimators=100, max_depth=6, min_samples_split=10,
        min_samples_leaf=5, class_weight="balanced", random_state=42,
    )
    rf_metrics, rf_overfit = walk_forward_evaluate(X, y, rf_fn, args.folds)

    # Train final models on 80/20 split (last 20% as holdout)
    split_idx = int(len(X) * 0.8)
    X_train, X_test = X[:split_idx], X[split_idx:]
    y_train, y_test = y[:split_idx], y[split_idx:]

    print(f"\nFinal training: {len(X_train)} train, {len(X_test)} test")

    xgb_model, xgb_final = train_xgboost(X_train, y_train, X_test, y_test)
    rf_model, rf_final = train_random_forest(X_train, y_train, X_test, y_test)

    # Weighted ensemble evaluation
    print("\n=== Ensemble Evaluation (60% XGB + 40% RF) ===")
    xgb_proba = xgb_model.predict_proba(X_test)[:, 1]
    rf_proba = rf_model.predict_proba(X_test)[:, 1]
    ensemble_proba = 0.6 * xgb_proba + 0.4 * rf_proba
    ensemble_pred = (ensemble_proba >= 0.55).astype(int)
    ensemble_acc = accuracy_score(y_test, ensemble_pred)
    ensemble_auc = roc_auc_score(y_test, ensemble_proba)
    print(f"  Ensemble accuracy (threshold=0.55): {ensemble_acc:.3f}")
    print(f"  Ensemble AUC: {ensemble_auc:.3f}")
    print(classification_report(y_test, ensemble_pred, target_names=["Loss", "Win"]))

    # Export to ONNX
    print("=== Exporting ONNX Models ===")
    xgb_path = os.path.join(args.output, "xgboost.onnx")
    rf_path = os.path.join(args.output, "random_forest.onnx")

    export_xgboost_onnx(xgb_model, xgb_path, n_features)
    export_rf_onnx(rf_model, rf_path, n_features)

    # Verify ONNX round-trip
    print("\n=== ONNX Verification ===")
    verify_onnx(xgb_path, X_test, xgb_proba, "XGBoost")
    verify_onnx(rf_path, X_test, rf_proba, "RandomForest")

    # Summary
    print("\n" + "=" * 60)
    print("TRAINING COMPLETE")
    print("=" * 60)
    print(f"Models saved to: {args.output}")
    print(f"  xgboost.onnx:       AUC={xgb_final['auc']:.3f}, Acc={xgb_final['accuracy']:.3f}")
    print(f"  random_forest.onnx: AUC={rf_final['auc']:.3f}, Acc={rf_final['accuracy']:.3f}")
    print(f"  Ensemble:           AUC={ensemble_auc:.3f}, Acc={ensemble_acc:.3f}")
    print(f"\nWalk-forward overfitting:")
    print(f"  XGBoost: {xgb_overfit:.3f}")
    print(f"  RF:      {rf_overfit:.3f}")
    print(f"\nUsage:")
    print(f"  cargo run -- backtest -s 2024-01-01 -e 2026-02-08 --ensemble {args.output}")


if __name__ == "__main__":
    main()
