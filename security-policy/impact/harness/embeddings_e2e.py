"""server.py's /v1/embeddings path end to end: mlx_embeddings.load -> generate -> text_embeds,
plus the one safetensors-package call on mlx-vlm's load path (safe_open header read).
Usage: <python> embeddings_e2e.py <checkpoint from make_tiny_qwen3.py>
"""
import sys, hashlib, numpy as np, transformers, tokenizers, safetensors
import mlx.core as _mx; _mx.disable_compile()  # MLX CPU JIT vs container GCC; Metal unaffected
print(f"transformers={transformers.__version__} tokenizers={tokenizers.__version__} safetensors={safetensors.__version__}")
ck = sys.argv[1]
# 1) the only safetensors-package call on the mlx-vlm load path
with safetensors.safe_open(ck + "/model.safetensors", framework="np") as f:
    print("safe_open metadata format:", f.metadata().get("format"), "| keys:", len(list(f.keys())))
# 2) server.py /v1/embeddings path: mlx_embeddings.load -> generate -> text_embeds
from mlx_embeddings import load, generate
model, tok = load(ck)
out = generate(model, tok, texts=["rust terminal ui is fast", "semantic search for tweets about ratatui"])
e = np.array(out.text_embeds, dtype=np.float32)
print("tokenizer:", type(tok).__name__, "| embeds shape:", e.shape, "| norms:", np.round(np.linalg.norm(e, axis=1), 5).tolist())
print("embeds sha:", hashlib.sha256(np.round(e, 6).tobytes()).hexdigest()[:16], "| cos(0,1):", round(float(e[0] @ e[1]), 6))
