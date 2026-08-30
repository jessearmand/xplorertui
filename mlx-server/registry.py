"""Model registry and helper utilities for the MLX embedding server."""

from __future__ import annotations

import enum
import os
from typing import Any

import logging

import mlx.core as mx
import numpy as np

logger = logging.getLogger("mlx-server")

DEFAULT_MODEL = os.environ.get(
    "MLX_DEFAULT_MODEL", "mlx-community/Qwen3-Embedding-0.6B-mxfp8"
)

DEFAULT_CHAT_MODEL = os.environ.get(
    "MLX_DEFAULT_CHAT_MODEL", "mlx-community/Qwen3.5-0.8B-OptiQ-4bit"
)


# ---------------------------------------------------------------------------
# Model registry — lazy-loads models on first use
# ---------------------------------------------------------------------------


class ChatBackend(enum.Enum):
    """Which library loaded a chat model."""

    MLX_LM = "mlx_lm"
    MLX_VLM = "mlx_vlm"


class ModelRegistry:
    """Manages loaded MLX models with lazy initialization."""

    def __init__(self, default_model: str | None = None) -> None:
        self._text_models: dict[str, tuple[Any, Any]] = {}
        self._vl_models: dict[str, tuple[Any, Any]] = {}
        self._chat_models: dict[str, tuple[ChatBackend, Any, Any]] = {}
        self.default_model = default_model

    def get_text_model(self, model_id: str) -> tuple[Any, Any]:
        """Return (model, tokenizer) for a text embedding model."""
        if model_id not in self._text_models:
            from mlx_embeddings import load

            model, tokenizer = load(model_id)
            self._text_models[model_id] = (model, tokenizer)
        return self._text_models[model_id]

    def get_chat_model(self, model_id: str) -> tuple[ChatBackend, Any, Any]:
        """Return (backend, model, tokenizer/processor) for a chat model.

        Tries mlx_lm first (text-only LLMs). If the model architecture is
        unsupported, falls back to mlx_vlm (vision-language models like gemma-4).
        """
        if model_id not in self._chat_models:
            try:
                from mlx_lm import load

                logger.info("Trying mlx_lm for: %s", model_id)
                loaded = load(model_id, lazy=True)
                model = loaded[0]
                tokenizer = loaded[1]
                self._chat_models[model_id] = (ChatBackend.MLX_LM, model, tokenizer)
                logger.info("Loaded %s via mlx_lm", model_id)
            except (ValueError, KeyError) as exc:
                # Architecture not supported by mlx_lm — try mlx_vlm
                logger.info(
                    "mlx_lm unsupported for %s (%s), falling back to mlx_vlm",
                    model_id,
                    exc,
                )
                from mlx_vlm import load

                model, processor = load(model_id)
                self._chat_models[model_id] = (ChatBackend.MLX_VLM, model, processor)
                logger.info("Loaded %s via mlx_vlm", model_id)
        return self._chat_models[model_id]

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
        chat_ids = list(self._chat_models.keys())
        return text_ids + vl_ids + chat_ids


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def mx_to_list(arr: mx.array) -> list[list[float]]:
    """Convert MLX array to a list of float lists."""
    return np.array(arr).tolist()
