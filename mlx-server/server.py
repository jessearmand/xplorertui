"""
MLX Embedding Server — OpenAI-compatible REST API for local embedding inference.

Supports text embeddings via mlx-embeddings and multimodal (image+text)
embeddings via mlx-vlm.  Designed to be called from the xplorertui Rust TUI.

Usage:
    uv run server.py --model mlx-community/Qwen3-Embedding-0.6B-mxfp8 --port 8678
"""

from __future__ import annotations

import argparse
import base64
import io
import time
from contextlib import asynccontextmanager
from typing import Any

import mlx.core as mx
import numpy as np
import uvicorn
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

# ---------------------------------------------------------------------------
# Request / response schemas (OpenAI-compatible)
# ---------------------------------------------------------------------------


class EmbeddingRequest(BaseModel):
    model: str
    input: list[str]


class EmbeddingData(BaseModel):
    object: str = "embedding"
    embedding: list[float]
    index: int


class EmbeddingUsage(BaseModel):
    prompt_tokens: int = 0
    total_tokens: int = 0


class EmbeddingResponse(BaseModel):
    object: str = "list"
    data: list[EmbeddingData]
    model: str
    usage: EmbeddingUsage = Field(default_factory=EmbeddingUsage)


class MultimodalEmbeddingRequest(BaseModel):
    model: str
    texts: list[str]
    images: list[str] = Field(
        default_factory=list,
        description="Image URLs or base64-encoded image data",
    )


class ModelInfo(BaseModel):
    id: str
    object: str = "model"
    owned_by: str = "mlx-community"


class ModelsResponse(BaseModel):
    object: str = "list"
    data: list[ModelInfo]


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


registry = ModelRegistry()


# ---------------------------------------------------------------------------
# FastAPI app
# ---------------------------------------------------------------------------


@asynccontextmanager
async def lifespan(app: FastAPI):
    # Pre-load default model at startup if specified.
    if registry.default_model:
        print(f"Pre-loading default model: {registry.default_model}")
        registry.get_text_model(registry.default_model)
        print("Default model loaded.")
    yield


app = FastAPI(
    title="MLX Embedding Server",
    version="0.1.0",
    lifespan=lifespan,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _mx_to_list(arr: mx.array) -> list[list[float]]:
    """Convert MLX array to a list of float lists."""
    return np.array(arr).tolist()


def _decode_image(image_str: str) -> Any:
    """Decode an image from a URL or base64 string."""
    from PIL import Image

    if image_str.startswith("data:"):
        # data:image/...;base64,<data>
        _, encoded = image_str.split(",", 1)
        image_bytes = base64.b64decode(encoded)
        return Image.open(io.BytesIO(image_bytes))
    elif image_str.startswith("http://") or image_str.startswith("https://"):
        import urllib.request

        with urllib.request.urlopen(image_str) as resp:
            image_bytes = resp.read()
        return Image.open(io.BytesIO(image_bytes))
    else:
        # Assume raw base64
        image_bytes = base64.b64decode(image_str)
        return Image.open(io.BytesIO(image_bytes))


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------


@app.get("/v1/models", response_model=ModelsResponse)
async def list_models():
    """List loaded and available models."""
    ids = registry.loaded_model_ids()
    if registry.default_model and registry.default_model not in ids:
        ids.insert(0, registry.default_model)
    return ModelsResponse(
        data=[ModelInfo(id=mid) for mid in ids],
    )


@app.post("/v1/embeddings", response_model=EmbeddingResponse)
async def create_embeddings(request: EmbeddingRequest):
    """Generate text embeddings (OpenAI-compatible)."""
    model_id = request.model or registry.default_model
    if not model_id:
        raise HTTPException(status_code=400, detail="No model specified")

    try:
        model, tokenizer = registry.get_text_model(model_id)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to load model: {e}")

    try:
        from typing import cast

        from mlx_embeddings import generate
        from mlx_embeddings.models.base import BaseModelOutput

        # generate() is typed as -> mx.array but actually returns
        # BaseModelOutput for text models (upstream type annotation issue).
        raw = generate(model, tokenizer, texts=request.input)
        output = cast(BaseModelOutput, raw)
        assert output.text_embeds is not None, "Model returned no text embeddings"
        embeddings = _mx_to_list(output.text_embeds)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Embedding failed: {e}")

    data = [EmbeddingData(embedding=emb, index=i) for i, emb in enumerate(embeddings)]

    return EmbeddingResponse(
        data=data,
        model=model_id,
        usage=EmbeddingUsage(
            prompt_tokens=sum(len(t.split()) for t in request.input),
            total_tokens=sum(len(t.split()) for t in request.input),
        ),
    )


@app.post("/v1/embeddings/multimodal", response_model=EmbeddingResponse)
async def create_multimodal_embeddings(request: MultimodalEmbeddingRequest):
    """Generate multimodal (text + image) embeddings."""
    model_id = request.model or registry.default_model
    if not model_id:
        raise HTTPException(status_code=400, detail="No model specified")

    try:
        model, processor = registry.get_vl_model(model_id)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to load VL model: {e}")

    try:
        from typing import cast

        from mlx_embeddings import generate
        from mlx_embeddings.models.base import ViTModelOutput

        decoded_images = (
            [_decode_image(img) for img in request.images] if request.images else []
        )

        # texts must be a non-empty list per generate() signature.
        texts: list[str] = request.texts if request.texts else [""]

        # generate() is typed as -> mx.array but actually returns
        # ViTModelOutput for VL models (upstream type annotation issue).
        raw = generate(
            model,
            processor,
            texts=texts,
            images=decoded_images if decoded_images else [],
        )
        output = cast(ViTModelOutput, raw)

        all_embeddings: list[list[float]] = []

        # Text embeddings come first
        if output.text_embeds is not None:
            text_embs = _mx_to_list(output.text_embeds)
            all_embeddings.extend(text_embs)

        # Then image embeddings
        if output.image_embeds is not None:
            img_embs = _mx_to_list(output.image_embeds)
            all_embeddings.extend(img_embs)

    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Multimodal embedding failed: {e}")

    data = [
        EmbeddingData(embedding=emb, index=i) for i, emb in enumerate(all_embeddings)
    ]

    return EmbeddingResponse(
        data=data,
        model=model_id,
        usage=EmbeddingUsage(
            prompt_tokens=len(request.texts) + len(request.images),
            total_tokens=len(request.texts) + len(request.images),
        ),
    )


@app.get("/health")
async def health():
    return {"status": "ok", "timestamp": time.time()}


# ---------------------------------------------------------------------------
# CLI entry point
# ---------------------------------------------------------------------------


def main():
    parser = argparse.ArgumentParser(description="MLX Embedding Server")
    parser.add_argument(
        "--model",
        default="mlx-community/Qwen3-Embedding-0.6B-mxfp8",
        help="Default model to load at startup",
    )
    parser.add_argument("--port", type=int, default=8678, help="Server port")
    parser.add_argument("--host", default="127.0.0.1", help="Server host")
    args = parser.parse_args()

    registry.default_model = args.model

    uvicorn.run(app, host=args.host, port=args.port)


if __name__ == "__main__":
    main()
