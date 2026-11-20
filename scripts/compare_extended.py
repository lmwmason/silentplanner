# SPDX-License-Identifier: Apache-2.0
"""Run the exploratory nested regression comparison."""

import _bootstrap  # noqa: F401

from src.common import run
from src.extended import main

if __name__ == "__main__":
    run(main)
