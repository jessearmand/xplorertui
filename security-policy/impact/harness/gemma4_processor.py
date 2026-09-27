"""Gemma 4 chat/vision preprocessing exactly as server.py drives it: mlx-vlm load order
(get_model_and_args, then load_processor), apply_chat_template(enable_thinking=False),
image encoding, and mlx-lm's tokenizer wrapper. Real config, processor config and chat
template from mlx-community/gemma-4-e2b-it-4bit; tokenizer.json is synthetic (the real one is
served from HF's Xet CDN) but carries the real special tokens. Usage: <python> gemma4_processor.py <workdir>
"""
import sys, json, shutil, pathlib, os
from huggingface_hub import hf_hub_download
import transformers, tokenizers
from tokenizers import Tokenizer, models, pre_tokenizers, decoders
REPO = "mlx-community/gemma-4-e2b-it-4bit"
ck = pathlib.Path(sys.argv[1]); shutil.rmtree(ck, ignore_errors=True); ck.mkdir(parents=True)
for f in ["config.json","processor_config.json","chat_template.jinja","tokenizer_config.json"]:
    shutil.copy(hf_hub_download(REPO, f), ck/f)
tc = json.load(open(ck/"tokenizer_config.json"))
specials = [v for k,v in tc.items() if k.endswith("_token") and isinstance(v,str)] + tc.get("extra_special_tokens",[])
words = "user model system label this cluster of tweets in 3 words describe the image \n".split(" ")
vocab = {t:i for i,t in enumerate(dict.fromkeys(specials + words + ["[UNK]"]))}
tok = Tokenizer(models.WordLevel(vocab, unk_token="[UNK]"))
tok.pre_tokenizer = pre_tokenizers.WhitespaceSplit(); tok.decoder = decoders.WordPiece()
tok.add_special_tokens(specials)
tok.save(str(ck/"tokenizer.json"))

print(f"transformers={transformers.__version__} tokenizers={tokenizers.__version__}")
from mlx_vlm.utils import load_processor, load_config, get_model_and_args
get_model_and_args(json.load(open(ck/"config.json")))  # same import load() does
from mlx_vlm.prompt_utils import apply_chat_template
cfg = load_config(ck)
proc = load_processor(ck, add_detokenizer=True)
print("processor:", type(proc).__module__ + "." + type(proc).__name__, "| tokenizer:", type(proc.tokenizer).__name__)
msgs = [{"role":"system","content":"You label clusters."},{"role":"user","content":"Label this cluster of tweets in 3 words."}]
prompt = apply_chat_template(proc, cfg, msgs, enable_thinking=False)
print("prompt:", repr(prompt))
from PIL import Image
prompt_img = apply_chat_template(proc, cfg, [{"role":"user","content":"describe the image"}], num_images=1, enable_thinking=False)
enc = proc(text=[prompt_img], images=[Image.new("RGB",(640,480),"red")], return_tensors="mlx", padding=True)
print("image enc:", {k: tuple(v.shape) for k,v in enc.items() if hasattr(v,"shape")})
print("image tokens in ids:", int((enc["input_ids"] == proc.tokenizer.convert_tokens_to_ids(tc["image_token"])).sum()))
from mlx_lm.utils import load_tokenizer
t = load_tokenizer(ck)
p2 = t.apply_chat_template(msgs, tokenize=False, add_generation_prompt=True, enable_thinking=False)
print("mlx_lm tokenizer:", type(t).__name__, "| same prompt as vlm path:", p2 == prompt, "| encode len:", len(t.encode(p2)))
import hashlib, numpy as np
print("mlx_lm prompt:", repr(p2))
print("ids sha:", hashlib.sha256(np.array(enc["input_ids"]).tobytes()).hexdigest()[:16], "pixels sha:", hashlib.sha256(np.array(enc["pixel_values"]).tobytes()).hexdigest()[:16])
print("decode roundtrip:", repr(proc.tokenizer.decode(proc.tokenizer.encode("Label this cluster"), skip_special_tokens=True)))
