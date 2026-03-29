# mlx-server

Local MLX embedding server exposing OpenAI-compatible REST endpoints for text and multimodal (image+text) embeddings. Runs on Apple Silicon via `mlx-embeddings` and `mlx-vlm`. Designed to be launched manually or spawned by xplorertui.

## Running

```bash
cd mlx-server
uv run fastapi run server.py --port 8678
```

Override the default model via environment variable:

```bash
MLX_DEFAULT_MODEL=mlx-community/Qwen3-Embedding-0.6B-mxfp8 uv run fastapi run server.py --port 8678
```

The server pre-loads the default model at startup. Additional models are lazy-loaded on first request.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/embeddings` | Text embeddings (OpenAI-compatible) |
| POST | `/v1/embeddings/multimodal` | Text + image embeddings |
| GET | `/v1/models` | List loaded/available models |
| GET | `/health` | Health check |

Interactive API docs are available at `/docs` when the server is running.

## Configuration

| Environment Variable | Default | Description |
|---|---|---|
| `MLX_DEFAULT_MODEL` | `mlx-community/Qwen3-Embedding-0.6B-mxfp8` | Model to pre-load at startup and use when requests omit `model` |

The xplorertui Rust client connects to this server when `mlx_server_url` is set in `~/.config/xplorertui/config.toml`:

```toml
mlx_server_url = "http://localhost:8678"
mlx_embedding_model = "mlx-community/Qwen3-Embedding-0.6B-mxfp8"  # optional
```

## Module Layout

- **`server.py`** — FastAPI app, lifespan, endpoints
- **`schemas.py`** — Pydantic request/response models
- **`registry.py`** — `ModelRegistry` (lazy model loading), image decode helpers, MLX array conversion

## Development

```bash
# Lint
ruff check .

# Type check
ty check .

# Format
ruff format .
```
