# SPDX-License-Identifier: Apache-2.0
"""Retrieve a verified, channel-matched subset of the publisher's ZIP64 archive."""

import csv
import logging
import re
import shutil
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any
from zipfile import BadZipFile, ZipFile

from src.common import DATA, RAW, configuration, digest, write_json

LOG = logging.getLogger(__name__)
REQUIRED_COLUMNS = {
    "recordings": {
        "recording_id",
        "session_id",
        "has_uav",
        "flight_state",
        "is_duplicate_file",
        "qc_status",
        "acquisition_batch_id",
        "nominal_condition_id",
        "scene_type",
        "horizontal_distance_m",
        "height_m",
        "azimuth_deg",
        "duration_seconds",
    },
    "files": {
        "recording_id",
        "channel_index",
        "relative_path",
        "sample_count",
        "file_size_bytes",
        "sha256",
    },
    "sessions": {"session_id"},
}


def fetch_range(cfg: dict[str, Any], start: int, end: int) -> Path:
    """Cache an exact HTTP 206 response and reject unexpected full responses."""
    cache = RAW / "ranges"
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / f"{start}-{end}"
    size = end - start + 1
    if path.exists() and path.stat().st_size == size:
        return path
    part = path.with_suffix(".part")
    url = f"{cfg['url']}?segment={start}-{end}"
    for attempt in range(3):
        response = subprocess.run(
            [
                "curl",
                "-fLsS",
                "--connect-timeout",
                "12",
                "--max-time",
                "90",
                "--max-filesize",
                str(size),
                "-r",
                f"{start}-{end}",
                "-D",
                "-",
                "-o",
                str(part),
                url,
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        expected = f"bytes {start}-{end}/{cfg['archive_size']}"
        if (
            response.returncode == 0
            and part.stat().st_size == size
            and re.search(
                r"^content-range:\s*" + re.escape(expected) + r"\s*$",
                response.stdout,
                flags=re.I | re.M,
            )
        ):
            part.replace(path)
            return path
        LOG.warning(
            "range_start=%s attempt=%s transfer_exit=%s", start, attempt + 1, response.returncode
        )
    raise ValueError(f"Could not acquire verified HTTP byte range {start}-{end}")


def metadata(name: str) -> list[dict[str, str]]:
    """Read the original publisher CSV selected by name."""
    with (RAW / f"{name}.csv").open(encoding="utf-8-sig", newline="") as stream:
        reader = csv.DictReader(stream)
        fields = reader.fieldnames or []
        if len(set(fields)) != len(fields) or not REQUIRED_COLUMNS[name] <= set(fields):
            raise ValueError(f"Unexpected publisher CSV schema: {name}")
        rows = list(reader)
    if not rows or any(None in row or None in row.values() for row in rows):
        raise ValueError(f"Malformed publisher CSV: {name}")
    return rows


def selection(cfg: dict[str, Any]) -> list[dict[str, str]]:
    """Join one fixed channel to each eligible physical recording."""
    recordings = [
        row
        for row in metadata("recordings")
        if row["has_uav"] == "true"
        and row["flight_state"] == "hover"
        and row["is_duplicate_file"] == "false"
        and row["qc_status"] in {"pass", "pass_with_warning"}
    ]
    channels = [row for row in metadata("files") if int(row["channel_index"]) == cfg["channel"]]
    sessions = {row["session_id"] for row in metadata("sessions")}
    by_recording = {row["recording_id"]: row for row in channels}
    if len(by_recording) != len(channels):
        raise ValueError("Duplicate channel entries in the publisher index")
    if any(row["session_id"] not in sessions for row in recordings):
        raise ValueError("Recording refers to a missing session")
    if any(row["recording_id"] not in by_recording for row in recordings):
        raise ValueError("Recording is missing its selected channel")
    rows = [row | by_recording[row["recording_id"]] for row in recordings]
    if len(rows) != 200 or any(
        len({row[key] for row in rows}) != len(rows)
        for key in ("recording_id", "relative_path", "sha256")
    ):
        raise ValueError("Unexpected subset size or duplicate recordings")
    return rows


def verify(path: Path, sha256: str, expected_size: int | None = None) -> None:
    """Verify file length and content digest against a pinned value."""
    if (expected_size is not None and path.stat().st_size != expected_size) or digest(
        path
    ) != sha256:
        raise ValueError(f"Integrity mismatch: {path.name}")


def download() -> None:
    """Resume at completed byte ranges and preserve only original WAV/CSV bytes."""
    cfg = configuration("dataset")
    RAW.mkdir(parents=True, exist_ok=True)
    if cfg["license"] != "CC-BY-4.0":
        raise ValueError("Review the dataset license before acquisition")
    sparse = RAW / "archive-index.sparse"
    with sparse.open("wb") as stream:
        for start, end in [
            (cfg["directory_start"], cfg["archive_size"] - 1),
            (cfg["metadata_start"], cfg["metadata_end"]),
        ]:
            stream.seek(start)
            stream.write(fetch_range(cfg, start, end).read_bytes())
    with ZipFile(sparse) as archive:
        for name, sha in cfg["metadata"].items():
            path = RAW / f"{name}.csv"
            path.write_bytes(archive.read(f"dataset/metadata/{name}.csv"))
            verify(path, sha)
        rows = selection(cfg)
        members = sorted(archive.infolist(), key=lambda member: member.header_offset)
        ends = {a.filename: b.header_offset - 1 for a, b in zip(members, members[1:])}
        jobs = [(row, archive.getinfo("dataset/" + row["relative_path"])) for row in rows]
    audio = RAW / "audio"
    audio.mkdir(exist_ok=True)
    needed = 20_000_000
    for row, member in jobs:
        if not (audio / f"{row['recording_id']}.wav").exists():
            needed += member.file_size
            span = RAW / "ranges" / f"{member.header_offset}-{ends[member.filename]}"
            if not span.exists():
                needed += ends[member.filename] - member.header_offset + 1
    if shutil.disk_usage(RAW).free < needed:
        raise ValueError(f"Insufficient disk space for remaining payload (need {needed} bytes)")

    def retrieve(job: tuple) -> dict[str, Any]:
        row, member = job
        path = audio / f"{row['recording_id']}.wav"
        start, end = member.header_offset, ends[member.filename]
        if not path.exists():
            span = fetch_range(cfg, start, end)
            # Each worker reads only its own original local-file record and the shared ZIP index.
            worker_zip = RAW / "ranges" / f"{start}.tmp"
            with worker_zip.open("wb") as stream:
                stream.seek(start)
                stream.write(span.read_bytes())
                stream.seek(cfg["directory_start"])
                stream.write(
                    fetch_range(cfg, cfg["directory_start"], cfg["archive_size"] - 1).read_bytes()
                )
            part = path.with_suffix(".part")
            try:
                with ZipFile(worker_zip) as archive, archive.open(member.filename) as source:
                    with part.open("wb") as target:
                        shutil.copyfileobj(source, target)
                verify(part, row["sha256"], int(row["file_size_bytes"]))
                part.replace(path)
            except (BadZipFile, ValueError) as error:
                span.unlink(missing_ok=True)
                raise ValueError(f"Invalid ZIP member: {member.filename}") from error
            finally:
                worker_zip.unlink(missing_ok=True)
        verify(path, row["sha256"], int(row["file_size_bytes"]))
        return {
            "recording_id": row["recording_id"],
            "member": member.filename,
            "sha256": row["sha256"],
            "bytes": path.stat().st_size,
            "range": [start, end],
        }

    with ThreadPoolExecutor(max_workers=8) as pool:
        entries = []
        for entry in pool.map(retrieve, jobs):
            entries.append(entry)
            if len(entries) % 20 == 0:
                LOG.info("verified_recordings=%s", len(entries))
    manifest = {
        "version": cfg["version"],
        "doi": cfg["doi"],
        "license": cfg["license"],
        "url": cfg["url"],
        "publisher_full_archive_sha256": cfg["archive_sha256"],
        "metadata_sha256": cfg["metadata"],
        "members": entries,
        "wav_bytes": sum(entry["bytes"] for entry in entries),
        "selected_range_bytes": sum(entry["range"][1] - entry["range"][0] + 1 for entry in entries)
        + cfg["archive_size"]
        - cfg["directory_start"]
        + cfg["metadata_end"]
        - cfg["metadata_start"]
        + 1,
    }
    write_json(DATA / "manifests" / "subset.json", manifest)


def validate() -> None:
    """Verify metadata and every selected WAV against the publisher hashes."""
    cfg = configuration("dataset")
    for name, sha in cfg["metadata"].items():
        verify(RAW / f"{name}.csv", sha)
    rows = selection(cfg)
    for row in rows:
        verify(
            RAW / "audio" / f"{row['recording_id']}.wav", row["sha256"], int(row["file_size_bytes"])
        )
    LOG.info("verified_recordings=%s", len(rows))
