#!/usr/bin/env python3
"""Small real-engine crash probe. This is not the 50 GB acceptance harness."""

import hashlib
import json
import pathlib
import selectors
import subprocess
import sys
import tempfile
import time


def run(binary, directory, query, *, failure=False):
    result = subprocess.run(
        [binary, directory, query], capture_output=True, text=True, timeout=60
    )
    if failure:
        assert result.returncode != 0, "invalid transaction unexpectedly succeeded"
        return {"exit": result.returncode, "stderr": result.stderr}
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)["rows"]


def kill_at(binary, directory, query, phase):
    child = subprocess.Popen(
        [binary, directory, query, phase],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(child.stderr, selectors.EVENT_READ)
            end = time.monotonic() + 60
            while time.monotonic() < end:
                assert child.poll() is None, "probe exited before its checkpoint"
                if selector.select(timeout=min(1, max(0, end - time.monotonic()))):
                    line = child.stderr.readline()
                    if line.strip() == "PROBE_CHECKPOINT:" + phase:
                        break
            else:
                raise TimeoutError("probe never reached " + phase)
        child.kill()
        stdout, stderr = child.communicate(timeout=10)
        assert child.returncode == -9, child.returncode
        if phase == "after-reply":
            assert json.loads(stdout)["rows"] == [[7]]
        else:
            assert not stdout, "reply escaped before the requested checkpoint"
        return {"phase": phase, "exit": child.returncode, "stderr": stderr}
    finally:
        if child.poll() is None:
            child.kill()
        child.communicate(timeout=10)


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: check_crash.py ABSOLUTE_PROBE_BINARY")
    binary = str(pathlib.Path(sys.argv[1]).resolve(strict=True))
    cgroup = pathlib.Path("/sys/fs/cgroup" + pathlib.Path("/proc/self/cgroup").read_text().strip().split("::")[1])
    limits = {name: (cgroup / name).read_text().strip() for name in ("memory.high", "memory.max", "memory.swap.max")}
    records = []
    transaction = (
        "CREATE (:Revision {key:7, content:'source body'}), "
        "(:Head {key:7, rev:7}), (:Receipt {request:'seven', seq:7}), "
        "(:Job {key:7, generation:1}), (:Counter {seq:7}) RETURN 7"
    )
    for phase in ("before-execute", "before-reply", "after-reply"):
        with tempfile.TemporaryDirectory(prefix="akasha-helix-crash-") as directory:
            records.append(kill_at(binary, directory, transaction, phase))
            expected = 0 if phase == "before-execute" else 1
            for label in ("Revision", "Head", "Receipt", "Job", "Counter"):
                rows = run(binary, directory, f"MATCH (n:{label}) RETURN count(n)")
                assert rows == [[expected]], (phase, label, rows)
            if expected:
                assert run(binary, directory, "MATCH (r:Revision) RETURN r.key,r.content") == [
                    [7, "source body"]
                ]
                assert run(binary, directory, "MATCH (r:Receipt) RETURN r.request,r.seq") == [
                    ["seven", 7]
                ]
    with tempfile.TemporaryDirectory(prefix="akasha-helix-rollback-") as directory:
        error = run(
            binary,
            directory,
            "CREATE (:Revision {key:8}), (:Receipt {invalid:[1,null]})",
            failure=True,
        )
        for label in ("Revision", "Receipt"):
            assert run(binary, directory, f"MATCH (n:{label}) RETURN count(n)") == [[0]]
        records.append({"phase": "runtime-error-rollback", **error})
    with open(binary, "rb") as artifact:
        binary_hash = hashlib.file_digest(artifact, "sha256").hexdigest()
    print(json.dumps({
        "result": "passed",
        "kind": "small_process_crash_probe",
        "binary_sha256": binary_hash,
        "cgroup_limits": limits,
        "cgroup_after": {name: (cgroup / name).read_text().strip() for name in ("memory.current", "memory.peak", "memory.events")},
        "records": records,
        "not_tested": ["power_loss", "50GB", "100_clients", "indexes", "historical_pagination"],
    }, indent=2))


if __name__ == "__main__":
    main()
