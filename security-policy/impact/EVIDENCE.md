# Impact of the starlette and transformers bumps on xplorertui

Upgrades checked (2026-09-24), resolved together by `uv lock` with no other package moving:

| Package | Locked | Target | Why it moves |
|---|---|---|---|
| starlette | 0.52.1 | 1.7.0 | security alert (policy: Held, `major_jump`) |
| transformers | 5.3.0 | 5.17.0 | Dependabot PR #75 (pinned `==5.3.0` in `mlx-server/pyproject.toml`) |
| tokenizers | 0.22.2 | 0.23.2 | pulled in by transformers 5.17 |
| safetensors | 0.7.0 | 0.8.0 | pulled in by transformers 5.17 |

FastAPI 0.135.1 already accepts starlette 1.x, so nothing else has to move.

**Result: no observable change on any path the TUI uses.** Every exposure below was run
old vs new, and the outputs were identical.

## Where the packages can reach

The Rust app never imports Python. It talks to mlx-server over HTTP through the optional
`MlxClient` (`src/mlx/client.rs`), so only four features can be affected at all:

| Feature | Endpoints (`src/app/dispatch.rs`, `src/app/mod.rs`) |
|---|---|
| startup / `:probe` capability check | `/health` |
| semantic re-rank | `/v1/embeddings` |
| timeline clustering | `/health`, `/v1/embeddings` |
| cluster topic labeling | `/v1/chat/completions` |

Timelines, search, profiles, threads, bookmarks, the JSONL CLI, auth, and the HF and
OpenRouter model pickers never call mlx-server. `MlxClient::embed_multimodal` has no callers,
so no feature reaches `/v1/embeddings/multimodal`.

What each endpoint imports:

- **starlette**: every endpoint (FastAPI routing, validation and error responses).
  `server.py` uses only the `lifespan=` context manager and plain JSON routes; none of the
  APIs removed in starlette 1.0 (`on_startup`/`on_shutdown`, `@app.on_event`, decorator
  routes or middleware, old `TemplateResponse` signature).
- **transformers / tokenizers**: embeddings (mlx-embeddings `load_tokenizer`) and chat.
  For Gemma 4, mlx-vlm 0.4.4 patches `AutoProcessor.from_pretrained` and substitutes its own
  numpy `Gemma4Processor`. transformers supplies only base classes, image utilities and
  `AutoTokenizer`, so the torchvision requirement of transformers' own Gemma 4 processor never
  applies. (transformers 5.3.0 has no `gemma4` model type at all. Under the current pin,
  Gemma 4 support comes entirely from mlx-vlm.)
- **safetensors**: chat only. mlx-vlm calls `safetensors.safe_open(...).metadata()` once, to
  read the `format: mlx` header. Weights load through `mx.load` everywhere, and mlx-embeddings
  never imports the package.

## The reason for the 5.3.0 pin no longer applies

The pin guarded against a 5.5.x loader regression: `tokenization_mistral_common.py` imported
`ReasoningEffort` from `mistral_common`. In 5.17.0 that import sits behind
`is_mistral_common_available(min_version)`, and `mistral_common` isn't in `mlx-server/uv.lock`.
The failing import can't run.

## Old-vs-new runs (`harness/run.sh`)

`harness/run.sh` builds old and new venvs and diffs each check's output. It ignores version
banners and timestamped log lines, and normalizes object addresses.

| Check | What runs | Result |
|---|---|---|
| `web_layer.py` | real `server.py` app through `TestClient` with MLX stubbed. Covers every endpoint: 200, 422 validation, 500 load failure, 400 image rejection, 404; valid and invalid images through `image_security`'s spawn-context decoder | identical |
| `gemma4_processor.py` | mlx-vlm's real load order for `mlx-community/gemma-4-e2b-it-4bit` config, processor config and chat template; `apply_chat_template(enable_thinking=False)`; image encoding (266 soft tokens); mlx-lm's `TokenizerWrapper` prompt | identical, including token-ID and pixel hashes |
| `embeddings_e2e.py` | a tiny random-weight Qwen3 embedding checkpoint in MLX format, built once from the real `Qwen3-Embedding-0.6B-mxfp8` config; `mlx_embeddings.load` then `generate`, as `server.py` calls them; plus the `safe_open` header read | identical embeddings (hash, norms, cosine) |

### Limits, stated plainly

- **Synthetic tokenizers.** Hugging Face serves `tokenizer.json` and weights from its Xet CDN
  (`us.aws.cdn.hf.co`), which this environment's network policy blocks; `huggingface.co`
  itself is reachable. The checks use the real configs and chat templates with synthetic
  tokenizers that carry the real special tokens. They exercise the same library code, but
  not the real vocabularies.
- **Nothing ran on Metal.** Runs used MLX's Linux CPU backend with `mx.disable_compile()`:
  MLX 0.31.1's CPU JIT fails to build fused kernels with this container's GCC, identically on
  old and new. Generation math is MLX's, and MLX isn't part of either bump.
- **No real model generated text.** Real Gemma 4 or Qwen3 weights were never loaded, so
  no model produced output here.

A final smoke test on the Mac covers all three limits: `uv sync` on the upgraded lock, then
one topic-labeling run and one cluster run.

## Bend: the blast radius, proven

`model.bend` encodes the tables above: features to endpoints, endpoints to packages, and the
evidence per endpoint and package. `LAWS.bend` states the claims, and `PROOF.bend` proves them
for every feature × package pair. Gate: `bend security-policy/impact/PROOF.bend` must print
`All terms check.`

- `x_features_unaffected`: no non-MLX feature can be affected by any of the four packages
- `nothing_needs_device_check`: every exposure is covered by an identical old-vs-new run
- `starlette_verified_for_mlx_features`: starlette reaches all four MLX features, and each
  exposure was verified
- `probe_only_sees_starlette`: the ML packages can't affect the capability probe
- `safetensors_only_through_chat`
- `multimodal_unreachable`, `multimodal_gap_recorded`: the multimodal model path was not run,
  and that's safe only because nothing calls it
- `gaps_are_flagged`: an exposure without evidence yields `NeedsDeviceCheck`

The proofs fail when they should. Each of these edits to `model.bend` turns the gate red:
topic labeling calling `/v1/embeddings/multimodal`; dropping the Gemma processor evidence;
`/health` importing transformers; the home timeline calling embeddings.

The laws are only as true as the tables. The tables come from reading the code and from
`harness/run.sh`. After editing a table, run `python3 gen_proof.py` and then the gate.
