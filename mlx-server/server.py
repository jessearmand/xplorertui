"""MLX Server — OpenAI-compatible REST API for local embedding and chat inference.

Supports text embeddings via mlx-embeddings, multimodal (image+text)
embeddings via mlx-vlm, and chat completions via mlx-lm.
Designed to be called from the xplorertui Rust TUI.

Usage:
    uv run uvicorn server:app --host 0.0.0.0 --port 8678
    MLX_DEFAULT_MODEL=my-model uv run uvicorn server:app --host 0.0.0.0 --port 8678
"""

from __future__ import annotations

import asyncio
import logging
import time
from contextlib import asynccontextmanager
from typing import cast

import httpx
from fastapi import FastAPI, HTTPException

from registry import (
    ChatBackend,
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

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s [%(name)s] %(message)s",
    datefmt="%H:%M:%S",
)
logger = logging.getLogger("mlx-server")

registry = ModelRegistry(default_model=DEFAULT_MODEL)


# ---------------------------------------------------------------------------
# Lifespan — shared resources
# ---------------------------------------------------------------------------


@asynccontextmanager
async def lifespan(app: FastAPI):
    # Pre-load default model at startup if specified.
    if registry.default_model:
        logger.info("Pre-loading default model: %s", registry.default_model)
        registry.get_text_model(registry.default_model)
        logger.info("Default model loaded.")

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
        logger.info("Loading embedding model: %s", model_id)
        model, tokenizer = registry.get_text_model(model_id)
    except Exception as e:
        logger.error("Failed to load embedding model %s: %s", model_id, e)
        raise HTTPException(status_code=500, detail=f"Failed to load model: {e}")

    try:
        from mlx_embeddings import generate
        from mlx_embeddings.models.base import BaseModelOutput

        t0 = time.perf_counter()
        # generate() is typed as -> mx.array but actually returns
        # BaseModelOutput for text models (upstream type annotation issue).
        raw = generate(model, tokenizer, texts=request.input)
        output = cast(BaseModelOutput, raw)
        assert output.text_embeds is not None, "Model returned no text embeddings"
        embeddings = mx_to_list(output.text_embeds)
        elapsed = time.perf_counter() - t0
        logger.info(
            "Embedding complete: %d texts, %.2fs, model=%s",
            len(request.input),
            elapsed,
            model_id,
        )
    except Exception as e:
        logger.error("Embedding failed for model %s: %s", model_id, e)
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
    """Generate a chat completion (OpenAI-compatible).

    Supports both text-only models (via mlx-lm) and vision-language models
    (via mlx-vlm, e.g. gemma-4).  The backend is auto-detected when the
    model is first loaded.
    """
    model_id = request.model or DEFAULT_CHAT_MODEL
    if not model_id:
        raise HTTPException(status_code=400, detail="No model specified")

    try:
        logger.info("Loading chat model: %s", model_id)
        backend, model, tokenizer = registry.get_chat_model(model_id)
        logger.info("Chat model ready: %s (backend=%s)", model_id, backend.value)
    except Exception as e:
        logger.error("Failed to load chat model %s: %s", model_id, e)
        raise HTTPException(status_code=500, detail=f"Failed to load model: {e}")

    messages = [{"role": m.role, "content": m.content} for m in request.messages]
    max_tokens = request.max_tokens or 512
    temp = request.temperature if request.temperature is not None else 0.0

    logger.info(
        "Generating: model=%s, backend=%s, messages=%d, max_tokens=%d, temp=%.2f",
        model_id,
        backend.value,
        len(messages),
        max_tokens,
        temp,
    )
    t0 = time.perf_counter()

    try:
        if backend == ChatBackend.MLX_LM:
            (
                text,
                prompt_tokens,
                completion_tokens,
                finish_reason,
            ) = await _generate_mlx_lm(model, tokenizer, messages, max_tokens, temp)
        else:
            (
                text,
                prompt_tokens,
                completion_tokens,
                finish_reason,
            ) = await _generate_mlx_vlm(model, tokenizer, messages, max_tokens, temp)
    except Exception as e:
        logger.error("Generation failed for %s: %s", model_id, e, exc_info=True)
        raise HTTPException(status_code=500, detail=f"Generation failed: {e}")

    elapsed = time.perf_counter() - t0
    tps = completion_tokens / elapsed if elapsed > 0 else 0
    logger.info(
        "Generation complete: %d prompt + %d completion tokens, %.2fs (%.1f tok/s)",
        prompt_tokens,
        completion_tokens,
        elapsed,
        tps,
    )

    return ChatCompletionResponse(
        choices=[
            ChatChoice(
                message=ChatChoiceMessage(content=text),
                finish_reason=finish_reason,
            )
        ],
        model=model_id,
        usage=ChatUsage(
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            total_tokens=prompt_tokens + completion_tokens,
        ),
    )


async def _generate_mlx_lm(
    model, tokenizer, messages: list[dict], max_tokens: int, temp: float
) -> tuple[str, int, int, str]:
    """Generate text using mlx-lm (text-only LLMs)."""
    from mlx_lm import generate
    from mlx_lm.sample_utils import make_sampler

    prompt = tokenizer.apply_chat_template(
        messages,
        tokenize=False,
        add_generation_prompt=True,
        enable_thinking=False,
    )
    sampler = make_sampler(temp=temp)

    text = await asyncio.to_thread(
        generate,
        model,
        tokenizer,
        prompt=prompt,
        max_tokens=max_tokens,
        sampler=sampler,
    )

    text = _strip_thinking(text)
    prompt_tokens = len(tokenizer.encode(prompt))
    completion_tokens = len(tokenizer.encode(text))
    return (
        text,
        prompt_tokens,
        completion_tokens,
        _infer_finish_reason(text, completion_tokens, max_tokens),
    )


async def _generate_mlx_vlm(
    model, processor, messages: list[dict], max_tokens: int, temp: float
) -> tuple[str, int, int, str]:
    """Generate text using mlx-vlm (vision-language models like gemma-4)."""
    from mlx_vlm import generate
    from mlx_vlm.prompt_utils import apply_chat_template

    prompt = cast(
        str,
        apply_chat_template(processor, model.config, messages, enable_thinking=False),
    )

    result = await asyncio.to_thread(
        generate,
        model,
        processor,
        prompt,
        max_tokens=max_tokens,
        temperature=temp,
        enable_thinking=False,
    )

    text = _strip_thinking(result.text)
    return (
        text,
        result.prompt_tokens,
        result.generation_tokens,
        _infer_finish_reason(text, result.generation_tokens, max_tokens),
    )


def _infer_finish_reason(text: str, completion_tokens: int, max_tokens: int) -> str:
    """Best-effort OpenAI-style finish reason for MLX backends."""
    if completion_tokens >= max_tokens:
        return "length"
    if not text:
        return "length"
    return "stop"


def _strip_thinking(text: str) -> str:
    """Remove reasoning/thinking blocks from generated text.

    Handles multiple formats:
    - <think>...</think> (Qwen, DeepSeek)
    - <channel|>...<|channel> (Gemma 4)
    """
    import re

    # <think>...</think> — greedy within blocks, DOTALL for multiline
    text = re.sub(r"<think>.*?</think>", "", text, flags=re.DOTALL)
    # Unclosed <think> at the start — strip everything from <think> onward
    if "<think>" in text:
        text = text[: text.index("<think>")]
    # <channel|>...<|channel> (Gemma 4 thinking format)
    text = re.sub(r"<channel\|>.*?<\|channel>", "", text, flags=re.DOTALL)
    if "<channel|>" in text:
        text = text[: text.index("<channel|>")]
    return text.strip()


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
