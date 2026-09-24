"""mlx-server HTTP surface under a given starlette: every endpoint the Rust MlxClient can hit,
plus validation (422), load-failure (500), image-rejection (400) and 404 paths. MLX is stubbed
out: this checks the web layer only. Diff the output of two venvs
(timestamps and object addresses are normalized).
"""
import sys, types, json, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[3] / "mlx-server"))
mlx = types.ModuleType("mlx"); core = types.ModuleType("mlx.core"); mlx.core = core
sys.modules["mlx"] = mlx; sys.modules["mlx.core"] = core
import base64, io, re


def main():
    import starlette, server
    from PIL import Image
    from fastapi.testclient import TestClient
    buf = io.BytesIO(); Image.new("RGB", (8, 8), "red").save(buf, "PNG")
    png = base64.b64encode(buf.getvalue()).decode()
    print("starlette", starlette.__version__)
    server.registry.default_model = None
    def boom(*a, **k): raise RuntimeError("stub: no mlx")
    server.registry.get_text_model = server.registry.get_chat_model = server.registry.get_vl_model = boom
    with TestClient(server.app) as c:   # runs lifespan startup/shutdown
        for method, path, body in [
            ("get","/health",None), ("get","/v1/models",None),
            ("post","/v1/embeddings",{"input":["hi"],"model":"x"}),
            ("post","/v1/embeddings",{"bogus":1}),
            ("post","/v1/chat/completions",{"model":"x","messages":[{"role":"user","content":"hi"}]}),
            ("post","/v1/chat/completions",{}),
            ("post","/v1/embeddings/multimodal",{"model":"x","images":["aGVsbG8="]}),
            ("post","/v1/embeddings/multimodal",{}),
            ("post","/v1/embeddings/multimodal",{"model":"x","texts":["a"],"images":["aGVsbG8="]}),
            ("post","/v1/embeddings/multimodal",{"model":"x","texts":["a"],"images":[png]}),
            ("post","/v1/embeddings/multimodal",{"model":"x","texts":["a"]}),
            ("get","/nope",None),
        ]:
            r = getattr(c, method)(path, json=body) if body is not None else getattr(c, method)(path)
            j = r.json()
            if "timestamp" in j: j["timestamp"] = 0
            # PIL error text embeds object addresses, which differ run to run.
            text = re.sub(r"0x[0-9a-f]+", "0x…", json.dumps(j, sort_keys=True, ensure_ascii=False))
            print(r.status_code, path, r.headers.get("content-type"), text[:160])


# image_security decodes in a spawn-context child, which re-imports __main__.
if __name__ == "__main__":
    main()
