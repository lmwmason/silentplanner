# SPDX-License-Identifier: Apache-2.0
"""Check integrity enforcement and HTTP range caching without external requests."""

import hashlib
from pathlib import Path
from subprocess import CompletedProcess

import pytest

from src import datasets


def test_integrity(tmp_path: Path) -> None:
    path = tmp_path / "original"
    path.write_bytes(b"original")
    sha = hashlib.sha256(b"original").hexdigest()
    datasets.verify(path, sha, 8)
    path.write_bytes(b"modified")
    with pytest.raises(ValueError, match="Integrity mismatch"):
        datasets.verify(path, sha, 8)


def test_range_cache(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setattr(datasets, "RAW", tmp_path)
    calls = []

    def transport(args: list[str], **kwargs: object) -> CompletedProcess:
        calls.append(args)
        Path(args[args.index("-o") + 1]).write_bytes(b"abc")
        return CompletedProcess(args, 0, "content-range: bytes 2-4/10\r\n", "")

    monkeypatch.setattr(datasets.subprocess, "run", transport)
    cfg = {"archive_size": 10, "url": "https://example.invalid/archive"}
    assert datasets.fetch_range(cfg, 2, 4).read_bytes() == b"abc"
    assert datasets.fetch_range(cfg, 2, 4).read_bytes() == b"abc"
    assert len(calls) == 1


def test_publisher_csv_schema(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setattr(datasets, "RAW", tmp_path)
    (tmp_path / "sessions.csv").write_text("incorrect_column\nb-1\n")
    with pytest.raises(ValueError, match="schema"):
        datasets.metadata("sessions")
