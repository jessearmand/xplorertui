"""Fetch a real X corpus via search/recent (bearer injected by proxy). Weak label = query bucket."""
import json, urllib.request, urllib.parse, re, sys, os
sys.path.insert(0, os.path.dirname(__file__))
from common import ROOT
B = " lang:en -is:retweet -is:reply"
NARROW = {
 "local_inference": '("llama.cpp" OR gguf OR ollama OR "mlx-lm" OR "run locally" OR "local llm")',
 "model_releases": '("open weights" OR "open-weight" OR "new model" OR "model card") (release OR released OR launches OR benchmark)',
 "chips_datacenters": '(nvidia OR tsmc OR "data center" OR datacenter OR hbm) (gpu OR gpus OR chips OR capex OR fab)',
 "ai_policy": '("AI regulation" OR "AI Act" OR "AI bill" OR "AI policy" OR "AI governance")',
 "rust": '(rustlang OR "written in rust" OR "rust crate" OR "cargo build" OR "borrow checker" OR "in Rust")',
 "terminal_tools": '(neovim OR tmux OR ghostty OR "terminal emulator" OR zsh OR dotfiles OR wezterm)',
 "coding_agents": '("claude code" OR "codex cli" OR "coding agent" OR "coding agents" OR "cursor agent")',
 "startup_funding": '("seed round" OR "series a" OR "series b" OR "pre-seed") (raised OR raise OR led)',
 "crypto": '(bitcoin OR ethereum OR solana OR btc OR eth) (price OR etf OR onchain OR defi)',
}
BROAD = {
 "sports": '(nba OR "premier league" OR nfl OR "champions league") (game OR win OR season)',
 "cooking": '(recipe OR "made dinner" OR baking OR sourdough)',
 "politics": '(congress OR senate OR election OR parliament) (vote OR bill OR poll)',
 "music": '("new album" OR "new single" OR tour OR concert) (listen OR tickets OR song)',
 "film_tv": '(movie OR film OR "tv show" OR series) (review OR watched OR trailer)',
 "fitness": '(workout OR "personal record" OR marathon OR gym OR lifting)',
 "travel": '(travel OR trip OR flight OR itinerary) (visited OR vacation OR backpacking)',
 "gaming": '(gaming OR "video game" OR ps5 OR nintendo OR steam) (playing OR played OR review)',
}
def search(q, n):
    url = "https://api.x.com/2/tweets/search/recent?" + urllib.parse.urlencode(
        {"query": q + B, "max_results": n, "tweet.fields": "author_id,created_at,public_metrics"})
    with urllib.request.urlopen(url, timeout=60) as r:
        return json.load(r).get("data", [])
def clean(t):
    t = re.sub(r"https://t\.co/\S+", "", t).strip()
    return t
out = []
seen = set()
for corpus, buckets, n, keep in (("narrow", NARROW, 40, 16), ("broad", BROAD, 25, 10)):
    for label, q in buckets.items():
        got = 0
        for tw in search(q, n):
            t = clean(tw["text"])
            key = t[:80].lower()
            if len(t) < 70 or key in seen: continue
            seen.add(key)
            out.append({"id": tw["id"], "corpus": corpus, "label": label, "text": t})
            got += 1
            if got >= keep: break
        print(corpus, label, got, file=sys.stderr)
json.dump(out, open(f"{ROOT}/corpus.json", "w"), indent=1)
