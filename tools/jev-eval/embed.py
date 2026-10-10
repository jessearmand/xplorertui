import json, sys, os, numpy as np
sys.path.insert(0, os.path.dirname(__file__))
from common import ROOT
from sentence_transformers import SentenceTransformer
d = json.load(open(f"{ROOT}/corpus.json"))
texts = [x["text"] for x in d]
which = sys.argv[1]
if which == "qwen":
    m = SentenceTransformer("Qwen/Qwen3-Embedding-0.6B", device="cpu")
    e = m.encode(texts, batch_size=16, normalize_embeddings=True, show_progress_bar=False)
else:
    m = SentenceTransformer("google/embeddinggemma-2", device="cpu",
                            config_kwargs={"vision_config": None, "audio_config": None})
    e = m.encode(texts, prompt_name="Clustering", batch_size=16, normalize_embeddings=True, show_progress_bar=False)
np.save(f"{ROOT}/emb_{which}.npy", e); print(which, e.shape)
