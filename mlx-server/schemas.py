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


# ---------------------------------------------------------------------------
# Chat completion schemas
# ---------------------------------------------------------------------------


class ChatMessage(BaseModel):
    role: str
    content: str


class ChatCompletionRequest(BaseModel):
    model: str
    messages: list[ChatMessage]
    max_tokens: int | None = None
    temperature: float | None = None


class ChatChoiceMessage(BaseModel):
    role: str = "assistant"
    content: str | None = None


class ChatChoice(BaseModel):
    index: int = 0
    message: ChatChoiceMessage
    finish_reason: str | None = None


class ChatUsage(BaseModel):
    prompt_tokens: int = 0
    completion_tokens: int = 0
    total_tokens: int = 0


class ChatCompletionResponse(BaseModel):
    id: str = ""
    object: str = "chat.completion"
    choices: list[ChatChoice]
    model: str
    usage: ChatUsage = Field(default_factory=ChatUsage)


# ---------------------------------------------------------------------------
# Multimodal embedding schemas
# ---------------------------------------------------------------------------


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
