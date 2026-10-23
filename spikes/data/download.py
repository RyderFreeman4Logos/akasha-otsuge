#!/usr/bin/env python3
"""Fetch pinned public corpus shards, resuming partial files and checking SHA-256."""

import concurrent.futures
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

RESERVE = 30 * 1024**3


def verify(path, item):
    if path.stat().st_size != item["size"]:
        raise ValueError(f"size mismatch: {path.name}")
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    if digest != item["sha256"]:
        raise ValueError(f"SHA-256 mismatch: {path.name}")


def fetch(directory, source, index, item):
    prefix = "vectors" if source["repository"].startswith("Qdrant/") else "text"
    target = directory / f"{prefix}-{index:05d}.parquet"
    if target.exists():
        verify(target, item)
        return {"file": target.name, "status": "verified_existing", "sha256": item["sha256"]}
    part = target.with_suffix(".parquet.part")
    if shutil.disk_usage(directory).free < RESERVE + item["size"]:
        raise RuntimeError("download would consume the 30 GiB free-space reserve")
    url = f"https://huggingface.co/datasets/{source['repository']}/resolve/{source['revision']}/{item['path']}"
    started = time.monotonic()
    subprocess.run([
        "curl", "--fail", "--location", "--silent", "--show-error", "--continue-at", "-",
        "--retry", "3", "--retry-delay", "10", "--connect-timeout", "20",
        "--max-time", "14400", "--limit-rate", "20M", "--output", str(part), url,
    ], check=True)
    verify(part, item)
    # A hard link publishes only a complete verified file and never replaces an existing one.
    os.link(part, target)
    part.unlink()
    return {"file": target.name, "status": "download_verified", "sha256": item["sha256"],
            "bytes": item["size"], "seconds": time.monotonic() - started}


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: download.py OWNED_DIRECTORY TEXT_SHARD_COUNT")
    directory = pathlib.Path(sys.argv[1]).resolve(strict=True)
    if not (directory / "OWNER").is_file():
        raise SystemExit("requires a corpus directory with an OWNER marker")
    sources = json.loads(pathlib.Path(__file__).with_name("sources.json").read_text())["sources"]
    count = int(sys.argv[2])
    if not 1 <= count <= len(sources[1]["files"]):
        raise SystemExit("invalid text shard count")
    jobs = [(source, i, item) for source in sources
            for i, item in enumerate(source["files"] if source is sources[0] else source["files"][:count])]
    remaining = sum(item["size"] for source, i, item in jobs
                    if not (directory / f"{'vectors' if source is sources[0] else 'text'}-{i:05d}.parquet").exists())
    if shutil.disk_usage(directory).free < RESERVE + remaining:
        raise SystemExit("insufficient disk for selected shards plus the 30 GiB reserve")
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(fetch, directory, *job) for job in jobs]
        for future in concurrent.futures.as_completed(futures):
            print(json.dumps(future.result()), flush=True)


if __name__ == "__main__":
    main()
