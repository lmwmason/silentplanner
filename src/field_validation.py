# SPDX-License-Identifier: Apache-2.0
"""Evaluate paired source-position observations against a frozen map plan."""

import argparse
import json
from pathlib import Path
from typing import Any

import numpy as np
import pandas as pd
from scipy.stats import binomtest

from src.common import digest, write_json

COLUMNS = (
    "session_id",
    "pair_id",
    "role",
    "receiver_id",
    "source_on_laeq_dba",
    "background_laeq_dba",
    "duration_s",
    "device_id",
    "reference_id",
    "calibration_checked",
)
MIN_MARGIN_DB = 6.0


def evaluate(plan: dict[str, Any], observations: pd.DataFrame) -> dict[str, Any]:
    """Estimate the directional agreement of complete and controlled pairs."""
    if tuple(observations.columns) != COLUMNS:
        raise ValueError("Measurements do not match the field validation template.")
    if observations.empty:
        return {"status": "awaiting_measurements", "rows": 0}
    ids = {"session_id", "pair_id", "role", "receiver_id", "device_id", "reference_id"}
    if any(
        observations[key].isna().any() or observations[key].astype(str).str.strip().eq("").any()
        for key in ids
    ):
        raise ValueError("Identifiers must be filled in.")
    allowed = {pair["id"] for pair in plan["pairs"]}
    weights = {
        f"R{index:02}": float(zone["weight"])
        for index, zone in enumerate(plan["receiver_zones"], 1)
    }
    if not all(0.1 <= weight <= 1 for weight in weights.values()):
        raise ValueError("Frozen receiver sensitivities must be between 0.1 and 1.")
    expected = {(role, receiver) for role in ("lower", "higher") for receiver in weights}
    rows = observations.copy()
    if not rows["pair_id"].isin(allowed).all() or not expected:
        raise ValueError("Observed pair or receiver is absent from the frozen plan.")
    columns = ["source_on_laeq_dba", "background_laeq_dba", "duration_s"]
    for key in columns:
        rows[key] = pd.to_numeric(rows[key], errors="raise")
    if not np.isfinite(rows[columns].to_numpy()).all() or not rows["duration_s"].eq(60).all():
        raise ValueError("Use finite readings and a duration of exactly 60 seconds.")
    if not rows["calibration_checked"].astype(str).str.lower().eq("yes").all():
        raise ValueError("A checked meter is required for every validation measurement.")
    if not rows[columns[:2]].apply(lambda series: series.between(0, 150).all()).all():
        raise ValueError("Field sound levels must be between 0 and 150 dBA.")
    if rows["reference_id"].nunique() != 1:
        raise ValueError("Use the same reference source throughout the experiment.")
    pairs, excluded = [], []
    for (session, pair), group in rows.groupby(["session_id", "pair_id"]):
        entries = list(zip(group["role"], group["receiver_id"], strict=True))
        if len(set(entries)) != len(entries) or not set(entries) <= expected:
            raise ValueError(f"Duplicate or unknown role and receiver in {session}/{pair}.")
        on, bg = group["source_on_laeq_dba"], group["background_laeq_dba"]
        if set(entries) != expected or group["device_id"].nunique() != 1:
            excluded.append(
                {"session": session, "pair": pair, "reason": "incomplete or mixed device"}
            )
            continue
        if (on - bg < MIN_MARGIN_DB).any():
            excluded.append(
                {"session": session, "pair": pair, "reason": "insufficient background margin"}
            )
            continue
        energy = group["receiver_id"].map(weights) * (10 ** (on / 10) - 10 ** (bg / 10))
        by_role = energy.groupby(group["role"]).max()
        difference = 10 * np.log10(by_role["higher"] / by_role["lower"])
        pairs.append({"session": session, "pair": pair, "higher_minus_lower_db": float(difference)})
    summary: dict[str, Any] = {
        "status": "evaluated" if pairs else "no_eligible_pairs",
        "rows": len(rows),
        "pairs": pairs,
        "excluded": excluded,
    }
    if pairs:
        sessions = pd.DataFrame(pairs).groupby("session")["higher_minus_lower_db"].median()
        wins, losses = int((sessions > 0).sum()), int((sessions < 0).sum())
        probability = (
            binomtest(wins, wins + losses, alternative="greater").pvalue if wins + losses else None
        )
        summary.update(
            independent_sessions=len(sessions),
            sessions_agree=wins,
            sessions_disagree=losses,
            session_median_delta_db=float(sessions.median()),
            one_sided_sign_test_p=probability,
        )
    return summary


def main() -> None:
    """Load a frozen field plan and write a reproducible validation summary."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", type=Path)
    parser.add_argument("measurements", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text())
    observations = pd.read_csv(args.measurements, dtype=str)
    report = evaluate(plan, observations)
    report.update(plan_sha256=digest(args.plan), measurements_sha256=digest(args.measurements))
    write_json(args.output, report)
    print(report["status"])
