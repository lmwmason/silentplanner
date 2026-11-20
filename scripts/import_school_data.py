# SPDX-License-Identifier: Apache-2.0
"""Import school observations."""

import _bootstrap  # noqa: F401

from src.common import run
from src.import_school_data import main

if __name__ == "__main__":
    run(main)
