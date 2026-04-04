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

Gemma 4 models use `Gemma4ForConditionalGeneration` — a vision-language architecture. They require `mlx-vlm >= 0.4.3` (not mlx-lm).

### Gemma-4 Known Issues (mlx-vlm v0.4.3)

v0.4.3 shipped with multiple bugs affecting gemma-4. Many fixes are merged to `main` but not yet released as v0.4.4. Install from main for fixes: `pip install git+https://github.com/Blaizzy/mlx-vlm.git`

| Issue | Affects | Status | Link |
|-------|---------|--------|------|
| Weights load as zeros (sanitize() bug) | e4b/e2b 4bit/8bit | **OPEN** | [#912](https://github.com/Blaizzy/mlx-vlm/issues/912) |
| Tool parser breaks on nested args | All gemma-4 | **OPEN** | [#914](https://github.com/Blaizzy/mlx-vlm/issues/914) |
| TurboQuant KV-cache crashes with MoE | 26b-a4b all formats | **OPEN** | [#904](https://github.com/Blaizzy/mlx-vlm/issues/904) |
| 4bit word repetition / unusable MMLU | 31b 4bit | Fixed on main | [#902](https://github.com/Blaizzy/mlx-vlm/issues/902), [#895](https://github.com/Blaizzy/mlx-vlm/issues/895) |
| Q8 garbled thinking output | 31b/e4b 8bit | Fixed on main | [#892](https://github.com/Blaizzy/mlx-vlm/issues/892) |
| E2B vision encoder degradation | e2b all | Fixed on main | [#905](https://github.com/Blaizzy/mlx-vlm/issues/905) |
| E2B/E4B audio gibberish | e2b/e4b all | Fixed on main | [#903](https://github.com/Blaizzy/mlx-vlm/issues/903) |

### E4B Variants (4 billion effective parameters)

These are the smaller variants. **Warning: quantized e4b models are currently broken** (weights load as zeros, [#912](https://github.com/Blaizzy/mlx-vlm/issues/912)).

| Model ID | Format | Pipeline | Status |
|----------|--------|----------|--------|
| `mlx-community/gemma-4-e4b-it-bf16` | bf16 | any-to-any | Works (after main-branch audio/vision fixes) |
| `mlx-community/gemma-4-e4b-it-8bit` | INT8 | any-to-any | BROKEN — weights zero ([#912](https://github.com/Blaizzy/mlx-vlm/issues/912)) |
| `mlx-community/gemma-4-e4b-it-mxfp8` | MXFP8 | any-to-any | Untested |
| `mlx-community/gemma-4-e4b-it-6bit` | INT6 | any-to-any | Untested |
| `mlx-community/gemma-4-e4b-it-5bit` | INT5 | any-to-any | Untested |
| `mlx-community/gemma-4-e4b-it-4bit` | INT4 | any-to-any | BROKEN — weights zero ([#912](https://github.com/Blaizzy/mlx-vlm/issues/912)) |
| `mlx-community/gemma-4-e4b-it-mxfp4` | MXFP4 | any-to-any | Untested (1 download) |
| `mlx-community/gemma-4-e4b-it-nvfp4` | NVFP4 | any-to-any | Untested (1 download) |

### 31B Variants (dense, vision)

| Model ID | Format | Status |
|----------|--------|--------|
| `mlx-community/gemma-4-31b-it-bf16` | bf16 | Works in v0.4.3 |
| `mlx-community/gemma-4-31b-it-8bit` | INT8 | Fixed on main ([#893](https://github.com/Blaizzy/mlx-vlm/pull/893)) |
| `mlx-community/gemma-4-31b-it-4bit` | INT4 | Fixed on main ([#901](https://github.com/Blaizzy/mlx-vlm/pull/901)) |
| `mlx-community/gemma-4-31b-it-mxfp8` | MXFP8 | Needs main-branch fixes |

### 26B-A4B Variants (26 billion params, 4B active — MoE)

Larger MoE models. Avoid mxfp4/nvfp4 — MoE + microscaling has known issues ([#778](https://github.com/Blaizzy/mlx-vlm/issues/778)). TurboQuant KV-cache is broken for MoE ([#904](https://github.com/Blaizzy/mlx-vlm/issues/904)).

| Model ID | Format | Status |
|----------|--------|--------|
| `mlx-community/gemma-4-26b-a4b-it-bf16` | bf16 | Works |
| `mlx-community/gemma-4-26b-a4b-it-8bit` | INT8 | Needs main-branch fixes |
| `mlx-community/gemma-4-26b-a4b-it-4bit` | INT4 | Needs main-branch fixes |
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
