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
import tomllib

RESERVE = 30 * 1024**3
TRANSIENT_CURL = {5, 6, 7, 16, 18, 28, 52, 55, 56, 92, 95}


def publish(path, value):
    partial = path.with_suffix(path.suffix + ".tmp")
    with partial.open("w", encoding="utf-8") as stream:
        json.dump(value, stream, sort_keys=True)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(partial, path)
    descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def retryable(code, http_status):
    return code in TRANSIENT_CURL or (code == 22 and (http_status == 429 or 500 <= http_status <= 599))


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
    policy_bytes = pathlib.Path(__file__).with_name("download.toml").read_bytes()
    policy = tomllib.loads(policy_bytes.decode())["reliability"]
    if not (1 <= policy["max_attempts"] <= 10 and 0 < policy["base_backoff_s"] <= policy["max_backoff_s"] <= 60
            and 60 <= policy["request_timeout_s"] <= 86400):
        raise ValueError("invalid download reliability policy")
    receipt_path = target.with_suffix(".download.json")
    if receipt_path.exists():
        publish(target.with_suffix(f".download-{time.time_ns()}.json"), json.loads(receipt_path.read_text()))
    receipt = {"file": target.name, "source_sha256": item["sha256"], "source_url": url,
               "config_epoch": hashlib.sha256(policy_bytes).hexdigest(), "policy": policy,
               "pid": os.getpid(), "attempts": [], "phase": "admitted"}
    publish(receipt_path, receipt)
    for attempt in range(1, policy["max_attempts"] + 1):
        if part.exists() and part.stat().st_size == item["size"]:
            break
        event = {"attempt": attempt, "phase": "running", "resumed_bytes": part.stat().st_size if part.exists() else 0}
        receipt["attempts"].append(event)
        publish(receipt_path, receipt)
        result = subprocess.run([
            "curl", "--fail", "--location", "--silent", "--show-error", "--continue-at", "-",
            "--connect-timeout", "20", "--max-time", str(policy["request_timeout_s"]),
            "--limit-rate", "20M", "--write-out", "%{http_code}", "--output", str(part), url,
        ], capture_output=True, text=True)
        status = int(result.stdout) if result.stdout.isdigit() else 0
        event.update(phase="finished", exit_code=result.returncode, http_status=status, error=result.stderr[-4096:])
        publish(receipt_path, receipt)
        if result.returncode == 0:
            break
        if attempt == policy["max_attempts"] or not retryable(result.returncode, status):
            receipt["phase"] = "failed"
            publish(receipt_path, receipt)
            raise RuntimeError(f"download failed: {target.name}, curl={result.returncode}, HTTP={status}; see {receipt_path.name}")
        time.sleep(min(policy["max_backoff_s"], policy["base_backoff_s"] * 2 ** (attempt - 1)))
    try:
        verify(part, item)
    except ValueError as error:
        receipt.update(phase="verification_failed", error=str(error))
        publish(receipt_path, receipt)
        raise
    # A hard link publishes only a complete verified file and never replaces an existing one.
    os.link(part, target)
    part.unlink()
    receipt["phase"] = "verified"
    publish(receipt_path, receipt)
    return {"file": target.name, "status": "download_verified", "sha256": item["sha256"],
            "bytes": item["size"], "seconds": time.monotonic() - started}


def main():
    if len(sys.argv) not in (3, 4) or (len(sys.argv) == 4 and sys.argv[3] != "--resume"):
        raise SystemExit("usage: download.py OWNED_DIRECTORY TEXT_SHARD_COUNT [--resume]")
    directory = pathlib.Path(sys.argv[1]).resolve(strict=True)
    if not (directory / "OWNER").is_file():
        raise SystemExit("requires a corpus directory with an OWNER marker")
    sources = json.loads(pathlib.Path(__file__).with_name("sources.json").read_text())["sources"]
    count = int(sys.argv[2])
    if not 1 <= count <= len(sources[1]["files"]):
        raise SystemExit("invalid text shard count")
    jobs = [(source, i, item) for source in sources
            for i, item in enumerate(source["files"] if source is sources[0] else source["files"][:count])]
    state_path = directory / "download-state.json"
    identity = hashlib.sha256(json.dumps(jobs, sort_keys=True).encode()).hexdigest()
    state = {"identity": identity, "total": len(jobs), "completed": [], "failed": [], "pid": os.getpid(),
             "start_ticks": pathlib.Path("/proc/self/stat").read_text().rsplit(")", 1)[1].split()[19],
             "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    if state_path.exists():
        previous = json.loads(state_path.read_text())
        if len(sys.argv) != 4 or previous["identity"] != identity:
            raise SystemExit("existing download run requires --resume and identical pinned source set")
        state["previous_run"] = previous["started_utc"]
        publish(directory / ("download-state-" + previous["started_utc"].replace(":", "") + ".json"), previous)
    publish(state_path, state)
    remaining = sum(item["size"] for source, i, item in jobs
                    if not (directory / f"{'vectors' if source is sources[0] else 'text'}-{i:05d}.parquet").exists())
    if shutil.disk_usage(directory).free < RESERVE + remaining:
        raise SystemExit("insufficient disk for selected shards plus the 30 GiB reserve")
    started = time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(fetch, directory, *job) for job in jobs]
        for future in concurrent.futures.as_completed(futures):
            try:
                result = future.result()
                state["completed"].append(result)
            except Exception as error:
                result = {"error": str(error)}
                state["failed"].append(result)
            elapsed = time.monotonic() - started
            state["elapsed_s"] = elapsed
            publish(state_path, state)
            completed = len(state["completed"])
            progress = {"completed": completed, "total": len(jobs), "elapsed_s": elapsed,
                        "rate_per_s": completed / elapsed, "eta_s": None, "failed": len(state["failed"]),
                        "pid": state["pid"], "start_ticks": state["start_ticks"]}
            publish(directory / "download-progress.json", progress)
            print(json.dumps(result | {"progress": progress}), flush=True)
    if state["failed"]:
        raise SystemExit("one or more shards failed; verified files and partial GETs retained for explicit resume")


if __name__ == "__main__":
    main()
