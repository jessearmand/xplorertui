# mlx-server

Local MLX server exposing OpenAI-compatible REST endpoints for text embeddings, multimodal embeddings, and chat completions. Runs on Apple Silicon via `mlx-embeddings`, `mlx-vlm`, and `mlx-lm`. Designed to be launched manually or spawned by xplorertui.

## Running

```bash
cd mlx-server
uv run uvicorn server:app --host 127.0.0.1 --port 8678
```

Override the default model via environment variable:

```bash
MLX_DEFAULT_MODEL=mlx-community/Qwen3-Embedding-0.6B-mxfp8 uv run uvicorn server:app --host 127.0.0.1 --port 8678
```

The server pre-loads the default embedding model at startup. Chat and additional models are lazy-loaded on first request. The server does not read xplorertui's `config.toml`: a client that asks for a different embedding model gets it loaded on its first request, in addition to the pre-loaded default. To avoid loading (and downloading) a default you do not use, start the server with `MLX_DEFAULT_MODEL` set to the model the client asks for, or set it to an empty string to skip the pre-load.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/embeddings` | Text embeddings (OpenAI-compatible) |
| POST | `/v1/embeddings/multimodal` | Text + image embeddings |
| POST | `/v1/embeddings/documents` | One vector per text + image document, in the text-embedding space (EmbeddingGemma 2) |
| POST | `/v1/chat/completions` | Chat completions (OpenAI-compatible) |
| GET | `/v1/models` | List loaded/available models |
| GET | `/health` | Health check |

Interactive API docs are available at `/docs` when the server is running.

## Configuration

| Environment Variable | Default | Description |
|---|---|---|
| `MLX_DEFAULT_MODEL` | `mlx-community/Qwen3-Embedding-0.6B-mxfp8` | Embedding model to pre-load at startup; empty to skip the pre-load |
| `MLX_DEFAULT_CHAT_MODEL` | `mlx-community/Qwen3.5-0.8B-OptiQ-4bit` | Chat model (lazy-loaded on first request) |
| `MLX_ALLOW_REMOTE_IMAGES` | unset | Opt in to public HTTPS image downloads; redirects and private addresses remain blocked |

The xplorertui Rust client connects to this server when `mlx_server_url` is set in `~/.config/xplorertui/config.toml`:

```toml
mlx_server_url = "http://localhost:8678"
mlx_embedding_model = "mlx-community/embeddinggemma-2-mxfp8"  # optional
mlx_chat_model = "mlx-community/Qwen3.5-0.8B-OptiQ-4bit"      # optional
```

## Embedding models

`mlx-community/Qwen3-Embedding-0.6B-mxfp8` is the default, used when nothing else is configured. `mlx-community/embeddinggemma-2-mxfp8` (EmbeddingGemma 2, 768 dimensions, about 1.2 GB) is the preferred model; select it as shown above and start the server with the same id:

```bash
MLX_DEFAULT_MODEL=mlx-community/embeddinggemma-2-mxfp8 uv run uvicorn server:app --host 127.0.0.1 --port 8678
```

`ModelRegistry.get_text_model` tries `mlx-embeddings` first and falls back to `mlx-vlm`'s embedding loader when the architecture is unsupported. EmbeddingGemma 2 (`model_type: embedding_gemma2`) takes the fallback. Notes for that path:

- Use an MLX conversion (`mlx-community/embeddinggemma-2-*`). `mxfp8` is the one tested here; loading `google/embeddinggemma-2` directly is untested.
- When the fallback is taken, `mlx-embeddings` logs `ERROR [root] Model type embedding_gemma2 not supported.` once. That is expected.
- Only the text encoder is loaded; the vision and audio towers are dropped at load. `/v1/embeddings/multimodal` does not support this model yet.
- Never cast the weights to float16: the model returns NaN or silently degraded vectors. The `bf16` and `mxfp8` conversions are safe as stored.
- `mlx-vlm` is pinned to the commit that added the model, because no PyPI release has it yet. Its local `EmbeddingGemma2Processor` works with the pinned `transformers`.

### Task prompts

`POST /v1/embeddings` accepts an optional `task` (an extension to the OpenAI schema): `clustering`, `search_query`, `document`, `classification`, or `similarity`. The server prepends the matching prompt from the model's own catalog (`config_sentence_transformers.json` in the checkpoint), so the wording stays with the model and clients stay model-agnostic. `prompts.py` maps each task to catalog prompt names.

| `task` | EmbeddingGemma 2 | Qwen3-Embedding |
|---|---|---|
| `clustering` | `task: clustering \| query: ` | none |
| `search_query` | `task: search result \| query: ` | its retrieval instruction |
| `document` | `title: none \| text: ` | none |

A model with no catalog, or no prompt for the task, embeds the text as given; so does a request without `task`. Queries and documents take different prompts, so a search sends two requests. xplorertui sends `clustering` for `:cluster`, and `search_query` plus `document` for search ranking.

## Chat models

`ModelRegistry.get_chat_model` tries `mlx-lm` first and falls back to `mlx-vlm`. Thinking is always asked off: the server passes `enable_thinking=False` to the model's chat template and strips any thinking block from the answer. There is no request field for it.

`enable_thinking` is a template variable, so a model whose chat template does not read it thinks anyway. To keep a short budget for the answer, `max_tokens` caps the answer only. Thinking tokens are not counted against it; they have their own limit, `THINKING_HEADROOM_TOKENS` (2,048) in `server.py`. A model that does not think stops at `max_tokens` as before.

The response reports what happened:

- `usage.completion_tokens` counts every generated token, thinking included, so it can be larger than `max_tokens`.
- `usage.completion_tokens_details.reasoning_tokens` counts the thinking tokens that were stripped.
- `finish_reason` is `length` when the answer reached `max_tokens` or the thinking used all its headroom. In the second case the content is empty.

## Module Layout

- **`server.py`** — FastAPI app, lifespan, endpoints
- **`schemas.py`** — Pydantic request/response models
- **`registry.py`** — `ModelRegistry` (lazy model loading, backend fallbacks), MLX array conversion
- **`prompts.py`** — task prompt lookup in a model's Sentence Transformers prompt catalog

### Threading

All MLX work (model loads, generation, embedding, `mx_to_list`) goes through `run_mlx()` in `server.py`, which runs it on one dedicated thread. Since MLX 0.32, a lazy array created on one thread cannot be evaluated on another (`There is no Stream(gpu, 0) in current thread`); Gemma 4's RoPE frequencies hit this. Never call registry loaders or MLX generation directly from an endpoint or via `asyncio.to_thread`. `test_mlx_thread.py` guards this.

## Development

```bash
# Lint
ruff check .

# Type check
ty check .

# Format
ruff format .

# Tests (the dev group brings pytest, pytest-asyncio and httpx2)
uv run pytest -q
```
