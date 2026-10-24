#!/usr/bin/env python3
"""Exercise native build checkpoints, reuse and pre-open deadline validation."""

import json
import pathlib
import subprocess
import sys
import tempfile


def main():
    binary, fixture = map(lambda value: pathlib.Path(value).resolve(strict=True), sys.argv[1:])
    queries = json.loads(fixture.read_text())["queries"]
    with tempfile.TemporaryDirectory(prefix="akasha-index-progress-") as directory:
        directory = pathlib.Path(directory)
        root, source = directory / "database", directory / "input.jsonl"
        source.write_text("".join(json.dumps({"key": row["key"], "partition": row["partition"],
            "body": row["term"], "embedding": row["embedding"]}) + "\n" for row in queries))
        def run(*args):
            result = subprocess.run([str(binary), str(root), *args], capture_output=True, text=True, timeout=180)
            if result.returncode:
                raise RuntimeError(result.stderr)
            return json.loads(result.stdout)
        run("--load", str(source))
        result = run("--build-indexes", "120")
        for family in ["text", "vector"]:
            progress = json.loads((root / (family + "-build-progress.json")).read_text())
            assert progress["status"]["operation"]["status"] == "succeeded"
            assert progress["observation_deadline_s"] == 120
            assert progress["completed"] >= len(queries) and progress["total"] is None
        reused = run("--build-indexes", "120")
        assert all(reused[family]["index"]["kind"] == "already_active" for family in ["text", "vector"])
        for invalid in ["0", "604801", "not-a-duration"]:
            absent = directory / "must-not-open"
            bad = subprocess.run([str(binary), str(absent), "--build-indexes", invalid], capture_output=True, timeout=30)
            assert bad.returncode != 0 and not absent.exists()
        print(json.dumps({"status":"passed", "checks":["native text/vector completion checkpoints", "already-active reuse", "invalid deadline before database open"], "input_vectors":len(queries), "build":result}))


if __name__ == "__main__":
    main()
