# SPDX-License-Identifier: Apache-2.0
"""Build or verify the research ZIP."""

import _bootstrap  # noqa: F401

from src.common import run
from src.release import main

if __name__ == "__main__":
    run(main)
