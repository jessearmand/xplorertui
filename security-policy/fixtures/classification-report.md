# Dependabot classification report

Rules: `security-policy/LAWS.bend` (executable: `classify.py`).

An *update* is one version bump of one package release line in one manifest.
A patched fix is MustMerge unless something holds it; severity only sets the order.

## Counts

| Decision | Updates | Alerts |
|---|---:|---:|
| MustMerge | 15 | 72 |
| Held | 3 | 9 |
| Blocked | 0 | 0 |
| **Total** | **18** | **81** |

## MustMerge (patched, nothing in the way) (15)

| Package | Manifest | Locked | Bump to | Max sev | Dep | Held by | Alerts closed | Still unpatched |
|---|---|---|---|---|---|---|---|---|
| `anyio` | `mlx-server/uv.lock` | `4.12.1` | `4.14.2` | critical | transitive | — | #83, #84 | — |
| `openssl` | `Cargo.lock` | `0.10.75` | `0.10.80` | high | transitive | — | #23, #24, #25, #26, #27, #34, #36, #40 | — |
| `quinn-proto` | `Cargo.lock` | `0.11.14` | `0.11.15` | high | transitive | — | #76 | — |
| `rustls-webpki` | `Cargo.lock` | `0.103.9` | `0.103.13` | high | transitive | — | #3, #20, #21, #29 | — |
| `aiohttp` | `mlx-server/uv.lock` | `3.13.3` | `3.14.3` | high | transitive | — | #6, #7, #8, #9, #10, #11, #12, #13, #14, #15, #41, #42, #48, #49, #50, #51, #52, #53, #54, #55, #56, #77, #78, #79 | — |
| `pillow` | `mlx-server/uv.lock` | `12.1.1` | `12.3.0` | high | direct | — | #16, #30, #31, #32, #33, #63, #64, #65, #66, #67, #68, #69, #70, #71, #72, #73, #74, #75 | — |
| `python-multipart` | `mlx-server/uv.lock` | `0.0.22` | `0.0.31` | high | transitive | — | #19, #35, #44, #45, #46, #47 | — |
| `urllib3` | `mlx-server/uv.lock` | `2.6.3` | `2.7.0` | high | transitive | — | #37, #38 | — |
| `idna` | `mlx-server/uv.lock` | `3.11` | `3.15` | medium | direct | — | #39 | — |
| `pydantic-settings` | `mlx-server/uv.lock` | `2.13.1` | `2.14.2` | medium | transitive | — | #61 | — |
| `requests` | `mlx-server/uv.lock` | `2.32.5` | `2.33.0` | medium | transitive | — | #4 | — |
| `rand` | `Cargo.lock` | `0.8.5` | `0.8.6` | low | direct | — | #28 | — |
| `rand` | `Cargo.lock` | `0.9.2` | `0.9.3` | low | direct | — | #22 | — |
| `rand` | `Cargo.lock` | `0.10.0` | `0.10.1` | low | direct | — | #17 | — |
| `pygments` | `mlx-server/uv.lock` | `2.19.2` | `2.20.0` | low | transitive | — | #5 | — |

## Held (patched, but something blocks a plain merge) (3)

| Package | Manifest | Locked | Bump to | Max sev | Dep | Held by | Alerts closed | Still unpatched |
|---|---|---|---|---|---|---|---|---|
| `transformers` | `mlx-server/pyproject.toml` | `5.3.0` | `5.10.0` | high | direct | declared `==5.3.0` excludes 5.10.0 | #80, #82 | — |
| `starlette` | `mlx-server/uv.lock` | `0.52.1` | `1.3.1` | high | transitive | 0.52.1 -> 1.3.1 is a breaking upgrade | #43, #57, #58, #59, #60 | — |
| `transformers` | `mlx-server/uv.lock` | `5.3.0` | `5.10.0` | high | direct | declared `==5.3.0` excludes 5.10.0 | #62, #81 | — |

## Blocked (no patched version exists) (0)

_None._

## Alert detail

| # | Sev | Package | Patched | Decision | Summary |
|---:|---|---|---|---|---|
| 83 | medium | `anyio` | `4.14.2` | MustMerge | AnyIO process-pool workers can block indefinitely on undrained stderr |
| 84 | critical | `anyio` | `4.14.2` | MustMerge | AnyIO: TLSStream IDNA 2003 host name encoding enables potential TLS certificate  |
| 23 | high | `openssl` | `0.10.78` | MustMerge | rust-openssl: rustMdCtxRef::digest_final() writes past caller buffer with no len |
| 24 | high | `openssl` | `0.10.78` | MustMerge | rust-openssl: Unchecked callback length in PSK/cookie trampolines leaks adjacent |
| 25 | high | `openssl` | `0.10.78` | MustMerge | rust-openssl has incorrect bounds assertion in aes key wrap |
| 26 | low | `openssl` | `0.10.78` | MustMerge | rust-opennssl has an Out-of-bounds read in PEM password callback when returning  |
| 27 | high | `openssl` | `0.10.78` | MustMerge | rust-openssl: Deriver::derive and PkeyCtxRef::derive can overflow short buffers  |
| 34 | high | `openssl` | `0.10.79` | MustMerge | rust-openssl has undefined behavior in X509Ref::ocsp_responders for certificates |
| 36 | medium | `openssl` | `0.10.79` | MustMerge | rust-openssl vulnerable to heap buffer overflow when encrypting with AES key-wra |
| 40 | medium | `openssl` | `0.10.80` | MustMerge | rust-openssl: Potential out-of-bounds write in `CipherCtxRef::cipher_update_inpl |
| 76 | high | `quinn-proto` | `0.11.15` | MustMerge | Quinn: Remote memory exhaustion in quinn-proto from unbounded out-of-order strea |
| 3 | medium | `rustls-webpki` | `0.103.10` | MustMerge | webpki: CRLs not considered authoritative by Distribution Point due to faulty ma |
| 20 | low | `rustls-webpki` | `0.103.12` | MustMerge | webpki: Name constraints were accepted for certificates asserting a wildcard nam |
| 21 | low | `rustls-webpki` | `0.103.12` | MustMerge | webpki: Name constraints for URI names were incorrectly accepted |
| 29 | high | `rustls-webpki` | `0.103.13` | MustMerge | rustls-webpki: Denial of service via panic on malformed CRL BIT STRING |
| 6 | medium | `aiohttp` | `3.13.4` | MustMerge | aiohttp allows unlimited trailer headers, leading to possible uncapped memory us |
| 7 | low | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP Affected by Denial of Service (DoS) via Unbounded DNS Cache in TCPConnec |
| 8 | low | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP has CRLF injection through multipart part content type header constructi |
| 9 | medium | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP affected by UNC SSRF/NTLMv2 Credential Theft/Local File Read in static r |
| 10 | medium | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP has a Multipart Header Size Bypass |
| 11 | low | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP has late size enforcement for non-file multipart fields causes memory Do |
| 12 | low | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP leaks Cookie and Proxy-Authorization headers on cross-origin redirect |
| 13 | low | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP has HTTP response splitting via \r in reason phrase |
| 14 | low | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP's C parser (llhttp) accepts null bytes and control characters in respons |
| 15 | medium | `aiohttp` | `3.13.4` | MustMerge | AIOHTTP accepts duplicate Host headers |
| 41 | medium | `aiohttp` | `3.14.0` | MustMerge | AIOHTTP is Vulnerable to Deserialization of Untrusted Data |
| 42 | medium | `aiohttp` | `3.14.0` | MustMerge | AIOHTTP is vulnerable to cross-origin redirect with per-request cookies |
| 48 | low | `aiohttp` | `3.14.1` | MustMerge | aiohttp: Host-Only Cookies Become Domain Cookies After CookieJar Persistence |
| 49 | low | `aiohttp` | `3.14.0` | MustMerge | aiohttp: CRLF injection in multipart headers |
| 50 | medium | `aiohttp` | `3.14.1` | MustMerge | aiohttp: Unread Compressed Request Bodies Bypass client_max_size During Cleanup |
| 51 | low | `aiohttp` | `3.14.1` | MustMerge | aiohttp: Payload Response Resources Are Not Closed After Mid-Body Disconnect |
| 52 | medium | `aiohttp` | `3.14.1` | MustMerge | aiohttp: C HTTP Parser Bypasses max_line_size for Fragmented Lines |
| 53 | medium | `aiohttp` | `3.14.1` | MustMerge | aiohttp: HTTP/1 Pipelined Requests Queue Without Limit |
| 54 | medium | `aiohttp` | `3.14.1` | MustMerge | aiohttp: Incomplete websocket frame payloads bypass memory limits |
| 55 | medium | `aiohttp` | `3.14.1` | MustMerge | aiohttp: DigestAuthMiddleware Applies Credentials to Cross-Origin Redirect Chall |
| 56 | low | `aiohttp` | `3.14.1` | MustMerge | aiohttp: TLS Server Hostname Override Is Ignored When Reusing HTTPS Connections |
| 77 | medium | `aiohttp` | `3.14.2` | MustMerge | AIOHTTP: WebSocket client accepts compressed frames without negotiated permessag |
| 78 | medium | `aiohttp` | `3.14.2` | MustMerge | AIOHTTP: HTTP request smuggling via WebSocket upgrade |
| 79 | high | `aiohttp` | `3.14.3` | MustMerge | AIOHTTP: Out-of-bounds heap read in C HTTP response parser error path (malformed |
| 16 | high | `pillow` | `12.2.0` | MustMerge | FITS GZIP decompression bomb in Pillow |
| 30 | medium | `pillow` | `12.2.0` | MustMerge | Pillow has a heap buffer overflow with nested list coordinates |
| 31 | high | `pillow` | `12.2.0` | MustMerge | Pillow has an OOB Write with Invalid PSD Tile Extents (Integer Overflow) |
| 32 | medium | `pillow` | `12.2.0` | MustMerge | Pillow has an integer overflow when processing fonts |
| 33 | medium | `pillow` | `12.2.0` | MustMerge | Pillow has a PDF Parsing Trailer Infinite Loop (DoS) |
| 63 | medium | `pillow` | `12.3.0` | MustMerge | Pillow EpsImagePlugin negative %%BeginBinary byte count causes infinite loop den |
| 64 | high | `pillow` | `12.3.0` | MustMerge | Pillow: Out-of-bounds read via attacker-controlled row stride on Pillow's mmap p |
| 65 | high | `pillow` | `12.3.0` | MustMerge | Pillow: `FontFile.compile()`: `Image.new()` called without `_decompression_bomb_ |
| 66 | high | `pillow` | `12.3.0` | MustMerge | Pillow `PcfFontFile._load_bitmaps()`: `Image.frombytes()` called without `_decom |
| 67 | high | `pillow` | `12.3.0` | MustMerge | Pillow `BdfFontFile`: `Image.new()` called without `_decompression_bomb_check()` |
| 68 | high | `pillow` | `12.3.0` | MustMerge | Pillow `GdImageFile._open()`: image dimensions accepted without `_decompression_ |
| 69 | medium | `pillow` | `12.3.0` | MustMerge | Pillow: WindowsViewer.get_command() OS command injection via unescaped shell pat |
| 70 | high | `pillow` | `12.3.0` | MustMerge | Pillow: Heap out-of-bounds write `Image.paste()` / `Image.crop()` via signed coo |
| 71 | high | `pillow` | `12.3.0` | MustMerge | Pillow: Heap out-of-bounds write in `ImageFilter.RankFilter` via integer overflo |
| 72 | high | `pillow` | `12.3.0` | MustMerge | Pillow: Controlled heap out-of-bounds write in Pillow `ImageCmsTransform.apply() |
| 73 | high | `pillow` | `12.3.0` | MustMerge | Pillow JPEG2000 tiled decode retains a growing scratch buffer and can be used fo |
| 74 | medium | `pillow` | `12.3.0` | MustMerge | Pillow TGA RLE encoder can serialize up to ~57 KB of adjacent heap data into gen |
| 75 | high | `pillow` | `12.3.0` | MustMerge | Pillow: Decompression Bomb DoS via PdfParser.PdfStream.decode() |
| 19 | medium | `python-multipart` | `0.0.26` | MustMerge | python-multipart affected by Denial of Service via large multipart preamble or e |
| 35 | high | `python-multipart` | `0.0.27` | MustMerge | python-multipart has Denial of Service via unbounded multipart part headers |
| 44 | low | `python-multipart` | `0.0.30` | MustMerge | python-multipart: Content-Disposition parameter smuggling via RFC 2231/5987 exte |
| 45 | low | `python-multipart` | `0.0.30` | MustMerge | python-multipart: Semicolon treated as querystring field separator enables param |
| 46 | low | `python-multipart` | `0.0.31` | MustMerge | python-multipart: Negative Content-Length in parse_form buffers the entire body  |
| 47 | high | `python-multipart` | `0.0.30` | MustMerge | python-multipart: Quadratic-time querystring parsing with semicolon separators c |
| 37 | high | `urllib3` | `2.7.0` | MustMerge | urllib3: Decompression-bomb safeguards bypassed in parts of the streaming API |
| 38 | high | `urllib3` | `2.7.0` | MustMerge | urllib3: Sensitive headers forwarded across origins in proxied low-level redirec |
| 39 | medium | `idna` | `3.15` | MustMerge | Internationalized Domain Names in Applications (IDNA): Specially crafted inputs  |
| 61 | medium | `pydantic-settings` | `2.14.2` | MustMerge | pydantic-settings: NestedSecretsSettingsSource follows symlinks outside secrets_ |
| 4 | medium | `requests` | `2.33.0` | MustMerge | Requests has Insecure Temp File Reuse in its extract_zipped_paths() utility func |
| 28 | low | `rand` | `0.8.6` | MustMerge | Rand is unsound with a custom logger using rand::rng() |
| 22 | low | `rand` | `0.9.3` | MustMerge | Rand is unsound with a custom logger using rand::rng() |
| 17 | low | `rand` | `0.10.1` | MustMerge | Rand is unsound with a custom logger using rand::rng() |
| 5 | low | `pygments` | `2.20.0` | MustMerge | Pygments has Regular Expression Denial of Service (ReDoS) due to Inefficient Reg |
| 80 | high | `transformers` | `5.5.0` | Held | huggingface/transformers: Arbitrary Code Execution During Model Initialization i |
| 82 | high | `transformers` | `5.10.0` | Held | Transformers save_pretrained path traversal allows arbitrary file writes through |
| 43 | medium | `starlette` | `1.0.1` | Held | Starlette has missing Host header validation that poisons request.url.path, bypa |
| 57 | medium | `starlette` | `1.1.0` | Held | Starlette: Arbitrary HTTP method dispatched to `HTTPEndpoint` attributes via `ge |
| 58 | high | `starlette` | `1.1.0` | Held | Starlette: SSRF and NTLM credential theft via UNC paths in StaticFiles on Window |
| 59 | low | `starlette` | `1.3.0` | Held | Starlette: Unvalidated request path concatenated into authority poisons request. |
| 60 | high | `starlette` | `1.3.1` | Held | Starlette: request.form() limits silently ignored for application/x-www-form-url |
| 62 | high | `transformers` | `5.5.0` | Held | huggingface/transformers: Arbitrary Code Execution During Model Initialization i |
| 81 | high | `transformers` | `5.10.0` | Held | Transformers save_pretrained path traversal allows arbitrary file writes through |
