#!/usr/bin/env python3
"""Kill a real import after a durable batch; reconcile its prefix before resume."""

import hashlib
import json
import os
import pathlib
import selectors
import subprocess
import sys
import tempfile
import time


def invoke(binary, database, *args, success=True):
    result = subprocess.run([str(binary), str(database), *map(str, args)], capture_output=True, text=True, timeout=60)
    assert (result.returncode == 0) == success, result.stderr
    return result


def count(binary, database):
    return json.loads(invoke(binary, database, "MATCH (d:Document) RETURN count(d)").stdout)["rows"][0][0]


def main():
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    resume_binary = pathlib.Path(sys.argv[2]).resolve(strict=True) if len(sys.argv) == 3 else binary
    with tempfile.TemporaryDirectory(prefix="akasha-import-contract-") as temporary:
        root = pathlib.Path(temporary)
        database, source, manifest = root / "database", root / "source.jsonl", root / "manifest.json"
        rows = [{"key": f"doc:{i}", "body": f"fixture {i} " + "x" * 4096, "partition": f"p{i % 8}"} for i in range(1057)]
        data = b"".join((json.dumps(row, separators=(",", ":")) + "\n").encode() for row in rows)
        source.write_bytes(data)
        identity = {"corpus": {"documents": 1057, "vectors": 0, "prepared_bytes": len(data),
                              "prepared_sha256": hashlib.sha256(data).hexdigest(),
                              "indexed_body_utf8_bytes": sum(len(row["body"].encode()) for row in rows)}}
        manifest.write_text(json.dumps(identity))
        child = subprocess.Popen([str(binary), str(database), "--load", str(source), str(manifest)],
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                 env=dict(os.environ, AKASHA_SPIKE_IMPORT_HOLD_AFTER="32"))
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(child.stderr, selectors.EVENT_READ)
                deadline = time.monotonic() + 60
                while time.monotonic() < deadline:
                    assert child.poll() is None, "import exited before durable checkpoint: " + child.stderr.read()
                    if selector.select(timeout=1):
                        if child.stderr.readline().strip() == "IMPORT_CHECKPOINT:32":
                            break
                else:
                    raise TimeoutError("durable import checkpoint never arrived")
            child.kill()
            child.communicate(timeout=10)
            assert child.returncode == -9
        finally:
            if child.poll() is None:
                child.kill()
            child.communicate(timeout=10)
        assert count(binary, database) == 32
        initial_binary = binary
        binary = resume_binary
        assert count(binary, database) == 32, "replacement binary cannot read the durable prefix"
        source.write_bytes(data.replace(b"fixture 0", b"changed 0", 1))
        rejected = invoke(binary, database, "--resume-load", source, manifest, success=False)
        assert "committed prefix" in rejected.stderr, rejected.stderr
        assert count(binary, database) == 32, "changed prefix must not add records"
        source.write_bytes(data)
        invoke(binary, database, "--resume-load", source, manifest)
        assert count(binary, database) == 1057
        invoke(binary, database, "--resume-load", source, manifest)
        assert count(binary, database) == 1057, "repeated resume duplicated documents"
        stored = json.loads(invoke(binary, database, "MATCH (s:ImportState) RETURN s.documents,s.complete").stdout)["rows"]
        assert stored == [[1057, True]], stored
        progress = json.loads((database / "import-progress.json").read_text())
        assert progress["phase"] == "import_complete" and progress["completed"] == progress["total"] == 1057
        assert progress["eta_s"] == 0
        identity["corpus"]["documents"] = 1058
        manifest.write_text(json.dumps(identity))
        rejected = invoke(binary, database, "--resume-load", source, manifest, success=False)
        assert "manifest" in rejected.stderr
        assert count(binary, database) == 1057
        legacy = root / "legacy"
        invoke(binary, legacy, "--load", source)
        assert count(binary, legacy) == 1057, "unpinned diagnostic import changed"
        print(json.dumps({"result": "passed", "documents": 1057, "killed_after_durable_documents": 32,
                          "initial_binary_sha256": hashlib.sha256(initial_binary.read_bytes()).hexdigest(),
                          "resume_binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                          "checks": ["process_crash", "changed_prefix_rejected", "no_duplicate_resume", "manifest_change_rejected"],
                          "not_tested": ["50GB", "disk_full", "100_clients"]}))


if __name__ == "__main__":
    main()
