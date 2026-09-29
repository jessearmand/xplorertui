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

The server pre-loads the default embedding model at startup. Chat and additional models are lazy-loaded on first request.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/embeddings` | Text embeddings (OpenAI-compatible) |
| POST | `/v1/embeddings/multimodal` | Text + image embeddings |
| POST | `/v1/chat/completions` | Chat completions (OpenAI-compatible) |
| GET | `/v1/models` | List loaded/available models |
| GET | `/health` | Health check |

Interactive API docs are available at `/docs` when the server is running.

## Configuration

| Environment Variable | Default | Description |
|---|---|---|
| `MLX_DEFAULT_MODEL` | `mlx-community/Qwen3-Embedding-0.6B-mxfp8` | Embedding model to pre-load at startup |
| `MLX_DEFAULT_CHAT_MODEL` | `mlx-community/Qwen3.5-0.8B-OptiQ-4bit` | Chat model (lazy-loaded on first request) |
| `MLX_ALLOW_REMOTE_IMAGES` | unset | Opt in to public HTTPS image downloads; redirects and private addresses remain blocked |

The xplorertui Rust client connects to this server when `mlx_server_url` is set in `~/.config/xplorertui/config.toml`:

```toml
mlx_server_url = "http://localhost:8678"
mlx_embedding_model = "mlx-community/Qwen3-Embedding-0.6B-mxfp8"  # optional
mlx_chat_model = "mlx-community/Qwen3.5-0.8B-OptiQ-4bit"          # optional
```

## Module Layout

- **`server.py`** — FastAPI app, lifespan, endpoints
- **`schemas.py`** — Pydantic request/response models
- **`registry.py`** — `ModelRegistry` (lazy model loading), image decode helpers, MLX array conversion

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

# Tests (test_mlx_thread.py needs mlx and mlx-lm; it is skipped without them)
uv run --with pytest --with pytest-asyncio pytest -q
```
