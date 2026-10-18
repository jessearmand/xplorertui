# Jev cluster-label evaluation

Scripts behind §9 of `docs/jev-design.md`: why Jev's taxonomy labels collide on
real timelines, and how propose-then-pick compares. `exp_images.py` is behind
§10: Jev 1.13 against 1.14 on posts in six languages, and what photos cost.

Data (post text, embeddings, API cache) is written to `scratchpad/jev-eval/`
(override with `JEV_EVAL_DATA`) and is not committed. `corpus_ids.json` lists
the post IDs and query buckets used for the published numbers.

```bash
python -m venv scratchpad/.venv
scratchpad/.venv/bin/pip install numpy scipy scikit-learn sentence-transformers pillow

# Needs X API bearer auth and an OpenRouter key on the requests. The scripts
# send no auth headers themselves (they ran behind an auth-injecting proxy);
# add an Authorization header in common.py/fetch.py to run them elsewhere.
python3 tools/jev-eval/fetch.py                       # X search -> corpus.json
scratchpad/.venv/bin/python tools/jev-eval/embed.py qwen   # Qwen3-Embedding-0.6B
scratchpad/.venv/bin/python tools/jev-eval/embed.py gemma  # EmbeddingGemma 2
scratchpad/.venv/bin/python tools/jev-eval/exp_cluster.py 3  # results_cluster.json
scratchpad/.venv/bin/python tools/jev-eval/exp_posts.py      # results_posts.json
```

- `exp_cluster.py`: k-means at k = 5 and 8 over both corpora and embedders,
  then each labelling setup (taxonomy, 30-topic taxonomy, two-level,
  chat-written, propose-then-pick) scored by duplicate count and
  distinguishability.
- `exp_posts.py`: per-post topic accuracy of Jev vs Luna against Claude
  Sonnet 5.5 reference labels.
- `exp_images.py`: per-post topics on multilingual posts (Jev 1.13 and 1.14
  against Claude Sonnet 5.5 reference labels), photo posts with text only,
  text and photo, and photo only, photo tokens by resolution, and photos in
  one cluster request. It reads `xplorertui search` output and calls
  OpenRouter with `OPENROUTER_API_KEY` or the key `xplorertui
  openrouter-auth` stored. Its data goes to `scratchpad/jev-eval/images/`.

  ```bash
  mkdir -p scratchpad/jev-eval/images/posts
  fnox exec -- xplorertui search "(AI OR GPU OR football) has:images -is:retweet lang:en" \
    > scratchpad/jev-eval/images/posts/en_img.jsonl   # one file per query
  uv run --with pillow --with requests python tools/jev-eval/exp_images.py ref     # reference labels
  uv run --with pillow --with requests python tools/jev-eval/exp_images.py text    # text only
  uv run --with pillow --with requests python tools/jev-eval/exp_images.py images  # photo runs
  uv run --with pillow --with requests python tools/jev-eval/exp_images.py summary # results_images.json
  ```
- `results_*.json`: the run reported in the design doc (labels and scores,
  no post text).
