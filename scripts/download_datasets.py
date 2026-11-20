# SPDX-License-Identifier: Apache-2.0
"""Acquire the pinned Zenodo subset."""

import _bootstrap  # noqa: F401

from src.common import run
from src.datasets import download

if __name__ == "__main__":
    run(download)
