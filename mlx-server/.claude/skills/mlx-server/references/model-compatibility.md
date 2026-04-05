# Model Compatibility Reference

## Backend Auto-Detection

The mlx-server automatically selects the right backend when loading a chat model:

| Backend | Library | Model Types | Example Architectures |
|---------|---------|-------------|----------------------|
| `mlx_lm` | mlx-lm | Text-only LLMs | Qwen, Llama, Mistral, Phi |
| `mlx_vlm` | mlx-vlm | Vision-language / multimodal | Gemma 4, Gemma 3n, Llava, Pixtral |

The registry tries `mlx_lm.load()` first. If the architecture is unsupported (raises `ValueError` or `KeyError`), it falls back to `mlx_vlm.load()`. You can see which backend was selected in the server logs:

```
INFO [mlx-server] Trying mlx_lm for: mlx-community/gemma-4-e4b-it-8bit
INFO [mlx-server] mlx_lm unsupported for gemma-4-e4b-it-8bit (gemma4), falling back to mlx_vlm
INFO [mlx-server] Loaded mlx-community/gemma-4-e4b-it-8bit via mlx_vlm
```

## Quantization Formats

MLX community models are published in several precision formats. Understanding the differences helps choose between memory usage, speed, and quality.

### Full Precision

| Format | Bits | Description |
|--------|------|-------------|
| **bf16** | 16 | BFloat16 — full precision, no quantization. Best quality, largest size. |

### Integer Quantization

| Format | Bits | Description |
|--------|------|-------------|
| **3bit** | 3 | Aggressive compression. Noticeable quality loss. |
| **4bit** | 4 | Standard integer quantization. Good compression/quality tradeoff. Well-tested. |
| **5bit** | 5 | Middle ground between 4-bit and 6-bit. |
| **6bit** | 6 | Moderate compression. Better quality than 4-bit. |
| **8bit** | 8 | Light compression. Near-full quality. |

Integer quantization stores each weight as an N-bit integer with a per-group scale factor. Simple, fast, and well-supported across all MLX tooling.

### Microscaling Floating-Point (MXFP)

| Format | Bits | Block Size | Scale Type | Description |
|--------|------|-----------|------------|-------------|
| **mxfp4** | 4 | 32 elements | E8M0 (8-bit) | OCP standard. 4-bit floating-point (E2M1) with shared block scale. Better at preserving outlier values than INT4. |
| **mxfp8** | 8 | 32 elements | E8M0 (8-bit) | 8-bit floating-point with shared block scale. Better dynamic range than INT8. |

MXFP formats use floating-point representation for individual weights (not integers), giving them better dynamic range. The shared scale per block of 32 elements adapts to local weight distributions.

### NVIDIA Floating-Point (NVFP)

| Format | Bits | Block Size | Scale Type | Description |
|--------|------|-----------|------------|-------------|
| **nvfp4** | 4 | 16 elements | FP8 E4M3 | NVIDIA's improved 4-bit format. Half the block size of MXFP4 = finer-grained scaling. |

Key differences from MXFP4:
- **Smaller blocks** (16 vs 32): twice as many scale factors, better local adaptation
- **FP8 E4M3 scales** (vs E8M0): more precise scaling factors
- Generally better accuracy than MXFP4 at the same bit width

### Comparison: Same-Bitwidth Formats

#### 4-bit: INT4 vs MXFP4 vs NVFP4

| | 4bit (INT4) | mxfp4 | nvfp4 |
|---|---|---|---|
| **Quality** | Good | Better (preserves outliers) | Best (finest granularity) |
| **Speed** | Fastest | Fast | Fast |
| **Maturity** | Most tested | Established (OCP standard) | Newer, some MLX issues reported |
| **Recommendation** | Safe default | Good alternative | Best quality, but check compatibility |

#### 8-bit: INT8 vs MXFP8

| | 8bit (INT8) | mxfp8 |
|---|---|---|
| **Quality** | Very good | Slightly better dynamic range |
| **Speed** | Fast | Fast |
| **Size** | Same | Same |
| **Recommendation** | Safe default | Marginal improvement |

### Practical Guidance

For most users on Apple Silicon:
- **Best quality**: bf16 (if RAM allows)
- **Good balance**: 8bit or mxfp8
- **Memory constrained**: 4bit (most tested) or mxfp4
- **Avoid for now**: nvfp4 has known MLX issues with some model architectures (see below)

### Known Issues

- **nvfp4/mxfp4 with MoE models**: Garbage output reported for some Mixture-of-Experts models ([mlx-vlm#778](https://github.com/Blaizzy/mlx-vlm/issues/778))
- **nvfp4 scale format**: MLX uses signed E4M3 scales instead of unsigned UE4M3, causing reduced dynamic range compared to NVIDIA's Blackwell implementation ([mlx#2962](https://github.com/ml-explore/mlx/issues/2962))

## Gemma 4 Models (mlx-community)

Gemma 4 models use `Gemma4ForConditionalGeneration` — a vision-language architecture. They require `mlx-vlm >= 0.4.4` (not mlx-lm).

### Gemma-4 Fixes in mlx-vlm v0.4.4

v0.4.4 (released April 2026) bundles critical gemma-4 fixes that were broken in v0.4.3:

| Fix | PR | Impact |
|-----|-----|--------|
| Chunked prefill for KV-shared models + thinking | [#901](https://github.com/Blaizzy/mlx-vlm/pull/901) | Fixes 4bit word repetition, unusable MMLU scores, thinking token leakage |
| Vision + text degradation, missing processor config | [#906](https://github.com/Blaizzy/mlx-vlm/pull/906) | Fixes E2B/E4B vision encoder, processor config loading |
| Tool parser for nested arguments | [#916](https://github.com/Blaizzy/mlx-vlm/pull/916) | Fixes tool calls with array/object args |
| TurboQuant Metal kernel optimization | [#909](https://github.com/Blaizzy/mlx-vlm/pull/909) | 0.85-1.90x speedup, 89% KV savings |
| VisionFeatureCache for multi-turn | [#913](https://github.com/Blaizzy/mlx-vlm/pull/913) | Caches image features across turns |

### Remaining Known Issues

| Issue | Affects | Status | Link |
|-------|---------|--------|------|
| Weights load as zeros (sanitize() bug) | e4b/e2b 4bit/8bit | **OPEN** | [#912](https://github.com/Blaizzy/mlx-vlm/issues/912) |
| nvfp4/mxfp4 with MoE models | 26b-a4b microscaling formats | **Known risk** | [#778](https://github.com/Blaizzy/mlx-vlm/issues/778) |
| nvfp4 scale format (signed vs unsigned) | All nvfp4 | **MLX core issue** | [mlx#2962](https://github.com/ml-explore/mlx/issues/2962) |

### E4B Variants (4 billion effective parameters)

These are the smaller variants. Quantized e4b models may still be affected by the weights-zero bug ([#912](https://github.com/Blaizzy/mlx-vlm/issues/912)) — use bf16 for reliability.

| Model ID | Format | Pipeline | Status (v0.4.4) |
|----------|--------|----------|-----------------|
| `mlx-community/gemma-4-e4b-it-bf16` | bf16 | any-to-any | Works |
| `mlx-community/gemma-4-e4b-it-8bit` | INT8 | any-to-any | Caution — may hit [#912](https://github.com/Blaizzy/mlx-vlm/issues/912) |
| `mlx-community/gemma-4-e4b-it-mxfp8` | MXFP8 | any-to-any | Untested |
| `mlx-community/gemma-4-e4b-it-6bit` | INT6 | any-to-any | Untested |
| `mlx-community/gemma-4-e4b-it-5bit` | INT5 | any-to-any | Untested |
| `mlx-community/gemma-4-e4b-it-4bit` | INT4 | any-to-any | Caution — may hit [#912](https://github.com/Blaizzy/mlx-vlm/issues/912) |
| `mlx-community/gemma-4-e4b-it-mxfp4` | MXFP4 | any-to-any | Untested |
| `mlx-community/gemma-4-e4b-it-nvfp4` | NVFP4 | any-to-any | Untested |

### 31B Variants (dense, vision)

| Model ID | Format | Status (v0.4.4) |
|----------|--------|-----------------|
| `mlx-community/gemma-4-31b-it-bf16` | bf16 | Works |
| `mlx-community/gemma-4-31b-it-8bit` | INT8 | Fixed in v0.4.4 ([#901](https://github.com/Blaizzy/mlx-vlm/pull/901)) |
| `mlx-community/gemma-4-31b-it-4bit` | INT4 | Fixed in v0.4.4 ([#901](https://github.com/Blaizzy/mlx-vlm/pull/901)) |
| `mlx-community/gemma-4-31b-it-mxfp8` | MXFP8 | Should work with v0.4.4 fixes |

### 26B-A4B Variants (26 billion params, 4B active — MoE)

Larger MoE models. Avoid mxfp4/nvfp4 — MoE + microscaling has known issues ([#778](https://github.com/Blaizzy/mlx-vlm/issues/778)).

| Model ID | Format | Status (v0.4.4) |
|----------|--------|-----------------|
| `mlx-community/gemma-4-26b-a4b-it-bf16` | bf16 | Works |
| `mlx-community/gemma-4-26b-a4b-it-8bit` | INT8 | Fixed in v0.4.4 |
| `mlx-community/gemma-4-26b-a4b-it-4bit` | INT4 | Fixed in v0.4.4 |
| `mlx-community/gemma-4-26b-a4b-it-mxfp4` | MXFP4 | Avoid (MoE + microscaling risk) |
| `mlx-community/gemma-4-26b-a4b-it-nvfp4` | NVFP4 | Avoid (MoE + microscaling risk) |

## Embedding Models

Embedding models use `mlx-embeddings` (a separate library from mlx-lm/mlx-vlm):

| Model ID | Description |
|----------|-------------|
| `mlx-community/Qwen3-Embedding-0.6B-mxfp8` | Default. Small, fast, good quality. |

## Memory Estimates

Rough guidelines for Apple Silicon unified memory:

| Format | ~4B params | ~8B params | ~26B MoE |
|--------|-----------|-----------|----------|
| bf16 | ~8 GB | ~16 GB | ~52 GB |
| 8bit | ~4 GB | ~8 GB | ~26 GB |
| 4bit | ~2 GB | ~4 GB | ~13 GB |

Add ~1-2 GB overhead for tokenizer, KV cache, and framework.
