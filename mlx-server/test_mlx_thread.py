"""Model loading and generation must share one thread.

Since MLX 0.32, a lazy array built on one thread cannot be evaluated on another
("There is no Stream(gpu, 0) in current thread"). Gemma 4 hits this through its
RoPE frequencies, which are computed lazily at load time and first evaluated
during generation. These tests fake the model and mlx-lm's generate, but use
real MLX arrays, so on MLX >= 0.32 they fail if the server ever loads and
generates on different threads.
"""

import threading

import pytest

mx = pytest.importorskip("mlx.core")
mlx_lm = pytest.importorskip("mlx_lm")

from fastapi.testclient import TestClient  # noqa: E402

import server  # noqa: E402
from registry import ChatBackend  # noqa: E402


class FakeTokenizer:
    def apply_chat_template(self, messages, **kwargs):
        return " ".join(m["content"] for m in messages)

    def encode(self, text):
        return text.split()


class FakeModel:
    def __init__(self):
        # Lazy and not a parameter, like Gemma 4's RoPE `_freqs`.
        self.freqs = mx.exp(mx.arange(8, dtype=mx.float32) * 0.1)
        self.load_thread = threading.get_ident()


@pytest.fixture
def fake_chat(monkeypatch):
    seen = {}

    def get_chat_model(model_id):
        model = FakeModel()
        seen["load"] = model.load_thread
        return ChatBackend.MLX_LM, model, FakeTokenizer()

    def generate(model, tokenizer, prompt, **kwargs):
        seen["generate"] = threading.get_ident()
        mx.eval(model.freqs)  # raises across threads on MLX >= 0.32
        return "topic label"

    monkeypatch.setattr(server.registry, "default_model", None)
    monkeypatch.setattr(server.registry, "get_chat_model", get_chat_model)
    monkeypatch.setattr(mlx_lm, "generate", generate)
    return seen


def test_chat_loads_and_generates_on_the_mlx_thread(fake_chat):
    with TestClient(server.app) as client:
        response = client.post(
            "/v1/chat/completions",
            json={
                "model": "fake/model",
                "messages": [{"role": "user", "content": "label these"}],
                "max_tokens": 8,
            },
        )
    assert response.status_code == 200, response.json()
    assert response.json()["choices"][0]["message"]["content"] == "topic label"
    assert fake_chat["load"] == fake_chat["generate"]
    assert fake_chat["load"] != threading.get_ident()  # off the caller's thread


@pytest.mark.asyncio
async def test_run_mlx_keeps_lazy_arrays_usable_across_calls():
    lazy = await server.run_mlx(lambda: mx.arange(4) * 2)
    total = await server.run_mlx(lambda: (mx.eval(lazy), lazy.sum().item())[1])
    assert total == 12
