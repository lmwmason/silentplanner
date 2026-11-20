# SPDX-License-Identifier: Apache-2.0
"""Run the evaluate stage."""

import _bootstrap  # noqa: F401

from src.common import run
from src.evaluation import evaluate

if __name__ == "__main__":
    run(evaluate)
