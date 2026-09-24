"""Build a tiny random-weight Qwen3 embedding checkpoint in MLX format from the real
mlx-community/Qwen3-Embedding-0.6B-mxfp8 config and tokenizer config (shrunk, unquantized),
with a trained byte-level BPE tokenizer. Build once, then run embeddings_e2e.py from each venv
against the same directory. Usage: <python> make_tiny_qwen3.py <workdir>
"""
import json, pathlib, shutil, sys
from huggingface_hub import hf_hub_download
import mlx.core as mx
from mlx.utils import tree_flatten
from tokenizers import Tokenizer, models, pre_tokenizers, decoders
REPO = "mlx-community/Qwen3-Embedding-0.6B-mxfp8"
ck = pathlib.Path(sys.argv[1]); shutil.rmtree(ck, ignore_errors=True); ck.mkdir()
tc = json.load(open(hf_hub_download(REPO, "tokenizer_config.json")))
specials = [v["content"] for v in tc.pop("added_tokens_decoder").values()]
json.dump(tc, open(ck/"tokenizer_config.json","w"))
words = "home timeline rust is fast ratatui terminal ui semantic search for tweets about".split()
from tokenizers import trainers
tok = Tokenizer(models.BPE()); tok.pre_tokenizer = pre_tokenizers.ByteLevel(add_prefix_space=False); tok.decoder = decoders.ByteLevel()
tr = trainers.BpeTrainer(vocab_size=400, special_tokens=specials, initial_alphabet=pre_tokenizers.ByteLevel.alphabet())
tok.train_from_iterator([" ".join(words)] * 50, tr); tok.save(str(ck/"tokenizer.json"))
vocab = tok.get_vocab()
cfg = json.load(open(hf_hub_download(REPO, "config.json"))); cfg.pop("quantization")
cfg.update(hidden_size=64, head_dim=16, num_attention_heads=4, num_key_value_heads=2, num_hidden_layers=2,
           intermediate_size=128, vocab_size=len(vocab), bos_token_id=vocab["<|endoftext|>"], eos_token_id=vocab["<|endoftext|>"])
json.dump(cfg, open(ck/"config.json","w"), indent=1)
from mlx_embeddings.models import qwen3
mx.random.seed(0)
m = qwen3.Model(qwen3.ModelArgs.from_dict(cfg))
mx.save_safetensors(str(ck/"model.safetensors"), dict(tree_flatten(m.parameters())), metadata={"format": "mlx"})
print("built", ck, "vocab", len(vocab), "params", len(tree_flatten(m.parameters())))
