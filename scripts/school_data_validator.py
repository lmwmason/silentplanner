# SPDX-License-Identifier: Apache-2.0
"""Validate a school measurement CSV without writing an import."""

import argparse
import logging
from pathlib import Path

import _bootstrap  # noqa: F401

from src.common import run
from src.school_data_validator import validate_school_csv


def main() -> None:
    """Validate the file given on the command line."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("csv", type=Path)
    args = parser.parse_args()
    logging.info("validated_school_measurements=%s", len(validate_school_csv(args.csv)))


if __name__ == "__main__":
    run(main)
