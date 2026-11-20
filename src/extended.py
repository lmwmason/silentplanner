# SPDX-License-Identifier: Apache-2.0
"""Compare larger regressors with nested leave-one-acquisition-batch-out validation."""

import logging
import platform
import warnings
from datetime import UTC, datetime
from importlib.metadata import version
from typing import Any

import joblib
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
from sklearn.base import clone
from sklearn.compose import TransformedTargetRegressor
from sklearn.ensemble import ExtraTreesRegressor, HistGradientBoostingRegressor
from sklearn.exceptions import ConvergenceWarning
from sklearn.linear_model import Ridge
from sklearn.model_selection import LeaveOneGroupOut
from sklearn.neural_network import MLPRegressor
from sklearn.pipeline import make_pipeline
from sklearn.preprocessing import PolynomialFeatures, StandardScaler
from sklearn.svm import SVR
from threadpoolctl import threadpool_limits

from src.common import DATA, MODELS, RESULTS, configuration, digest, write_json
from src.evaluation import save_figure
from src.models import GROUP, estimators, scores
from src.preprocessing import TABLE, TARGET

LOG = logging.getLogger(__name__)
OUTPUT = RESULTS / "extended"
ARTIFACT = MODELS / "extended_selected.joblib"


def candidates(training: dict, settings: dict) -> dict[str, list]:
    """Construct the four reference regressors and five additional model families."""
    seed = training["seed"]
    return {name: [model] for name, model in estimators(training).items()} | {
        "polynomial_ridge": [
            make_pipeline(
                StandardScaler(),
                PolynomialFeatures(degree=option["degree"], include_bias=False),
                StandardScaler(),
                Ridge(alpha=option["alpha"]),
            )
            for option in settings["polynomial_ridge"]
        ],
        "extra_trees": [
            ExtraTreesRegressor(random_state=seed, n_jobs=1, **option)
            for option in settings["extra_trees"]
        ],
        "hist_gradient_boosting": [
            HistGradientBoostingRegressor(random_state=seed, early_stopping=False, **option)
            for option in settings["hist_gradient_boosting"]
        ],
        "rbf_svr": [
            TransformedTargetRegressor(
                regressor=make_pipeline(StandardScaler(), SVR(**option)),
                transformer=StandardScaler(),
            )
            for option in settings["rbf_svr"]
        ],
        "mlp": [
            TransformedTargetRegressor(
                regressor=make_pipeline(
                    StandardScaler(),
                    MLPRegressor(random_state=seed, **settings["mlp_common"], **option),
                ),
                transformer=StandardScaler(),
            )
            for option in settings["mlp"]
        ],
    }


def fit(estimator: Any, x: pd.DataFrame, y: pd.Series) -> tuple[Any, bool]:
    """Fit a fresh estimator and capture any iteration-limit warning."""
    with warnings.catch_warnings(record=True) as messages:
        warnings.simplefilter("always", ConvergenceWarning)
        model = clone(estimator).fit(x, y)
    return model, any(issubclass(item.category, ConvergenceWarning) for item in messages)


def grouped_validation(frame: pd.DataFrame, choices: dict, features: list[str]) -> pd.DataFrame:
    """Estimate each configuration's MAE by holding out every eligible batch once."""
    rows = []
    x, y = frame[features], frame[TARGET]
    folds = list(LeaveOneGroupOut().split(x, y, groups=frame[GROUP]))
    for family, options in choices.items():
        for index, estimator in enumerate(options):
            for train_ids, valid_ids in folds:
                model, iteration_limit = fit(estimator, x.iloc[train_ids], y.iloc[train_ids])
                prediction = model.predict(x.iloc[valid_ids])
                outcome = scores(y.iloc[valid_ids], prediction)
                rows.append(
                    {
                        "family": family,
                        "option": index,
                        "batch": frame.iloc[valid_ids[0]][GROUP],
                        "mae": outcome["mae"],
                        "iteration_limit": iteration_limit,
                    }
                )
    return pd.DataFrame(rows)


def rank_options(validation: pd.DataFrame) -> pd.DataFrame:
    """Rank by equal-weight batch MAE, using predefined family order for ties."""
    ranking = validation.groupby(["family", "option"], sort=False, as_index=False).agg(
        batch_mae=("mae", "mean"), warning_fits=("iteration_limit", "sum")
    )
    return ranking.sort_values("batch_mae", kind="stable").reset_index(drop=True)


def nested_predictions(
    frame: pd.DataFrame, choices: dict, features: list[str]
) -> tuple[pd.DataFrame, pd.DataFrame]:
    """Tune on inner folds, then predict each untouched outer batch once per family."""
    rows, tuning = [], []
    x, y = frame[features], frame[TARGET]
    for train_ids, test_ids in LeaveOneGroupOut().split(x, y, groups=frame[GROUP]):
        training, test = frame.iloc[train_ids], frame.iloc[test_ids]
        held_out = test[GROUP].iloc[0]
        inner = grouped_validation(training, choices, features).assign(outer_batch=held_out)
        tuning.append(inner)
        ranked = rank_options(inner)
        winner = ranked.iloc[0]["family"]
        for choice in ranked.drop_duplicates("family").itertuples(index=False):
            model, iteration_limit = fit(
                choices[choice.family][choice.option], training[features], training[TARGET]
            )
            predicted = model.predict(test[features])
            part = test[["recording_id", GROUP, "scene_type", TARGET]].copy()
            part["predicted_dbfs"] = predicted
            part["family"] = choice.family
            part["option"] = choice.option
            part["outer_selected"] = choice.family == winner
            part["iteration_limit"] = iteration_limit
            rows.append(part)
        LOG.info("outer_batch=%s inner_selected=%s", held_out, winner)
    return pd.concat(rows, ignore_index=True), pd.concat(tuning, ignore_index=True)


def summarize(predictions: pd.DataFrame) -> tuple[pd.DataFrame, pd.DataFrame]:
    """Score model families and the nested selection procedure."""
    selected = predictions[predictions["outer_selected"]].assign(family="nested_selection")
    expanded = pd.concat([predictions, selected], ignore_index=True)
    batch_rows = []
    summary_rows = []
    for family, family_rows in expanded.groupby("family", sort=False):
        batches = []
        for batch, part in family_rows.groupby(GROUP):
            outcome = scores(part[TARGET], part["predicted_dbfs"])
            batches.append(
                {
                    "family": family,
                    "batch": batch,
                    "scene": part["scene_type"].iloc[0],
                    "n": len(part),
                    "option": int(part["option"].iloc[0]),
                    **outcome,
                }
            )
        batch_rows.extend(batches)
        summary_rows.append(
            {
                "family": family,
                "batch_mae": np.mean([row["mae"] for row in batches]),
                **scores(family_rows[TARGET], family_rows["predicted_dbfs"]),
            }
        )
    by_batch = pd.DataFrame(batch_rows)
    summary = pd.DataFrame(summary_rows).sort_values("batch_mae")
    baseline = by_batch.loc[by_batch["family"] == "baseline"].set_index("batch")["mae"]
    by_batch["delta_mae_vs_baseline"] = by_batch["mae"] - by_batch["batch"].map(baseline)
    return summary, by_batch


def plot(summary: pd.DataFrame, by_batch: pd.DataFrame) -> None:
    """Show mean batch error and the range of paired batch differences."""
    fig, axes = plt.subplots(1, 2, sharey=True, figsize=(11, 5.5))
    for index, row in enumerate(summary.itertuples(index=False)):
        color = "#566c88" if row.family != "nested_selection" else "#b3572c"
        axes[0].barh(index, row.batch_mae, color=color)
        delta = by_batch.loc[by_batch["family"] == row.family, "delta_mae_vs_baseline"]
        axes[1].scatter(delta, np.full(len(delta), index), color=color, alpha=0.55)
    axes[0].set_yticks(range(len(summary)), summary["family"].str.replace("_", " "))
    axes[0].invert_yaxis()
    axes[0].set_xlabel("Mean acquisition-batch MAE (dBFS)")
    axes[1].axvline(0, color="#333333", linestyle="--", linewidth=1)
    axes[1].set_xlabel("Batch MAE minus baseline MAE (dBFS)")
    axes[1].set_title("Each dot represents a held-out batch")
    fig.suptitle("Exploratory nested grouped regression comparison")
    save_figure(fig, OUTPUT / "comparison.png")


def main() -> None:
    """Run the exploratory comparison and fit the selected deployment candidate."""
    training, settings = configuration("training"), configuration("extended")
    features = training["features"]
    frame = pd.read_csv(TABLE)
    if frame[GROUP].isna().any() or frame[GROUP].nunique() < 3:
        raise ValueError("Nested validation requires at least three identifiable batches")
    for key in ("recording_id", "session_id", "sha256"):
        if frame[key].isna().any() or frame.groupby(key)[GROUP].nunique().gt(1).any():
            raise ValueError(f"Missing or overlapping acquisition provenance: {key}")
    options = candidates(training, settings)
    with threadpool_limits(limits=1):
        predictions, tuning = nested_predictions(frame, options, features)
        final_validation = grouped_validation(frame, options, features)
        ranking = rank_options(final_validation)
        winner = ranking.iloc[0]
        family, option = winner["family"], int(winner["option"])
        model, warning = fit(options[family][option], frame[features], frame[TARGET])

    OUTPUT.mkdir(parents=True, exist_ok=True)
    MODELS.mkdir(exist_ok=True)
    joblib.dump(model, ARTIFACT, compress=3)
    summary, by_batch = summarize(predictions)
    for name, table in {
        "summary": summary,
        "by_batch": by_batch,
        "predictions": predictions,
        "inner_tuning": tuning,
        "final_validation_folds": final_validation,
        "final_ranking": ranking,
    }.items():
        table.to_csv(OUTPUT / f"{name}.csv", index=False)
    plot(summary, by_batch)
    write_json(
        OUTPUT / "run.json",
        {
            "utc": datetime.now(UTC).isoformat(),
            "python": platform.python_version(),
            "libraries": {
                name: version(name) for name in ("numpy", "pandas", "scikit-learn", "joblib")
            },
            "device": "CPU",
            "rows": len(frame),
            "batches": int(frame[GROUP].nunique()),
            "dataset_version": configuration("dataset")["version"],
            "table_sha256": digest(TABLE),
            "subset_manifest_sha256": digest(DATA / "manifests" / "subset.json"),
            "training": training,
            "settings": settings,
            "selected_family": family,
            "selected_option": option,
            "final_validation_batch_mae": float(winner["batch_mae"]),
            "final_fit_iteration_limit": warning,
            "model_sha256": digest(ARTIFACT),
        },
    )
    LOG.info(
        "selected_family=%s option=%s validation_batch_mae=%.3f",
        family,
        option,
        winner["batch_mae"],
    )
