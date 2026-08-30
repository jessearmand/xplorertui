"""Security boundary for untrusted multimodal image inputs."""

from __future__ import annotations

import asyncio
import base64
import binascii
import ipaddress
import multiprocessing
import os
import socket
from io import BytesIO
from urllib.parse import urlsplit

MAX_IMAGE_BYTES = 10 * 1024 * 1024
MAX_IMAGE_PIXELS = 25_000_000
MAX_IMAGE_DIMENSION = 8192
MAX_IMAGES = 8
ALLOWED_FORMATS = frozenset({"JPEG", "PNG", "WEBP"})
DECODE_TIMEOUT_SECONDS = 10


class UnsafeImage(ValueError):
    """Raised when an image does not satisfy the input security policy."""


def _validate_public_url(value: str) -> tuple[str, int]:
    """Validate URL syntax before passing it to HTTPX/IDNA."""
    if len(value) > 2048 or any(ord(char) < 0x20 for char in value):
        raise UnsafeImage("invalid image URL")
    try:
        parsed = urlsplit(value)
        host = parsed.hostname
        port = parsed.port
    except ValueError as exc:
        raise UnsafeImage("invalid image URL") from exc
    if parsed.scheme != "https" or not host or parsed.username or parsed.password:
        raise UnsafeImage("only unauthenticated HTTPS image URLs are allowed")
    try:
        ascii_host = host.encode("idna").decode("ascii")
    except UnicodeError as exc:
        raise UnsafeImage("invalid image URL hostname") from exc
    if len(ascii_host) > 253 or any(len(label) > 63 for label in ascii_host.split(".")):
        raise UnsafeImage("invalid image URL hostname")
    return ascii_host, port or 443


async def _reject_non_public_addresses(host: str, port: int) -> None:
    try:
        records = await asyncio.get_running_loop().getaddrinfo(
            host, port, type=socket.SOCK_STREAM
        )
    except socket.gaierror as exc:
        raise UnsafeImage("image URL hostname could not be resolved") from exc
    if not records:
        raise UnsafeImage("image URL hostname could not be resolved")
    for record in records:
        address = ipaddress.ip_address(record[4][0])
        if not address.is_global:
            raise UnsafeImage("image URL resolves to a non-public address")


async def _download_image(value: str, client: object) -> bytes:
    host, port = _validate_public_url(value)
    if os.environ.get("MLX_ALLOW_REMOTE_IMAGES", "").lower() not in {"1", "true"}:
        raise UnsafeImage("remote image URLs are disabled")
    await _reject_non_public_addresses(host, port)
    chunks: list[bytes] = []
    size = 0
    async with client.stream(  # type: ignore[attr-defined]
        "GET", value, follow_redirects=False
    ) as response:
        response.raise_for_status()
        content_length = response.headers.get("content-length")
        if content_length and int(content_length) > MAX_IMAGE_BYTES:
            raise UnsafeImage("image exceeds byte limit")
        async for chunk in response.aiter_bytes():
            size += len(chunk)
            if size > MAX_IMAGE_BYTES:
                raise UnsafeImage("image exceeds byte limit")
            chunks.append(chunk)
    return b"".join(chunks)


def _decode_base64(value: str) -> bytes:
    encoded = value
    if value.startswith("data:"):
        header, separator, encoded = value.partition(",")
        if not separator or header.lower() not in {
            "data:image/jpeg;base64",
            "data:image/png;base64",
            "data:image/webp;base64",
        }:
            raise UnsafeImage("unsupported image data URL")
    if len(encoded) > ((MAX_IMAGE_BYTES + 2) // 3) * 4:
        raise UnsafeImage("image exceeds byte limit")
    try:
        result = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as exc:
        raise UnsafeImage("invalid base64 image") from exc
    if len(result) > MAX_IMAGE_BYTES:
        raise UnsafeImage("image exceeds byte limit")
    return result


def _decoder_worker(data: bytes, connection: object) -> None:
    """Parse an image in a disposable, resource-limited child process."""
    try:
        import resource

        resource.setrlimit(resource.RLIMIT_CPU, (5, 5))
        memory = 512 * 1024 * 1024
        resource.setrlimit(resource.RLIMIT_AS, (memory, memory))
    except (ImportError, OSError, ValueError):
        pass

    try:
        from PIL import Image

        Image.MAX_IMAGE_PIXELS = MAX_IMAGE_PIXELS
        with Image.open(BytesIO(data), formats=list(ALLOWED_FORMATS)) as image:
            if (
                image.width > MAX_IMAGE_DIMENSION
                or image.height > MAX_IMAGE_DIMENSION
                or image.width * image.height > MAX_IMAGE_PIXELS
            ):
                raise UnsafeImage("image dimensions exceed limit")
            image.load()
            safe = image.convert("RGB")
            connection.send((True, safe.size, safe.tobytes()))  # type: ignore[attr-defined]
    except Exception as exc:
        connection.send((False, str(exc)))  # type: ignore[attr-defined]
    finally:
        connection.close()  # type: ignore[attr-defined]


def _decode_isolated(data: bytes):
    from PIL import Image

    context = multiprocessing.get_context("spawn")
    parent, child = context.Pipe(duplex=False)
    process = context.Process(target=_decoder_worker, args=(data, child), daemon=True)
    process.start()
    child.close()
    if not parent.poll(DECODE_TIMEOUT_SECONDS):
        process.kill()
        process.join()
        raise UnsafeImage("image decoding timed out")
    try:
        result = parent.recv()
    except EOFError as exc:
        process.join()
        raise UnsafeImage("image decoder terminated unexpectedly") from exc
    process.join(timeout=1)
    if process.is_alive():
        process.kill()
        process.join()
    if not result[0]:
        raise UnsafeImage(f"unsafe or invalid image: {result[1]}")
    _, size, pixels = result
    return Image.frombytes("RGB", size, pixels)


async def decode_image(value: str, client: object):
    if value.startswith(("http://", "https://")):
        data = await _download_image(value, client)
    else:
        data = _decode_base64(value)
    return await asyncio.to_thread(_decode_isolated, data)


async def decode_images(values: list[str], client: object) -> list[object]:
    if len(values) > MAX_IMAGES:
        raise UnsafeImage(f"at most {MAX_IMAGES} images are allowed")
    # Decode sequentially so one request cannot multiply the resource limits.
    return [await decode_image(value, client) for value in values]
