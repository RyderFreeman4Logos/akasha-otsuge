"""Verify the captured bytes of a completed interval against its closed native store."""

import argparse
import hashlib
import json
import os
import subprocess
import tempfile
import tomllib
from pathlib import Path

from spikes.helix.run_case import digest

from .journal import records
from .operations_by_role import body
from .schedule import clients, offers


def original_oracles(run, admission, result):
    plan = admission["plan"]
    duration = admission["config"]["execution"]["seconds"]
    run_id = plan["run_id"]
    expected = {
        f"{run_id}/client/{c['client']}/request/{n}": (c["client"], c["kind"], n)
        for c in clients()
        for n, _ in offers(c, duration)
    }
    originals = {}
    offered = set()
    accepted = set()
    conflicts = 0
    for row in records(run / "requests.jsonl"):
        event = row["event"]
        if event == "interval":
            if (
                row["run_id"] != run_id
                or row["clients"] != clients()
                or row["duration_s"] != duration
            ):
                raise ValueError("interval identity differs from admission")
        elif event in ("offered", "outcome"):
            key = row["identity"]
            if expected.get(key) != (row["client"], row["kind"], row["ordinal"]):
                raise ValueError("unexpected request identity")
            if event == "offered":
                if key in offered:
                    raise ValueError("duplicate offered identity")
                offered.add(key)
            elif row["kind"] == "original_write" and row["outcome"]["status"] == "success":
                if key in accepted or row["outcome"]["duplicate_reused_exact_receipt"] != (
                    row["ordinal"] % 10 == 9
                ):
                    raise ValueError("inconsistent original receipt")
                accepted.add(key)
            elif row["kind"] == "correction" and row["outcome"]["status"] == "conflict":
                conflicts += 1
    if offered != expected.keys() or not result["requests"]["complete"]:
        raise ValueError("incomplete interval")
    for row in records(run / "preparations.jsonl"):
        if row["kind"] != "original_write":
            continue
        key = row["identity"]
        if key not in accepted or key in originals:
            raise ValueError("unreconciled original admission")
        oracle = row["oracle"]
        client, kind, n = expected[key]
        raw = body(key).encode()
        if (
            len(raw) != 4096
            or oracle["raw_sha256"] != hashlib.sha256(raw).hexdigest()
            or oracle["text_sha256"] != oracle["raw_sha256"]
            or oracle["source"]
            != {
                "adapter": "fixture",
                "profile": "native-workload",
                "session": run_id,
                "thread": f"client-{client}",
                "agent": "benchmark",
            }
            or oracle["event_id"] != f"original-{n}"
            or oracle["stream"] != "events"
            or oracle["scope"]
            != {
                "user": f"uid:{os.geteuid()}",
                "project": f"workload/{run_id}/{client}",
                "session": None,
                "branch_or_version": None,
            }
        ):
            raise ValueError("original expectation differs from offered source")
        originals[key] = oracle
    if not originals or originals.keys() != accepted:
        raise ValueError("missing original preparations")
    return originals, conflicts


def verify(run: Path, reader: Path):
    admission = json.loads((run / "admission.json").read_bytes())
    result = json.loads((run / "result.json").read_bytes())
    if not result["assessment"]["interval_thresholds_pass"]:
        raise ValueError("reconcile the failed interval before verifying accepted writes")
    for name in ("requests.jsonl", "preparations.jsonl", "metrics.jsonl"):
        if digest(run / name) != result["journal_sha256"][name]:
            raise ValueError(f"journal changed: {name}")
    cfg = Path(admission["plan"]["daemon_config"])
    if digest(cfg) != admission["plan"]["daemon_config_sha256"]:
        raise ValueError("admitted daemon configuration changed")
    configured = tomllib.loads(cfg.read_text())["paths"]["data_dir"]
    data = Path(configured).resolve(strict=True)
    if not data.is_dir() or not any(data.iterdir()):
        raise ValueError("closed native store expected")
    originals, conflicts = original_oracles(run, admission, result)
    exe = reader.resolve(strict=True)
    snapshot = None
    # Both pipes can exceed the kernel buffer on the full run. Bound scratch to
    # small oracle frames and acknowledgments rather than exporting the store.
    with tempfile.TemporaryFile(dir=run) as source, tempfile.TemporaryFile(dir=run) as replies:
        for row in originals.values():
            source.write(json.dumps(row, separators=(",", ":")).encode() + b"\n")
        source.seek(0)
        subprocess.run([str(exe), str(data)], stdin=source, stdout=replies, check=True)
        replies.seek(0)
        for n, row in enumerate(originals.values(), 1):
            line = replies.readline(16 * 1024 + 1)
            if not line.endswith(b"\n") or len(line) > 16 * 1024:
                raise ValueError("missing or oversized verifier response")
            ack = json.loads(line)
            if snapshot is None:
                snapshot = ack["read_seq"]
            if (
                ack["read_seq"] != snapshot
                or ack["verified"] != n
                or ack["raw_sha256"] != row["raw_sha256"]
                or ack["text_sha256"] != row["text_sha256"]
                or not 0 < ack["event_revision"] <= snapshot
            ):
                raise ValueError("native acknowledgment disagrees with original")
        if replies.read(1):
            raise ValueError("unexpected extra acknowledgment")
    return dict(
        verified_originals=len(originals),
        verified_bytes=len(originals) * 4096,
        read_seq=snapshot,
        revision_conflicts=conflicts,
        reader_sha256=digest(exe),
        run_result_sha256=digest(run / "result.json"),
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("reader", type=Path)
    args = parser.parse_args()
    print(json.dumps(verify(args.run, args.reader)))
