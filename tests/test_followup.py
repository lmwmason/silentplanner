# SPDX-License-Identifier: Apache-2.0
"""Verify the interpretation and input handling of the follow-up experiments."""

import numpy as np
import pandas as pd
import pytest

from src.attenuation import DISTANCE, evaluate_fold
from src.field_validation import COLUMNS, evaluate
from src.preprocessing import TARGET


def test_distance_models_recover_a_synthetic_free_field() -> None:
    frame = pd.DataFrame({DISTANCE: [0.5, 1.0, 1.5], TARGET: [-30.0, -40.0, -50.0]})
    result = evaluate_fold(frame.iloc[:2], frame.iloc[2:], [DISTANCE])
    assert result["median"][0] == -35
    np.testing.assert_allclose(result["fixed_free_field_slope"], [-50])
    np.testing.assert_allclose(result["fitted_distance_only"], [-50])


def test_field_empty_form_reports_no_measurements() -> None:
    assert evaluate({}, pd.DataFrame(columns=COLUMNS))["status"] == "awaiting_measurements"


def test_field_agreement_requires_matched_pairs_and_counts_sessions() -> None:
    plan = {"pairs": [{"id": "P01"}], "receiver_zones": [{"weight": 1.0}]}
    rows = pd.DataFrame(
        [
            ["S01", "P01", "lower", "R01", 50, 35, 60, "M1", "A", "yes"],
            ["S01", "P01", "higher", "R01", 60, 35, 60, "M1", "A", "yes"],
            ["S02", "P01", "lower", "R01", 51, 35, 60, "M1", "A", "yes"],
        ],
        columns=COLUMNS,
    )
    actual = evaluate(plan, rows)
    assert actual["independent_sessions"] == 1
    assert actual["sessions_agree"] == 1
    assert len(actual["excluded"]) == 1
    rows.loc[0, "calibration_checked"] = "no"
    with pytest.raises(ValueError, match="checked meter"):
        evaluate(plan, rows)
