"""Task prompts for instruction-tuned embedding models.

Checkpoints published for Sentence Transformers carry a prompt catalog in
``config_sentence_transformers.json`` (prompt name -> text to prepend). Reading
it from the checkpoint keeps the prompt wording with the model: EmbeddingGemma 2
answers ``clustering`` with ``task: clustering | query: ``, Qwen3-Embedding
answers ``search_query`` with its retrieval instruction, and a model with no
catalog embeds the text unchanged.
"""

from __future__ import annotations

import json
import logging
from pathlib import Path

from schemas import EmbeddingTask

logger = logging.getLogger("mlx-server")

PROMPT_CATALOG_FILE = "config_sentence_transformers.json"

# Catalog prompt names for each task, most specific first. Names follow the
# Sentence Transformers conventions used by published checkpoints.
TASK_PROMPT_NAMES: dict[EmbeddingTask, tuple[str, ...]] = {
    EmbeddingTask.CLUSTERING: ("Clustering",),
    EmbeddingTask.SEARCH_QUERY: ("SearchQuery", "Retrieval-query", "query"),
    EmbeddingTask.DOCUMENT: ("Document", "Retrieval-document", "document"),
    EmbeddingTask.CLASSIFICATION: ("Classification",),
    EmbeddingTask.SIMILARITY: ("SentenceSimilarity", "STS"),
}


def find_prompt_catalog(model_id: str) -> Path | None:
    """Locate the prompt catalog of an already-downloaded model.

    ``model_id`` is a local directory or a Hugging Face repo id. Only the local
    cache is consulted, so call this after the model has been loaded.
    """
    local = Path(model_id) / PROMPT_CATALOG_FILE
    if local.is_file():
        return local

    from huggingface_hub import try_to_load_from_cache

    try:
        cached = try_to_load_from_cache(model_id, PROMPT_CATALOG_FILE)
    except ValueError:  # not a valid repo id, e.g. a missing local path
        return None
    # A str is a cached file; anything else means absent or not cached.
    return Path(cached) if isinstance(cached, str) else None


def load_prompt_catalog(model_id: str) -> dict[str, str]:
    """Return the model's prompt catalog, or an empty one if it has none."""
    path = find_prompt_catalog(model_id)
    if path is None:
        return {}
    try:
        catalog = json.loads(path.read_text())
    except (OSError, ValueError) as exc:
        logger.warning("Ignoring unreadable prompt catalog %s: %s", path, exc)
        return {}
    prompts = catalog.get("prompts") if isinstance(catalog, dict) else None
    if not isinstance(prompts, dict):
        return {}
    return {
        name: text
        for name, text in prompts.items()
        if isinstance(name, str) and isinstance(text, str)
    }


def task_prompt(catalog: dict[str, str], task: EmbeddingTask | None) -> str:
    """Return the text to prepend for ``task``; empty if the model has none."""
    if task is None:
        return ""
    for name in TASK_PROMPT_NAMES[task]:
        if name in catalog:
            return catalog[name]
    return ""


def apply_task_prompt(
    catalog: dict[str, str], task: EmbeddingTask | None, texts: list[str]
) -> list[str]:
    """Prepend the task prompt to each text, as Sentence Transformers does."""
    prompt = task_prompt(catalog, task)
    return [prompt + text for text in texts] if prompt else texts
