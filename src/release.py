# SPDX-License-Identifier: Apache-2.0
"""Assemble and verify an explicit, hash-indexed release archive."""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path, PurePosixPath
from zipfile import ZIP_DEFLATED, ZipFile

from src.common import ROOT, digest

PREFIX = "school-acoustic-ml"
NAME = "school-acoustic-ml-complete.zip"
INDEX = "RELEASE_CONTENTS.json"
REQUIRED = {
    "README.md",
    "LICENSE",
    "planner/Cargo.toml",
    "planner/Cargo.lock",
    "planner/README.md",
    "planner/src/main.rs",
    "NOTICE",
    "THIRD_PARTY_NOTICES.md",
    "requirements.txt",
    "requirements-lock.txt",
    "data/manifests/subset.json",
    "data/processed/recordings.csv",
    "results/metrics/test.csv",
    "results/predictions/test.csv",
    "results/figures/virtual_school.png",
    "models/baseline.joblib",
    "models/linear.joblib",
    "models/random_forest.joblib",
    "models/gradient_boosting.joblib",
    "models/extended_selected.joblib",
    "results/extended/run.json",
    "results/extended/summary.csv",
    "results/extended/by_batch.csv",
    "results/extended/predictions.csv",
    "results/extended/inner_tuning.csv",
    "results/extended/final_validation_folds.csv",
    "results/extended/final_ranking.csv",
    "results/extended/comparison.png",
    "models/cuda_residual_mlp_large.pt",
    "models/cuda_residual_mlp_xl.pt",
    "models/cuda_feature_transformer.pt",
    "results/cuda/run.json",
    "results/cuda/summary.csv",
    "results/cuda/original_split.csv",
    "results/cuda/by_batch.csv",
    "results/cuda/predictions.csv",
    "results/cuda/fits.csv",
    "results/cuda/comparison.png",
    "results/cuda_scale/run.json",
    "results/cuda_scale/summary.csv",
    "results/cuda_scale/original_split.csv",
    "results/cuda_scale/by_batch.csv",
    "results/cuda_scale/predictions.csv",
    "results/cuda_scale/fits.csv",
    "results/cuda_scale/comparison.png",
    "results/followup/attenuation_metrics.csv",
    "results/followup/stability_summary.json",
    "results/followup/field_plan.json",
    "results/followup/field_validation_status.json",
    "results/followup/sensitivity_demo.png",
}


def safe_name(name: str) -> bool:
    """Allow normal relative POSIX paths only."""
    path = PurePosixPath(name)
    return bool(
        name
        and not path.is_absolute()
        and ".." not in path.parts
        and "\\" not in name
        and path.as_posix() == name
    )


def write_archive(root: Path, paths: list[str], output: Path) -> None:
    """Write files and their SHA-256 digests into a new ZIP."""
    if len(set(paths)) != len(paths) or INDEX in paths:
        raise ValueError("Duplicate or reserved release paths")
    with ZipFile(output, "w", ZIP_DEFLATED, compresslevel=6) as archive:
        manifest = {}
        for name in sorted(paths):
            if not safe_name(name) or not (root / name).is_file() or (root / name).is_symlink():
                raise ValueError(f"Invalid release file: {name}")
            path = root / name
            archive.write(path, f"{PREFIX}/{name}")
            manifest[name] = digest(path)
        archive.writestr(f"{PREFIX}/{INDEX}", json.dumps(manifest, indent=2) + "\n")


def verify_archive(path: Path, required: set[str]) -> None:
    """Check the central directory, CRCs, paths, required files and SHA-256s."""
    with ZipFile(path) as archive:
        if archive.testzip() is not None:
            raise ValueError("Archive CRC verification failed")
        index_name = f"{PREFIX}/{INDEX}"
        manifest = json.loads(archive.read(index_name))
        names = archive.namelist()
        if (
            len(names) != len(set(names))
            or not required <= manifest.keys()
            or set(names) != {index_name, *(f"{PREFIX}/{name}" for name in manifest)}
        ):
            raise ValueError("Release contents differ from the required manifest")
        for name, expected in manifest.items():
            if not safe_name(name):
                raise ValueError(f"Unsafe archive member: {name}")
            with archive.open(f"{PREFIX}/{name}") as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != expected:
                    raise ValueError(f"Archive member SHA-256 mismatch: {name}")


def main() -> None:
    """Build from clean committed sources or verify an existing ZIP."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", type=Path)
    args = parser.parse_args()
    if args.verify is not None:
        verify_archive(args.verify, REQUIRED)
        print(f"Verified {args.verify}")
        return
    changes = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)
    if changes:
        raise ValueError("Commit source changes before packaging")
    status_path = ROOT / "results" / "followup" / "field_validation_status.json"
    if json.loads(status_path.read_text()).get("status") != "awaiting_measurements":
        raise ValueError("Regenerate the fictional field demo before packaging")
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode()
    paths = set(tracked.rstrip("\0").split("\0"))
    paths.add("data/processed/recordings.csv")
    paths.update(name for name in REQUIRED if name.endswith(".pt"))
    for pattern in (
        "models/*.joblib",
        "results/metrics/*",
        "results/figures/*.png",
        "results/predictions/*.csv",
        "results/experiments/*",
        "results/simulation/*.csv",
        "results/extended/*",
        "results/cuda/*",
        "results/cuda_scale/*",
        "results/followup/attenuation_*.csv",
        "results/followup/attenuation_run.json",
        "results/followup/stability_summary.json",
        "results/followup/sensitivity.json",
        "results/followup/field_plan.json",
        "results/followup/field_validation_status.json",
        "results/followup/sensitivity_demo.png",
    ):
        paths.update(
            path.relative_to(ROOT).as_posix() for path in ROOT.glob(pattern) if path.is_file()
        )
    if any(
        name.endswith((".wav", ".flac")) or name.startswith(("data/raw/", "data/school/"))
        for name in paths
    ):
        raise ValueError("Release paths contain raw audio or private school observations")
    artifact = ROOT / NAME
    temporary = artifact.with_suffix(".zip.tmp")
    try:
        write_archive(ROOT, sorted(paths), temporary)
        verify_archive(temporary, REQUIRED)
        temporary.replace(artifact)
    finally:
        temporary.unlink(missing_ok=True)
    print(f"Created and verified {artifact}")
