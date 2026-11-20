# SPDX-License-Identifier: Apache-2.0
"""Verify release archive checking and required-member handling."""

from pathlib import Path
from zipfile import ZipFile

import pytest

from src.release import PREFIX, verify_archive, write_archive


def test_archive_round_trip(tmp_path: Path) -> None:
    (tmp_path / "LICENSE").write_text("license")
    path = tmp_path / "release.zip"
    write_archive(tmp_path, ["LICENSE"], path)
    verify_archive(path, {"LICENSE"})
    with pytest.raises(ValueError, match="required manifest"):
        verify_archive(path, {"README.md"})
    with ZipFile(path, "a") as archive:
        archive.writestr(f"{PREFIX}/extra", b"extra")
    with pytest.raises(ValueError, match="required manifest"):
        verify_archive(path, {"LICENSE"})


def test_archive_rejects_parent_path(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="Invalid release file"):
        write_archive(tmp_path, ["../sample.wav"], tmp_path / "release.zip")
