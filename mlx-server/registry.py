"""Model registry and helper utilities for the MLX embedding server."""

from __future__ import annotations

import asyncio
import base64
import io
import os
from typing import Any

import httpx
import mlx.core as mx
import numpy as np

DEFAULT_MODEL = os.environ.get(
    "MLX_DEFAULT_MODEL", "mlx-community/Qwen3-Embedding-0.6B-mxfp8"
)


# ---------------------------------------------------------------------------
# Model registry — lazy-loads models on first use
# ---------------------------------------------------------------------------


class ModelRegistry:
    """Manages loaded MLX models with lazy initialization."""

    def __init__(self, default_model: str | None = None) -> None:
        self._text_models: dict[str, tuple[Any, Any]] = {}
        self._vl_models: dict[str, tuple[Any, Any]] = {}
        self.default_model = default_model

    def get_text_model(self, model_id: str) -> tuple[Any, Any]:
        """Return (model, tokenizer) for a text embedding model."""
        if model_id not in self._text_models:
            from mlx_embeddings import load

            model, tokenizer = load(model_id)
            self._text_models[model_id] = (model, tokenizer)
        return self._text_models[model_id]

    def get_vl_model(self, model_id: str) -> tuple[Any, Any]:
        """Return (model, processor) for a vision-language model."""
        if model_id not in self._vl_models:
            from mlx_vlm import load

            model, processor = load(model_id)
            self._vl_models[model_id] = (model, processor)
        return self._vl_models[model_id]

    def loaded_model_ids(self) -> list[str]:
        text_ids = list(self._text_models.keys())
        vl_ids = list(self._vl_models.keys())
        return text_ids + vl_ids


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def mx_to_list(arr: mx.array) -> list[list[float]]:
    """Convert MLX array to a list of float lists."""
    return np.array(arr).tolist()


async def decode_image(image_str: str, client: httpx.AsyncClient) -> Any:
    """Decode an image from a URL or base64 string.

    Uses the provided shared httpx client for URL fetches to benefit
    from connection pooling.
    """
    from PIL import Image

    if image_str.startswith("data:"):
        # data:image/...;base64,<data>
        _, encoded = image_str.split(",", 1)
        image_bytes = base64.b64decode(encoded)
        return Image.open(io.BytesIO(image_bytes))
    elif image_str.startswith("http://") or image_str.startswith("https://"):
        resp = await client.get(image_str)
        resp.raise_for_status()
        return Image.open(io.BytesIO(resp.content))
    else:
        # Assume raw base64
        image_bytes = base64.b64decode(image_str)
        return Image.open(io.BytesIO(image_bytes))


async def decode_images(image_strs: list[str], client: httpx.AsyncClient) -> list[Any]:
    """Decode multiple images concurrently."""
    if not image_strs:
        return []
    return list(
        await asyncio.gather(*[decode_image(img, client) for img in image_strs])
    )
