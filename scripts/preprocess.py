# SPDX-License-Identifier: Apache-2.0
"""Run the preprocess stage."""

import _bootstrap  # noqa: F401

from src.common import run
from src.preprocessing import preprocess

if __name__ == "__main__":
    run(preprocess)
