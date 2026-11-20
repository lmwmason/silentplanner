# SPDX-License-Identifier: Apache-2.0
"""Exercise scenario predictions with a numerical test estimator."""

import numpy as np
import pandas as pd
import pytest
from sklearn.dummy import DummyRegressor

from src.common import configuration
from src.simulation import simulate_zones


def test_zone_output_and_bounds() -> None:
    scenarios = configuration("scenarios")
    features = configuration("training")["features"]
    model = DummyRegressor().fit(pd.DataFrame([[1, 10, 0]], columns=features), [-60])
    record = {
        "training": {"features": features},
        "validation_error_radius_dbfs": 5,
        "training_geometry_bounds": {
            "quiet_outdoor": {"horizontal_distance_m": [5, 70], "height_m": [5, 30]}
        },
    }
    output = simulate_zones(scenarios, record, model)
    assert len(output) == 8
    np.testing.assert_allclose(output["predicted_impact"], output["sensitivity_assumption"])
    assert (output["lower_dbfs"] == -65).all()
    scenarios["height_m"] = 100
    with pytest.raises(ValueError, match="training range"):
        simulate_zones(scenarios, record, model)
