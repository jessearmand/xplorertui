"""OpenAI-compatible request/response schemas for the MLX embedding server."""

from __future__ import annotations

import enum
from typing import Annotated

from pydantic import BaseModel, Field, StringConstraints


# ---------------------------------------------------------------------------
# Embedding schemas
# ---------------------------------------------------------------------------


class EmbeddingTask(str, enum.Enum):
    """What the embeddings are for.

    Instruction-tuned embedding models (EmbeddingGemma 2, Qwen3-Embedding)
    expect a task prompt in front of each text. The server looks the prompt up
    in the model's own catalog, so clients name the task and stay
    model-agnostic. See ``prompts.py``.
    """

    CLUSTERING = "clustering"
    SEARCH_QUERY = "search_query"
    DOCUMENT = "document"
    CLASSIFICATION = "classification"
    SIMILARITY = "similarity"


class EmbeddingRequest(BaseModel):
    model: str
    input: list[str]
    # Extension to the OpenAI schema. Omit it to embed the text as given.
    task: EmbeddingTask | None = None


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


class CompletionTokensDetails(BaseModel):
    # Generated tokens that went to thinking blocks the server stripped from
    # the answer. They do not count against `max_tokens`.
    reasoning_tokens: int = 0


class ChatUsage(BaseModel):
    prompt_tokens: int = 0
    completion_tokens: int = 0
    total_tokens: int = 0
    completion_tokens_details: CompletionTokensDetails = Field(
        default_factory=CompletionTokensDetails
    )


class ChatCompletionResponse(BaseModel):
    id: str = ""
    object: str = "chat.completion"
    choices: list[ChatChoice]
    model: str
    usage: ChatUsage = Field(default_factory=ChatUsage)


# ---------------------------------------------------------------------------
# Multimodal embedding schemas
# ---------------------------------------------------------------------------


ImageInput = Annotated[str, StringConstraints(max_length=14_000_000)]


class MultimodalEmbeddingRequest(BaseModel):
    model: str
    texts: list[str]
    images: list[ImageInput] = Field(
        default_factory=list,
        max_length=8,
        description="Image URLs or base64-encoded image data",
    )


class DocumentInput(BaseModel):
    """One document: its text and, optionally, one image."""

    text: str = ""
    image: ImageInput | None = None


class DocumentEmbeddingRequest(BaseModel):
    """One embedding per document, with the image inside the text sequence.

    The vectors share the space of ``/v1/embeddings`` vectors from the same
    model and task, so the two can be mixed in one clustering run.
    """

    model: str
    input: list[DocumentInput] = Field(min_length=1, max_length=32)
    task: EmbeddingTask | None = None


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
