# SPDX-License-Identifier: Apache-2.0
"""Validate privacy-conscious background noise measurement CSV files."""

import csv
import math
from datetime import datetime
from pathlib import Path

import pandas as pd

COLUMNS = (
    "measurement_id",
    "timestamp",
    "location_id",
    "location_type",
    "background_level",
    "unit",
    "duration_s",
    "device_model",
    "calibration_status",
    "temperature_c",
    "humidity_pct",
    "weather",
    "notes",
)
REQUIRED = COLUMNS[:9]
UNITS = {"LAeq_dBA", "LZeq_dB_SPL", "app_estimate_dBA"}
CALIBRATION = {"field_checked", "not_checked", "unknown"}


def validate_school_csv(path: Path) -> pd.DataFrame:
    """Return typed measurements or raise with the physical CSV line number."""
    rows = []
    seen = set()
    with path.open(encoding="utf-8-sig", newline="") as stream:
        reader = csv.DictReader(stream)
        if tuple(reader.fieldnames or ()) != COLUMNS:
            raise ValueError("School CSV columns must match the measurement template")
        for row in reader:
            try:
                if None in row or any(value is None for value in row.values()):
                    raise ValueError("Unexpected number of fields")
                row = {key: value.strip() for key, value in row.items()}
                if any(not row[key] for key in REQUIRED):
                    raise ValueError("A required field is empty")
                if row["measurement_id"] in seen:
                    raise ValueError("Duplicate measurement ID")
                timestamp = datetime.fromisoformat(row["timestamp"])
                if timestamp.utcoffset() is None:
                    raise ValueError("Timestamp requires a UTC offset")
                if row["unit"] not in UNITS or row["calibration_status"] not in CALIBRATION:
                    raise ValueError("Unsupported unit or calibration status")
                if (
                    row["unit"] == "app_estimate_dBA"
                    and row["calibration_status"] == "field_checked"
                ):
                    raise ValueError(
                        "Smartphone app estimates must remain in their own quality tier"
                    )
                for key in ("background_level", "duration_s", "temperature_c", "humidity_pct"):
                    row[key] = float(row[key]) if row[key] else None
                    if row[key] is not None and not math.isfinite(row[key]):
                        raise ValueError(f"Nonfinite value for {key}")
                if row["duration_s"] <= 0:
                    raise ValueError("Duration must be positive")
                if row["humidity_pct"] is not None and not 0 <= row["humidity_pct"] <= 100:
                    raise ValueError("Humidity must be between zero and 100 percent")
                seen.add(row["measurement_id"])
                rows.append(row)
            except ValueError as error:
                raise ValueError(f"School CSV line {reader.line_num}: {error}") from error
    return pd.DataFrame(rows, columns=COLUMNS)
