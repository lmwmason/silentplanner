# SPDX-License-Identifier: Apache-2.0
"""Shared project paths and artifact I/O."""

import hashlib
import json
import logging
from pathlib import Path
from typing import Any

import yaml

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data"
RAW = DATA / "raw"
MODELS = ROOT / "models"
RESULTS = ROOT / "results"


def configuration(name: str) -> dict[str, Any]:
    """Read a project YAML configuration."""
    return yaml.safe_load((ROOT / "configs" / f"{name}.yaml").read_text())


def digest(path: Path) -> str:
    """Hash a file without loading it into memory."""
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path: Path, data: Any) -> None:
    """Write strict JSON and create its parent directory."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, allow_nan=False) + "\n")


def run(entrypoint: Any) -> None:
    """Configure CLI logging and surface actionable operational errors."""
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
    try:
        entrypoint()
    except (OSError, ValueError) as error:
        logging.error("%s", error)
        raise SystemExit(1) from error
