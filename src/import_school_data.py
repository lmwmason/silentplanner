# SPDX-License-Identifier: Apache-2.0
"""Import validated school observations for independent background analysis."""

import argparse
import logging
from pathlib import Path

import numpy as np
import pandas as pd

from src.common import DATA
from src.school_data_validator import validate_school_csv

LOG = logging.getLogger(__name__)
GROUPS = [
    "location_id",
    "location_type",
    "unit",
    "duration_s",
    "device_model",
    "calibration_status",
]


def summarize(frame: pd.DataFrame) -> pd.DataFrame:
    """Calculate energy means within matched non-app strata."""
    rows = []
    for values, group in frame.groupby(GROUPS):
        levels = group["background_level"]
        peak = levels.max()
        energy_mean = None
        if values[2] != "app_estimate_dBA":
            energy_mean = peak + 10 * np.log10(np.power(10, (levels - peak) / 10).mean())
        rows.append(
            dict(zip(GROUPS, values, strict=True))
            | {
                "measurements": len(group),
                "minimum": float(levels.min()),
                "maximum": float(peak),
                "energy_mean": energy_mean,
            }
        )
    return pd.DataFrame(
        rows, columns=[*GROUPS, "measurements", "minimum", "maximum", "energy_mean"]
    )


def main() -> None:
    """Validate CSV input and export summaries outside the model pipeline."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("csv", type=Path)
    parser.add_argument("--output", type=Path, default=DATA / "school" / "imported")
    args = parser.parse_args()
    frame = validate_school_csv(args.csv)
    args.output.mkdir(parents=True, exist_ok=True)
    frame.to_csv(args.output / "measurements.csv", index=False)
    summarize(frame).to_csv(args.output / "background_summary.csv", index=False)
    LOG.info("school_measurements=%s output=%s", len(frame), args.output)
