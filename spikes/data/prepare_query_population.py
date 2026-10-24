#!/usr/bin/env python3
"""Freeze distributed real queries with a bounded, seekable vector sidecar."""

import argparse
import fcntl
import hashlib
import json
import os
import pathlib
import re
import shutil
import struct
import time

import pyarrow.parquet as parquet

from download import publish, verify
from prepare_corpus import vector_fingerprint

OWNER = "akasha-otsuge MVP-003 owned query preparation\n"
PER_SHARD = 128
CONFIG = '[execution]\nqueries_per_shard = 128\nconfig_epoch = 1\n[reliability]\nmax_attempts = 1\n'


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["input", "manifest", "output"]:
        parser.add_argument(name, type=pathlib.Path)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    inputs, manifest_path, output = args.input.resolve(strict=True), args.manifest.resolve(strict=True), args.output.resolve()
    if manifest_path.stat().st_size > 1024**2:
        raise ValueError("manifest exceeds 1 MiB")
    manifest = json.loads(manifest_path.read_text())
    sources_path = pathlib.Path(__file__).with_name("sources.json")
    if manifest["code_sha256"]["sources.json"] != digest(sources_path):
        raise ValueError("source inventory changed")
    sources = json.loads(sources_path.read_text())["sources"][0]["files"][:manifest["source_counts"][0]]
    if not 1 <= len(sources) <= 26 or manifest["dimensions"] != 1536:
        raise ValueError("unexpected source population")
    identity = {"manifest_sha256":digest(manifest_path), "source_sha256":digest(sources_path),
                "script_sha256":digest(pathlib.Path(__file__)), "preparer_sha256":digest(pathlib.Path(__file__).with_name("prepare_corpus.py")),
                "input":str(inputs), "queries_per_shard":PER_SHARD, "config_epoch":1, "max_attempts":1,
                "config_sha256":hashlib.sha256(CONFIG.encode()).hexdigest()}
    if args.resume:
        if (output / "OWNER").read_text() != OWNER:
            raise ValueError("foreign output directory")
    else:
        output.mkdir(mode=0o700)
        (output / "OWNER").write_text(OWNER)
        (output / "run.toml").write_text(CONFIG)
    owner = (output / "lock").open("a")
    fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
    state_path = output / "state.json"
    state = json.loads(state_path.read_text()) if args.resume else {"identity":identity,"completed":{}}
    if state["identity"] != identity or digest(output / 'run.toml') != identity['config_sha256']:
        raise ValueError("query preparation identity changed")
    if state.get('phase') == 'completed':
        if digest(output / 'queries.json') != state['result']['queries_sha256'] or digest(output / 'queries.f32') != state['result']['vectors_sha256']:
            raise ValueError('completed query population changed')
        print(json.dumps(state['result']))
        return
    stat = pathlib.Path('/proc/self/stat').read_text().rsplit(')', 1)[1].split()
    started = time.monotonic()
    previously_completed = len(state['completed'])
    state.update(pid=os.getpid(), start_ticks=stat[19], phase="preparing")
    publish(state_path, state)
    all_queries, ordinal_base = [], 0
    for index, item in enumerate(sources):
        key = str(index)
        unit_json, unit_vector = output / f"unit-{index}.json", output / f"unit-{index}.f32"
        if key in state["completed"]:
            old = state["completed"][key]
            if digest(unit_json) != old["json_sha256"] or digest(unit_vector) != old["vectors_sha256"]:
                raise ValueError("completed preparation unit changed")
            unit = json.loads(unit_json.read_text())
        else:
            elapsed = time.monotonic()-started
            rate = (index-previously_completed)/elapsed
            publish(output / "progress.json", {"phase":"verify_and_select", "completed":index,"total":len(sources),
                "elapsed_s":elapsed,"rate_per_s":rate,"eta_s":(len(sources)-index)/rate if rate else None,"active_unit":index,
                "pid":os.getpid(),"start_ticks":stat[19],"config_epoch":1})
            source = inputs / f"vectors-{index:05d}.parquet"
            verify(source, item)
            table = parquet.ParquetFile(source)
            selected, vectors, ordinal = [], bytearray(), 0
            for batch in table.iter_batches(batch_size=64, columns=["_id","title","text","text-embedding-3-large-1536-embedding"]):
                for row in batch.to_pylist():
                    body = row["title"] + "\n" + row["text"]
                    terms = sorted(set(re.findall(r"\b[a-z]{8,24}\b", body.lower())), key=lambda term:(-len(term),term))
                    if terms:
                        memory_key = "dbpedia:" + row["_id"]
                        vector = row["text-embedding-3-large-1536-embedding"]
                        vector_fingerprint(vector)
                        selected.append({"key":memory_key, "partition":"p"+str(hashlib.sha256(memory_key.encode()).digest()[0]%8),
                            "term":terms[0], "memory":1_000_000+ordinal_base+ordinal, "source_row":ordinal,
                            "body_sha256":hashlib.sha256(body.encode()).hexdigest()})
                        vectors.extend(struct.pack('<1536f', *vector))
                    ordinal += 1
                    if len(selected) == PER_SHARD:
                        break
                if len(selected) == PER_SHARD:
                    break
            if len(selected) != PER_SHARD:
                raise ValueError("source lacks enough eligible queries")
            unit = {"source":item,"rows":table.metadata.num_rows,"queries":selected}
            temporary = unit_vector.with_suffix('.tmp')
            with temporary.open('wb') as stream:
                stream.write(vectors);stream.flush();os.fsync(stream.fileno())
            os.replace(temporary, unit_vector)
            publish(unit_json, unit)
            state["completed"][key] = {"json_sha256":digest(unit_json),"vectors_sha256":digest(unit_vector)}
            publish(state_path, state)
        for query in unit["queries"]:
            query["vector_offset"] = len(all_queries)*1536*4
            all_queries.append(query)
        ordinal_base += unit["rows"]
    if ordinal_base != manifest["corpus"]["vectors"]:
        raise ValueError("query sources do not match corpus vector population")
    vector_path = output / "queries.f32"
    temporary = output / "queries.f32.tmp"
    with temporary.open('wb') as target:
        for index in range(len(sources)):
            with (output / f'unit-{index}.f32').open('rb') as source:
                shutil.copyfileobj(source, target)
        target.flush();os.fsync(target.fileno())
    os.replace(temporary, vector_path)
    result = {"source_sha256":identity["manifest_sha256"],"corpus_manifest_sha256":identity["manifest_sha256"],
              "model":"text-embedding-3-large","dimension":1536,"queries":all_queries,
              "vector_file":{"name":"queries.f32","sha256":digest(vector_path),"bytes":vector_path.stat().st_size},
              "selection":"first 128 eligible rows of each pinned vector shard; one longest ASCII term per body",
              "not_proven":["BM25 expected rank","ANN recall ground truth","file-cache-cold"]}
    publish(output / "queries.json", result)
    state.update(phase="completed", result={"queries":len(all_queries),"queries_sha256":digest(output / "queries.json"),"vectors_sha256":digest(vector_path)})
    publish(state_path, state)
    elapsed = time.monotonic()-started
    publish(output / "progress.json", {"phase":"completed","completed":len(sources),"total":len(sources),"elapsed_s":elapsed,
        "rate_per_s":len(sources)/elapsed,"eta_s":0,"pid":os.getpid(),"start_ticks":stat[19],"config_epoch":1})
    print(json.dumps(state["result"]))


if __name__ == "__main__":
    main()
