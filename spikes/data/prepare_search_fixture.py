#!/usr/bin/env python3
"""Select eight reproducible real-document search oracles from the first shard."""

import collections
import hashlib
import json
import pathlib
import re
import sys

import pyarrow.parquet as parquet


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: prepare_search_fixture.py VERIFIED_FIRST_PARQUET NEW_JSON")
    source, output = map(pathlib.Path, sys.argv[1:])
    expected = json.loads((pathlib.Path(__file__).with_name("sources.json")).read_text())["sources"][0]["files"][0]
    with source.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    if digest != expected["sha256"] or source.stat().st_size != expected["size"]:
        raise ValueError("source shard checksum mismatch")
    rows = parquet.read_table(source, columns=["_id", "title", "text"]).to_pylist()
    terms = [set(re.findall(r"\b[a-z]{8,24}\b", (row["title"] + " " + row["text"]).lower())) for row in rows]
    frequencies = collections.Counter(term for words in terms for term in words)
    selected = {}
    for ordinal, (row, words) in enumerate(zip(rows, terms)):
        key = "dbpedia:" + row["_id"]
        partition = "p" + str(hashlib.sha256(key.encode()).digest()[0] % 8)
        unique = sorted(term for term in words if frequencies[term] == 1)
        if partition not in selected and unique:
            selected[partition] = {"ordinal": ordinal, "key": key, "partition": partition, "term": unique[0]}
    if len(selected) != 8:
        raise ValueError("source does not cover all eight search partitions")
    ordinals = {value["ordinal"]: value for value in selected.values()}
    ordinal = 0
    for batch in parquet.ParquetFile(source).iter_batches(batch_size=64, columns=["text-embedding-3-large-1536-embedding"]):
        for row in batch.to_pylist():
            if ordinal in ordinals:
                ordinals[ordinal]["embedding"] = row["text-embedding-3-large-1536-embedding"]
            ordinal += 1
        if all("embedding" in value for value in selected.values()):
            break
    fixture = {"source_sha256": digest, "model": "text-embedding-3-large", "dimension": 1536,
               "queries": [selected[key] for key in sorted(selected)]}
    with output.open("x", encoding="utf-8") as stream:
        json.dump(fixture, stream, ensure_ascii=False, separators=(",", ":"))
        stream.write("\n")
    print(json.dumps({"queries": len(selected), "source_sha256": digest,
                      "fixture_sha256": hashlib.sha256(output.read_bytes()).hexdigest()}))


if __name__ == "__main__":
    main()
