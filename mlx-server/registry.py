"""Model registry and helper utilities for the MLX embedding server."""

from __future__ import annotations

import enum
import os
from dataclasses import dataclass, field
from typing import Any

import logging

import mlx.core as mx
import numpy as np

from prompts import load_prompt_catalog

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


class EmbedBackend(enum.Enum):
    """Which library loaded a text embedding model."""

    MLX_EMBEDDINGS = "mlx_embeddings"
    MLX_VLM = "mlx_vlm"


@dataclass(frozen=True)
class TextEmbedder:
    """A loaded text embedding model and what is needed to call it."""

    backend: EmbedBackend
    model: Any
    tokenizer: Any
    # Prompt name -> text to prepend; see ``prompts.py``.
    prompts: dict[str, str] = field(default_factory=dict)


# Encoders that embed other modalities. A text embedder drops them at load.
_NON_TEXT_TOWERS = ("vision_config", "audio_config")


def _load_text_embedder_mlx_vlm(model_id: str) -> tuple[Any, Any]:
    """Load a text embedding model through mlx-vlm, without its media towers.

    Weights keep the dtype stored in the checkpoint. Do not cast them:
    EmbeddingGemma 2 overflows float16 and returns NaN or degraded vectors.
    """
    from mlx_vlm.embedding_loader import load_embedding_model
    from mlx_vlm.utils import get_model_path, load_config
    from transformers import AutoTokenizer

    path = get_model_path(model_id)
    config = load_config(path)
    for tower in _NON_TEXT_TOWERS:
        if config.get(tower) is not None:
            config[tower] = None
    model = load_embedding_model(path, config=config)
    tokenizer = AutoTokenizer.from_pretrained(path)
    return model, tokenizer


class ModelRegistry:
    """Manages loaded MLX models with lazy initialization."""

    def __init__(self, default_model: str | None = None) -> None:
        self._text_models: dict[str, TextEmbedder] = {}
        self._vl_models: dict[str, tuple[Any, Any]] = {}
        self._chat_models: dict[str, tuple[ChatBackend, Any, Any]] = {}
        self.default_model = default_model

    def get_text_model(self, model_id: str) -> TextEmbedder:
        """Return the loaded text embedding model for ``model_id``.

        Tries mlx_embeddings first. If the model architecture is unsupported,
        falls back to mlx_vlm (e.g. EmbeddingGemma 2).
        """
        if model_id not in self._text_models:
            try:
                from mlx_embeddings import load

                logger.info("Trying mlx_embeddings for: %s", model_id)
                model, tokenizer = load(model_id)
                backend = EmbedBackend.MLX_EMBEDDINGS
            except (ValueError, KeyError) as exc:
                # Architecture not supported by mlx_embeddings — try mlx_vlm
                logger.info(
                    "mlx_embeddings unsupported for %s (%s), falling back to mlx_vlm",
                    model_id,
                    exc,
                )
                model, tokenizer = _load_text_embedder_mlx_vlm(model_id)
                backend = EmbedBackend.MLX_VLM
            logger.info("Loaded %s via %s", model_id, backend.value)
            self._text_models[model_id] = TextEmbedder(
                backend, model, tokenizer, load_prompt_catalog(model_id)
            )
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

    def clear(self) -> None:
        """Drop every cached model.

        Models hold MLX arrays bound to the thread that loaded them, so they
        must not outlive that thread. Call this on the MLX thread before it
        exits.
        """
        self._text_models.clear()
        self._vl_models.clear()
        self._chat_models.clear()

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
