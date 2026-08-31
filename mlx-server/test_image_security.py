import ssl
from collections.abc import Iterable
from io import BytesIO

import httpcore
import pytest

from image_security import (
    UnsafeImage,
    _decode_base64,
    _decode_isolated,
    _pinned_connection_pool,
    _PinnedNetworkBackend,
    _validate_public_url,
)


def test_url_validation_rejects_unsafe_urls():
    for value in (
        "http://example.com/a.png",
        "https://user@example.com/a",
        "https://[bad",
    ):
        with pytest.raises(UnsafeImage):
            _validate_public_url(value)


def test_url_validation_accepts_https_hostname():
    assert _validate_public_url("https://example.com/a.png") == ("example.com", 443)


def test_base64_is_strict_and_bounded():
    with pytest.raises(UnsafeImage):
        _decode_base64("not base64!")
    with pytest.raises(UnsafeImage):
        _decode_base64("data:image/gif;base64,R0lGODlh")


class _RecordingStream(httpcore.AsyncMockStream):
    def __init__(self) -> None:
        super().__init__(
            [b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"]
        )
        self.server_hostname: str | None = None
        self.writes: list[bytes] = []

    async def start_tls(
        self,
        ssl_context: ssl.SSLContext,
        server_hostname: str | None = None,
        timeout: float | None = None,
    ) -> httpcore.AsyncNetworkStream:
        self.server_hostname = server_hostname
        return self

    async def write(self, buffer: bytes, timeout: float | None = None) -> None:
        self.writes.append(buffer)


class _RecordingBackend(httpcore.AsyncNetworkBackend):
    def __init__(self) -> None:
        self.targets: list[tuple[str, int]] = []
        self.stream = _RecordingStream()

    async def connect_tcp(
        self,
        host: str,
        port: int,
        timeout: float | None = None,
        local_address: str | None = None,
        socket_options: Iterable[httpcore.SOCKET_OPTION] | None = None,
    ) -> httpcore.AsyncNetworkStream:
        self.targets.append((host, port))
        return self.stream

    async def connect_unix_socket(
        self,
        path: str,
        timeout: float | None = None,
        socket_options: Iterable[httpcore.SOCKET_OPTION] | None = None,
    ) -> httpcore.AsyncNetworkStream:
        raise AssertionError("unexpected Unix socket connection")

    async def sleep(self, seconds: float) -> None:
        pass


@pytest.mark.asyncio
async def test_pinned_backend_connects_to_validated_address():
    delegate = _RecordingBackend()
    backend = _PinnedNetworkBackend(
        "images.example.com",
        443,
        ("203.0.113.10",),
        backend=delegate,
    )

    await backend.connect_tcp("images.example.com", 443)

    assert delegate.targets == [("203.0.113.10", 443)]


@pytest.mark.asyncio
async def test_pinned_backend_rejects_a_different_origin():
    backend = _PinnedNetworkBackend(
        "images.example.com",
        443,
        ("203.0.113.10",),
        backend=_RecordingBackend(),
    )

    with pytest.raises(httpcore.ConnectError):
        await backend.connect_tcp("rebound.example.com", 443)


@pytest.mark.asyncio
async def test_pinned_pool_preserves_host_header_and_tls_hostname():
    delegate = _RecordingBackend()
    async with (
        _pinned_connection_pool(
            "images.example.com",
            443,
            ("203.0.113.10",),
            backend=delegate,
        ) as pool,
        pool.stream("GET", "https://images.example.com/image.png") as response,
    ):
        assert await response.aread() == b"ok"

    request = b"".join(delegate.stream.writes)
    assert delegate.targets == [("203.0.113.10", 443)]
    assert delegate.stream.server_hostname == "images.example.com"
    assert b"Host: images.example.com\r\n" in request


def test_isolated_decoder_accepts_png_and_normalizes_rgb():
    Image = pytest.importorskip("PIL.Image")
    source = BytesIO()
    Image.new("RGBA", (2, 3), (1, 2, 3, 4)).save(source, format="PNG")
    result = _decode_isolated(source.getvalue())
    assert result.mode == "RGB"
    assert result.size == (2, 3)


def test_isolated_decoder_rejects_unlisted_format():
    Image = pytest.importorskip("PIL.Image")
    source = BytesIO()
    Image.new("RGB", (2, 3)).save(source, format="GIF")
    with pytest.raises(UnsafeImage):
        _decode_isolated(source.getvalue())
