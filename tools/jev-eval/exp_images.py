"""Jev 1.13 vs 1.14: multilingual per-post topics, and image input cost/latency.

Behind §10 of docs/jev-design.md. Posts come from `xplorertui search` JSONL
files in $JEV_EVAL_DATA/images/posts/ (see README.md). Data and the API
cache stay in $JEV_EVAL_DATA/images/, out of git.

Run: uv run --with pillow --with requests python tools/jev-eval/exp_images.py <step>
Steps: ref | text | images | report | summary
"""

import base64
import glob
import hashlib
import io
import json
import os
import re
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

import requests
from PIL import Image

_REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DATA = os.path.join(os.environ.get("JEV_EVAL_DATA", os.path.join(_REPO, "scratchpad", "jev-eval")), "images")
HERE = DATA  # cache, reference labels and raw results
os.makedirs(os.path.join(DATA, "posts"), exist_ok=True)
SUMMARY = os.path.join(os.path.dirname(os.path.abspath(__file__)), "results_images.json")


def _api_key():
    """OPENROUTER_API_KEY, else the key `xplorertui openrouter-auth` stored."""
    if key := os.environ.get("OPENROUTER_API_KEY"):
        return key
    path = os.path.expanduser("~/.config/xplorertui/openrouter_tokens.json")
    return json.load(open(path))["key"]


KEY = _api_key()
DECIDE = "https://openrouter.ai/api/alpha/decisions"
CHAT = "https://openrouter.ai/api/v1/chat/completions"
MODELS = ["typesafe/jev-1.13", "typesafe/jev-1.14"]
REF_MODEL = "anthropic/claude-sonnet-5.5"
CACHE = os.path.join(HERE, "cache.jsonl")

TOPICS = {
    "ai_ml": "AI, machine learning, LLMs, model releases",
    "programming": "Software engineering, programming languages, developer tools",
    "science_tech": "Science and technology (other than AI or programming)",
    "crypto": "Cryptocurrency, blockchain, web3",
    "business": "Business, startups, entrepreneurs, products",
    "finance": "Finance, markets, investing, the economy",
    "news_politics": "News, politics, government, social issues",
    "arts_culture": "Arts and culture, books, design",
    "pop_culture": "Celebrities and pop culture",
    "film_tv": "Film, TV, and video",
    "music": "Music",
    "gaming": "Gaming",
    "sports": "Sports",
    "fitness_health": "Fitness and health",
    "food_dining": "Food and dining",
    "travel": "Travel and adventure",
    "fashion": "Fashion and style",
    "education": "Learning and education",
    "daily_life": "Daily life, family, relationships, personal updates",
    "adult": "Adult or sexual content",
    "other": "None of the above",
}
TOPIC_Q = {
    "type": "choice",
    "instructions": "What is the main topic of the post?",
    "criteria": TOPICS,
}

_lock = threading.Lock()
_cache = {}
if os.path.exists(CACHE):
    for line in open(CACHE):
        r = json.loads(line)
        _cache[r["k"]] = r["v"]
SESSION = requests.Session()


def post(url, body, headers=None, use_cache=True):
    raw = json.dumps(body, sort_keys=True) + json.dumps(headers or {}, sort_keys=True)
    k = hashlib.sha256((url + raw).encode()).hexdigest()
    if use_cache:
        with _lock:
            if k in _cache:
                return _cache[k]
    h = {"Authorization": f"Bearer {KEY}", **(headers or {})}
    for attempt in range(4):
        t0 = time.time()
        r = SESSION.post(url, json=body, headers=h, timeout=120)
        lat = time.time() - t0
        if r.status_code in (429, 500, 502, 503, 504) and attempt < 3:
            time.sleep(2**attempt)
            continue
        v = {"status": r.status_code, "lat": lat, "body": r.json() if r.content else None}
        if r.status_code != 200:
            v["body"] = r.text[:400]
        break
    with _lock:
        _cache[k] = v
        with open(CACHE, "a") as f:
            f.write(json.dumps({"k": k, "v": v}) + "\n")
    return v


def pmap(fn, items, n=8):
    with ThreadPoolExecutor(n) as ex:
        return list(ex.map(fn, items))


def load_posts():
    seen, posts = set(), []
    for f in sorted(glob.glob(os.path.join(DATA, "posts", "*.jsonl"))):
        for line in open(f):
            r = json.loads(line)
            t = r["tweet"]
            if t["id"] in seen:
                continue
            seen.add(t["id"])
            photo = next((m for m in r.get("media") or [] if m["type"] == "photo" and m.get("url")), None)
            posts.append({"id": t["id"], "lang": t.get("lang"), "text": t["text"][:600],
                          "photo": photo["url"] if photo else None})
    return posts


def photo_url(url, name):
    stem = url.rsplit(".", 1)[0]
    return f"{stem}?format=jpg&name={name}"


# ---------------------------------------------------------------- images
MIN_SIDE, MAX_SIDE = 280, 1568


def prepare(url, name):
    """Download an X photo variant and fit it to Jev's limits. Returns (data_url, info)."""
    t0 = time.time()
    raw = SESSION.get(photo_url(url, name), timeout=60).content
    dl = time.time() - t0
    img = Image.open(io.BytesIO(raw)).convert("RGB")
    w, h = img.size
    scale = min(1.0, MAX_SIDE / max(w, h))
    if scale < 1.0:
        img = img.resize((round(w * scale), round(h * scale)))
    w2, h2 = img.size
    if min(w2, h2) < MIN_SIDE:  # pad the short side, keep pixels as sent
        canvas = Image.new("RGB", (max(w2, MIN_SIDE), max(h2, MIN_SIDE)), (0, 0, 0))
        canvas.paste(img, ((canvas.width - w2) // 2, (canvas.height - h2) // 2))
        img = canvas
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=85)
    data = base64.b64encode(buf.getvalue()).decode()
    return f"data:image/jpeg;base64,{data}", {
        "orig": [w, h], "sent": list(img.size), "bytes": len(buf.getvalue()), "dl_s": dl, "padded": (w2, h2) != img.size}


def image_part(data_url):
    return {"type": "image_url", "image_url": {"url": data_url}}


# ---------------------------------------------------------------- steps
def step_ref():
    posts = load_posts()
    keys = ", ".join(TOPICS)

    def ref(p):
        content = [{"type": "text", "text": f"Post:\n{p['text']}"}]
        if p["photo"]:
            content.append({"type": "image_url", "image_url": {"url": photo_url(p["photo"], "small")}})
        body = {"model": REF_MODEL, "max_tokens": 3000, "reasoning": {"effort": "low", "exclude": True},
                "messages": [{"role": "system", "content":
                              f"Classify the X post (text and attached image, if any) into exactly one topic key. "
                              f"Topics: {json.dumps(TOPICS)}. Reply with only the line TOPIC=<key>, where key is one of: {keys}. No other text."},
                             {"role": "user", "content": content}]}
        v = post(CHAT, body)
        if v["status"] != 200:
            return None
        c = (v["body"]["choices"][0]["message"].get("content") or "").strip()
        m = re.search(r"TOPIC=\s*`?([a-z_]+)", c)
        return m.group(1) if m else None

    labels = pmap(ref, posts)
    out = {p["id"]: l for p, l in zip(posts, labels)}
    json.dump(out, open(os.path.join(HERE, "ref.json"), "w"), indent=1)
    print(f"{sum(1 for l in labels if l in TOPICS)}/{len(posts)} reference labels")


def jev_topic(model, state, headers=None):
    v = post(DECIDE, {"model": model, "state": state, "questions": {"topic": TOPIC_Q}}, headers)
    if v["status"] != 200:
        return {"err": v["status"], "msg": str(v["body"])[:200], "lat": v["lat"]}
    a = v["body"]["answers"]["topic"]
    return {"choice": a["choice"], "conf": a.get("confidence"), "lat": v["lat"],
            "tok": v["body"]["usage"]["input_tokens"], "cost": v["body"]["usage"].get("cost", 0)}


def step_text():
    posts = load_posts()
    res = {}
    for m in MODELS:
        out = pmap(lambda p: jev_topic(m, {"post": p["text"]}), posts)
        res[m] = {p["id"]: o for p, o in zip(posts, out)}
    json.dump(res, open(os.path.join(HERE, "text.json"), "w"), indent=1)


def step_images():
    posts = [p for p in load_posts() if p["photo"]]
    prepared = {}

    def prep(p):
        try:
            return prepare(p["photo"], "small")
        except Exception as e:  # noqa: BLE001
            return None, {"error": str(e)}

    for p, (data, info) in zip(posts, pmap(prep, posts)):
        prepared[p["id"]] = (data, info)
    posts = [p for p in posts if prepared[p["id"]][0]]
    res = {"info": {pid: v[1] for pid, v in prepared.items()}}

    # 1. Header probe: one image post, both models, with and without the TypeSafe header.
    p0 = posts[0]
    st = {"post": p0["text"], "image": image_part(prepared[p0["id"]][0])}
    res["header_probe"] = {
        f"{m}|{h}": jev_topic(m, st, {"TypeSafe-Danger-Zone-Images": "true"} if h else None)
        for m in MODELS for h in (False, True)}

    # 2. Per post, both models: text+image and image only.
    for m in MODELS:
        res[m] = {}
        res[m]["text_image"] = dict(zip([p["id"] for p in posts], pmap(
            lambda p: jev_topic(m, {"post": p["text"], "image": image_part(prepared[p["id"]][0])}), posts)))
        res[m]["image_only"] = dict(zip([p["id"] for p in posts], pmap(
            lambda p: jev_topic(m, [image_part(prepared[p["id"]][0])]), posts)))

    # 3. Token cost by resolution on 6 posts: name=small (680) vs medium (1200) vs large->1568.
    sizes = {}
    for p in posts[:6]:
        for name in ("small", "medium", "large"):
            data, info = prepare(p["photo"], name)
            r = jev_topic(MODELS[1], [image_part(data)])
            sizes.setdefault(p["id"], {})[name] = {**info, **{k: r.get(k) for k in ("tok", "lat", "err")}}
    res["sizes"] = sizes

    # 4. Cluster-shaped request: 8 posts, 0/2/4/8 images in one state.
    group = posts[:8]
    clus = {}
    for n_img in (0, 2, 4, 8):
        items = []
        for i, p in enumerate(group):
            item = {"text": p["text"][:280]}
            if i < n_img:
                item["image"] = image_part(prepared[p["id"]][0])
            items.append(item)
        clus[n_img] = jev_topic(MODELS[1], {"posts": items})
    res["cluster"] = clus
    json.dump(res, open(os.path.join(HERE, "images.json"), "w"), indent=1)


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(q * len(xs)))] if xs else float("nan")


def summarize(rows, ref):
    ok = [(pid, r) for pid, r in rows.items() if "choice" in r and ref.get(pid) in TOPICS]
    acc = sum(r["choice"] == ref[pid] for pid, r in ok) / max(1, len(ok))
    hi = [(pid, r) for pid, r in ok if (r["conf"] or 0) >= 0.8]
    acc_hi = sum(r["choice"] == ref[pid] for pid, r in hi) / max(1, len(hi))
    lats = [r["lat"] for _, r in ok]
    toks = [r["tok"] for _, r in ok]
    errs = sum(1 for r in rows.values() if "err" in r)
    return (f"n={len(ok):3d} acc={acc:.2f} acc@0.8={acc_hi:.2f} (n={len(hi)}) "
            f"p50={pct(lats, .5):.2f}s p90={pct(lats, .9):.2f}s tok_p50={pct(toks, .5)} errs={errs}")


def step_report():
    posts = {p["id"]: p for p in load_posts()}
    ref = json.load(open(os.path.join(HERE, "ref.json")))
    text = json.load(open(os.path.join(HERE, "text.json")))
    print("## Text only, by language")
    langs = sorted({p["lang"] for p in posts.values()})
    for m in MODELS:
        print(m, "all  ", summarize(text[m], ref))
        for lang in langs:
            sub = {pid: r for pid, r in text[m].items() if posts[pid]["lang"] == lang}
            print(m, f"{lang:5s}", summarize(sub, ref))
    if os.path.exists(os.path.join(HERE, "images.json")):
        im = json.load(open(os.path.join(HERE, "images.json")))
        print("\n## Header probe")
        for k, v in im["header_probe"].items():
            print(" ", k, v)
        print("\n## Image posts")
        for m in MODELS:
            ids = list(im[m]["text_image"])
            print(m, "text only  ", summarize({i: text[m][i] for i in ids}, ref))
            print(m, "text+image ", summarize(im[m]["text_image"], ref))
            print(m, "image only ", summarize(im[m]["image_only"], ref))
        print("\n## Prepared images")
        infos = [v for v in im["info"].values() if "sent" in v]
        print(f"  n={len(infos)} padded={sum(v['padded'] for v in infos)} "
              f"bytes_p50={pct([v['bytes'] for v in infos], .5)} dl_p50={pct([v['dl_s'] for v in infos], .5):.2f}s "
              f"dl_p90={pct([v['dl_s'] for v in infos], .9):.2f}s")
        print("\n## Tokens by resolution (jev-1.14, image only)")
        for pid, by in im["sizes"].items():
            print(" ", pid, {n: (v["sent"], v["bytes"], v["tok"], round(v["lat"] or 0, 2)) for n, v in by.items()})
        print("\n## Cluster request, 8 posts, n images (jev-1.14)")
        for n, v in im["cluster"].items():
            print(" ", n, v)


def step_summary():
    """Write the numbers §10 reports, with post IDs but no post text."""
    posts = {p["id"]: p for p in load_posts()}
    ref = json.load(open(os.path.join(HERE, "ref.json")))
    text = json.load(open(os.path.join(HERE, "text.json")))
    im = json.load(open(os.path.join(HERE, "images.json")))

    def scores(rows):
        ok = [(pid, r) for pid, r in rows.items() if "choice" in r and ref.get(pid) in TOPICS]
        hi = [(pid, r) for pid, r in ok if (r["conf"] or 0) >= 0.8]
        return {
            "n": len(ok),
            "acc": round(sum(r["choice"] == ref[pid] for pid, r in ok) / max(1, len(ok)), 3),
            "acc_conf_0.8": round(sum(r["choice"] == ref[pid] for pid, r in hi) / max(1, len(hi)), 3),
            "n_conf_0.8": len(hi),
            "latency_p50": round(pct([r["lat"] for _, r in ok], 0.5), 3),
            "latency_p90": round(pct([r["lat"] for _, r in ok], 0.9), 3),
            "input_tokens_p50": pct([r["tok"] for _, r in ok], 0.5),
        }

    langs = sorted({p["lang"] for p in posts.values()})
    out = {"reference_model": REF_MODEL, "models": {}}
    for m in MODELS:
        out["models"][m] = {
            "text_all": scores(text[m]),
            "text_by_lang": {
                lang: scores({i: r for i, r in text[m].items() if posts[i]["lang"] == lang})
                for lang in langs
            },
            "photo_posts_text_only": scores({i: text[m][i] for i in im[m]["text_image"]}),
            "photo_posts_text_image": scores(im[m]["text_image"]),
            "photo_posts_image_only": scores(im[m]["image_only"]),
            "labels": {i: r.get("choice") for i, r in text[m].items()},
        }
    out["reference_labels"] = ref
    out["header_probe"] = {k: v.get("tok", v.get("err")) for k, v in im["header_probe"].items()}
    out["tokens_by_resolution"] = {
        pid: {n: {"sent": v["sent"], "tokens": v["tok"]} for n, v in by.items()}
        for pid, by in im["sizes"].items()
    }
    out["cluster_request_photos"] = {
        n: {"latency": round(v["lat"], 3), "tokens": v.get("tok")} for n, v in im["cluster"].items()
    }
    infos = [v for v in im["info"].values() if "dl_s" in v]
    out["downloads"] = {
        "n": len(infos),
        "under_0.1s": sum(v["dl_s"] < 0.1 for v in infos),
        "max_s": round(max(v["dl_s"] for v in infos), 2),
    }
    json.dump(out, open(SUMMARY, "w"), indent=1, ensure_ascii=False)
    print(f"wrote {SUMMARY}")


if __name__ == "__main__":
    steps = {"ref": step_ref, "text": step_text, "images": step_images, "report": step_report,
             "summary": step_summary}
    steps[sys.argv[1]]()
