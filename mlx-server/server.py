"""MLX Server — OpenAI-compatible REST API for local embedding and chat inference.

Supports text embeddings via mlx-embeddings, multimodal (image+text)
embeddings via mlx-vlm, and chat completions via mlx-lm.
Designed to be called from the xplorertui Rust TUI.

Usage:
    uv run fastapi run server.py --port 8678
    MLX_DEFAULT_MODEL=my-model uv run fastapi run server.py --port 8678
"""

from __future__ import annotations

import asyncio
import time
from contextlib import asynccontextmanager
from typing import cast

import httpx
from fastapi import FastAPI, HTTPException

from registry import (
    DEFAULT_CHAT_MODEL,
    DEFAULT_MODEL,
    ModelRegistry,
    decode_images,
    mx_to_list,
)
from schemas import (
    ChatChoice,
    ChatChoiceMessage,
    ChatCompletionRequest,
    ChatCompletionResponse,
    ChatUsage,
    EmbeddingData,
    EmbeddingRequest,
    EmbeddingResponse,
    EmbeddingUsage,
    ModelInfo,
    ModelsResponse,
    MultimodalEmbeddingRequest,
)

registry = ModelRegistry(default_model=DEFAULT_MODEL)


# ---------------------------------------------------------------------------
# Lifespan — shared resources
# ---------------------------------------------------------------------------


@asynccontextmanager
async def lifespan(app: FastAPI):
    # Pre-load default model at startup if specified.
    if registry.default_model:
        print(f"Pre-loading default model: {registry.default_model}")
        registry.get_text_model(registry.default_model)
        print("Default model loaded.")

    # Shared httpx client for image downloads (connection pooling).
    async with httpx.AsyncClient(timeout=30.0) as client:
        app.state.http_client = client
        yield


app = FastAPI(
    title="MLX Embedding Server",
    version="0.1.0",
    lifespan=lifespan,
)


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------


@app.get("/v1/models", response_model=ModelsResponse)
async def list_models():
    """List loaded and available models."""
    ids = registry.loaded_model_ids()
    if registry.default_model and registry.default_model not in ids:
        ids.insert(0, registry.default_model)
    if DEFAULT_CHAT_MODEL and DEFAULT_CHAT_MODEL not in ids:
        ids.append(DEFAULT_CHAT_MODEL)
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
        from mlx_embeddings import generate
        from mlx_embeddings.models.base import BaseModelOutput

        # generate() is typed as -> mx.array but actually returns
        # BaseModelOutput for text models (upstream type annotation issue).
        raw = generate(model, tokenizer, texts=request.input)
        output = cast(BaseModelOutput, raw)
        assert output.text_embeds is not None, "Model returned no text embeddings"
        embeddings = mx_to_list(output.text_embeds)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Embedding failed: {e}")

    data = [EmbeddingData(embedding=emb, index=i) for i, emb in enumerate(embeddings)]
    token_count = sum(len(t.split()) for t in request.input)

    return EmbeddingResponse(
        data=data,
        model=model_id,
        usage=EmbeddingUsage(
            prompt_tokens=token_count,
            total_tokens=token_count,
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
        from mlx_embeddings import generate
        from mlx_embeddings.models.base import ViTModelOutput

        http_client: httpx.AsyncClient = app.state.http_client
        decoded_images = await decode_images(request.images, http_client)

        # texts must be a non-empty list per generate() signature.
        texts: list[str] = request.texts if request.texts else [""]

        # generate() is typed as -> mx.array but actually returns
        # ViTModelOutput for VL models (upstream type annotation issue).
        raw = generate(
            model,
            processor,
            texts=texts,
            images=decoded_images,
        )
        output = cast(ViTModelOutput, raw)

        all_embeddings: list[list[float]] = []

        if output.text_embeds is not None:
            all_embeddings.extend(mx_to_list(output.text_embeds))

        if output.image_embeds is not None:
            all_embeddings.extend(mx_to_list(output.image_embeds))

    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Multimodal embedding failed: {e}")

    data = [
        EmbeddingData(embedding=emb, index=i) for i, emb in enumerate(all_embeddings)
    ]
    token_count = len(request.texts) + len(request.images)

    return EmbeddingResponse(
        data=data,
        model=model_id,
        usage=EmbeddingUsage(
            prompt_tokens=token_count,
            total_tokens=token_count,
        ),
    )


@app.post("/v1/chat/completions", response_model=ChatCompletionResponse)
async def chat_completions(request: ChatCompletionRequest):
    """Generate a chat completion (OpenAI-compatible)."""
    model_id = request.model or DEFAULT_CHAT_MODEL
    if not model_id:
        raise HTTPException(status_code=400, detail="No model specified")

    try:
        model, tokenizer = registry.get_chat_model(model_id)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to load model: {e}")

    messages = [{"role": m.role, "content": m.content} for m in request.messages]
    prompt = tokenizer.apply_chat_template(
        messages, tokenize=False, add_generation_prompt=True
    )

    max_tokens = request.max_tokens or 512
    temp = request.temperature if request.temperature is not None else 0.0

    try:
        from mlx_lm import generate
        from mlx_lm.sample_utils import make_sampler

        sampler = make_sampler(temp=temp)

        # Run in thread pool — mlx_lm.generate is synchronous and
        # compute-bound; blocking the event loop would stall other endpoints.
        text = await asyncio.to_thread(
            generate,
            model,
            tokenizer,
            prompt=prompt,
            max_tokens=max_tokens,
            sampler=sampler,
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Generation failed: {e}")

    # Approximate token counts via tokenizer.
    prompt_tokens = len(tokenizer.encode(prompt))
    completion_tokens = len(tokenizer.encode(text))

    return ChatCompletionResponse(
        choices=[
            ChatChoice(
                message=ChatChoiceMessage(content=text),
                finish_reason="stop",
            )
        ],
        model=model_id,
        usage=ChatUsage(
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            total_tokens=prompt_tokens + completion_tokens,
        ),
    )


@app.get("/health")
async def health():
    return {
        "status": "ok",
        "timestamp": time.time(),
        "capabilities": ["embeddings", "chat"],
    }


if __name__ == "__main__":
    import uvicorn

    uvicorn.run("server:app", host="0.0.0.0", port=8678)
