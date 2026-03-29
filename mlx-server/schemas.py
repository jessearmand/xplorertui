"""OpenAI-compatible request/response schemas for the MLX embedding server."""

from __future__ import annotations

from pydantic import BaseModel, Field


# ---------------------------------------------------------------------------
# Embedding schemas
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


# ---------------------------------------------------------------------------
# Model info schemas
# ---------------------------------------------------------------------------


class ModelInfo(BaseModel):
    id: str
    object: str = "model"
    owned_by: str = "mlx-community"


class ModelsResponse(BaseModel):
    object: str = "list"
    data: list[ModelInfo]
