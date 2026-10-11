"""Per-post topic classification: Jev vs Luna on the fine taxonomy, reference = Claude Sonnet 5.5."""
import json, sys, os, re, numpy as np
sys.path.insert(0, os.path.dirname(__file__))
from common import *
corpus = json.load(open(f"{ROOT}/corpus.json"))
TAGS = {"is_question": "Is this post asking a question?", "is_news": "Is this post announcing or reporting news?",
        "is_promo": "Is this post promotional, an advertisement, or spam?"}
tax = "\n".join(f"- {k}: {v}" for k, v in FINE_CRIT.items())
REF_SYS = f"Classify each post into exactly one topic key from this list:\n{tax}\nReply with one line per post: `<index>: <key>`. Nothing else."
def ref_batch(start):
    batch = corpus[start:start + 20]
    user = "\n".join(f"{start+i}: {clean(x['text'], 1000)}" for i, x in enumerate(batch))
    out = chat("anthropic/claude-sonnet-5.5", REF_SYS, user, max_tokens=4000, temperature=0, reasoning={"effort": "low", "exclude": True})
    return {int(m.group(1)): m.group(2) for m in re.finditer(r"^\s*(\d+)\s*:\s*([a-z_]+)", out, re.M)}
ref = {}
for d in pmap(ref_batch, range(0, len(corpus), 20), n=4): ref.update(d)
def cls(args):
    model, i = args
    q = {"topic": {"type": "choice", "instructions": "Which topic is this post about?", "criteria": FINE_CRIT}}
    q.update({k: {"type": "noul", "instructions": v} for k, v in TAGS.items()})
    r = decide(model, {"post": clean(corpus[i]["text"], 1000)}, q)
    a = r["answers"]["topic"]
    return {"choice": a["choice"], "conf": a["confidence"], "lat": r["_latency"], "usage": r.get("usage", {})}
out = {"ref": ref}
for m in (JEV, LUNA):
    out[m] = pmap(cls, [(m, i) for i in range(len(corpus))])
json.dump(out, open(f"{ROOT}/results_posts.json", "w"), indent=1)
idx = [i for i in range(len(corpus)) if i in ref and ref[i] in FINE_CRIT]
print("ref coverage", len(idx), "/", len(corpus))
for m in (JEV, LUNA):
    rs = out[m]
    acc = np.mean([rs[i]["choice"] == ref[i] for i in idx])
    hi = [i for i in idx if rs[i]["conf"] >= 0.8]
    lat = np.array([r["lat"] for r in rs])
    tin = np.mean([r["usage"].get("input_tokens", 0) for r in rs]); cost = sum(r["usage"].get("cost", 0) for r in rs)
    print(m, f"acc={acc:.3f} hi_n={len(hi)} hi_acc={np.mean([rs[i]['choice']==ref[i] for i in hi]):.3f} "
          f"p50={np.percentile(lat,50):.2f} p90={np.percentile(lat,90):.2f} in_tok={tin:.0f} cost/1k=${cost/len(rs)*1000:.4f}")
agree = np.mean([out[JEV][i]["choice"] == out[LUNA][i]["choice"] for i in range(len(corpus))])
print("jev-luna agreement", round(agree, 3))
# confusion summary of reference
from collections import Counter
print(Counter(ref[i] for i in idx).most_common())
