"""MLX Server — OpenAI-compatible REST API for local embedding and chat inference.

Supports text embeddings via mlx-embeddings (or mlx-vlm), multimodal (image+text)
embeddings via mlx-vlm, and chat completions via mlx-lm.
Designed to be called from the xplorertui Rust TUI.

Usage:
    uv run uvicorn server:app --host 127.0.0.1 --port 8678
    MLX_DEFAULT_MODEL=my-model uv run uvicorn server:app --host 127.0.0.1 --port 8678
"""

from __future__ import annotations

import asyncio
import logging
import time
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from contextlib import asynccontextmanager, closing
from dataclasses import dataclass, replace
from functools import partial
from typing import Any, TypeVar, cast

from fastapi import FastAPI, HTTPException

from image_security import UnsafeImage, decode_images
from registry import (
    DEFAULT_CHAT_MODEL,
    DEFAULT_MODEL,
    ChatBackend,
    EmbedBackend,
    ModelRegistry,
    TextEmbedder,
    mx_to_list,
)
from prompts import apply_task_prompt
from schemas import (
    ChatChoice,
    ChatChoiceMessage,
    ChatCompletionRequest,
    ChatCompletionResponse,
    ChatUsage,
    CompletionTokensDetails,
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
_OPTIQ_ATTENTION_PATCHED = False

registry = ModelRegistry(default_model=DEFAULT_MODEL)

T = TypeVar("T")

# ---------------------------------------------------------------------------
# MLX thread — every model load and every evaluation runs here
# ---------------------------------------------------------------------------
#
# Since MLX 0.32, streams belong to the thread that created them. A lazy array
# built on one thread (e.g. RoPE frequencies computed at model load) cannot be
# evaluated on another: "There is no Stream(gpu, 0) in current thread". So all
# MLX work, from loading to converting results to lists, goes through one
# dedicated thread. That also keeps inference off the event loop and
# serializes access to the device.

_mlx_executor: ThreadPoolExecutor | None = None


def _get_mlx_executor() -> ThreadPoolExecutor:
    global _mlx_executor
    if _mlx_executor is None:
        _mlx_executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="mlx")
    return _mlx_executor


async def run_mlx(fn: Callable[..., T], /, *args: Any, **kwargs: Any) -> T:
    """Run ``fn`` on the MLX thread without blocking the event loop."""
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(_get_mlx_executor(), partial(fn, *args, **kwargs))


# ---------------------------------------------------------------------------
# Lifespan — shared resources
# ---------------------------------------------------------------------------


@asynccontextmanager
async def lifespan(app: FastAPI):
    global _mlx_executor
    # Pre-load default model at startup if specified.
    if registry.default_model:
        logger.info("Pre-loading default model: %s", registry.default_model)
        await run_mlx(registry.get_text_model, registry.default_model)
        logger.info("Default model loaded.")

    try:
        yield
    finally:
        if _mlx_executor is not None:
            # Cached models are bound to this MLX thread; drop them on it so a
            # later lifespan reloads them on its own thread instead of
            # evaluating stale arrays across threads.
            await run_mlx(registry.clear)
            # Join the MLX thread before the interpreter tears down: MLX aborts
            # if a thread holding its streams is still alive at exit.
            _mlx_executor.shutdown(wait=True)
            _mlx_executor = None


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
        embedder = await run_mlx(registry.get_text_model, model_id)
    except Exception as e:
        logger.error("Failed to load embedding model %s: %s", model_id, e)
        raise HTTPException(status_code=500, detail=f"Failed to load model: {e}")

    try:
        t0 = time.perf_counter()
        texts = apply_task_prompt(embedder.prompts, request.task, request.input)
        embeddings = await run_mlx(_embed_texts, embedder, texts)
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
        decoded_images = await decode_images(request.images)
    except UnsafeImage as e:
        raise HTTPException(status_code=400, detail=str(e)) from e

    try:
        model, processor = await run_mlx(registry.get_vl_model, model_id)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to load VL model: {e}")

    try:
        # texts must be a non-empty list per generate() signature.
        texts: list[str] = request.texts if request.texts else [""]
        all_embeddings = await run_mlx(
            _embed_multimodal, model, processor, texts, decoded_images
        )
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
        backend, model, tokenizer = await run_mlx(registry.get_chat_model, model_id)
        logger.info("Chat model ready: %s (backend=%s)", model_id, backend.value)
    except Exception as e:
        logger.error("Failed to load chat model %s: %s", model_id, e)
        raise HTTPException(status_code=500, detail=f"Failed to load model: {e}")

    messages = [{"role": m.role, "content": m.content} for m in request.messages]
    # Caps the answer; thinking the model does anyway gets its own headroom.
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
            result = await run_mlx(
                _generate_mlx_lm,
                model,
                tokenizer,
                messages,
                max_tokens,
                temp,
                model_id,
            )
        else:
            result = await run_mlx(
                _generate_mlx_vlm, model, tokenizer, messages, max_tokens, temp
            )
    except Exception as e:
        logger.error("Generation failed for %s: %s", model_id, e, exc_info=True)
        raise HTTPException(status_code=500, detail=f"Generation failed: {e}")

    elapsed = time.perf_counter() - t0
    tps = result.completion_tokens / elapsed if elapsed > 0 else 0
    logger.info(
        "Generation complete: %d prompt + %d completion tokens (%d reasoning), "
        "finish=%s, %.2fs (%.1f tok/s)",
        result.prompt_tokens,
        result.completion_tokens,
        result.reasoning_tokens,
        result.finish_reason,
        elapsed,
        tps,
    )

    return ChatCompletionResponse(
        choices=[
            ChatChoice(
                message=ChatChoiceMessage(content=result.text),
                finish_reason=result.finish_reason,
            )
        ],
        model=model_id,
        usage=ChatUsage(
            prompt_tokens=result.prompt_tokens,
            completion_tokens=result.completion_tokens,
            total_tokens=result.prompt_tokens + result.completion_tokens,
            completion_tokens_details=CompletionTokensDetails(
                reasoning_tokens=result.reasoning_tokens
            ),
        ),
    )


# Texts per forward pass on the mlx-vlm path. A batch is padded to its longest
# text, so this bounds memory when a request carries many or long texts.
_EMBED_BATCH_SIZE = 32


def _embed_texts(embedder: TextEmbedder, texts: list[str]) -> list[list[float]]:
    """Text embeddings, one vector per text. Runs on the MLX thread."""
    if not texts:
        return []
    if embedder.backend == EmbedBackend.MLX_VLM:
        return _embed_texts_mlx_vlm(embedder.model, embedder.tokenizer, texts)
    return _embed_texts_mlx_embeddings(embedder.model, embedder.tokenizer, texts)


def _embed_texts_mlx_vlm(model, tokenizer, texts: list[str]) -> list[list[float]]:
    """Text embeddings via an mlx-vlm embedding model (e.g. EmbeddingGemma 2).

    The model mean-pools over the attention mask and L2-normalizes, so
    ``text_embeds`` is ready for cosine similarity.
    """
    import mlx.core as mx

    embeddings: list[list[float]] = []
    for start in range(0, len(texts), _EMBED_BATCH_SIZE):
        inputs = tokenizer(
            texts[start : start + _EMBED_BATCH_SIZE],
            padding=True,
            truncation=True,
            return_tensors="np",
        )
        output = model(
            input_ids=mx.array(inputs["input_ids"]),
            attention_mask=mx.array(inputs["attention_mask"]),
        )
        embeddings.extend(mx_to_list(output.text_embeds))
    return embeddings


def _embed_texts_mlx_embeddings(
    model, tokenizer, texts: list[str]
) -> list[list[float]]:
    """Text embeddings via mlx-embeddings."""
    from mlx_embeddings import generate
    from mlx_embeddings.models.base import BaseModelOutput

    # generate() is typed as -> mx.array but actually returns
    # BaseModelOutput for text models (upstream type annotation issue).
    raw = generate(model, tokenizer, texts=texts)
    output = cast(BaseModelOutput, raw)
    assert output.text_embeds is not None, "Model returned no text embeddings"
    return mx_to_list(output.text_embeds)


def _embed_multimodal(model, processor, texts: list[str], images) -> list[list[float]]:
    """Text + image embeddings via mlx-embeddings. Runs on the MLX thread."""
    from mlx_embeddings import generate
    from mlx_embeddings.models.base import ViTModelOutput

    # generate() is typed as -> mx.array but actually returns
    # ViTModelOutput for VL models (upstream type annotation issue).
    raw = generate(model, processor, texts=texts, images=images)
    output = cast(ViTModelOutput, raw)

    all_embeddings: list[list[float]] = []
    if output.text_embeds is not None:
        all_embeddings.extend(mx_to_list(output.text_embeds))
    if output.image_embeds is not None:
        all_embeddings.extend(mx_to_list(output.image_embeds))
    return all_embeddings


# Extra tokens a chat reply may spend on thinking, on top of `max_tokens`.
THINKING_HEADROOM_TOKENS = 2048


@dataclass(frozen=True)
class ChatGeneration:
    """One chat reply, with thinking blocks already stripped from `text`."""

    text: str
    prompt_tokens: int
    # Every generated token, thinking included.
    completion_tokens: int
    reasoning_tokens: int
    finish_reason: str


def _generate_mlx_lm(
    model,
    tokenizer,
    messages: list[dict],
    max_tokens: int,
    temp: float,
    model_id: str,
) -> ChatGeneration:
    """Generate text using mlx-lm (text-only LLMs). Runs on the MLX thread."""
    from mlx_lm import stream_generate
    from mlx_lm.sample_utils import make_sampler

    prompt = tokenizer.apply_chat_template(
        messages,
        tokenize=False,
        add_generation_prompt=True,
        enable_thinking=False,
    )
    sampler = make_sampler(temp=temp)
    generate_kwargs = {
        "prompt": prompt,
        "max_tokens": max_tokens + THINKING_HEADROOM_TOKENS,
        "sampler": sampler,
    }
    prompt_cache = _maybe_build_optiq_prompt_cache(model, model_id)
    if prompt_cache is not None:
        generate_kwargs["prompt_cache"] = prompt_cache

    stream = cast(Any, stream_generate)(model, tokenizer, **generate_kwargs)
    # `tokenizer.detokenizer` is a new detokenizer on each access.
    return _collect_reply(stream, prompt, max_tokens, lambda: tokenizer.detokenizer)


def _generate_mlx_vlm(
    model, processor, messages: list[dict], max_tokens: int, temp: float
) -> ChatGeneration:
    """Generate text using mlx-vlm (vision-language models). Runs on the MLX thread."""
    from mlx_vlm import stream_generate
    from mlx_vlm.prompt_utils import apply_chat_template
    from mlx_vlm.tokenizer_utils import make_streaming_detokenizer

    prompt = cast(
        str,
        apply_chat_template(processor, model.config, messages, enable_thinking=False),
    )

    # `mlx_vlm.generate` does these two steps around its own stream.
    tokenizer = getattr(processor, "tokenizer", processor)
    tokenizer.stopping_criteria.reset(model.config.eos_token_id)
    stream = stream_generate(
        model,
        processor,
        prompt,
        max_tokens=max_tokens + THINKING_HEADROOM_TOKENS,
        temperature=temp,
        enable_thinking=False,
    )
    reply = _collect_reply(
        stream, prompt, max_tokens, lambda: make_streaming_detokenizer(processor)
    )
    clean_output = getattr(processor, "clean_output", None)
    if callable(clean_output):
        reply = replace(reply, text=_strip_thinking(clean_output(reply.text)))
    return reply


def _collect_reply(
    stream,
    prompt: str,
    max_tokens: int,
    new_detokenizer: Callable[[], Any],
) -> ChatGeneration:
    """Read one reply from a token stream, stopping at `max_tokens` of answer.

    `enable_thinking=False` only works when the model's chat template reads
    it. A model that thinks anyway would spend the budget on text that is
    stripped from the answer, so thinking tokens are not counted against
    `max_tokens`. The caller caps the stream at `max_tokens` plus
    `THINKING_HEADROOM_TOKENS`, which bounds the thinking.

    `new_detokenizer` returns a fresh streaming detokenizer of the kind the
    stream uses. It is needed when the reply is stopped here: see
    `_detokenize`.
    """
    # Some templates end the prompt with an open thinking block, so the
    # output starts inside it and carries only the closing tag.
    opened_by_prompt = prompt.rstrip().endswith("<think>")
    prefix = "<think>" if opened_by_prompt else ""
    thinking = _ThinkingTracker(inside=opened_by_prompt)
    segments: list[str] = []
    tokens: list[Any] = []
    prompt_tokens = generated = reasoning_tokens = 0
    # Tokens whose text may be the start of a thinking tag split across
    # segments: charged to neither budget until the tag is complete or ruled out.
    pending = 0
    finish_reason = None
    stopped_here = False

    with closing(stream):
        for response in stream:
            if getattr(response, "is_draft", False):
                continue
            segments.append(response.text)
            step = response.generation_tokens - generated
            generated = response.generation_tokens
            prompt_tokens = response.prompt_tokens
            finish_reason = response.finish_reason
            if thinking.feed(response.text):
                reasoning_tokens += step + pending
                pending = 0
            elif thinking.partial_tag:
                pending += step
            else:
                # Not a tag after all: the held tokens were answer text.
                pending = 0
            if finish_reason is not None:
                # The stream's last response: it has flushed its text.
                break
            tokens.append(response.token)
            excess = generated - reasoning_tokens - pending - max_tokens
            if excess >= 0:
                # Held tokens that turned out to be answer text are all
                # counted at once and can pass the cap: drop the extra ones.
                if excess:
                    del tokens[-excess:]
                    generated -= excess
                stopped_here = True
                finish_reason = "length"
                break

    raw = _detokenize(new_detokenizer(), tokens) if stopped_here else "".join(segments)
    return ChatGeneration(
        _strip_thinking(prefix + raw),
        prompt_tokens,
        generated,
        reasoning_tokens,
        finish_reason or "stop",
    )


def _detokenize(detokenizer, tokens: list[Any]) -> str:
    """The full text of `tokens`.

    A streaming detokenizer can hold back the text of the newest tokens (an
    unfinished word or UTF-8 sequence) until a later token or the end of the
    stream. A stream that is closed early never flushes that text, so its
    segments can be short of the tokens it generated.
    """
    detokenizer.reset()
    for token in tokens:
        detokenizer.add_token(token)
    detokenizer.finalize()
    return detokenizer.text


# Opening tag of a thinking block -> the tag that closes it. Gemma 4 emits
# its channel tokens in either order (see `_strip_gemma_channel_blocks`).
_THINKING_TAGS = {
    "<think>": "</think>",
    "<|channel>": "<channel|>",
    "<channel|>": "<|channel>",
}
_LONGEST_THINKING_TAG = max(len(tag) for pair in _THINKING_TAGS.items() for tag in pair)


class _ThinkingTracker:
    """Tells, segment by segment, whether a stream is inside a thinking block.

    Looks only at each new segment plus a few carried characters (a tag can
    be split across segments), so the work per token does not grow with the
    length of the reply.
    """

    def __init__(self, inside: bool = False) -> None:
        self._closing_tag = "</think>" if inside else None
        self._carry = ""

    def feed(self, segment: str) -> bool:
        """Add a segment; True if any of it belongs to a thinking block."""
        text = self._carry + segment
        thinking = self._closing_tag is not None
        pos = 0
        while True:
            if self._closing_tag is None:
                found = [
                    (at, tag)
                    for tag in _THINKING_TAGS
                    if (at := text.find(tag, pos)) != -1
                ]
                if not found:
                    break
                at, tag = min(found)
                self._closing_tag = _THINKING_TAGS[tag]
                pos = at + len(tag)
                thinking = True
            else:
                at = text.find(self._closing_tag, pos)
                if at == -1:
                    break
                pos = at + len(self._closing_tag)
                self._closing_tag = None
        # Keep only what could still be the start of a tag.
        self._carry = text[pos:][-(_LONGEST_THINKING_TAG - 1) :]
        return thinking

    @property
    def partial_tag(self) -> bool:
        """True if the stream, outside a thinking block, ends with the start
        of an opening tag (e.g. `<th`), so its last tokens can't be classified
        yet."""
        if self._closing_tag is not None:
            return False
        return any(
            self._carry.endswith(tag[:n])
            for tag in _THINKING_TAGS
            for n in range(1, len(tag))
        )


def _maybe_build_optiq_prompt_cache(model, model_id: str):
    """Create a TurboQuant prompt cache for OptiQ mlx-lm models when possible."""
    if "optiq" not in model_id.lower():
        return None

    try:
        from optiq.core.turbo_kv_cache import TurboQuantKVCache, patch_attention
    except ImportError:
        logger.warning(
            "OptiQ model %s selected but mlx-optiq is unavailable; using standard cache",
            model_id,
        )
        return None

    try:
        global _OPTIQ_ATTENTION_PATCHED
        if not _OPTIQ_ATTENTION_PATCHED:
            patch_attention()
            _OPTIQ_ATTENTION_PATCHED = True

        cache = model.make_cache()
        patched_layers = 0
        for i, layer in enumerate(_iter_attention_layers(model)):
            attn = getattr(layer, "self_attn", None)
            head_dim = getattr(attn, "head_dim", None)
            if head_dim is None:
                continue
            cache[i] = TurboQuantKVCache(head_dim=head_dim, bits=4, seed=42 + i)
            patched_layers += 1

        if patched_layers == 0:
            logger.warning(
                "OptiQ model %s exposed no self-attention layers for TurboQuant cache",
                model_id,
            )
            return None

        logger.info(
            "Enabled mlx-optiq TurboQuant cache for %s across %d layers",
            model_id,
            patched_layers,
        )
        return cache
    except Exception:
        logger.warning(
            "Failed to enable mlx-optiq cache for %s; falling back to standard cache",
            model_id,
            exc_info=True,
        )
        return None


def _iter_attention_layers(model):
    """Return the model layer sequence used to build prompt caches."""
    direct_layers = getattr(model, "layers", None)
    if direct_layers is not None:
        return direct_layers

    nested_model = getattr(model, "model", None)
    nested_layers = getattr(nested_model, "layers", None)
    if nested_layers is not None:
        return nested_layers

    return []


def _strip_thinking(text: str) -> str:
    """Remove reasoning/thinking blocks from generated text.

    Handles multiple formats:
    - <think>...</think> (Qwen, DeepSeek)
    - <channel|>...<|channel> and <|channel>...<channel|> (Gemma 4)
    """
    return _remove_thinking(text).strip()


def _remove_thinking(text: str) -> str:
    """`_strip_thinking` without trimming the whitespace around the answer."""
    import re

    # <think>...</think> — greedy within blocks, DOTALL for multiline
    text = re.sub(r"<think>.*?</think>", "", text, flags=re.DOTALL)
    # Unclosed <think> at the start — strip everything from <think> onward
    if "<think>" in text:
        text = text[: text.index("<think>")]
    return _strip_gemma_channel_blocks(text)


def _strip_gemma_channel_blocks(text: str) -> str:
    """Strip Gemma 4 reasoning blocks in either channel-token order."""
    result: list[str] = []
    rest = text

    while True:
        normal = rest.find("<channel|>")
        reversed_ = rest.find("<|channel>")

        if normal == -1 and reversed_ == -1:
            break

        if normal != -1 and (reversed_ == -1 or normal <= reversed_):
            start, open_tag, close_tag = normal, "<channel|>", "<|channel>"
        else:
            start, open_tag, close_tag = reversed_, "<|channel>", "<channel|>"

        result.append(rest[:start])
        rest = rest[start + len(open_tag) :]

        end = rest.find(close_tag)
        if end == -1:
            return "".join(result)
        rest = rest[end + len(close_tag) :]

    result.append(rest)
    return "".join(result)


@app.get("/health")
async def health():
    return {
        "status": "ok",
        "timestamp": time.time(),
        "capabilities": ["embeddings", "chat"],
    }


if __name__ == "__main__":
    import uvicorn

    uvicorn.run("server:app", host="127.0.0.1", port=8678)
