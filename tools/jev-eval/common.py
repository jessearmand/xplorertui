import json, os, time, hashlib, threading, re, urllib.request, urllib.error
from concurrent.futures import ThreadPoolExecutor
# Data (corpus text, embeddings, API cache, results) stays out of git.
_REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ROOT = os.environ.get("JEV_EVAL_DATA", os.path.join(_REPO, "scratchpad", "jev-eval"))
os.makedirs(ROOT, exist_ok=True)
CACHE_F = os.path.join(ROOT, "cache.jsonl")
_cache, _lock = {}, threading.Lock()
if os.path.exists(CACHE_F):
    for l in open(CACHE_F):
        r = json.loads(l); _cache[r["k"]] = r["v"]
STATS = {"cost": 0.0, "calls": 0, "lat": {}}

def _post(url, body, timeout=120):
    key = hashlib.sha256((url + json.dumps(body, sort_keys=True)).encode()).hexdigest()
    with _lock:
        if key in _cache: return _cache[key]
    for attempt in range(5):
        t0 = time.time()
        try:
            req = urllib.request.Request(url, json.dumps(body).encode(), {"content-type": "application/json"})
            with urllib.request.urlopen(req, timeout=timeout) as r:
                v = json.load(r)
            v["_latency"] = time.time() - t0
            break
        except urllib.error.HTTPError as e:
            msg = e.read().decode()[:300]
            if e.code in (429, 500, 502, 503, 504) and attempt < 4:
                time.sleep(2 ** attempt); continue
            raise RuntimeError(f"{e.code} {msg}")
        except Exception:
            if attempt < 4: time.sleep(2 ** attempt); continue
            raise
    with _lock:
        _cache[key] = v
        with open(CACHE_F, "a") as f: f.write(json.dumps({"k": key, "v": v}) + "\n")
    return v

def decide(model, state, questions):
    return _post("https://openrouter.ai/api/alpha/decisions", {"model": model, "state": state, "questions": questions})

def chat(model, system, user, max_tokens=800, temperature=0.3, reasoning=None):
    body = {"model": model, "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "max_tokens": max_tokens, "temperature": temperature, "reasoning": reasoning or {"enabled": False}}
    r = _post("https://openrouter.ai/api/v1/chat/completions", body)
    c = r["choices"][0]["message"].get("content") or ""
    return re.sub(r"<think>.*?</think>", "", c, flags=re.S).strip()

def pmap(fn, items, n=8):
    with ThreadPoolExecutor(n) as ex: return list(ex.map(fn, items))

def clean(t, n=280): return t[:n].replace("\n", " ")

JEV, LUNA = "typesafe/jev-1.13", "openai/gpt-6-luna-decisions"
QWEN9 = "qwen/qwen3.5-9b"

# ---- taxonomies -------------------------------------------------------------
DEFAULT = {
 "ai_ml": "AI, machine learning, LLMs, model releases",
 "programming": "Software engineering, programming languages, developer tools",
 "science_tech": "Science and technology (other than AI or programming)",
 "crypto": "Cryptocurrency, blockchain, web3",
 "business": "Business, startups, entrepreneurs, products",
 "finance": "Finance, markets, investing, the economy",
 "news_politics": "News, politics, government, social issues",
 "arts_culture": "Arts and culture, books, design",
 "pop_culture": "Celebrities and pop culture",
 "film_tv": "Film, TV, and video", "music": "Music", "gaming": "Gaming", "sports": "Sports",
 "fitness_health": "Fitness and health", "food_dining": "Food and dining",
 "travel": "Travel and adventure", "fashion": "Fashion and style",
 "education": "Learning and education",
 "daily_life": "Daily life, family, relationships, personal updates",
 "other": "None of the above",
}
def topic_display(k, topics):  # mirrors decisions.rs
    if k == "other" or k not in topics: return k.replace("_", " ")
    return re.split(r"[,(]", topics[k])[0].strip()

# fine taxonomy: key -> (parent, display name, description)
FINE = {
 "local_ai": ("ai_ml", "Local AI inference", "Running AI models locally: quantization, llama.cpp, MLX, Ollama, on-device"),
 "model_releases": ("ai_ml", "AI model releases", "New AI model releases, open weights, benchmarks and evals"),
 "ai_chips": ("ai_ml", "AI chips & datacenters", "AI chips, GPUs, Nvidia, semiconductors, datacenters and compute capex"),
 "ai_policy": ("ai_ml", "AI policy", "AI policy, regulation, safety and governance"),
 "ai_products": ("ai_ml", "AI products", "AI products, assistants and agents for end users; AI industry commentary"),
 "rust_lang": ("programming", "Rust", "The Rust programming language, crates, Rust projects"),
 "terminal_cli": ("programming", "Terminal & CLI tools", "Terminals, shells, text editors like Neovim, CLI tools, dotfiles"),
 "coding_agents": ("programming", "AI coding agents", "AI coding agents and assistants for developers (Claude Code, Codex, Cursor)"),
 "software_eng": ("programming", "Software engineering", "Other software engineering, programming languages, open source"),
 "startup_funding": ("business", "Startup funding", "Startup fundraising, venture capital rounds, valuations"),
 "business_news": ("business", "Business news", "Companies, products, earnings, deals and industry business news"),
 "markets": ("finance", "Stock markets", "Stock markets, trading, investing, stock picks"),
 "economy": ("finance", "Economy", "Macroeconomy, inflation, jobs, interest rates"),
 "crypto_markets": ("crypto", "Crypto markets", "Cryptocurrency prices, trading, ETFs and flows"),
 "crypto_tech": ("crypto", "Crypto tech", "Blockchain technology, DeFi, onchain activity, web3 projects"),
 "politics": ("news_politics", "Politics", "Elections, parties, legislatures, government and politicians"),
 "world_news": ("news_politics", "World news", "World news, conflicts, diplomacy, social issues"),
 "science": ("science_tech", "Science", "Science, research and non-AI technology"),
 "sports": ("sports", "Sports", "Sports games, teams, leagues and betting"),
 "arts_culture": ("arts_culture", "Arts & books", "Arts and culture, books, design"),
 "pop_culture": ("pop_culture", "Pop culture", "Celebrities and pop culture"),
 "film_tv": ("film_tv", "Film & TV", "Film, TV, and video"),
 "music": ("music", "Music", "Music, albums, concerts"),
 "gaming": ("gaming", "Gaming", "Video games and gaming hardware"),
 "fitness_health": ("fitness_health", "Fitness & health", "Fitness, workouts, running, health"),
 "food_dining": ("food_dining", "Food & cooking", "Food, cooking, recipes and dining"),
 "travel": ("travel", "Travel", "Travel, trips and destinations"),
 "fashion": ("fashion", "Fashion", "Fashion and style"),
 "education": ("education", "Education", "Learning and education"),
 "daily_life": ("daily_life", "Daily life", "Daily life, family, relationships, personal updates"),
 "other": ("other", "Other", "None of the above"),
}
FINE_CRIT = {k: v[2] for k, v in FINE.items()}
def fine_display(k): return FINE.get(k, (None, k))[1]
