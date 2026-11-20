# SPDX-License-Identifier: Apache-2.0
"""Evaluate frozen regressors and plot recording-level diagnostics."""

import json
import logging
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import joblib
import matplotlib as mpl
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns
from sklearn.inspection import permutation_importance

from src.common import DATA, MODELS, RESULTS, digest, write_json
from src.models import GROUP, partitions, scores
from src.preprocessing import TABLE, TARGET

LOG = logging.getLogger(__name__)
TRAINING = RESULTS / "experiments" / "training.json"
METRICS = RESULTS / "metrics"
FIGURES = RESULTS / "figures"
PREDICTIONS = RESULTS / "predictions" / "test.csv"
FEATURE_LABELS = {
    "log10_slant_m": "Log10 slant distance (m)",
    "height_m": "Nominal height (m)",
    "urban_scene": "Road-adjacent scene indicator",
}


def training_record() -> dict[str, Any]:
    """Read the frozen experiment configuration and provenance."""
    return json.loads(TRAINING.read_text())


def load_model(name: str, record: dict[str, Any]) -> Any:
    """Load only a project model that matches the experiment manifest."""
    path = MODELS / f"{name}.joblib"
    if digest(path) != record["models"][name]["artifact_sha256"]:
        raise ValueError(f"Model differs from its training record: {name}")
    return joblib.load(path)


def evaluate() -> None:
    """Predict held-out recordings once per frozen experiment and retain diagnostics."""
    record = training_record()
    if (
        digest(TABLE) != record["table_sha256"]
        or digest(DATA / "manifests" / "subset.json") != record["subset_manifest_sha256"]
    ):
        raise ValueError("Data differ from the frozen training record")
    cfg = record["training"]
    evaluation = record["evaluation"]
    split = partitions(pd.read_csv(TABLE), cfg)
    test = split["test"]
    x, y = test[cfg["features"]], test[TARGET]
    parts = []
    summary = []
    for name in record["models"]:
        model = load_model(name, record)
        prediction = model.predict(x)
        summary.append(
            {
                "model": name,
                **scores(y, prediction),
                "validation_mae": record["models"][name]["validation"]["mae"],
                "mean_cv_mae": np.mean(record["models"][name]["cv_fold_mae"]),
                "fit_seconds": record["models"][name]["fit_seconds"],
                **record["counts"],
            }
        )
        part = test[
            [
                "recording_id",
                GROUP,
                "scene_type",
                "slant_m",
                "horizontal_distance_m",
                "height_m",
                "azimuth_deg",
                TARGET,
            ]
        ].copy()
        part["model"] = name
        part["predicted_dbfs"] = prediction
        part["residual_dbfs"] = y - prediction
        part["absolute_error_dbfs"] = abs(part["residual_dbfs"])
        parts.append(part)
    result = pd.DataFrame(summary)
    base_mae = result.set_index("model").loc["baseline", "mae"]
    result["mae_improvement_pct"] = np.where(
        base_mae > 0, 100 * (base_mae - result["mae"]) / base_mae, np.nan
    )
    base_rmse = result.set_index("model").loc["baseline", "rmse"]
    result["rmse_improvement_pct"] = np.where(
        base_rmse > 0, 100 * (base_rmse - result["rmse"]) / base_rmse, np.nan
    )
    result["r2_change"] = result["r2"] - result.set_index("model").loc["baseline", "r2"]
    predictions = pd.concat(parts, ignore_index=True)
    predictions["distance_group_m"] = pd.cut(
        predictions["slant_m"], evaluation["distance_bin_edges_m"], right=False
    )
    if predictions["distance_group_m"].isna().any():
        raise ValueError("Distance bins do not cover all test data")
    PREDICTIONS.parent.mkdir(parents=True, exist_ok=True)
    predictions.to_csv(PREDICTIONS, index=False)
    result.to_csv(METRICS / "test.csv", index=False)
    for key in (GROUP, "scene_type", "distance_group_m", "height_m", "azimuth_deg"):
        summary_rows = []
        for (name, condition), values in predictions.groupby(["model", key], observed=True):
            summary_rows.append(
                {
                    "model": name,
                    key: condition,
                    "n": len(values),
                    **scores(values[TARGET], values["predicted_dbfs"]),
                }
            )
        pd.DataFrame(summary_rows).to_csv(METRICS / f"by_{key}.csv", index=False)

    linear = load_model("linear", record)
    pd.DataFrame({"feature": cfg["features"], "coefficient": linear.coef_}).to_csv(
        METRICS / "linear_coefficients.csv", index=False
    )
    pd.DataFrame(
        {
            "feature": cfg["features"],
            "impurity_importance": load_model("random_forest", record).feature_importances_,
        }
    ).to_csv(METRICS / "forest_impurity.csv", index=False)
    model = load_model(record["selected"], record)
    importance = permutation_importance(
        model,
        x,
        y,
        scoring="neg_mean_absolute_error",
        random_state=cfg["seed"],
        n_repeats=evaluation["permutation_repeats"],
    )
    pd.DataFrame(
        {
            "feature": cfg["features"],
            "mean_mae_increase": importance.importances_mean,
            "shuffle_sd": importance.importances_std,
        }
    ).to_csv(METRICS / "permutation_importance.csv", index=False)
    selected = predictions[predictions["model"] == record["selected"]]
    coverage = (selected["absolute_error_dbfs"] <= record["validation_error_radius_dbfs"]).mean()
    write_json(
        METRICS / "evaluation.json",
        {
            "utc": datetime.now(UTC).isoformat(),
            "training_record_sha256": digest(TRAINING),
            "selected": record["selected"],
            "linear_intercept": float(linear.intercept_),
            "observed_test_band_coverage": float(coverage),
        },
    )
    LOG.info("held_out_recordings=%s selected=%s", len(test), record["selected"])


def save_figure(fig: mpl.figure.Figure, path: Path) -> None:
    """Write a headless PNG and release the figure."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fig.tight_layout()
    fig.savefig(path, dpi=160, bbox_inches="tight")
    plt.close(fig)


def analyze() -> None:
    """Render figures from saved predictions and measured audio features."""
    sns.set_theme(style="whitegrid")
    summary = pd.read_csv(METRICS / "test.csv")
    predictions = pd.read_csv(PREDICTIONS)
    record = training_record()
    frame = pd.read_csv(TABLE)
    fig, axes = plt.subplots(1, 3, figsize=(12, 4))
    for axis, metric in zip(axes, ("mae", "rmse", "r2"), strict=True):
        axis.barh(summary["model"].str.replace("_", " "), summary[metric], color="#386b8c")
        axis.axvline(0, color="black", linewidth=0.6)
        axis.set_title(metric.upper() + (" (dBFS)" if metric != "r2" else ""))
    fig.suptitle("Held-out model comparison")
    save_figure(fig, FIGURES / "model_comparison.png")

    fig, axes = plt.subplots(2, 2, sharex=True, sharey=True, figsize=(10, 8))
    limits = [
        predictions[[TARGET, "predicted_dbfs"]].min().min(),
        predictions[[TARGET, "predicted_dbfs"]].max().max(),
    ]
    for axis, name in zip(axes.flat, summary["model"], strict=True):
        series = predictions[predictions["model"] == name]
        sns.scatterplot(
            data=series,
            x=TARGET,
            y="predicted_dbfs",
            hue="scene_type",
            legend=name == record["selected"],
            ax=axis,
        )
        axis.plot(limits, limits, color="black", linestyle="--")
        title = name.replace("_", " ")
        if name == record["selected"]:
            title += " (validation winner)"
        axis.set(
            xlabel="Measured digital RMS (dBFS)", ylabel="Predicted digital RMS (dBFS)", title=title
        )
        axis.label_outer()
    save_figure(fig, FIGURES / "actual_vs_predicted.png")

    fig, axis = plt.subplots(figsize=(8, 5))
    sns.histplot(
        data=predictions,
        x="residual_dbfs",
        hue="model",
        element="step",
        fill=False,
        bins=16,
        ax=axis,
    )
    axis.axvline(0, color="black", linewidth=0.7)
    axis.set(xlabel="Measured minus predicted (dBFS)", title="Held-out residual distributions")
    save_figure(fig, FIGURES / "residuals.png")

    fig, axes = plt.subplots(1, 2, figsize=(11, 4))
    for axis, key, label in zip(
        axes,
        ("scene_type", "distance_group_m"),
        ("Recording scene", "Slant distance group (m)"),
        strict=True,
    ):
        grouped = pd.read_csv(METRICS / f"by_{key}.csv")
        sns.barplot(data=grouped, x=key, y="mae", hue="model", ax=axis)
        axis.set(xlabel=label, ylabel="MAE (dBFS)")
    save_figure(fig, FIGURES / "condition_errors.png")

    in_range = pd.Series(False, index=predictions.index)
    for scene, bounds in record["training_geometry_bounds"].items():
        scene_valid = predictions["scene_type"] == scene
        for name, (low, high) in bounds.items():
            scene_valid &= predictions[name].between(low, high)
        in_range |= scene_valid
    supported = predictions.assign(within_training_geometry_bounds=in_range)
    supported.groupby(["model", "scene_type", "within_training_geometry_bounds"])[
        "absolute_error_dbfs"
    ].agg(n="size", mae="mean").to_csv(METRICS / "geometry_support.csv")

    fig, axes = plt.subplots(1, 2, figsize=(10, 4.8))
    for axis, file, column, label in [
        (axes[0], "linear_coefficients", "coefficient", "OLS coefficient (dBFS per input unit)"),
        (axes[1], "permutation_importance", "mean_mae_increase", "Test MAE increase (dBFS)"),
    ]:
        data = pd.read_csv(METRICS / f"{file}.csv")
        axis.barh(
            data["feature"].map(FEATURE_LABELS),
            data[column],
            xerr=data["shuffle_sd"] if "shuffle_sd" in data else None,
            color="#477a65",
        )
        axis.axvline(0, color="black", linewidth=0.7)
        axis.set_xlabel(label)
    save_figure(fig, FIGURES / "explainability.png")

    fig, axes = plt.subplots(1, 2, figsize=(11, 4))
    sns.scatterplot(data=frame, x="slant_m", y=TARGET, hue="scene_type", alpha=0.7, ax=axes[0])
    axes[0].set(xlabel="Nominal slant distance (m)", ylabel="Measured digital RMS (dBFS)")
    sns.histplot(data=frame, x=TARGET, hue="scene_type", bins=20, ax=axes[1])
    axes[1].set_xlabel("Measured digital RMS (dBFS)")
    save_figure(fig, FIGURES / "dataset_distributions.png")
