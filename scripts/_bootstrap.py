# SPDX-License-Identifier: Apache-2.0
"""Put the project root on the import path for direct script execution."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
