# SPDX-License-Identifier: Apache-2.0
"""Check all acquired files."""

import _bootstrap  # noqa: F401

from src.common import run
from src.datasets import validate

if __name__ == "__main__":
    run(validate)
