#!/usr/bin/env python3
"""Freeze or reproduce a deduplicated public corpus without a second JSONL copy."""

import collections
import hashlib
import json
import math
import os
import pathlib
import struct
import sys

import pyarrow
import pyarrow.parquet as parquet

from download import verify

TARGET_BYTES = 50_000_000_000
CHUNK_BYTES = 64 * 1024


def chunks(text):
    encoded = text.encode("utf-8")
    start = 0
    while start < len(encoded):
        end = min(start + CHUNK_BYTES, len(encoded))
        while end < len(encoded) and encoded[end] & 0xC0 == 0x80:
            end -= 1
        yield encoded[start:end].decode("utf-8")
        start = end


def vector_fingerprint(vector):
    if len(vector) != 1536 or not all(map(math.isfinite, vector)) or not any(vector):
        raise ValueError("invalid real embedding")
    try:
        encoded = struct.pack("<1536f", *vector)
    except (OverflowError, struct.error) as error:
        raise ValueError("embedding does not fit float32") from error
    if not any(struct.unpack("<1536f", encoded)):
        raise ValueError("embedding underflows to zero")
    return hashlib.sha256(encoded).digest()


def records(directory, counts, statistics):
    inventory = json.loads(pathlib.Path(__file__).with_name("sources.json").read_text())["sources"]
    seen_content, seen_vectors, seen_keys = set(), set(), set()
    partitions = {f"p{i}": collections.Counter() for i in range(8)}
    statistics.update(documents=0, source_rows=0, unique_content_utf8_bytes=0,
                      indexed_body_utf8_bytes=0, vectors=0, unique_vectors=0,
                      duplicate_content_utf8_bytes=0, unvectorized_unique_utf8_bytes=0,
                      added_separator_bytes=0, inputs=[])
    for group, count in enumerate(counts):
        source = inventory[group]
        if not 0 <= count <= len(source["files"]):
            raise ValueError("invalid source shard count")
        for index, item in enumerate(source["files"][:count]):
            path = directory / f"{'vectors' if group == 0 else 'text'}-{index:05d}.parquet"
            verify(path, item)
            statistics["inputs"].append({"file": path.name, "source": item,
                "repository": source["repository"], "revision": source["revision"],
                "license": source["license"]})
            columns = ["_id", "title", "text", "text-embedding-3-large-1536-embedding"] if group == 0 else ["text"]
            for batch in parquet.ParquetFile(path).iter_batches(batch_size=64, columns=columns):
                for row in batch.to_pylist():
                    statistics["source_rows"] += 1
                    vector = row["text-embedding-3-large-1536-embedding"] if group == 0 else None
                    if vector is not None:
                        seen_vectors.add(vector_fingerprint(vector))
                        body = row["title"] + "\n" + row["text"]
                        if len(body.encode()) > 128 * 1024:
                            raise ValueError("vector document exceeds importer bound")
                        parts = [body]
                    else:
                        parts = chunks(row["text"])
                    for body in parts:
                        encoded = body.encode("utf-8")
                        digest = hashlib.sha256(encoded).digest()
                        content_bytes = len(encoded) - (1 if vector is not None else 0)
                        duplicate = digest in seen_content
                        if duplicate:
                            statistics["duplicate_content_utf8_bytes"] += content_bytes
                            if vector is None:
                                continue
                        key = "dbpedia:" + row["_id"] if vector is not None else "fineweb:" + digest.hex()
                        if vector is not None:
                            if key in seen_keys:
                                raise ValueError("duplicate vector source identity")
                            seen_keys.add(key)
                        partition = "p" + str(hashlib.sha256(key.encode()).digest()[0] % 8)
                        record = {"key": key, "body": body, "partition": partition}
                        if vector is not None:
                            record["embedding"] = vector
                            statistics["vectors"] += 1
                            statistics["added_separator_bytes"] += 1
                            partitions[partition]["vectors"] += 1
                        elif not duplicate:
                            statistics["unvectorized_unique_utf8_bytes"] += content_bytes
                        statistics["documents"] += 1
                        statistics["indexed_body_utf8_bytes"] += len(encoded)
                        partitions[partition]["documents"] += 1
                        if not duplicate:
                            seen_content.add(digest)
                            statistics["unique_content_utf8_bytes"] += content_bytes
                            partitions[partition]["unique_content_utf8_bytes"] += content_bytes
                        yield record
                        if group == 1 and statistics["unique_content_utf8_bytes"] >= TARGET_BYTES:
                            statistics["unique_vectors"] = len(seen_vectors)
                            statistics["partitions"] = partitions
                            return
            print(json.dumps({"prepared": path.name, "documents": statistics["documents"],
                              "unique_content_utf8_bytes": statistics["unique_content_utf8_bytes"]}), file=sys.stderr, flush=True)
    statistics["unique_vectors"] = len(seen_vectors)
    statistics["partitions"] = partitions


def prepare(directory, counts, output=None):
    statistics = {}
    digest = hashlib.sha256()
    prepared_bytes = 0
    for record in records(directory, counts, statistics):
        line = (json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8")
        digest.update(line)
        prepared_bytes += len(line)
        if output is not None:
            output.write(line)
    statistics.update(prepared_sha256=digest.hexdigest(), prepared_bytes=prepared_bytes)
    return statistics


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if (mode == "scan" and len(sys.argv) != 6) or (mode == "stream" and len(sys.argv) != 4) or mode not in ("scan", "stream"):
        raise SystemExit("usage: prepare_corpus.py scan DIR VECTOR_SHARDS TEXT_SHARDS NEW_MANIFEST | stream DIR MANIFEST")
    directory = pathlib.Path(sys.argv[2]).resolve(strict=True)
    code = {name: hashlib.sha256(pathlib.Path(__file__).with_name(name).read_bytes()).hexdigest()
            for name in ("prepare_corpus.py", "download.py", "sources.json")}
    environment = {"python": sys.version.split()[0], "pyarrow": pyarrow.__version__}
    if mode == "scan":
        counts = list(map(int, sys.argv[3:5]))
        output = pathlib.Path(sys.argv[5])
        if output.exists():
            raise FileExistsError(output)
        statistics = prepare(directory, counts)
        full = statistics["unique_content_utf8_bytes"] >= TARGET_BYTES and statistics["unique_vectors"] >= 1_000_000
        manifest = {"status": "full_corpus_prepared" if full else "diagnostic_corpus_prepared",
                    "seed": 0, "embedding_model": "text-embedding-3-large", "dimensions": 1536,
                    "precomputed_at": "2024-02 (publisher)", "chunk_bytes": CHUNK_BYTES,
                    "hot_partitions": ["p0", "p1"], "cold_partitions": [f"p{i}" for i in range(2, 8)],
                    "source_counts": counts, "code_sha256": code, "environment": environment,
                    "corpus": statistics, "indexed": False, "acceptance": "not_run"}
        partial = output.with_suffix(output.suffix + ".part")
        with partial.open("x", encoding="utf-8") as stream:
            json.dump(manifest, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.link(partial, output)
        partial.unlink()
        descriptor = os.open(output.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        print(json.dumps({"manifest": str(output), "status": manifest["status"],
                          "unique_content_utf8_bytes": statistics["unique_content_utf8_bytes"],
                          "unique_vectors": statistics["unique_vectors"]}))
    else:
        manifest = json.loads(pathlib.Path(sys.argv[3]).read_text())
        if manifest["code_sha256"] != code or manifest["environment"] != environment:
            raise ValueError("preparation code or environment differs from the frozen manifest")
        statistics = prepare(directory, manifest["source_counts"], sys.stdout.buffer)
        sys.stdout.buffer.flush()
        if statistics != manifest["corpus"]:
            raise ValueError("reproduced stream does not match the frozen manifest; reject this import")
        print(json.dumps({"stream_verified": statistics["prepared_sha256"]}), file=sys.stderr)


if __name__ == "__main__":
    main()
