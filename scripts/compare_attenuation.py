# SPDX-License-Identifier: Apache-2.0
"""Run the distance-attenuation baseline comparison."""

import _bootstrap  # noqa: F401

from src.attenuation import main
from src.common import run

if __name__ == "__main__":
    run(main)
