"""Exact-input benchmark embeddings from retained, hash-pinned Parquet files."""

import hashlib
import http.server
import json
import pathlib

import pyarrow.parquet as parquet

from download import verify
from prepare_corpus import vector_fingerprint

VECTOR = "text-embedding-3-large-1536-embedding"


def stamp(path):
    value = path.stat()
    return value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns


def body(row, layout):
    return row["title"] + "\n" + row["text"] if layout == "dbpedia" else row["word"]


class Lookup:
    """Keep text hashes and row addresses; retain at most one decoded row group."""

    def __init__(self, manifest, progress=None):
        if (manifest.get("dimensions") != 1536
                or not isinstance(manifest.get("model"), str)
                or not 1 <= len(manifest["model"]) <= 128
                or not isinstance(manifest.get("sources"), list)
                or not 1 <= len(manifest["sources"]) <= 27):
            raise ValueError("invalid precomputed manifest")
        self.model = manifest["model"]
        self.files, self.index = [], {}
        self.cached = None
        self.excluded = self.requests = self.returned = self.failures = 0
        self.witness = hashlib.sha256()
        try:
            for number, source in enumerate(manifest["sources"]):
                if source.get("layout") not in ("dbpedia", "dictionary"):
                    raise ValueError("unknown precomputed source layout")
                path = pathlib.Path(source["path"]).resolve(strict=True)
                before = stamp(path)
                verify(path, source)
                # Retained readers must not each keep a second column prefetch cache.
                reader = parquet.ParquetFile(path, pre_buffer=False)
                self.files.append((path, before, reader, source["layout"]))
                columns = ["title", "text"] if source["layout"] == "dbpedia" else ["word"]
                for group in range(reader.num_row_groups):
                    if reader.metadata.row_group(group).total_byte_size > 64 * 1024**2:
                        raise ValueError("precomputed row group exceeds the 64 MiB decode bound")
                    offset = 0
                    for batch in reader.iter_batches(row_groups=[group], columns=columns, batch_size=1024):
                        for row in batch.to_pylist():
                            text = body(row, source["layout"])
                            encoded = text.encode()
                            if not text.strip():
                                raise ValueError("empty precomputed input")
                            if len(encoded) <= 4096:
                                key = hashlib.sha256(encoded).digest()
                                if key in self.index:
                                    raise ValueError("duplicate precomputed input")
                                self.index[key] = (number, group, offset)
                            else:
                                self.excluded += 1
                            offset += 1
                if stamp(path) != before:
                    raise ValueError("precomputed source changed during admission")
                if progress is not None:
                    progress(number + 1, len(manifest["sources"]), self.snapshot())
        except BaseException:
            self.close()
            raise

    def vector(self, text):
        key = hashlib.sha256(text.encode()).digest()
        address = self.index.get(key)
        if address is None:
            raise ValueError("input has no retained embedding")
        number, group, offset = address
        path, original, reader, layout = self.files[number]
        if stamp(path) != original:
            raise ValueError("precomputed source changed after admission")
        if self.cached is None or self.cached[:2] != (number, group):
            columns = ["title", "text", VECTOR] if layout == "dbpedia" else ["word", "embedding"]
            self.cached = (number, group, reader.read_row_group(group, columns=columns))
        row = self.cached[2].slice(offset, 1).to_pylist()[0]
        if body(row, layout) != text:
            raise ValueError("precomputed input address mismatch")
        vector = row[VECTOR] if layout == "dbpedia" else row["embedding"]
        vector_fingerprint(vector)
        if stamp(path) != original:
            raise ValueError("precomputed source changed during lookup")
        return vector

    def respond(self, request):
        self.requests += 1
        try:
            if (not isinstance(request, dict)
                    or set(request) != {"model", "input", "encoding_format"}
                    or request["model"] != self.model
                    or request["encoding_format"] != "float"
                    or not isinstance(request["input"], list)
                    or not 1 <= len(request["input"]) <= 16
                    or not all(isinstance(t, str) and t.strip() and len(t.encode()) <= 4096
                               for t in request["input"])):
                raise ValueError("invalid precomputed request")
            vectors = [self.vector(text) for text in request["input"]]
            for text, vector in zip(request["input"], vectors):
                self.witness.update(hashlib.sha256(text.encode()).digest())
                self.witness.update(vector_fingerprint(vector))
            self.returned += len(vectors)
            return {"model": self.model, "data": [
                {"index": index, "embedding": vector} for index, vector in enumerate(vectors)]}
        except (ValueError, OSError):
            self.failures += 1
            raise

    def snapshot(self):
        return {"model": self.model, "dimensions": 1536, "eligible_inputs": len(self.index),
                "oversized_inputs": self.excluded, "requests": self.requests,
                "returned_vectors": self.returned, "failed_requests": self.failures,
                "input_vector_witness_sha256": self.witness.hexdigest(),
                "live_model_inference": False}

    def close(self):
        self.cached = None
        for _, _, reader, _ in self.files:
            reader.close()
        self.files.clear()
        self.index.clear()


def server(lookup, port=0):
    """One bounded local request at a time; the controller owns stop and receipts."""
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            self.connection.settimeout(30)
            try:
                lengths = self.headers.get_all("Content-Length", [])
                if (self.path != "/v1/embeddings" or len(lengths) != 1
                        or self.headers.get("Transfer-Encoding") is not None
                        or not lengths[0].isascii() or not lengths[0].isdigit()
                        or not 0 < int(lengths[0]) <= 512 * 1024):
                    raise ValueError("invalid precomputed HTTP frame")
                raw = self.rfile.read(int(lengths[0]))
                if len(raw) != int(lengths[0]):
                    raise ValueError("incomplete precomputed HTTP frame")
                response = lookup.respond(json.loads(raw))
                code = 200
            except (ValueError, OSError):
                response = {"error": {"code": "PRECOMPUTED_INPUT_UNAVAILABLE"}}
                code = 400
            encoded = json.dumps(response, separators=(",", ":"), allow_nan=False).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, *_):
            pass

    return http.server.HTTPServer(("127.0.0.1", port), Handler)
