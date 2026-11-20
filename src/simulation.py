# SPDX-License-Identifier: Apache-2.0
"""Compare model predictions at fictional school receiver zones."""

from typing import Any

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

from src.common import RESULTS, configuration
from src.evaluation import FIGURES, load_model, save_figure, training_record
from src.preprocessing import geometry


def simulate_zones(cfg: dict[str, Any], record: dict[str, Any], model: Any) -> pd.DataFrame:
    """Predict digital levels and compute a dimensionless sensitivity-weighted proxy."""
    scene = cfg["scene_type"]
    height = float(cfg["height_m"])
    bounds = record["training_geometry_bounds"].get(scene)
    if not bounds:
        raise ValueError(f"Unseen training scene: {scene}")
    rows = []
    for label, distances in cfg["scenarios"].items():
        if distances.keys() != cfg["zones"].keys():
            raise ValueError(f"Scenario must define every receiver zone: {label}")
        for zone, distance in distances.items():
            attributes = cfg["zones"][zone]
            rows.append(
                {
                    "scenario": label,
                    "zone": zone,
                    "space_type": attributes["type"],
                    "sensitivity_assumption": float(attributes["sensitivity"]),
                    "horizontal_distance_m": float(distance),
                    "height_m": height,
                    "scene_type": scene,
                }
            )
    frame = geometry(pd.DataFrame(rows))
    if not frame["sensitivity_assumption"].between(0, 1).all():
        raise ValueError("Sensitivity must be between zero and one")
    for name, (low, high) in bounds.items():
        if not frame[name].between(low, high).all():
            raise ValueError(f"Scenario exceeds the training range for {name}")
    frame["predicted_dbfs"] = model.predict(frame[record["training"]["features"]])
    radius = record["validation_error_radius_dbfs"]
    frame["lower_dbfs"] = frame["predicted_dbfs"] - radius
    frame["upper_dbfs"] = frame["predicted_dbfs"] + radius
    for prefix in ("predicted", "lower", "upper"):
        frame[f"{prefix}_impact"] = frame["sensitivity_assumption"] * np.power(
            10.0, (frame[f"{prefix}_dbfs"] + 60) / 10
        )
    return frame


def simulate() -> None:
    """Export scenario tables and empirical error bands."""
    record = training_record()
    frame = simulate_zones(
        configuration("scenarios"), record, load_model(record["selected"], record)
    )
    target = RESULTS / "simulation"
    target.mkdir(parents=True, exist_ok=True)
    frame.to_csv(target / "zones.csv", index=False)
    fig, axes = plt.subplots(1, 2, figsize=(13, 5))
    zones = frame["zone"].unique().tolist()
    scenarios = list(frame.groupby("scenario", sort=False))
    width = 0.8 / len(scenarios)
    for index, (label, values) in enumerate(scenarios):
        values = values.set_index("zone").loc[zones]
        pos = np.arange(len(zones)) + width * (index - (len(scenarios) - 1) / 2)
        axes[0].errorbar(
            pos,
            values["predicted_dbfs"],
            yerr=record["validation_error_radius_dbfs"],
            fmt="o",
            capsize=3,
            label=label,
        )
        axes[1].errorbar(
            pos,
            values["predicted_impact"],
            yerr=[
                values["predicted_impact"] - values["lower_impact"],
                values["upper_impact"] - values["predicted_impact"],
            ],
            fmt="o",
            capsize=3,
            label=label,
        )
    axes[0].set_ylabel("Predicted digital RMS (dBFS)")
    axes[1].set_ylabel("Assumption-weighted relative energy")
    axes[1].set_yscale("log")
    for axis in axes:
        axis.set_xticks(np.arange(len(zones)), zones)
        axis.legend(fontsize=8)
    fig.suptitle("Fictional school receiver zones | empirical validation error bands")
    save_figure(fig, FIGURES / "virtual_school.png")
