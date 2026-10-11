"""`max_tokens` caps the answer, not the thinking a model does anyway.

`enable_thinking=False` is a chat-template variable, so a model whose template
ignores it still thinks. The thinking is stripped from the answer; if it also
used `max_tokens`, a short budget would leave no answer. The server gives
thinking its own headroom in the same run and reports what it used.
"""

from types import SimpleNamespace

import pytest

mlx_lm = pytest.importorskip("mlx_lm")

from fastapi.testclient import TestClient  # noqa: E402

import server  # noqa: E402
from registry import ChatBackend  # noqa: E402

MAX_TOKENS = 4
HEADROOM = 6


class FakeTokenizer:
    def __init__(self, prompt_suffix: str = ""):
        self.prompt_suffix = prompt_suffix

    def apply_chat_template(self, messages, **kwargs):
        return " ".join(m["content"] for m in messages) + self.prompt_suffix


@pytest.fixture
def chat(monkeypatch):
    """Post a chat request to a model that wants to generate `segments`,
    one token each. Like mlx-lm, the model stops at the `max_tokens` it is
    given, or when the segments run out."""

    def post(segments: list[str], prompt_suffix: str = "") -> dict:
        def stream_generate(model, tokenizer, prompt, **kwargs):
            cap = kwargs["max_tokens"]
            assert cap == MAX_TOKENS + HEADROOM
            for i, segment in enumerate(segments[:cap], start=1):
                finish_reason = None
                if i == cap:
                    finish_reason = "length"
                elif i == len(segments):
                    finish_reason = "stop"
                yield SimpleNamespace(
                    text=segment,
                    prompt_tokens=3,
                    generation_tokens=i,
                    finish_reason=finish_reason,
                )

        monkeypatch.setattr(mlx_lm, "stream_generate", stream_generate)
        monkeypatch.setattr(
            server.registry,
            "get_chat_model",
            lambda model_id: (
                ChatBackend.MLX_LM,
                object(),
                FakeTokenizer(prompt_suffix),
            ),
        )
        with TestClient(server.app) as client:
            response = client.post(
                "/v1/chat/completions",
                json={
                    "model": "fake/model",
                    "messages": [{"role": "user", "content": "label these posts"}],
                    "max_tokens": MAX_TOKENS,
                },
            )
        assert response.status_code == 200, response.json()
        body = response.json()
        return {
            "content": body["choices"][0]["message"]["content"],
            "finish_reason": body["choices"][0]["finish_reason"],
            "completion_tokens": body["usage"]["completion_tokens"],
            "reasoning_tokens": body["usage"]["completion_tokens_details"][
                "reasoning_tokens"
            ],
        }

    monkeypatch.setattr(server.registry, "default_model", None)
    monkeypatch.setattr(server, "THINKING_HEADROOM_TOKENS", HEADROOM)
    return post


def test_short_answer_stops_on_its_own(chat):
    assert chat(["Rust ", "tooling"]) == {
        "content": "Rust tooling",
        "finish_reason": "stop",
        "completion_tokens": 2,
        "reasoning_tokens": 0,
    }


def test_long_answer_is_cut_at_max_tokens_without_using_the_headroom(chat):
    assert chat(["a ", "b\n", "c ", "d ", "e ", "f ", "g"]) == {
        "content": "a b\nc d",
        "finish_reason": "length",
        "completion_tokens": MAX_TOKENS,
        "reasoning_tokens": 0,
    }


def test_thinking_does_not_use_the_answer_budget(chat):
    # 5 thinking tokens (tags included) and a 4-token answer: more than
    # max_tokens in total, and the answer is whole.
    segments = ["<think>", "about ", "Rust ", "then", "</think>"]
    segments += ["Cluster ", "0: ", "Rust ", "tools"]
    assert chat(segments) == {
        "content": "Cluster 0: Rust tools",
        "finish_reason": "stop",
        "completion_tokens": 9,
        "reasoning_tokens": 5,
    }


def test_answer_after_thinking_is_still_cut_at_max_tokens(chat):
    segments = ["<think>", "hm", "</think>", "a ", "b ", "c ", "d ", "e ", "f"]
    assert chat(segments) == {
        "content": "a b c d",
        "finish_reason": "length",
        "completion_tokens": 7,
        "reasoning_tokens": 3,
    }


def test_thinking_that_never_ends_stops_at_the_headroom(chat):
    result = chat(["<think>"] + ["hm "] * 50)
    assert result == {
        "content": "",
        "finish_reason": "length",
        "completion_tokens": MAX_TOKENS + HEADROOM,
        "reasoning_tokens": MAX_TOKENS + HEADROOM,
    }


def test_thinking_block_opened_by_the_prompt(chat):
    # The template ended the prompt with `<think>`, so the output has only
    # the closing tag.
    segments = ["about ", "Rust", "</think>", "Rust ", "tooling"]
    assert chat(segments, prompt_suffix="\nassistant: <think>\n") == {
        "content": "Rust tooling",
        "finish_reason": "stop",
        "completion_tokens": 5,
        "reasoning_tokens": 3,
    }


def test_gemma_channel_thinking(chat):
    segments = ["<|channel>", "thought\n", "hm", "<channel|>", "Rust"]
    assert chat(segments) == {
        "content": "Rust",
        "finish_reason": "stop",
        "completion_tokens": 5,
        "reasoning_tokens": 4,
    }


def test_no_generated_tokens_is_an_empty_stop(chat):
    assert chat([]) == {
        "content": "",
        "finish_reason": "stop",
        "completion_tokens": 0,
        "reasoning_tokens": 0,
    }


def test_thinking_tags_split_across_tokens(chat):
    segments = ["<th", "ink>", "hm", "</", "think>", "Rust ", "tooling"]
    result = chat(segments)
    assert result["content"] == "Rust tooling"
    assert result["finish_reason"] == "stop"
    # The first fragment cannot be told from answer text yet.
    assert result["reasoning_tokens"] == 4


def test_tracking_work_does_not_grow_with_the_reply():
    # Each token is checked against a few carried characters, not the reply
    # so far: the carry stays shorter than a tag however long the reply is.
    tracker = server._ThinkingTracker()
    assert tracker.feed("<think>")
    for _ in range(10_000):
        assert tracker.feed("still thinking ")
        assert len(tracker._carry) < server._LONGEST_THINKING_TAG
    assert tracker.feed("</think>")
    assert not tracker.feed("Rust")
