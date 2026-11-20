# SPDX-License-Identifier: Apache-2.0
"""Run the large-model benchmark on an NVIDIA GPU."""

import _bootstrap  # noqa: F401

from src.common import run
from src.cuda_benchmark import main

if __name__ == "__main__":
    run(main)
