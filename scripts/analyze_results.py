# SPDX-License-Identifier: Apache-2.0
"""Run the analyze_results stage."""

import _bootstrap  # noqa: F401

from src.common import run
from src.evaluation import analyze

if __name__ == "__main__":
    run(analyze)
