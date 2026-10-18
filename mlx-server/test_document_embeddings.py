"""`/v1/embeddings/documents`: one vector per text + image document.

The model and processor are faked; real MLX arrays pass through the
server's usual conversion.
"""

import pytest

mx = pytest.importorskip("mlx.core")

from fastapi.testclient import TestClient  # noqa: E402

import registry  # noqa: E402
import server  # noqa: E402
from image_security import UnsafeImage  # noqa: E402

IMAGE_TOKEN = "<image>"
GEMMA_PROMPTS = {"Clustering": "task: clustering | query: "}


class FakeProcessor:
    """Records each call. Every character is a token; an image adds 3."""

    image_token = IMAGE_TOKEN

    def __init__(self):
        self.calls: list[tuple[str, int]] = []

    def __call__(self, text, images=None, return_tensors=None):
        (prompt,) = text
        n_images = len(images or [])
        self.calls.append((prompt, n_images))
        length = len(prompt.replace(IMAGE_TOKEN, "")) + 3 * n_images
        return {
            "input_ids": mx.ones((1, length)),
            "attention_mask": mx.ones((1, length)),
            "pixel_values": mx.ones((1, 1)) if n_images else None,
        }


class FakeDocumentModel:
    """Embeds a document as [token count, has image]."""

    def __call__(self, input_ids, attention_mask, pixel_values=None):
        length = attention_mask.sum(axis=-1, keepdims=True)
        has_image = mx.full(length.shape, 0.0 if pixel_values is None else 1.0)
        embeds = mx.concatenate([length, has_image], axis=-1)
        return type("Output", (), {"text_embeds": embeds})()


@pytest.fixture
def document_model(monkeypatch, tmp_path):
    processor = FakeProcessor()

    def load(model_id):
        if model_id.endswith("text-only"):
            raise ValueError(f"{model_id} has no vision tower")
        return FakeDocumentModel(), processor

    async def decode(values):
        for value in values:
            if value == "bad":
                raise UnsafeImage("unsupported image data URL")
        return [f"decoded:{v}" for v in values]

    monkeypatch.setattr(server.registry, "default_model", None)
    monkeypatch.setattr(registry, "_load_document_embedder_mlx_vlm", load)
    monkeypatch.setattr(registry, "load_prompt_catalog", lambda _: GEMMA_PROMPTS)
    monkeypatch.setattr(server, "decode_images", decode)
    return {"model_id": str(tmp_path), "processor": processor}


def post(client, model_id, documents, **extra):
    return client.post(
        "/v1/embeddings/documents",
        json={"model": model_id, "input": documents, **extra},
    )


def test_each_image_stays_with_its_document(document_model):
    docs = [
        {"text": "ramen", "image": "data:image/jpeg;base64,AAA"},
        {"text": "no photo"},
        {"text": "jersey", "image": "data:image/jpeg;base64,BBB"},
    ]
    with TestClient(server.app) as client:
        response = post(client, document_model["model_id"], docs, task="clustering")
    assert response.status_code == 200, response.json()
    vectors = [d["embedding"] for d in response.json()["data"]]
    prefix = "task: clustering | query: "
    assert document_model["processor"].calls == [
        (f"{prefix}ramen\n{IMAGE_TOKEN}", 1),
        (f"{prefix}no photo", 0),
        (f"{prefix}jersey\n{IMAGE_TOKEN}", 1),
    ]
    assert [v[1] for v in vectors] == [1.0, 0.0, 1.0]
    assert vectors[0][0] == len(prefix + "ramen\n") + 3
    assert response.json()["usage"]["prompt_tokens"] == sum(v[0] for v in vectors)


def test_model_without_vision_is_a_client_error(document_model):
    with TestClient(server.app) as client:
        response = post(client, "org/text-only", [{"text": "a", "image": "x"}])
    assert response.status_code == 400
    assert "cannot embed images" in response.json()["detail"]


def test_unsafe_image_is_rejected(document_model):
    with TestClient(server.app) as client:
        response = post(
            client, document_model["model_id"], [{"text": "a", "image": "bad"}]
        )
    assert response.status_code == 400


def test_empty_input_is_rejected(document_model):
    with TestClient(server.app) as client:
        response = post(client, document_model["model_id"], [])
    assert response.status_code == 422
