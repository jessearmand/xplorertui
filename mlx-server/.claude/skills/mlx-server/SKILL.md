---
name: mlx-server
description: Use this skill when the user wants to start, stop, test, configure, or troubleshoot the local MLX inference server. Trigger when the user mentions mlx-server, local model serving, embedding server, local chat completions, or asks about running MLX models as a service. Also trigger when the user encounters errors from the mlx-server, wants to switch between models, or asks about quantization formats (4-bit, 8-bit, mxfp4, mxfp8, nvfp4, bf16) and which models work with mlx-lm vs mlx-vlm.
---

# MLX Server

Local inference server for embeddings and chat completions on Apple Silicon. Serves OpenAI-compatible REST endpoints backed by `mlx-embeddings`, `mlx-lm`, and `mlx-vlm`.

## Quick Start

```bash
cd mlx-server
uv run uvicorn server:app --host 0.0.0.0 --port 8678
```

Or run directly (uses built-in `__main__` block):

```bash
cd mlx-server
uv run server.py
```

## Configuration

All configuration is via environment variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `MLX_DEFAULT_MODEL` | `mlx-community/Qwen3-Embedding-0.6B-mxfp8` | Embedding model (pre-loaded at startup) |
| `MLX_DEFAULT_CHAT_MODEL` | `mlx-community/Qwen3.5-0.8B-OptiQ-4bit` | Chat model (lazy-loaded on first request) |

Example with custom models:

```bash
MLX_DEFAULT_MODEL=mlx-community/Qwen3-Embedding-0.6B-mxfp8 \
MLX_DEFAULT_CHAT_MODEL=mlx-community/gemma-4-e4b-it-8bit \
uv run uvicorn server:app --host 0.0.0.0 --port 8678
```

## Endpoints

### POST /v1/embeddings
Text embeddings (OpenAI-compatible).

```bash
curl -X POST http://localhost:8678/v1/embeddings \
  -H 'Content-Type: application/json' \
  -d '{"model":"mlx-community/Qwen3-Embedding-0.6B-mxfp8","input":["Hello world"]}'
```

### POST /v1/chat/completions
Chat completions (OpenAI-compatible). Auto-detects backend: `mlx-lm` for text-only models, `mlx-vlm` for vision-language models (like gemma-4).

```bash
curl -X POST http://localhost:8678/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"mlx-community/Qwen3.5-0.8B-OptiQ-4bit","messages":[{"role":"user","content":"Hello"}],"max_tokens":128}'
```

### POST /v1/embeddings/multimodal
Text + image embeddings.

### GET /v1/models
List loaded and available models.

### GET /health
Health check with capabilities: `{"status": "ok", "capabilities": ["embeddings", "chat"]}`

## xplorertui Integration

Configure in `~/.config/xplorertui/config.toml`:

```toml
mlx_server_url = "http://localhost:8678"
mlx_embedding_model = "mlx-community/Qwen3-Embedding-0.6B-mxfp8"
mlx_chat_model = "mlx-community/Qwen3.5-0.8B-OptiQ-4bit"
```

The TUI probes `/health` at startup. If the server is down, it falls back to OpenRouter.

## Troubleshooting

**Server not detected by TUI** — The TUI probes capabilities once at startup. Restart the TUI after starting the server.

**Chat model fails to load** — Check server logs for backend auto-detection. Vision-language models (gemma-4, pixtral) need mlx-vlm, text-only models use mlx-lm. See `references/model-compatibility.md`.

**Slow first request** — Chat models are lazy-loaded on first request, including downloading from HuggingFace Hub if not cached.

**Out of memory** — Use 4-bit or 8-bit quantized variants. See memory estimates in `references/model-compatibility.md`.

**Choosing between quantization formats** — For details on 4-bit vs mxfp4 vs nvfp4, and 8-bit vs mxfp8, read `references/model-compatibility.md` which covers all formats, tradeoffs, and known issues.

## Development

```bash
ruff check .    # Lint
ruff format .   # Format
ty check .      # Type check
```

## Architecture

- **`server.py`** — FastAPI app, lifespan, endpoints, structured logging
- **`schemas.py`** — Pydantic request/response models (OpenAI-compatible)
- **`registry.py`** — `ModelRegistry` with lazy loading, `ChatBackend` auto-detection, image decode helpers
