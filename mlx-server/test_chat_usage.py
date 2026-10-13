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


class FakeDetokenizer:
    """Token `i` is `segments[i]`."""

    def __init__(self, segments: list[str]):
        self.segments = segments
        self.text = ""

    def reset(self):
        self.text = ""

    def add_token(self, token):
        self.text += self.segments[token]

    def finalize(self):
        pass


class FakeTokenizer:
    def __init__(self, segments: list[str], prompt_suffix: str):
        self.segments = segments
        self.prompt_suffix = prompt_suffix

    def apply_chat_template(self, messages, **kwargs):
        return " ".join(m["content"] for m in messages) + self.prompt_suffix

    @property
    def detokenizer(self):
        return FakeDetokenizer(self.segments)


def fake_stream(segments: list[str], cap: int, lag: int):
    """`mlx_lm.stream_generate` for a model that wants to generate `segments`,
    one token each, then stop.

    Like the real one: a response per token, then a last response that has
    the finish reason and any text not sent yet. At the cap it stops with
    "length"; otherwise the end-of-sequence token counts as generated. With
    `lag`, the text of the newest `lag` tokens is held back, as a streaming
    detokenizer does with an unfinished word.
    """
    held: list[str] = []
    for token, segment in enumerate(segments):
        held.append(segment)
        if token + 1 == cap:
            break
        ready = len(held) - lag
        text = "".join(held[:ready]) if ready > 0 else ""
        del held[: max(ready, 0)]
        yield SimpleNamespace(
            text=text,
            token=token,
            prompt_tokens=3,
            generation_tokens=token + 1,
            finish_reason=None,
        )
    at_cap = len(segments) >= cap
    yield SimpleNamespace(
        text="".join(held),
        token=None,
        prompt_tokens=3,
        generation_tokens=cap if at_cap else len(segments) + 1,
        finish_reason="length" if at_cap else "stop",
    )


@pytest.fixture
def chat(monkeypatch):
    """Post a chat request to a model that wants to generate `segments`."""

    def post(segments: list[str], prompt_suffix: str = "", lag: int = 0) -> dict:
        def stream_generate(model, tokenizer, prompt, **kwargs):
            assert kwargs["max_tokens"] == MAX_TOKENS + HEADROOM
            return fake_stream(segments, kwargs["max_tokens"], lag)

        monkeypatch.setattr(mlx_lm, "stream_generate", stream_generate)
        monkeypatch.setattr(
            server.registry,
            "get_chat_model",
            lambda model_id: (
                ChatBackend.MLX_LM,
                object(),
                FakeTokenizer(segments, prompt_suffix),
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
        "completion_tokens": 3,
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
    # 5 thinking tokens (tags included) and a 3-token answer: more than
    # max_tokens in total, and the answer is whole.
    segments = ["<think>", "about ", "Rust ", "then", "</think>"]
    segments += ["Cluster ", "0: ", "Rust"]
    assert chat(segments) == {
        "content": "Cluster 0: Rust",
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
        "completion_tokens": 6,
        "reasoning_tokens": 3,
    }


def test_gemma_channel_thinking(chat):
    segments = ["<|channel>", "thought\n", "hm", "<channel|>", "Rust"]
    assert chat(segments) == {
        "content": "Rust",
        "finish_reason": "stop",
        "completion_tokens": 6,
        "reasoning_tokens": 4,
    }


def test_model_that_stops_at_once_gives_an_empty_answer(chat):
    assert chat([]) == {
        "content": "",
        "finish_reason": "stop",
        "completion_tokens": 1,
        "reasoning_tokens": 0,
    }


def test_thinking_tags_split_across_tokens(chat):
    segments = ["<th", "ink>", "hm", "</", "think>", "Rust ", "tooling"]
    result = chat(segments)
    assert result["content"] == "Rust tooling"
    assert result["finish_reason"] == "stop"
    # The first fragment is held back, then counted as thinking.
    assert result["reasoning_tokens"] == 5


def test_split_opening_tag_does_not_use_the_answer_budget(chat):
    # MAX_TOKENS is 4: charging `<th` to the answer would cut "d".
    segments = ["<th", "ink>", "hm", "</think>", "a ", "b ", "c ", "d"]
    result = chat(segments)
    assert result["content"] == "a b c d"
    assert result["reasoning_tokens"] == 4


def test_split_closing_tag_with_thinking_opened_by_the_prompt(chat):
    segments = ["hm", "</th", "ink>", "a ", "b ", "c ", "d"]
    result = chat(segments, prompt_suffix="\nassistant: <think>\n")
    assert result["content"] == "a b c d"
    assert result["reasoning_tokens"] == 3


def test_text_that_only_looks_like_a_tag_start_is_answer(chat):
    result = chat(["a ", "<", "b ", "c ", "d ", "e"])
    assert result["content"] == "a <b c"
    assert result["finish_reason"] == "length"
    assert result["reasoning_tokens"] == 0


def test_held_tokens_that_are_answer_text_do_not_pass_the_cap(chat):
    # `<`, `t`, `h` are held as a possible `<think>`; `x` rules it out and
    # all four count as answer at once, three more than the cap allows.
    assert chat(["a ", "b ", "c ", "<", "t", "h", "x ", "y"]) == {
        "content": "a b c <",
        "finish_reason": "length",
        "completion_tokens": MAX_TOKENS,
        "reasoning_tokens": 0,
    }


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


def test_text_held_back_by_the_stream_is_kept_at_the_answer_cap(chat):
    # The stream is one token behind when the server stops it, and a closed
    # stream never sends the rest.
    assert chat(["a ", "b ", "c ", "d ", "e ", "f ", "g"], lag=1) == {
        "content": "a b c d",
        "finish_reason": "length",
        "completion_tokens": MAX_TOKENS,
        "reasoning_tokens": 0,
    }


def test_held_back_text_after_thinking_is_kept_at_the_answer_cap(chat):
    segments = ["<think>", "hm", "</think>", "a ", "b ", "c ", "d ", "e ", "f"]
    result = chat(segments, lag=1)
    assert result["content"] == "a b c d"
    assert result["finish_reason"] == "length"


def test_held_back_text_is_sent_when_the_model_stops_itself(chat):
    assert chat(["Rust ", "tooling"], lag=1)["content"] == "Rust tooling"
