# SPDX-License-Identifier: Apache-2.0
"""Large PyTorch models for the three-feature acoustic regression task."""

from typing import Any

import torch
from torch import nn


class ResidualBlock(nn.Module):
    """Apply a normalized nonlinear residual update."""

    def __init__(self, width: int, dropout: float) -> None:
        super().__init__()
        self.update = nn.Sequential(
            nn.LayerNorm(width),
            nn.Linear(width, width),
            nn.GELU(),
            nn.Dropout(dropout),
            nn.Linear(width, width),
            nn.Dropout(dropout),
        )

    def forward(self, values: torch.Tensor) -> torch.Tensor:
        """Add a learned update to the input."""
        return values + self.update(values)


class FeatureTransformer(nn.Module):
    """Encode one token per feature and predict with a learned summary token."""

    def __init__(self, count: int, width: int, depth: int, heads: int, dropout: float) -> None:
        super().__init__()
        self.weight = nn.Parameter(torch.randn(count, width) * 0.02)
        self.bias = nn.Parameter(torch.zeros(count, width))
        self.summary = nn.Parameter(torch.zeros(1, 1, width))
        self.encoder = nn.TransformerEncoder(
            nn.TransformerEncoderLayer(
                width,
                heads,
                dim_feedforward=4 * width,
                dropout=dropout,
                batch_first=True,
                norm_first=True,
                activation="gelu",
            ),
            num_layers=depth,
            enable_nested_tensor=False,
        )
        for layer in self.encoder.layers:
            # Encoder layers are cloned by PyTorch and therefore need independent initialization.
            for parameter in layer.parameters():
                if parameter.ndim > 1:
                    nn.init.xavier_uniform_(parameter)
        self.readout = nn.Sequential(nn.LayerNorm(width), nn.Linear(width, 1))

    def forward(self, values: torch.Tensor) -> torch.Tensor:
        """Form feature and summary tokens, then read the encoded summary."""
        tokens = values.unsqueeze(-1) * self.weight + self.bias
        tokens = torch.cat((self.summary.expand(len(values), -1, -1), tokens), dim=1)
        return self.readout(self.encoder(tokens)[:, 0])


def build_model(spec: dict[str, Any], features: int, dropout: float) -> nn.Module:
    """Instantiate one configured architecture."""
    width, depth = spec["width"], spec["depth"]
    if spec["architecture"] == "residual_mlp":
        return nn.Sequential(
            nn.Linear(features, width),
            *(ResidualBlock(width, dropout) for _ in range(depth)),
            nn.LayerNorm(width),
            nn.Linear(width, 1),
        )
    if spec["architecture"] == "transformer":
        return FeatureTransformer(features, width, depth, spec["heads"], dropout)
    raise ValueError(f"Unknown architecture: {spec['architecture']}")
