# SPDX-License-Identifier: Apache-2.0
"""Check model shapes and perform one CUDA optimizer step when available."""

import pytest

torch = pytest.importorskip("torch")

from src.cuda_benchmark import require_cuda  # noqa: E402
from src.cuda_models import build_model  # noqa: E402


@pytest.mark.parametrize("architecture", ["residual_mlp", "transformer"])
def test_model_forward(architecture: str) -> None:
    spec = {"architecture": architecture, "width": 16, "heads": 4, "depth": 2}
    model = build_model(spec, 3, 0.1)
    assert model(torch.zeros(5, 3)).shape == (5, 1)


@pytest.mark.skipif(not torch.cuda.is_available(), reason="Requires an NVIDIA CUDA device")
def test_gpu_step() -> None:
    device = require_cuda()
    model = build_model(
        {"architecture": "transformer", "width": 16, "heads": 4, "depth": 2}, 3, 0.1
    ).to(device)
    optimizer = torch.optim.AdamW(model.parameters())
    target = torch.ones((5, 1), device=device)
    x = torch.zeros((5, 3), device=device)
    before = model.readout[-1].bias.detach().clone()
    loss = (model(x) - target).square().mean()
    loss.backward()
    optimizer.step()
    assert not torch.equal(before, model.readout[-1].bias.detach())
