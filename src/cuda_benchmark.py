# SPDX-License-Identifier: Apache-2.0
"""Train large acoustic regressors on CUDA and compare acquisition-batch errors."""

import argparse
import logging
import os
import platform
from datetime import UTC, datetime
from pathlib import Path
from time import perf_counter
from typing import Any

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import torch
from sklearn.model_selection import LeaveOneGroupOut
from torch import nn

from src.common import DATA, MODELS, RESULTS, configuration, digest, write_json
from src.cuda_models import build_model
from src.evaluation import save_figure
from src.models import GROUP, estimators, partitions, scores
from src.preprocessing import TABLE, TARGET

LOG = logging.getLogger(__name__)


def require_cuda() -> torch.device:
    """Require an NVIDIA device and configure deterministic training."""
    os.environ.setdefault("CUBLAS_WORKSPACE_CONFIG", ":4096:8")
    if not torch.cuda.is_available():
        raise ValueError("CUDA is required for this benchmark")
    torch.use_deterministic_algorithms(True)
    return torch.device("cuda")


def fit_predict(
    name: str,
    spec: dict,
    cfg: dict,
    features: list[str],
    training: pd.DataFrame,
    targets: dict[str, pd.DataFrame],
    device: torch.device,
    checkpoint: Path | None = None,
) -> tuple[dict[str, np.ndarray], dict[str, Any]]:
    """Fit for a fixed schedule using only training-derived scaling."""
    torch.manual_seed(cfg["seed"])
    torch.cuda.reset_peak_memory_stats(device)
    start = perf_counter()
    x = training[features].to_numpy(dtype=np.float32)
    y = training[[TARGET]].to_numpy(dtype=np.float32)
    x_mean, y_mean = x.mean(axis=0), y.mean(axis=0)
    x_scale, y_scale = x.std(axis=0), y.std(axis=0)
    x_scale[x_scale == 0] = 1
    y_scale[y_scale == 0] = 1

    def transform(values: pd.DataFrame) -> torch.Tensor:
        return torch.as_tensor(
            (values[features].to_numpy(dtype=np.float32) - x_mean) / x_scale, device=device
        )

    history = []
    model = build_model(spec, len(features), cfg["dropout"]).to(device)
    optimizer = torch.optim.AdamW(
        model.parameters(), lr=cfg["learning_rate"], weight_decay=cfg["weight_decay"], foreach=False
    )
    x_tensor = transform(training)
    y_tensor = torch.as_tensor((y - y_mean) / y_scale, device=device)
    objective = nn.SmoothL1Loss()
    model.train()
    for _ in range(cfg["epochs"]):
        optimizer.zero_grad(set_to_none=True)
        loss = objective(model(x_tensor), y_tensor)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        optimizer.step()
        history.append(loss.detach())
    final_loss = float(history[-1].item())
    if not np.isfinite(final_loss):
        raise ValueError(f"Nonfinite training loss: {name}")
    torch.cuda.synchronize(device)
    elapsed = perf_counter() - start
    model.eval()
    with torch.inference_mode():
        predictions = {
            label: model(transform(frame)).cpu().numpy().ravel() * y_scale[0] + y_mean[0]
            for label, frame in targets.items()
        }
    result = {
        "parameters": sum(parameter.numel() for parameter in model.parameters()),
        "fit_seconds": elapsed,
        "peak_allocated_bytes": torch.cuda.max_memory_allocated(device),
        "initial_training_loss": float(history[0].item()),
        "final_training_loss": final_loss,
    }
    if checkpoint is not None:
        checkpoint.parent.mkdir(parents=True, exist_ok=True)
        torch.save(
            {
                "model": name,
                "architecture": spec,
                "dropout": cfg["dropout"],
                "features": features,
                "x_mean": x_mean.tolist(),
                "x_scale": x_scale.tolist(),
                "y_mean": y_mean.tolist(),
                "y_scale": y_scale.tolist(),
                "weights": {key: value.cpu() for key, value in model.state_dict().items()},
            },
            checkpoint,
        )
        result["checkpoint_sha256"] = digest(checkpoint)
    return predictions, result


def evaluate_models(
    frame: pd.DataFrame, training_cfg: dict, cfg: dict, device: torch.device, profile: str
) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame]:
    """Evaluate the original split plus an exploratory leave-one-batch-out replay."""
    features = training_cfg["features"]
    original = partitions(frame, training_cfg)
    fixed, diagnostics, rows = [], [], []
    for name, spec in cfg["models"].items():
        out, metadata = fit_predict(
            name,
            spec,
            cfg,
            features,
            original["train"],
            {key: original[key] for key in ("validation", "test")},
            device,
            MODELS / f"{profile}_{name}.pt",
        )
        fixed.append(
            {
                "model": name,
                **metadata,
                **{
                    f"{key}_{metric}": value
                    for key, prediction in out.items()
                    for metric, value in scores(original[key][TARGET], prediction).items()
                },
            }
        )
        LOG.info(
            "mode=fixed model=%s parameters=%s seconds=%.2f",
            name,
            metadata["parameters"],
            metadata["fit_seconds"],
        )

    for train_ids, test_ids in LeaveOneGroupOut().split(frame, groups=frame[GROUP]):
        ordered_train, test = frame.iloc[train_ids], frame.iloc[test_ids]
        batch = test[GROUP].iloc[0]
        baseline = estimators(training_cfg)["baseline"].fit(
            ordered_train[features], ordered_train[TARGET]
        )
        predictions = {"baseline": baseline.predict(test[features])}
        for name, spec in cfg["models"].items():
            out, metadata = fit_predict(
                name,
                spec,
                cfg,
                features,
                ordered_train,
                {"test": test},
                device,
            )
            diagnostics.append({"model": name, "batch": batch, **metadata})
            predictions[name] = out["test"]
        for name, values in predictions.items():
            part = test[["recording_id", GROUP, "scene_type", TARGET]].copy()
            part["model"] = name
            part["predicted_dbfs"] = values
            rows.append(part)
        LOG.info("mode=leave_one_batch_out batch=%s", batch)
    return pd.DataFrame(fixed), pd.DataFrame(diagnostics), pd.concat(rows, ignore_index=True)


def write_results(
    fixed: pd.DataFrame, fits: pd.DataFrame, predictions: pd.DataFrame, output: Path
) -> None:
    """Write original-split results, batch diagnostics and a paired comparison plot."""
    batch_rows, summaries = [], []
    for name, part in predictions.groupby("model"):
        rows = [
            {
                "model": name,
                "batch": batch,
                "n": len(samples),
                **scores(samples[TARGET], samples["predicted_dbfs"]),
            }
            for batch, samples in part.groupby(GROUP)
        ]
        batch_rows.extend(rows)
        summaries.append(
            {
                "model": name,
                "batch_mae": np.mean([row["mae"] for row in rows]),
                **scores(part[TARGET], part["predicted_dbfs"]),
            }
        )
    by_batch = pd.DataFrame(batch_rows)
    summary = pd.DataFrame(summaries).sort_values("batch_mae")
    output.mkdir(parents=True, exist_ok=True)
    for name, table in {
        "original_split": fixed,
        "fits": fits,
        "predictions": predictions,
        "by_batch": by_batch,
        "summary": summary,
    }.items():
        table.to_csv(output / f"{name}.csv", index=False)
    fig, axis = plt.subplots(figsize=(9, 4))
    axis.barh(summary["model"].str.replace("_", " "), summary["batch_mae"], color="#376879")
    axis.invert_yaxis()
    axis.set(
        xlabel="Mean acquisition-batch MAE (dBFS)",
        title="CUDA models and median baseline | eight held-out batches",
    )
    axis.bar_label(axis.containers[0], fmt="%.3f", padding=4)
    axis.set_xlim(0, 1.14 * summary["batch_mae"].max())
    save_figure(fig, output / "comparison.png")


def main() -> None:
    """Run the CUDA experiment and record hardware and experiment provenance."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scale", action="store_true", help="Run the 100M-plus model profile")
    profile = "cuda_scale" if parser.parse_args().scale else "cuda"
    device = require_cuda()
    training_cfg, cfg = configuration("training"), configuration("cuda")
    if profile == "cuda_scale":
        cfg |= configuration(profile)
    output = RESULTS / profile
    frame = pd.read_csv(TABLE)
    fixed, fits, predictions = evaluate_models(frame, training_cfg, cfg, device, profile)
    write_results(fixed, fits, predictions, output)
    write_json(
        output / "run.json",
        {
            "profile": profile,
            "utc": datetime.now(UTC).isoformat(),
            "python": platform.python_version(),
            "torch": torch.__version__,
            "cuda_runtime": torch.version.cuda,
            "device": torch.cuda.get_device_name(device),
            "gpu_memory_bytes": torch.cuda.get_device_properties(device).total_memory,
            "compute_capability": list(torch.cuda.get_device_capability(device)),
            "dataset_version": configuration("dataset")["version"],
            "table_sha256": digest(TABLE),
            "subset_manifest_sha256": digest(DATA / "manifests" / "subset.json"),
            "settings": cfg,
            "training": training_cfg,
            "rows": len(frame),
            "groups": int(frame[GROUP].nunique()),
        },
    )
