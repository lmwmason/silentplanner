# SPDX-License-Identifier: Apache-2.0
"""Run the train stage."""

import _bootstrap  # noqa: F401

from src.common import run
from src.models import train

if __name__ == "__main__":
    run(train)
