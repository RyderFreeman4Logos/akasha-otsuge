#!/usr/bin/env python3
"""Reproduce a pinned corpus into a resumable import without a second JSONL copy."""

import argparse
import fcntl
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import uuid

from run_case import digest, process_identity, publish

OWNER = "akasha-otsuge MVP-003 owned corpus import and database\n"


def stop(unit, child):
    if child.poll() is None:
        subprocess.run(["systemctl", "--user", "stop", unit], check=False, timeout=60)
        child.wait(timeout=60)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=pathlib.Path)
    parser.add_argument("manifest", type=pathlib.Path)
    parser.add_argument("run", type=pathlib.Path)
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("python", type=pathlib.Path)
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    inputs, manifest_path, run = args.input.resolve(strict=True), args.manifest.resolve(strict=True), args.run.resolve()
    # Preserve the venv invocation path; resolving its symlink discards PyArrow.
    data_python = args.python.absolute()
    if not data_python.is_file():
        raise ValueError("missing corpus Python interpreter")
    if manifest_path.stat().st_size > 1024**2:
        raise ValueError("corpus manifest exceeds 1 MiB")
    manifest = json.loads(manifest_path.read_text())
    repo = pathlib.Path(__file__).resolve().parents[2]
    prepare = repo / "spikes/data/prepare_corpus.py"
    observer = repo / "spikes/measure.py"
    if manifest["code_sha256"] != {name: digest(prepare.with_name(name)) for name in
                                   ("download.py", "prepare_corpus.py", "sources.json")}:
        raise ValueError("corpus preparer differs from frozen manifest")
    # Planning envelopes are reservations, not measured index sizes or acceptance.
    plan = {"authority_bytes": 75_000_000_000, "current_index_bytes": 45_000_000_000,
            "next_index_bytes": 45_000_000_000, "scratch_bytes": 4_000_000_000,
            "free_reserve_bytes": 30 * 1024**3}
    reserved_during_import = sum(plan.values()) - plan["authority_bytes"]
    identity = {"input": str(inputs), "manifest_sha256": digest(manifest_path),
                "preparer_sha256": digest(prepare), "observer_sha256": digest(observer),
                "controller_sha256": digest(__file__), "data_python": str(data_python),
                "profile": "high", "producer_memory_max": 8 * 1024**3,
                "engine_memory_max": 2 * 1024**3, "runtime_max_seconds": 43200,
                "disk_plan": plan}
    state_path = run / "state.json"
    if args.resume:
        if (run / "OWNER").read_text() != OWNER or not state_path.is_file():
            raise ValueError("resume requires an owned import run and its state")
        owner_lock = (run / "controller.lock").open("a")
        fcntl.flock(owner_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        state = json.loads(state_path.read_text())
        if state["identity"] != identity or digest(run / "probe") != state["binary_sha256"]:
            raise ValueError("import identity changed")
        owner = process_identity(state["controller"]["pid"])
        if owner and owner["state"] != "Z" and owner["start_ticks"] == state["controller"]["start_ticks"]:
            raise ValueError("previous import controller is live")
        for attempt in state["attempts"]:
            for unit in attempt["units"]:
                active = subprocess.run(["systemctl", "--user", "show", unit, "-p", "ActiveState", "--value"], check=True, capture_output=True, text=True).stdout.strip()
                if active not in {"inactive", "failed"}:
                    raise ValueError("previous import unit is still live")
        if state["phase"] == "completed":
            for name, expected in state["result"]["artifact_sha256"].items():
                if digest(run / name) != expected:
                    raise ValueError("completed import artifact changed")
            print(json.dumps(state["result"]))
            return 0
        if not (run / "database").exists():
            raise ValueError("no import database to resume; inspect the failed admission")
    else:
        if shutil.disk_usage(run.parent).free < sum(plan.values()):
            raise ValueError("insufficient disk for authority, two index generations, scratch and reserve")
        run.mkdir(mode=0o700)
        owner_lock = (run / "controller.lock").open("a")
        fcntl.flock(owner_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        (run / "OWNER").write_text(OWNER)
        shutil.copyfile(args.binary.resolve(strict=True), run / "probe")
        (run / "probe").chmod(0o700)
        with (run / "probe").open("rb") as executable:
            os.fsync(executable.fileno())
        state = {"identity": identity, "binary_sha256": digest(run / "probe"), "attempts": []}
    attempt_id = uuid.uuid4().hex
    attempt_dir = run / attempt_id
    attempt_dir.mkdir()
    units = ["akasha-stream-" + attempt_id, "akasha-import-" + attempt_id]
    attempt = {"id": attempt_id, "units": units, "phase": "admitted", "started_unix_s": time.time()}
    state.update(controller=process_identity(os.getpid()), phase="running")
    state["attempts"].append(attempt)
    publish(state_path, state)
    common = ["systemd-run", "--user", "--quiet", "--wait", "--pipe", "--collect", "-p", "MemorySwapMax=0", "-p", "RuntimeMaxSec=43200"]
    producer_command = common + ["--unit=" + units[0], "-p", "MemoryMax=8589934592", str(data_python), str(prepare), "stream", str(inputs), str(manifest_path)]
    consumer_command = common + ["--unit=" + units[1], "-p", "MemoryMax=2147483648", "-p", "Environment=AKASHA_SPIKE_PROFILE=high",
                                 sys.executable, str(observer), str(attempt_dir / "memory.jsonl"), str(run / "probe"),
                                 str(run / "database"), "--resume-load" if args.resume else "--load", "/dev/stdin", str(manifest_path)]
    attempt.update(producer_command=producer_command, consumer_command=consumer_command)
    publish(state_path, state)
    started = time.monotonic()
    failure = None
    producer = consumer = None
    with (attempt_dir / "producer.log").open("x") as producer_log, (attempt_dir / "consumer.log").open("x") as consumer_log:
        try:
            producer = subprocess.Popen(producer_command, stdout=subprocess.PIPE, stderr=producer_log)
            consumer = subprocess.Popen(consumer_command, stdin=producer.stdout, stdout=consumer_log, stderr=subprocess.STDOUT)
            producer.stdout.close()
            while True:
                free = shutil.disk_usage(run).free
                if free < reserved_during_import:
                    raise RuntimeError("disk reservation reached; stop before consuming next-generation or safety reserve")
                progress_path = run / "database/import-progress.json"
                progress = json.loads(progress_path.read_text()) if progress_path.exists() else {"completed": 0, "total": manifest["corpus"]["documents"]}
                progress.update(controller=process_identity(os.getpid()), elapsed_controller_s=time.monotonic() - started,
                                free_bytes=free, reserved_bytes=reserved_during_import, active_attempt=attempt_id)
                publish(run / "progress.json", progress)
                codes = [producer.poll(), consumer.poll()]
                if any(code is not None and code != 0 for code in codes):
                    raise RuntimeError(f"stream/import process failed: {codes}")
                if all(code == 0 for code in codes):
                    break
                time.sleep(2)
        except Exception as error:
            failure = str(error)
        finally:
            for unit, child in zip(units, [producer, consumer]):
                if child is not None:
                    try:
                        stop(unit, child)
                    except (OSError, subprocess.TimeoutExpired) as error:
                        failure = f"{failure or 'cleanup failed'}; {unit}: {error}"
    attempt.update(producer_exit=producer.returncode if producer else None, consumer_exit=consumer.returncode if consumer else None,
                   elapsed_seconds=time.monotonic() - started)
    if failure is None:
        try:
            events = [json.loads(line) for line in (attempt_dir / "memory.jsonl").read_text().splitlines()]
            executed = [row for row in events if row["event"] == "executable"]
            terminal = [row for row in events if row["event"] == "exit"]
            verified = any(json.loads(line).get("stream_verified") == manifest["corpus"]["prepared_sha256"]
                           for line in (attempt_dir / "producer.log").read_text().splitlines() if line.startswith("{"))
            progress = json.loads((run / "database/import-progress.json").read_text())
            if len(executed) != 1 or executed[0]["sha256"] != state["binary_sha256"] or len(terminal) != 1 or terminal[0]["code"] != 0 or not verified or progress["phase"] != "import_complete":
                failure = "terminal executable, stream or import receipt mismatch"
        except (OSError, ValueError, KeyError) as error:
            failure = str(error)
    attempt["phase"] = "completed" if failure is None else "failed"
    attempt["failure"] = failure
    state.update(phase=attempt["phase"], result={"status": "passed" if failure is None else "failed", "failure": failure,
                 "documents": manifest["corpus"]["documents"], "manifest_sha256": identity["manifest_sha256"],
                 "binary_sha256": state["binary_sha256"], "attempt": attempt_id,
                 "elapsed_seconds": attempt["elapsed_seconds"], "indexes": "not_built", "acceptance": "not_run"})
    state["result"]["artifact_sha256"] = {str(path.relative_to(run)): digest(path) for path in
        [attempt_dir / "producer.log", attempt_dir / "consumer.log", attempt_dir / "memory.jsonl", run / "database/import-progress.json"] if path.exists()}
    publish(state_path, state)
    print(json.dumps(state["result"]))
    return int(failure is not None)


if __name__ == "__main__":
    raise SystemExit(main())
