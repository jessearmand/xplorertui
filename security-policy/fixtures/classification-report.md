# Dependabot classification report

Rules: `security-policy/LAWS.bend` (executable: `classify.py`).

## Counts

| Decision | Count |
|---|---:|
| MustMerge | 31 |
| Review | 29 |
| Defer | 21 |
| Blocked | 0 |
| **Total** | **81** |

## MustMerge (fix and merge)

| # | Sev | Package | Manifest | Patched | Summary |
|---:|---|---|---|---|---|
| 84 | critical | `anyio` | `mlx-server/uv.lock` | `4.14.2` | AnyIO: TLSStream IDNA 2003 host name encoding enables potential TLS certificate  |
| 16 | high | `pillow` | `mlx-server/uv.lock` | `12.2.0` | FITS GZIP decompression bomb in Pillow |
| 23 | high | `openssl` | `Cargo.lock` | `0.10.78` | rust-openssl: rustMdCtxRef::digest_final() writes past caller buffer with no len |
| 24 | high | `openssl` | `Cargo.lock` | `0.10.78` | rust-openssl: Unchecked callback length in PSK/cookie trampolines leaks adjacent |
| 25 | high | `openssl` | `Cargo.lock` | `0.10.78` | rust-openssl has incorrect bounds assertion in aes key wrap |
| 27 | high | `openssl` | `Cargo.lock` | `0.10.78` | rust-openssl: Deriver::derive and PkeyCtxRef::derive can overflow short buffers  |
| 29 | high | `rustls-webpki` | `Cargo.lock` | `0.103.13` | rustls-webpki: Denial of service via panic on malformed CRL BIT STRING |
| 31 | high | `pillow` | `mlx-server/uv.lock` | `12.2.0` | Pillow has an OOB Write with Invalid PSD Tile Extents (Integer Overflow) |
| 34 | high | `openssl` | `Cargo.lock` | `0.10.79` | rust-openssl has undefined behavior in X509Ref::ocsp_responders for certificates |
| 35 | high | `python-multipart` | `mlx-server/uv.lock` | `0.0.27` | python-multipart has Denial of Service via unbounded multipart part headers |
| 37 | high | `urllib3` | `mlx-server/uv.lock` | `2.7.0` | urllib3: Decompression-bomb safeguards bypassed in parts of the streaming API |
| 38 | high | `urllib3` | `mlx-server/uv.lock` | `2.7.0` | urllib3: Sensitive headers forwarded across origins in proxied low-level redirec |
| 47 | high | `python-multipart` | `mlx-server/uv.lock` | `0.0.30` | python-multipart: Quadratic-time querystring parsing with semicolon separators c |
| 58 | high | `starlette` | `mlx-server/uv.lock` | `1.1.0` | Starlette: SSRF and NTLM credential theft via UNC paths in StaticFiles on Window |
| 60 | high | `starlette` | `mlx-server/uv.lock` | `1.3.1` | Starlette: request.form() limits silently ignored for application/x-www-form-url |
| 62 | high | `transformers` | `mlx-server/uv.lock` | `5.5.0` | huggingface/transformers: Arbitrary Code Execution During Model Initialization i |
| 64 | high | `pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow: Out-of-bounds read via attacker-controlled row stride on Pillow's mmap p |
| 65 | high | `pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow: `FontFile.compile()`: `Image.new()` called without `_decompression_bomb_ |
| 66 | high | `pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow `PcfFontFile._load_bitmaps()`: `Image.frombytes()` called without `_decom |
| 67 | high | `pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow `BdfFontFile`: `Image.new()` called without `_decompression_bomb_check()` |
| 68 | high | `pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow `GdImageFile._open()`: image dimensions accepted without `_decompression_ |
| 70 | high | `Pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow: Heap out-of-bounds write `Image.paste()` / `Image.crop()` via signed coo |
| 71 | high | `Pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow: Heap out-of-bounds write in `ImageFilter.RankFilter` via integer overflo |
| 72 | high | `pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow: Controlled heap out-of-bounds write in Pillow `ImageCmsTransform.apply() |
| 73 | high | `pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow JPEG2000 tiled decode retains a growing scratch buffer and can be used fo |
| 75 | high | `Pillow` | `mlx-server/uv.lock` | `12.3.0` | Pillow: Decompression Bomb DoS via PdfParser.PdfStream.decode() |
| 76 | high | `quinn-proto` | `Cargo.lock` | `0.11.15` | Quinn: Remote memory exhaustion in quinn-proto from unbounded out-of-order strea |
| 79 | high | `aiohttp` | `mlx-server/uv.lock` | `3.14.3` | AIOHTTP: Out-of-bounds heap read in C HTTP response parser error path (malformed |
| 80 | high | `transformers` | `mlx-server/pyproject.toml` | `5.5.0` | huggingface/transformers: Arbitrary Code Execution During Model Initialization i |
| 81 | high | `transformers` | `mlx-server/uv.lock` | `5.10.0` | Transformers save_pretrained path traversal allows arbitrary file writes through |
| 82 | high | `transformers` | `mlx-server/pyproject.toml` | `5.10.0` | Transformers save_pretrained path traversal allows arbitrary file writes through |

## Review (medium in lockfiles) (29)

| # | Sev | Package | Manifest | Patched |
|---:|---|---|---|---|
| 83 | medium | `anyio` | `mlx-server/uv.lock` | `4.14.2` |
| 78 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.2` |
| 77 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.2` |
| 74 | medium | `Pillow` | `mlx-server/uv.lock` | `12.3.0` |
| 69 | medium | `Pillow` | `mlx-server/uv.lock` | `12.3.0` |
| 63 | medium | `pillow` | `mlx-server/uv.lock` | `12.3.0` |
| 61 | medium | `pydantic-settings` | `mlx-server/uv.lock` | `2.14.2` |
| 57 | medium | `starlette` | `mlx-server/uv.lock` | `1.1.0` |
| 55 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.1` |
| 54 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.1` |
| 53 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.1` |
| 52 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.1` |
| 50 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.1` |
| 43 | medium | `starlette` | `mlx-server/uv.lock` | `1.0.1` |
| 42 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.0` |
| 41 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.14.0` |
| 40 | medium | `openssl` | `Cargo.lock` | `0.10.80` |
| 39 | medium | `idna` | `mlx-server/uv.lock` | `3.15` |
| 36 | medium | `openssl` | `Cargo.lock` | `0.10.79` |
| 33 | medium | `pillow` | `mlx-server/uv.lock` | `12.2.0` |
| 32 | medium | `pillow` | `mlx-server/uv.lock` | `12.2.0` |
| 30 | medium | `pillow` | `mlx-server/uv.lock` | `12.2.0` |
| 19 | medium | `python-multipart` | `mlx-server/uv.lock` | `0.0.26` |
| 15 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 10 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 9 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 6 | medium | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 4 | medium | `requests` | `mlx-server/uv.lock` | `2.33.0` |
| 3 | medium | `rustls-webpki` | `Cargo.lock` | `0.103.10` |

## Defer (low) (21)

| # | Sev | Package | Manifest | Patched |
|---:|---|---|---|---|
| 59 | low | `Starlette` | `mlx-server/uv.lock` | `1.3.0` |
| 56 | low | `aiohttp` | `mlx-server/uv.lock` | `3.14.1` |
| 51 | low | `aiohttp` | `mlx-server/uv.lock` | `3.14.1` |
| 49 | low | `aiohttp` | `mlx-server/uv.lock` | `3.14.0` |
| 48 | low | `aiohttp` | `mlx-server/uv.lock` | `3.14.1` |
| 46 | low | `python-multipart` | `mlx-server/uv.lock` | `0.0.31` |
| 45 | low | `python-multipart` | `mlx-server/uv.lock` | `0.0.30` |
| 44 | low | `python-multipart` | `mlx-server/uv.lock` | `0.0.30` |
| 28 | low | `rand` | `Cargo.lock` | `0.8.6` |
| 26 | low | `openssl` | `Cargo.lock` | `0.10.78` |
| 22 | low | `rand` | `Cargo.lock` | `0.9.3` |
| 21 | low | `rustls-webpki` | `Cargo.lock` | `0.103.12` |
| 20 | low | `rustls-webpki` | `Cargo.lock` | `0.103.12` |
| 17 | low | `rand` | `Cargo.lock` | `0.10.1` |
| 14 | low | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 13 | low | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 12 | low | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 11 | low | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 8 | low | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 7 | low | `aiohttp` | `mlx-server/uv.lock` | `3.13.4` |
| 5 | low | `Pygments` | `mlx-server/uv.lock` | `2.20.0` |

## Blocked (no patch) (0)

_None._
