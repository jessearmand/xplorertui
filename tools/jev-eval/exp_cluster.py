"""Cluster-label experiment on the real X corpus.
Usage: python exp_cluster.py  -> writes results/cluster.json"""
import json, random, re, sys, os, numpy as np
from collections import Counter
from sklearn.cluster import KMeans
from scipy.optimize import linear_sum_assignment
sys.path.insert(0, os.path.dirname(__file__))
from common import *

corpus = json.load(open(f"{ROOT}/corpus.json"))
EMB = {e: np.load(f"{ROOT}/emb_{e}.npy") for e in ("qwen", "gemma")}

def timeline(corp, seed=0):
    idx = [i for i, x in enumerate(corpus) if x["corpus"] == corp]
    random.Random(1000 + seed).shuffle(idx)  # emulate time-ordered mixed timeline
    return idx

# ---- labelers: each returns k labels -------------------------------------
def reps(texts):
    return [clean(t) for t in texts[:8]]

def label_decision(model, clusters, topics, display):
    def one(texts):
        r = decide(model, {"posts": reps(texts)},
                   {"topic": {"type": "choice", "instructions": "Which topic do most of these posts share?", "criteria": topics}})
        a = r["answers"]["topic"]; ch, conf, probs = a["choice"], a["confidence"], a["probabilities"]
        if ch == "other" or conf < 0.5: return "Mixed"
        ru = [(k, p) for k, p in probs.items() if k not in (ch, "other") and p >= 0.25]
        prim = display(ch)
        if ru: prim += " · " + display(max(ru, key=lambda x: x[1])[0])
        return prim
    return pmap(one, clusters)

APP_SYS = ("For each cluster of tweets, analyze a common topic and generate a short descriptive topic label (3-5 words). "
           "Reply with exactly one line per cluster in the format `Cluster <index>: <label>`. Include every cluster 0 "
           "through N-1. No numbering, no preamble, no trailing explanations.")
def cluster_block(clusters):
    s = ""
    for c, texts in enumerate(clusters):
        s += f"## Cluster {c}\n" + "".join(f"- {t}\n" for t in reps(texts)) + "\n"
    return s
def parse_lines(content, k, pat=r"^\s*\**\s*Cluster\s+(\d+)\s*\**\s*[:\-–]\s*(.+)$"):
    out = [""] * k
    for line in content.splitlines():
        m = re.match(pat, line, re.I)
        if m and int(m.group(1)) < k and not out[int(m.group(1))]:
            out[int(m.group(1))] = m.group(2).strip().strip("*`\"")
    return out

def label_chat(model, clusters):
    return parse_lines(chat(model, APP_SYS, cluster_block(clusters), max_tokens=60 * len(clusters) + 200), len(clusters))

PROP_SYS = ("For each cluster of tweets, propose 3 distinct candidate topic labels (2-5 words each), ordered from most "
            "specific to most general. Reply with one line per cluster: `Cluster <index>: label A | label B | label C`. "
            "No preamble.")
def label_propose_pick(model, clusters, gen=QWEN9):
    k = len(clusters)
    raw = parse_lines(chat(gen, PROP_SYS, cluster_block(clusters), max_tokens=80 * k + 200), k)
    cands, seen = [], set()
    for line in raw:
        for c in line.split("|"):
            c = c.strip().strip("*`\"")
            if c and c.lower() not in seen: seen.add(c.lower()); cands.append(c)
    # Same acceptance rule as the app: every cluster proposed something and
    # the pool can give each cluster its own label.
    if any(not line for line in raw) or len(cands) < k:
        raise RuntimeError(f"proposal did not cover every cluster: {raw}")
    crit = {f"c{i}": c for i, c in enumerate(cands)}
    crit["other"] = "None of these"
    def one(texts):
        r = decide(model, {"posts": reps(texts)},
                   {"topic": {"type": "choice", "instructions": "Which label best describes the topic most of these posts share?", "criteria": crit}})
        return r["answers"]["topic"]["probabilities"]
    probs = pmap(one, clusters)
    out = [""] * k
    # As in the app: P(none of these) >= 0.5 -> Mixed, left out of the assignment.
    rows = [c for c in range(k) if probs[c].get("other", 0.0) < 0.5]
    for c in range(k):
        if c not in rows: out[c] = "Mixed"
    if rows:
        P = np.array([[probs[c].get(f"c{i}", 0.0) for i in range(len(cands))] for c in rows])
        r_idx, cols = linear_sum_assignment(-np.log(P + 1e-6))  # unique labels
        for r_, c_ in zip(r_idx, cols): out[rows[r_]] = cands[c_]
    return out, len(cands)

def label_two_level(model, clusters):
    """Coarse default taxonomy; clusters that collide on a parent are re-asked with its subtopics."""
    def coarse(texts):
        r = decide(model, {"posts": reps(texts)},
                   {"topic": {"type": "choice", "instructions": "Which topic do most of these posts share?", "criteria": DEFAULT}})
        return r["answers"]["topic"]["choice"]
    parents = pmap(coarse, clusters)
    cnt = Counter(parents)
    def fine(args):
        c, texts = args
        p = parents[c]
        subs = {k: v[2] for k, v in FINE.items() if v[0] == p}
        if cnt[p] < 2 or len(subs) < 2:
            return FINE[p][1] if p in FINE else topic_display(p, DEFAULT)
        subs["other"] = "None of these"
        r = decide(model, {"posts": reps(texts)},
                   {"topic": {"type": "choice", "instructions": "Which subtopic do most of these posts share?", "criteria": subs}})
        ch = r["answers"]["topic"]["choice"]
        return fine_display(ch) if ch != "other" else topic_display(p, DEFAULT)
    return pmap(fine, list(enumerate(clusters)))

# ---- metric -------------------------------------------------------------
REFUSALS = []
def distinguish(labels, held_out, matchers=(JEV, LUNA)):
    """For each cluster with held-out posts (more than the 8 the labeller saw)
    and a non-Mixed label, can a matcher pick its own label (by index) from
    all k labels? Returns (score, number of clusters scored)."""
    crit = {f"l{i}": (l or "(empty)") for i, l in enumerate(labels)}
    scored = [c for c in range(len(labels)) if held_out[c] and labels[c] != "Mixed"]
    if not scored: return float("nan"), 0
    scores = []
    for m in matchers:
        def one(c):
          try:
            r = decide(m, {"posts": [clean(t) for t in held_out[c][:8]]},
                       {"label": {"type": "choice", "instructions": "Which label best describes these posts?", "criteria": crit}})
            return r["answers"]["label"]["choice"] == f"l{c}"
          except RuntimeError as e:
            REFUSALS.append(str(e)[:120]); return False
        scores.append(np.mean(pmap(one, scored)))
    return float(np.mean(scores)), len(scored)

def run(corp, emb, k, seed):
    order = timeline(corp, seed)
    X = EMB[emb][order]
    lab = KMeans(k, n_init=1, random_state=seed).fit_predict(X)
    clusters = [[corpus[order[i]]["text"] for i in range(len(order)) if lab[i] == c] for c in range(k)]
    # held-out sample for the matcher: posts after the first 8 when possible
    # Matcher sees only posts the labeller did not (labellers get the first 8).
    held = [c[8:16] for c in clusters]
    purity = float(np.mean([Counter(corpus[order[i]]["label"] for i in range(len(order)) if lab[i] == c).most_common(1)[0][1]
                             / max(1, sum(lab == c)) for c in range(k)]))
    res = {"corpus": corp, "emb": emb, "k": k, "seed": seed, "sizes": [len(c) for c in clusters], "purity": purity,
           "buckets": [Counter(corpus[order[i]]["label"] for i in range(len(order)) if lab[i] == c).most_common(3) for c in range(k)],
           "setups": {}}
    setups = {
        "jev_default": lambda: label_decision(JEV, clusters, DEFAULT, lambda x: topic_display(x, DEFAULT)),
        "luna_default": lambda: label_decision(LUNA, clusters, DEFAULT, lambda x: topic_display(x, DEFAULT)),
        "qwen9_app": lambda: label_chat(QWEN9, clusters),
        "jev_fine": lambda: label_decision(JEV, clusters, FINE_CRIT, fine_display),
        "luna_fine": lambda: label_decision(LUNA, clusters, FINE_CRIT, fine_display),
        "jev_two_level": lambda: label_two_level(JEV, clusters),
        "jev_propose_pick": lambda: label_propose_pick(JEV, clusters)[0],
        "luna_propose_pick": lambda: label_propose_pick(LUNA, clusters)[0],
    }
    for name, fn in setups.items():
        try:
            labels = fn()
        except Exception as e:
            res["setups"][name] = {"error": str(e)[:200]}; continue
        named = [l.lower() for l in labels if l != "Mixed"]
        dist, n_scored = distinguish(labels, held)
        res["setups"][name] = {"labels": labels, "dup": len(named) - len(set(named)),
                               "mixed": labels.count("Mixed"), "dist": dist, "n_scored": n_scored}
    print(corp, emb, k, seed, {n: (s.get("dup"), round(s.get("dist", -1), 2)) for n, s in res["setups"].items()}, flush=True)
    return res

if __name__ == "__main__":
    seeds = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    out = []
    for corp in ("narrow", "broad"):
        for emb in ("qwen", "gemma"):
            for k in (5, 8):
                for s in range(seeds):
                    out.append(run(corp, emb, k, s))
                    print("refusals so far", len(REFUSALS), flush=True)
                    json.dump(out, open(f"{ROOT}/results_cluster.json", "w"), indent=1)
