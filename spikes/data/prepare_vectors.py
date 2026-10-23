#!/usr/bin/env python3
"""Prepare one checksum-pinned public vector shard for a diagnostic import."""

import collections
import hashlib
import json
import math
import os
import pathlib
import struct
import sys

import pyarrow.parquet as parquet


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: prepare_vectors.py VERIFIED_PARQUET NEW_JSONL")
    source, output = map(pathlib.Path, sys.argv[1:])
    inventory = json.loads(pathlib.Path(__file__).with_name("sources.json").read_text())["sources"][0]
    with source.open("rb") as stream:
        source_hash = hashlib.file_digest(stream, "sha256").hexdigest()
    expected = next((item for item in inventory["files"] if item["sha256"] == source_hash), None)
    if expected is None or source.stat().st_size != expected["size"]:
        raise ValueError("input is not a size/checksum-verified pinned vector shard")
    manifest_path = output.with_suffix(".manifest.json")
    if output.exists() or manifest_path.exists():
        raise FileExistsError("prepared output or manifest already exists")
    partial = output.with_suffix(output.suffix + ".part")
    partitions = collections.Counter()
    seen_content, seen_vectors = set(), set()
    count = content_bytes = unique_content_bytes = 0
    with partial.open("x", encoding="utf-8") as target:
        for batch in parquet.ParquetFile(source).iter_batches(batch_size=64):
            for row in batch.to_pylist():
                vector = row["text-embedding-3-large-1536-embedding"]
                if len(vector) != 1536 or not all(map(math.isfinite, vector)) or not any(vector):
                    raise ValueError("invalid 1536-dimensional vector")
                seen_vectors.add(hashlib.sha256(struct.pack("<1536f", *vector)).digest())
                body = row["title"] + "\n" + row["text"]
                encoded = body.encode("utf-8")
                if len(encoded) > 128 * 1024:
                    raise ValueError("source document exceeds diagnostic importer limit")
                digest = hashlib.sha256(encoded).digest()
                if digest not in seen_content:
                    unique_content_bytes += len(encoded)
                    seen_content.add(digest)
                key = "dbpedia:" + row["_id"]
                partition = "p" + str(hashlib.sha256(key.encode()).digest()[0] % 8)
                record = {"key": key, "body": body, "partition": partition, "embedding": vector}
                target.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
                partitions[partition] += 1
                count += 1
                content_bytes += len(encoded)
    with partial.open("rb") as stream:
        output_hash = hashlib.file_digest(stream, "sha256").hexdigest()
    manifest = {
        "status": "diagnostic_shard_not_full_acceptance",
        "repository": inventory["repository"], "revision": inventory["revision"],
        "source": expected, "license": inventory["license"],
        "embedding_model": "text-embedding-3-large", "dimension": 1536,
        "precomputed_at": "2024-02 (publisher)",
        "documents": count, "content_utf8_bytes": content_bytes,
        "unique_content_utf8_bytes_with_title_separator": unique_content_bytes,
        "unique_f32_vectors": len(seen_vectors), "partitions": dict(sorted(partitions.items())),
        "prepared_sha256": output_hash, "prepared_bytes": partial.stat().st_size,
        "lexical_coverage": "not_indexed", "vector_coverage": "not_indexed",
        "oracle": "not_prepared", "full_50GB_acceptance": "not_run",
    }
    os.link(partial, output)
    partial.unlink()
    with manifest_path.open("x", encoding="utf-8") as stream:
        json.dump(manifest, stream, indent=2)
        stream.write("\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
