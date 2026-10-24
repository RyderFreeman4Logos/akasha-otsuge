#!/usr/bin/env python3
"""Measure one engine command inside its dedicated cgroup; never touch other jobs."""

import hashlib
import json
import pathlib
import resource
import shutil
import subprocess
import sys
import tempfile
import time


def exited(pid):
    try:
        return pathlib.Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] in {"Z", "X", "x"}
    except (FileNotFoundError, ProcessLookupError):
        return True


def read_process_file(pid, name):
    path = pathlib.Path(f"/proc/{pid}/{name}")
    try:
        return path.read_text()
    except PermissionError:
        if exited(pid):
            raise ProcessLookupError(pid) from None
        # procfs access can briefly fail during exec/exit credential transitions.
        time.sleep(0.01)
        try:
            return path.read_text()
        except PermissionError:
            if exited(pid):
                raise ProcessLookupError(pid) from None
            raise


def memory(pid):
    values = {}
    try:
        for line in read_process_file(pid, "smaps_rollup").splitlines():
            key, _, value = line.partition(":")
            if key in ("Rss", "Pss"):
                values[key] = int(value.split()[0]) * 1024
    except (FileNotFoundError, ProcessLookupError):
        return {"Rss": 0, "Pss": 0}
    except PermissionError:
        if not exited(pid):
            raise
        return {"Rss": 0, "Pss": 0}
    return values


def process_io(pid):
    try:
        return {key: int(value) for key, value in
                (line.split(":", 1) for line in read_process_file(pid, "io").splitlines())}
    except (FileNotFoundError, ProcessLookupError):
        return None
    except PermissionError:
        if not exited(pid):
            raise
        return None


def clean_scratch(scratch, cgroup, observer):
    if not scratch.exists():
        return dict(path=str(scratch), removed=True)
    live = [int(pid) for pid in (cgroup / "cgroup.procs").read_text().split()
            if int(pid) != observer and not exited(int(pid))]
    if live:
        return dict(path=str(scratch), removed=False, live_pids=live)
    shutil.rmtree(scratch)
    return dict(path=str(scratch), removed=True)


def main():
    if len(sys.argv) < 3:
        raise SystemExit("usage: measure.py NEW_JSONL COMMAND [ARG ...]")
    import os

    cgroup = pathlib.Path("/sys/fs/cgroup" + pathlib.Path("/proc/self/cgroup").read_text().strip().split("::")[1])
    # Running this in a shared login scope would count unrelated processes.
    members = {int(pid) for pid in (cgroup / "cgroup.procs").read_text().split()}
    if members != {os.getpid()}:
        raise SystemExit("requires a fresh dedicated service cgroup containing only this observer")
    limits = {name: (cgroup / name).read_text().strip() for name in ("memory.high", "memory.max", "memory.swap.max")}
    if limits["memory.max"] == "max" or limits["memory.swap.max"] != "0":
        raise SystemExit("requires a memory hard limit and disabled cgroup swap")
    started = time.monotonic()
    with open(sys.argv[1], "x", encoding="utf-8") as samples:
        scratch = pathlib.Path(tempfile.mkdtemp(prefix="native-scratch-", dir=pathlib.Path(sys.argv[1]).resolve().parent))
        child = None
        try:
            env = dict(os.environ, TMPDIR=str(scratch))
            samples.write(json.dumps({"event": "start", "limits": limits,
                                      "nofile_soft_hard": resource.getrlimit(resource.RLIMIT_NOFILE),
                                      "native_environment": {name: env.get(name) for name in (
                                          "TMPDIR", "MALLOC_MMAP_THRESHOLD_", "GLIBC_TUNABLES",
                                          "LANCE_CPU_THREADS", "LANCE_DEFAULT_IO_BUFFER_SIZE",
                                          "LANCE_SHUFFLE_BATCH_BYTES",
                                          "LANCE_SHUFFLE_MAX_PRELOADED_OFFSETS_BYTES")},
                                      "command": sys.argv[2:]}) + "\n")
            child = subprocess.Popen(sys.argv[2:], env=env)
            with open(f"/proc/{child.pid}/exe", "rb") as executable:
                binary_hash = hashlib.file_digest(executable, "sha256").hexdigest()
            start_ticks = pathlib.Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()[19]
            samples.write(json.dumps({"event": "executable", "sha256": binary_hash,
                                      "pid": child.pid, "start_ticks": start_ticks,
                                      "cgroup": str(cgroup)}) + "\n")
            while True:
                pids = {int(pid) for pid in (cgroup / "cgroup.procs").read_text().split()}
                stats = [memory(pid) for pid in pids if pid != os.getpid()]
                try:
                    io_stat = {parts[0]: dict(field.split("=", 1) for field in parts[1:])
                               for parts in (line.split() for line in (cgroup / "io.stat").read_text().splitlines())}
                except FileNotFoundError:
                    io_stat = None
                sample = {
                    "event": "sample", "seconds": time.monotonic() - started,
                    "engine_rss_bytes": sum(item["Rss"] for item in stats),
                    "engine_pss_bytes": sum(item["Pss"] for item in stats),
                    "observer_rss_bytes": memory(os.getpid())["Rss"],
                    "cgroup_current_bytes": int((cgroup / "memory.current").read_text()),
                    "cgroup_peak_bytes": int((cgroup / "memory.peak").read_text()),
                    "memory_events": dict(line.split() for line in (cgroup / "memory.events").read_text().splitlines()),
                    "host_load_average": os.getloadavg(),
                    "host_pressure": {name: path.read_text() if path.exists() else None
                                      for name in ("cpu", "memory", "io")
                                      for path in [pathlib.Path("/proc/pressure") / name]},
                    "io_stat": io_stat,
                    "engine_process_io": {str(pid): process_io(pid) for pid in pids if pid != os.getpid()},
                    "io_coverage": "currently sampled processes; cgroup controller unavailable" if io_stat is None else "cgroup and sampled processes",
                }
                samples.write(json.dumps(sample) + "\n")
                samples.flush()
                result = child.poll()
                if result is not None:
                    cleanup = clean_scratch(scratch, cgroup, os.getpid())
                    code = result if cleanup['removed'] else 1
                    samples.write(json.dumps({"event": "exit", "code": code, "native_code": result,
                                              "scratch_cleanup": cleanup, "seconds": time.monotonic() - started}) + "\n")
                    raise SystemExit(code if code >= 0 else 128 - code)
                time.sleep(1)
        finally:
            if child is not None and child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
            clean_scratch(scratch, cgroup, os.getpid())


if __name__ == "__main__":
    main()
