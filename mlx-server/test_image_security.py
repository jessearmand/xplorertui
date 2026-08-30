from io import BytesIO

import pytest

from image_security import (
    UnsafeImage,
    _decode_base64,
    _decode_isolated,
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
