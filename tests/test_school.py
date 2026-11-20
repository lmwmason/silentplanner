# SPDX-License-Identifier: Apache-2.0
"""Verify school CSV validation and comparable summary strata."""

from pathlib import Path

import pytest

from src.common import ROOT
from src.import_school_data import summarize
from src.school_data_validator import validate_school_csv

TEMPLATE = ROOT / "school_collection" / "school_measurement_template.csv"
FIXTURE = "sample,2026-11-18T09:00:00+09:00,room,library,40,LAeq_dBA,60,meter,field_checked,,,dry,"


def test_template_is_header_only() -> None:
    assert validate_school_csv(TEMPLATE).empty


def test_energy_summary(tmp_path: Path) -> None:
    path = tmp_path / "input.csv"
    other = FIXTURE.replace("sample", "other").replace(",40,", ",50,")
    path.write_text(TEMPLATE.read_text() + FIXTURE + "\n" + other + "\n")
    stats = summarize(validate_school_csv(path))
    assert stats.loc[0, "energy_mean"] == pytest.approx(47.4036269)
    assert stats.loc[0, "measurements"] == 2


@pytest.mark.parametrize(
    ("old", "new"),
    [
        ("+09:00", ""),
        ("LAeq_dBA", "dBFS"),
        (",60,", ",0,"),
        (",40,", ",inf,"),
        (",dry,", ",dry,notes,extra"),
    ],
)
def test_invalid_school_value(tmp_path: Path, old: str, new: str) -> None:
    path = tmp_path / "input.csv"
    path.write_text(TEMPLATE.read_text() + FIXTURE.replace(old, new) + "\n")
    with pytest.raises(ValueError, match="line 2"):
        validate_school_csv(path)
