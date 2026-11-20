# SPDX-License-Identifier: Apache-2.0
"""Convert original PCM recordings and geometry into recording-level observations."""

import logging
from pathlib import Path
from typing import Any

import numpy as np
import pandas as pd
from scipy.io import wavfile

from src.common import DATA, RAW, RESULTS, configuration, write_json
from src.datasets import selection, validate

LOG = logging.getLogger(__name__)
TABLE = DATA / "processed" / "recordings.csv"
TARGET = "recorded_rms_dbfs"


def audio_features(path: Path, expected_samples: int) -> dict[str, float]:
    """Calculate full-recording RMS and clipping diagnostics from signed PCM."""
    rate, samples = wavfile.read(path, mmap=True)
    if rate != 48_000 or samples.dtype != np.int16 or samples.shape != (expected_samples,):
        raise ValueError(f"Unexpected WAV format or length: {path.name}")
    pcm = samples.astype(np.float64) / 32768
    rms = float(np.sqrt(np.dot(pcm, pcm) / pcm.size))
    if rms <= 0 or not np.isfinite(rms):
        raise ValueError(f"Silent or invalid recording: {path.name}")
    clipped = (samples == -32768) | (samples == 32767)
    return {
        "duration_s": float(pcm.size / rate),
        "rms_full_scale": rms,
        TARGET: float(20 * np.log10(rms)),
        "clipped_fraction": float(clipped.mean()),
    }


def geometry(frame: pd.DataFrame) -> pd.DataFrame:
    """Validate nominal geometry and compute model inputs shared by simulation."""
    frame = frame.copy()
    for name in ("horizontal_distance_m", "height_m"):
        frame[name] = pd.to_numeric(frame[name], errors="raise")
        if not np.isfinite(frame[name]).all() or (frame[name] <= 0).any():
            raise ValueError(f"Invalid or missing positive geometry: {name}")
    scenes = {"quiet_outdoor": 0, "urban_outdoor": 1}
    if not frame["scene_type"].isin(scenes).all():
        raise ValueError("Unknown or missing scene type")
    frame["slant_m"] = np.hypot(frame["horizontal_distance_m"], frame["height_m"])
    frame["log10_slant_m"] = np.log10(frame["slant_m"])
    frame["urban_scene"] = frame["scene_type"].map(scenes)
    return frame


def quality(frame: pd.DataFrame, source: list[dict[str, Any]]) -> dict[str, Any]:
    """Summarize missingness, duplicate signals, clipping and robust outlier flags."""
    numeric = frame.select_dtypes("number")
    quartiles = numeric.quantile([0.25, 0.75])
    spread = quartiles.loc[0.75] - quartiles.loc[0.25]
    low, high = quartiles.loc[0.25] - 1.5 * spread, quartiles.loc[0.75] + 1.5 * spread
    flagged = ((numeric < low) | (numeric > high)).sum()
    return {
        "rows": len(frame),
        "source_blank_fields": pd.DataFrame(source).eq("").sum().to_dict(),
        "processed_missing": frame.isna().sum().to_dict(),
        "duplicate_recording_ids": int(frame["recording_id"].duplicated().sum()),
        "duplicate_signal_hashes": int(frame["sha256"].duplicated().sum()),
        "clipped_recordings": int((frame["clipped_fraction"] > 0).sum()),
        "iqr_flag_counts": flagged.to_dict(),
        "numeric_distributions": numeric.describe().to_dict(),
        "scene_counts": frame["scene_type"].value_counts().to_dict(),
        "batch_counts": frame["acquisition_batch_id"].value_counts().to_dict(),
    }


def preprocess() -> None:
    """Build the immutable analysis table from verified original files."""
    validate()
    source = selection(configuration("dataset"))
    rows = []
    keys = (
        "recording_id",
        "session_id",
        "acquisition_batch_id",
        "nominal_condition_id",
        "scene_type",
        "horizontal_distance_m",
        "height_m",
        "azimuth_deg",
        "qc_status",
        "sha256",
    )
    for row in source:
        path = RAW / "audio" / f"{row['recording_id']}.wav"
        features = audio_features(path, int(row["sample_count"]))
        if abs(features["duration_s"] - float(row["duration_seconds"])) > 0.001:
            raise ValueError(f"Recording length differs from its metadata: {path.name}")
        rows.append({key: row[key] for key in keys} | features)
    frame = geometry(pd.DataFrame(rows)).sort_values("recording_id").reset_index(drop=True)
    frame["azimuth_deg"] = pd.to_numeric(frame["azimuth_deg"])
    if not np.isfinite(frame["azimuth_deg"]).all():
        raise ValueError("Invalid azimuth annotations")
    TABLE.parent.mkdir(parents=True, exist_ok=True)
    frame.to_csv(TABLE, index=False)
    write_json(RESULTS / "metrics" / "quality.json", quality(frame, source))
    LOG.info("processed_recordings=%s", len(frame))
