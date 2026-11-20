# SPDX-License-Identifier: Apache-2.0
"""Verify that nested tuning and predictions respect acquisition batches."""

import numpy as np
import pandas as pd
from sklearn.dummy import DummyRegressor
from sklearn.linear_model import LinearRegression

from src.extended import nested_predictions, summarize
from src.models import GROUP
from src.preprocessing import TARGET


def test_nested_predictions_and_summary() -> None:
    frame = pd.DataFrame(
        {
            "recording_id": range(12),
            GROUP: np.repeat(list("ABCD"), 3),
            "scene_type": "quiet_outdoor",
            "distance": np.arange(12),
        }
    )
    frame[TARGET] = -2.0 * frame["distance"] - 30.0
    choices = {"baseline": [DummyRegressor(strategy="median")], "linear": [LinearRegression()]}
    predicted, tuning = nested_predictions(frame, choices, ["distance"])

    assert len(predicted) == 24
    assert predicted.groupby(["family", "recording_id"]).size().eq(1).all()
    assert (tuning["outer_batch"] != tuning["batch"]).all()
    assert tuning.groupby(["outer_batch", "family"])["batch"].nunique().eq(3).all()
    assert predicted.loc[predicted["outer_selected"], "family"].eq("linear").all()

    summary, by_batch = summarize(predicted)
    assert set(summary["family"]) == {"baseline", "linear", "nested_selection"}
    assert by_batch.loc[by_batch["family"] == "linear", "delta_mae_vs_baseline"].lt(0).all()
