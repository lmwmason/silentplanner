# SPDX-License-Identifier: Apache-2.0
"""Compare constant, distance-only and geometry models on identical groups."""

from datetime import UTC, datetime

import numpy as np
import pandas as pd
from sklearn.linear_model import LinearRegression
from sklearn.model_selection import LeaveOneGroupOut

from src.common import RESULTS, configuration, digest, write_json
from src.models import GROUP, estimators, partitions, scores
from src.preprocessing import TABLE, TARGET

DISTANCE = "log10_slant_m"


def evaluate_fold(
    train: pd.DataFrame, test: pd.DataFrame, features: list[str], slope: float = -20.0
) -> dict[str, np.ndarray]:
    """Fit each candidate using a training fold and predict the held-out rows."""
    y = train[TARGET]
    intercept = float(np.median(y - slope * train[DISTANCE]))
    return {
        "median": np.full(len(test), y.median()),
        "fixed_free_field_slope": (intercept + slope * test[DISTANCE]).to_numpy(),
        "fitted_distance_only": LinearRegression()
        .fit(train[[DISTANCE]], y)
        .predict(test[[DISTANCE]]),
        "full_linear": LinearRegression().fit(train[features], y).predict(test[features]),
    }


def main() -> None:
    """Replay the original partitions and all eight held-out batches."""
    config = configuration("training")
    frame = pd.read_csv(TABLE)
    split = partitions(frame, config)
    train = split["train"]
    all_scores, predictions = [], []
    for name in ("validation", "test"):
        target = split[name]
        for model, prediction in evaluate_fold(train, target, config["features"]).items():
            all_scores.append({"split": name, "model": model, **scores(target[TARGET], prediction)})
    batch_rows = []
    for train_idx, test_idx in LeaveOneGroupOut().split(frame, groups=frame[GROUP]):
        fold_train, fold_test = frame.iloc[train_idx], frame.iloc[test_idx]
        fold = fold_test[GROUP].iloc[0]
        for model, prediction in evaluate_fold(fold_train, fold_test, config["features"]).items():
            batch_rows.append(
                {"batch": fold, "model": model, **scores(fold_test[TARGET], prediction)}
            )
            predictions.append(
                fold_test[["recording_id", GROUP, TARGET]].assign(
                    model=model, predicted_dbfs=prediction
                )
            )
    batch_scores = pd.DataFrame(batch_rows)
    pooled = pd.concat(predictions, ignore_index=True)
    for name, part in pooled.groupby("model"):
        batch_mae = batch_scores.loc[batch_scores["model"] == name, "mae"].mean()
        all_scores.append(
            {
                "split": "eight_batches",
                "model": name,
                "batch_mae": batch_mae,
                **scores(part[TARGET], part["predicted_dbfs"]),
            }
        )
    output = RESULTS / "followup"
    output.mkdir(parents=True, exist_ok=True)
    pd.DataFrame(all_scores).to_csv(output / "attenuation_metrics.csv", index=False)
    batch_scores.to_csv(output / "attenuation_by_batch.csv", index=False)
    pooled.to_csv(output / "attenuation_predictions.csv", index=False)
    linear = estimators(config)["linear"].fit(train[[DISTANCE]], train[TARGET])
    write_json(
        output / "attenuation_run.json",
        {
            "utc": datetime.now(UTC).isoformat(),
            "table_sha256": digest(TABLE),
            "fixed_slope_db_per_decade": -20,
            "fixed_intercept_fit": "training median residual",
            "original_fitted_distance_slope": float(linear.coef_[0]),
            "settings": config,
        },
    )
    print(pd.DataFrame(all_scores).to_string(index=False))
