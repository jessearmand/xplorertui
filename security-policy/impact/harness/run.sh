#!/usr/bin/env bash
# Old-vs-new harness for the starlette and transformers bumps. Builds four venvs under
# $WORK, runs each check in the old and new venv, and diffs the outputs.
# Needs: uv, network access to PyPI and huggingface.co (small config files only).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="${WORK:-$HERE/../../../scratchpad/impact}"
mkdir -p "$WORK"
venv() { # name, packages...
  local d="$WORK/$1"; shift
  [ -x "$d/bin/python" ] || uv venv -q -p 3.11 "$d"
  VIRTUAL_ENV="$d" uv pip install -q "$@"
}
# Version banners and timestamped log lines are expected to differ; everything else must not.
NOISE='^(starlette [0-9]|transformers=|[0-9]{2}:[0-9]{2}:[0-9]{2} )'
same() { # label, old, new
  if diff <(grep -vE "$NOISE" "$2") <(grep -vE "$NOISE" "$3"); then
    echo "SAME  $1 ($(grep -vcE "$NOISE" "$3") lines compared)"
  else echo "DIFF  $1"; fi
}
MLX=("mlx[cpu]==0.31.1" "mlx-vlm==0.4.4" "mlx-lm==0.31.2" pillow numpy
     "mlx-embeddings @ git+https://github.com/Blaizzy/mlx-embeddings.git@v0.1.0")
venv web-old "fastapi==0.135.1" "starlette==0.52.1" httpx numpy pillow
venv web-new "fastapi==0.135.1" "starlette==1.7.0" httpx numpy pillow
venv ml-old "transformers==5.3.0" "tokenizers==0.22.2" "safetensors==0.7.0" "${MLX[@]}"
venv ml-new "transformers==5.17.0" "tokenizers==0.23.2" "safetensors==0.8.0" "${MLX[@]}"

for v in old new; do "$WORK/web-$v/bin/python" -u -W ignore "$HERE/web_layer.py" 2>/dev/null > "$WORK/web-$v.txt"; done
same "web layer (starlette 0.52.1 -> 1.7.0)" "$WORK/web-old.txt" "$WORK/web-new.txt"

for v in old new; do "$WORK/ml-$v/bin/python" -u -W ignore "$HERE/gemma4_processor.py" "$WORK/gemma4-$v" 2>/dev/null > "$WORK/gemma4-$v.txt"; done
same "gemma 4 processor + prompts (transformers 5.3.0 -> 5.17.0)" "$WORK/gemma4-old.txt" "$WORK/gemma4-new.txt"

"$WORK/ml-old/bin/python" -u -W ignore "$HERE/make_tiny_qwen3.py" "$WORK/qwen3-tiny" >/dev/null 2>&1
for v in old new; do "$WORK/ml-$v/bin/python" -u -W ignore "$HERE/embeddings_e2e.py" "$WORK/qwen3-tiny" 2>/dev/null > "$WORK/emb-$v.txt"; done
same "embeddings end to end + safetensors header (transformers, tokenizers, safetensors)" "$WORK/emb-old.txt" "$WORK/emb-new.txt"
