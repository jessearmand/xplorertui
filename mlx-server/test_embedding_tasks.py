"""Task prompts and the mlx-vlm fallback for text embeddings.

The models are faked; real MLX arrays are used so results pass through the
server's usual conversion.
"""

import json

import pytest

mx = pytest.importorskip("mlx.core")
mlx_embeddings = pytest.importorskip("mlx_embeddings")

import numpy as np  # noqa: E402
from fastapi.testclient import TestClient  # noqa: E402

import prompts  # noqa: E402
import registry  # noqa: E402
import server  # noqa: E402
from registry import EmbedBackend  # noqa: E402
from schemas import EmbeddingTask  # noqa: E402

GEMMA_PROMPTS = {
    "Clustering": "task: clustering | query: ",
    "SearchQuery": "task: search result | query: ",
    "Document": "title: none | text: ",
    "query": "task: search result | query: ",
    "document": "title: none | text: ",
}
QWEN_PROMPTS = {"query": "Instruct: retrieve passages\nQuery:", "document": ""}


def write_catalog(directory, catalog):
    path = directory / prompts.PROMPT_CATALOG_FILE
    path.write_text(json.dumps({"prompts": catalog}))
    return path


# --- prompt catalog ---------------------------------------------------------


def test_catalog_is_read_from_a_local_model_directory(tmp_path):
    write_catalog(tmp_path, GEMMA_PROMPTS)
    assert prompts.load_prompt_catalog(str(tmp_path)) == GEMMA_PROMPTS


def test_model_without_a_catalog_has_no_prompts(tmp_path):
    assert prompts.load_prompt_catalog(str(tmp_path)) == {}
    assert prompts.load_prompt_catalog("not-cached/model") == {}


@pytest.mark.parametrize("content", ["not json", "[]", '{"prompts": ["a"]}'])
def test_malformed_catalog_is_ignored(tmp_path, content):
    (tmp_path / prompts.PROMPT_CATALOG_FILE).write_text(content)
    assert prompts.load_prompt_catalog(str(tmp_path)) == {}


@pytest.mark.parametrize(
    ("catalog", "task", "expected"),
    [
        (GEMMA_PROMPTS, EmbeddingTask.CLUSTERING, "task: clustering | query: "),
        (GEMMA_PROMPTS, EmbeddingTask.SEARCH_QUERY, "task: search result | query: "),
        (GEMMA_PROMPTS, EmbeddingTask.DOCUMENT, "title: none | text: "),
        # Falls back to the generic name when the specific one is absent.
        (
            QWEN_PROMPTS,
            EmbeddingTask.SEARCH_QUERY,
            "Instruct: retrieve passages\nQuery:",
        ),
        (QWEN_PROMPTS, EmbeddingTask.DOCUMENT, ""),
        # The model has no prompt for the task.
        (QWEN_PROMPTS, EmbeddingTask.CLUSTERING, ""),
        (GEMMA_PROMPTS, EmbeddingTask.CLASSIFICATION, ""),
        ({}, EmbeddingTask.CLUSTERING, ""),
        (GEMMA_PROMPTS, None, ""),
    ],
)
def test_task_prompt(catalog, task, expected):
    assert prompts.task_prompt(catalog, task) == expected


def test_every_task_has_prompt_names():
    assert set(prompts.TASK_PROMPT_NAMES) == set(EmbeddingTask)


def test_apply_task_prompt_prepends_without_a_separator():
    texts = ["rust 1.90 is out", "new M5 chips"]
    assert prompts.apply_task_prompt(
        GEMMA_PROMPTS, EmbeddingTask.CLUSTERING, texts
    ) == [
        "task: clustering | query: rust 1.90 is out",
        "task: clustering | query: new M5 chips",
    ]
    assert prompts.apply_task_prompt({}, EmbeddingTask.CLUSTERING, texts) == texts
    assert prompts.apply_task_prompt(GEMMA_PROMPTS, None, texts) == texts


# --- registry fallback and the endpoint -------------------------------------


class FakeTokenizer:
    """Records what it was asked to tokenize; one token per character."""

    def __init__(self):
        self.batches: list[list[str]] = []

    def __call__(self, texts, **kwargs):
        self.batches.append(list(texts))
        width = max(len(t) for t in texts)
        mask = np.array([[1] * len(t) + [0] * (width - len(t)) for t in texts])
        return {"input_ids": mask.copy(), "attention_mask": mask}

    @property
    def seen(self) -> list[str]:
        return [text for batch in self.batches for text in batch]


class FakeVlmModel:
    """Embeds each text as [its length, 1], so order is checkable."""

    def __call__(self, input_ids, attention_mask):
        lengths = attention_mask.sum(axis=-1, keepdims=True).astype(mx.float32)
        embeds = mx.concatenate([lengths, mx.ones_like(lengths)], axis=-1)
        return type("Output", (), {"text_embeds": embeds})()


@pytest.fixture
def vlm_embedder(monkeypatch, tmp_path):
    """A model mlx-embeddings rejects, served by a fake mlx-vlm loader."""
    write_catalog(tmp_path, GEMMA_PROMPTS)
    tokenizer = FakeTokenizer()
    vlm_loads = []

    def unsupported(model_id, **kwargs):
        raise ValueError("Model type embedding_gemma2 not supported.")

    def load_vlm(model_id):
        vlm_loads.append(model_id)
        return FakeVlmModel(), tokenizer

    monkeypatch.setattr(server.registry, "default_model", None)
    monkeypatch.setattr(mlx_embeddings, "load", unsupported)
    monkeypatch.setattr(registry, "_load_text_embedder_mlx_vlm", load_vlm)
    return {"model_id": str(tmp_path), "tokenizer": tokenizer, "loads": vlm_loads}


def embed(client, model_id, texts, **extra):
    return client.post(
        "/v1/embeddings", json={"model": model_id, "input": texts, **extra}
    )


def test_unsupported_architecture_falls_back_to_mlx_vlm(vlm_embedder):
    model_id = vlm_embedder["model_id"]
    with TestClient(server.app) as client:
        first = embed(client, model_id, ["ab", "abcd"])
        second = embed(client, model_id, ["a"])
        embedder = server.registry._text_models[model_id]
        assert embedder.backend == EmbedBackend.MLX_VLM
        assert embedder.prompts == GEMMA_PROMPTS
    assert first.status_code == 200, first.json()
    assert [d["embedding"] for d in first.json()["data"]] == [[2.0, 1.0], [4.0, 1.0]]
    assert second.status_code == 200, second.json()
    assert vlm_embedder["loads"] == [model_id]  # cached after the first request


@pytest.mark.parametrize(
    ("task", "prefix"),
    [
        ("clustering", "task: clustering | query: "),
        ("search_query", "task: search result | query: "),
        ("document", "title: none | text: "),
        ("classification", ""),  # not in this model's catalog
        (None, ""),
    ],
)
def test_task_selects_the_prompt_from_the_model_catalog(vlm_embedder, task, prefix):
    extra = {} if task is None else {"task": task}
    with TestClient(server.app) as client:
        response = embed(client, vlm_embedder["model_id"], ["hello"], **extra)
    assert response.status_code == 200, response.json()
    assert vlm_embedder["tokenizer"].seen == [prefix + "hello"]


def test_unknown_task_is_rejected(vlm_embedder):
    with TestClient(server.app) as client:
        response = embed(client, vlm_embedder["model_id"], ["hello"], task="poetry")
    assert response.status_code == 422


def test_large_requests_are_batched_in_input_order(vlm_embedder):
    texts = ["x" * (i + 1) for i in range(server._EMBED_BATCH_SIZE * 2 + 3)]
    with TestClient(server.app) as client:
        response = embed(client, vlm_embedder["model_id"], texts)
    assert response.status_code == 200, response.json()
    assert [d["embedding"][0] for d in response.json()["data"]] == [
        float(len(t)) for t in texts
    ]
    assert [len(b) for b in vlm_embedder["tokenizer"].batches] == [
        server._EMBED_BATCH_SIZE,
        server._EMBED_BATCH_SIZE,
        3,
    ]


def test_empty_input_returns_no_embeddings(vlm_embedder):
    with TestClient(server.app) as client:
        response = embed(client, vlm_embedder["model_id"], [])
    assert response.status_code == 200, response.json()
    assert response.json()["data"] == []
