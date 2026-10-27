#!/usr/bin/env python3
"""Exercise the offline capture command through its process/JSON boundary."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def check(binary):
    with tempfile.TemporaryDirectory(prefix="akasha-capture-command-") as directory:
        root = Path(directory)
        data, binding_path, source = (root / name for name in ("data", "binding.json", "events.jsonl"))
        binding = {
            "source": dict(adapter="fixture-v1", profile="profile", session="session", thread="thread", agent="agent"),
            "scope": dict(user="user", project="project", branch_or_version=None, session=None),
            "stream": "live", "coverage": "available_payloads",
        }
        binding_path.write_text(json.dumps(binding))

        def run(code, *args):
            result = subprocess.run([binary, *map(str, args)], stdin=subprocess.DEVNULL,
                                    capture_output=True, timeout=10, check=False)
            assert result.returncode == code, (result.returncode, result.stderr)
            if code in (0, 3):
                assert result.stderr == b"", result.stderr
                assert result.stdout.endswith(b"\n")
                return json.loads(result.stdout)
            assert result.stdout == b"", result.stdout
            return json.loads(result.stderr)

        def reconcile(code, pending):
            report = run(code, data, binding_path, source)
            assert report["checkpoint"]["checkpoint"]["pending"] == pending, report
            return report

        def frame(event_id, text):
            event = dict(event_id=event_id, source_version="1", runtime_instance="runtime",
                         branch="main", parent=None, fork_cutoff=None, event_time_ms=10,
                         observed_time_ms=20, visibility="user",
                         kind=dict(kind="message", message_id=event_id, phase=dict(phase="final")), text=text)
            return (json.dumps(dict(schema_version=1, event=event, raw_hex=b"\x00\xffraw".hex()),
                               ensure_ascii=False) + "\n").encode()

        missing = reconcile(3, "source_missing")
        assert missing["processed"] == 0
        first, second = frame("first", "原文\x00\n"), frame("second", "second")
        source.write_bytes(first + second[:-2])
        partial = reconcile(3, "partial_tail")
        assert partial["processed"] == 1 and partial["deduplicated"] == 0
        with source.open("ab") as handle:
            handle.write(second[-2:])
        complete = reconcile(0, None)
        assert complete["processed"] == 1
        unchanged = reconcile(0, None)
        assert unchanged["processed"] == 0 and unchanged["checkpoint"] == complete["checkpoint"]
        binding["stream"] = "offline-backfill"
        binding_path.write_text(json.dumps(binding))
        backfill = reconcile(0, None)
        assert backfill["processed"] == backfill["deduplicated"] == 2
        with source.open("ab") as handle:
            handle.write(b"invalid\n")
        invalid = reconcile(3, "invalid_frame")
        assert invalid["processed"] == 0
        assert invalid["checkpoint"]["checkpoint"]["cursor"] == backfill["checkpoint"]["checkpoint"]["cursor"]
        source.rename(root / "rotated.jsonl")
        source.write_bytes(first)
        rotated = reconcile(3, None)
        assert rotated["processed"] == rotated["deduplicated"] == 1
        assert rotated["checkpoint"]["checkpoint"]["loss"] == ["rotation"]
        source.write_bytes(b"")
        truncated = reconcile(3, None)
        assert truncated["checkpoint"]["checkpoint"]["loss"] == ["rotation", "truncation"]
        invalid_args = run(2, "private-argument-must-not-be-echoed")
        assert invalid_args == {"schema_version": 1, "error": {"code": "INVALID_ARGUMENT"}}
        fifo = root / "fifo"
        os.mkfifo(fifo)
        assert run(1, data, binding_path, fifo)["error"]["code"] == "INVALID_CAPTURE_FILE"
        assert run(1, data, fifo, source)["error"]["code"] == "INVALID_CAPTURE_BINDING"
    print("capture command: missing/partial/reopen/replay/backfill/invalid/rotation/truncation/FIFO PASS")


if __name__ == "__main__":
    assert len(sys.argv) == 2, "provide the built reconcile executable"
    check(str(Path(sys.argv[1]).resolve(strict=True)))
