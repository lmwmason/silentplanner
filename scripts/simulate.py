# SPDX-License-Identifier: Apache-2.0
"""Run the simulate stage."""

import _bootstrap  # noqa: F401

from src.common import run
from src.simulation import simulate

if __name__ == "__main__":
    run(simulate)
