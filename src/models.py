# SPDX-License-Identifier: Apache-2.0
"""Fit comparable regressors and select by acquisition-batch holdout MAE."""

import logging
import platform
from datetime import UTC, datetime
from importlib.metadata import version
from time import perf_counter
from typing import Any

import joblib
import numpy as np
import pandas as pd
from sklearn.base import RegressorMixin
from sklearn.dummy import DummyRegressor
from sklearn.ensemble import GradientBoostingRegressor, RandomForestRegressor
from sklearn.linear_model import LinearRegression
from sklearn.metrics import mean_absolute_error, r2_score, root_mean_squared_error
from sklearn.model_selection import GroupKFold, cross_val_score

from src.common import DATA, MODELS, RESULTS, configuration, digest, write_json
from src.preprocessing import TABLE, TARGET

LOG = logging.getLogger(__name__)
GROUP = "acquisition_batch_id"


def partitions(frame: pd.DataFrame, config: dict[str, Any]) -> dict[str, pd.DataFrame]:
    """Assign complete batches to exclusive splits and check recording provenance."""
    validation = set(config["validation_batches"])
    test = set(config["test_batches"])
    if validation & test or not (validation | test) <= set(frame[GROUP]):
        raise ValueError("Validation and test batches must exist and be disjoint")
    membership = np.select(
        [frame[GROUP].isin(validation), frame[GROUP].isin(test)],
        ["validation", "test"],
        default="train",
    )
    split = frame.assign(split=membership)
    for key in (GROUP, "session_id", "recording_id", "sha256"):
        if split[key].isna().any() or split.groupby(key)["split"].nunique().gt(1).any():
            raise ValueError(f"Missing provenance or leakage across splits: {key}")
        if split.groupby(key)[GROUP].nunique().gt(1).any():
            raise ValueError(f"Leakage across acquisition batches: {key}")
    result = {
        name: split[split["split"] == name].copy() for name in ("train", "validation", "test")
    }
    if any(part.empty for part in result.values()):
        raise ValueError("All three partitions must contain recordings")
    return result


def estimators(cfg: dict[str, Any]) -> dict[str, RegressorMixin]:
    """Build all fixed-hyperparameter models."""
    return {
        "baseline": DummyRegressor(strategy="median"),
        "linear": LinearRegression(),
        "random_forest": RandomForestRegressor(
            random_state=cfg["seed"], n_jobs=1, **cfg["random_forest"]
        ),
        "gradient_boosting": GradientBoostingRegressor(
            random_state=cfg["seed"], **cfg["gradient_boosting"]
        ),
    }


def scores(actual: Any, predicted: Any) -> dict[str, float | None]:
    """Calculate errors in dBFS and R-squared when the response varies."""
    return {
        "mae": float(mean_absolute_error(actual, predicted)),
        "rmse": float(root_mean_squared_error(actual, predicted)),
        "r2": float(r2_score(actual, predicted)) if np.ptp(actual) else None,
    }


def fit_models(
    split: dict[str, pd.DataFrame], cfg: dict[str, Any]
) -> tuple[dict[str, RegressorMixin], dict[str, Any]]:
    """Fit on training only, audit GroupKFold errors, then score validation."""
    training, validation = split["train"], split["validation"]
    features = cfg["features"]
    models = estimators(cfg)
    summaries = {}
    for name, model in models.items():
        cv = GroupKFold(n_splits=cfg["cv_folds"])
        errors = -cross_val_score(
            model,
            training[features],
            training[TARGET],
            groups=training[GROUP],
            cv=cv,
            scoring="neg_mean_absolute_error",
            error_score="raise",
        )
        start = perf_counter()
        model.fit(training[features], training[TARGET])
        fit_seconds = perf_counter() - start
        validation_prediction = model.predict(validation[features])
        summaries[name] = {
            "fit_seconds": fit_seconds,
            "parameters": model.get_params(),
            "cv_fold_mae": errors.tolist(),
            "validation": scores(validation[TARGET], validation_prediction),
        }
    return models, summaries


def train() -> None:
    """Freeze one validation-selected model before opening the test evaluation."""
    cfg = configuration("training")
    dataset = configuration("dataset")
    evaluation = configuration("evaluation")
    frame = pd.read_csv(TABLE)
    split = partitions(frame, cfg)
    models, summaries = fit_models(split, cfg)
    selected = min(summaries, key=lambda name: summaries[name]["validation"]["mae"])
    validation = split["validation"]
    errors = abs(validation[TARGET] - models[selected].predict(validation[cfg["features"]]))
    threshold = float(np.quantile(errors, evaluation["validation_error_quantile"], method="higher"))
    model_domain = split["train"].groupby("scene_type")[["horizontal_distance_m", "height_m"]]
    support = model_domain.agg(["min", "max"])
    bounds = {
        scene: {
            feature: [float(row[(feature, "min")]), float(row[(feature, "max")])]
            for feature in ("horizontal_distance_m", "height_m")
        }
        for scene, row in support.iterrows()
    }
    MODELS.mkdir(exist_ok=True)
    for name, model in models.items():
        model_file = MODELS / f"{name}.joblib"
        joblib.dump(model, model_file, compress=3)
        summaries[name]["artifact_sha256"] = digest(model_file)
    write_json(
        RESULTS / "experiments" / "training.json",
        {
            "utc": datetime.now(UTC).isoformat(),
            "python": platform.python_version(),
            "libraries": {
                name: version(name)
                for name in (
                    "numpy",
                    "pandas",
                    "scipy",
                    "scikit-learn",
                    "matplotlib",
                    "seaborn",
                    "PyYAML",
                    "joblib",
                    "pytest",
                    "ruff",
                )
            },
            "dataset_version": dataset["version"],
            "subset_manifest_sha256": digest(DATA / "manifests" / "subset.json"),
            "table_sha256": digest(TABLE),
            "training": cfg,
            "evaluation": evaluation,
            "counts": {key: len(part) for key, part in split.items()},
            "batch_counts": {key: part[GROUP].nunique() for key, part in split.items()},
            "selected": selected,
            "validation_error_radius_dbfs": threshold,
            "training_geometry_bounds": bounds,
            "models": summaries,
        },
    )
    pd.concat(split.values())[
        ["recording_id", "session_id", GROUP, "nominal_condition_id", "split"]
    ].to_csv(RESULTS / "experiments" / "split.csv", index=False)
    LOG.info("selected=%s validation_mae=%.3f", selected, summaries[selected]["validation"]["mae"])
