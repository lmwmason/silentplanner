# SPDX-License-Identifier: Apache-2.0
"""Build a reproducible fictional field plan and plot its ranking stability."""

import json
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path
from tempfile import TemporaryDirectory

import _bootstrap  # noqa: F401
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.colors import ListedColormap
from matplotlib.patches import Patch

from src.common import RESULTS, ROOT, run, write_json
from src.evaluation import save_figure


def main() -> None:
    """Export the fictional site and confirm its empty measurement workflow."""
    output = RESULTS / "followup"
    output.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory() as temp:
        bundle = Path(
            subprocess.check_output(
                [
                    "cargo",
                    "run",
                    "--quiet",
                    "--locked",
                    "--manifest-path",
                    "planner/Cargo.toml",
                    "--release",
                    "--",
                    "--demo-offline",
                    temp,
                ],
                cwd=ROOT,
                text=True,
            ).strip()
        )
        data = json.loads((bundle / "analysis.json").read_text())["analysis"]
        stability = json.loads((bundle / "sensitivity.json").read_text())
        for name in ("field_plan.json", "sensitivity.json"):
            shutil.copyfile(bundle / name, output / name)
    subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts" / "validate_field_rankings.py"),
            str(output / "field_plan.json"),
            str(ROOT / "school_collection" / "field_rankings_template.csv"),
            str(output / "field_validation_status.json"),
        ],
        check=True,
    )
    counts = Counter(cell["class"] for cell in stability["cells"])
    write_json(
        output / "stability_summary.json",
        {
            "counts": dict(counts),
            "completed": stability["completed"],
            "skipped": stability["skipped"],
            "planned_pairs": len(json.loads((output / "field_plan.json").read_text())["pairs"]),
        },
    )
    palette = {
        "unsupported": "#f5f7fa",
        "stable_other": "#5187a9",
        "uncertain": "#f4a94a",
        "stable_low": "#35daa7",
    }
    grid = np.array([list(palette).index(cell["class"]) for cell in stability["cells"]])
    bounds = data["bounds"]
    fig, ax = plt.subplots(figsize=(7, 6))
    ax.imshow(
        grid.reshape(data["rows"], data["columns"]),
        origin="lower",
        extent=(bounds["min"]["x"], bounds["max"]["x"], bounds["min"]["y"], bounds["max"]["y"]),
        interpolation="nearest",
        cmap=ListedColormap(list(palette.values())),
        vmin=0,
        vmax=3,
    )
    ax.set(
        title="Fictional site | ranking sensitivity",
        xlabel="Local easting (m)",
        ylabel="Local northing (m)",
    )
    ax.legend(
        handles=[
            Patch(facecolor=color, label=label.replace("_", " ").title())
            for label, color in palette.items()
        ],
        loc="upper right",
        fontsize=8,
    )
    save_figure(fig, output / "sensitivity_demo.png")
    print(dict(counts))


if __name__ == "__main__":
    run(main)
