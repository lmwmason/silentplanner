# SPDX-License-Identifier: Apache-2.0
"""Evaluate a frozen field plan against independently collected meter readings."""

import _bootstrap  # noqa: F401

from src.common import run
from src.field_validation import main

if __name__ == "__main__":
    run(main)
