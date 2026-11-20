# SPDX-License-Identifier: Apache-2.0
"""Exercise PCM conversion, grouped fitting and invariant predictions."""

from pathlib import Path

import joblib
import numpy as np
import pandas as pd
import pytest
from scipy.io.wavfile import write

from src.common import configuration
from src.models import GROUP, fit_models, partitions, scores
from src.preprocessing import TARGET, audio_features, geometry


def test_pcm_features(tmp_path: Path) -> None:
    path = tmp_path / "test.wav"
    write(path, 48_000, np.array([-16384, 16384], dtype=np.int16))
    outcome = audio_features(path, 2)
    assert outcome["rms_full_scale"] == 0.5
    assert outcome[TARGET] == pytest.approx(-6.0206)
    with pytest.raises(ValueError, match="format or length"):
        audio_features(path, 3)
    write(path, 48_000, np.zeros(2, dtype=np.int16))
    with pytest.raises(ValueError, match="Silent"):
        audio_features(path, 2)


@pytest.mark.parametrize("bad", [np.nan, -1, np.inf, "bad"])
def test_geometry_rejects_invalid(bad: object) -> None:
    with pytest.raises(ValueError):
        geometry(
            pd.DataFrame(
                {"height_m": [bad], "horizontal_distance_m": [10], "scene_type": ["quiet_outdoor"]}
            )
        )


def observations() -> tuple[pd.DataFrame, dict]:
    """Make isolated numerical fixtures for model and split unit tests."""
    cfg = configuration("training") | {"validation_batches": ["E"], "test_batches": ["F"]}
    pairs = [(batch, distance) for batch in "ABCDEF" for distance in (5, 10, 20, 30)]
    frame = pd.DataFrame(pairs, columns=[GROUP, "horizontal_distance_m"])
    frame["height_m"] = 10
    frame["scene_type"] = np.where(
        frame[GROUP].isin(["A", "C", "E"]), "quiet_outdoor", "urban_outdoor"
    )
    for key in ("recording_id", "session_id", "sha256"):
        frame[key] = frame.index.map(str)
    frame = geometry(frame)
    frame[TARGET] = -30 - 20 * frame["log10_slant_m"] + frame["urban_scene"]
    return frame, cfg


@pytest.mark.parametrize("key", ["session_id", "recording_id", "sha256"])
def test_partition_rejects_leakage(key: str) -> None:
    frame, cfg = observations()
    frame.loc[0, key] = frame.iloc[-1][key]
    with pytest.raises(ValueError, match="leakage"):
        partitions(frame, cfg)


def test_fit_save_reload_and_determinism(tmp_path: Path) -> None:
    frame, cfg = observations()
    split = partitions(frame, cfg)
    assert [len(split[key]) for key in ("train", "validation", "test")] == [16, 4, 4]
    models, summary = fit_models(split, cfg)
    # Deliberately change only the test labels to prove model fitting is isolated.
    split["test"][TARGET] = np.arange(len(split["test"])) + 100
    repeated, _ = fit_models(split, cfg)
    features = split["test"][cfg["features"]]
    for name, model in models.items():
        path = tmp_path / f"{name}.joblib"
        joblib.dump(model, path)
        expected = model.predict(features)
        np.testing.assert_array_equal(expected, joblib.load(path).predict(features))
        np.testing.assert_array_equal(expected, repeated[name].predict(features))
        assert len(summary[name]["cv_fold_mae"]) == 4
    assert summary["linear"]["validation"]["mae"] < 1e-10


def test_metrics() -> None:
    result = scores([0, 1, 2], [0, 2, 4])
    assert result == pytest.approx({"mae": 1, "rmse": np.sqrt(5 / 3), "r2": -1.5})
    assert scores([0, 0], [1, 1])["r2"] is None
