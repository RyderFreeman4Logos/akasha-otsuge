#!/usr/bin/env python3
"""Controlled external notifier used by daemon protocol tests."""
import json
import os
from pathlib import Path
import sys
import time

root = Path(__file__).parent
mode = (root / "plugin-mode").read_text() if (root / "plugin-mode").exists() else "normal"
request = json.loads(sys.stdin.buffer.read(8193))
assert request["schema_version"] == 1
if request["op"] == "capabilities":
    result = {"schema_version": 1, "send": True, "update": True,
              "idempotency": mode != "no-idempotency", "ack_callback": False, "rich_text": False}
else:
    assert request["op"] == "deliver"
    notice = request["notification"]
    (root / "plugin.pid").write_text(str(os.getpid()))
    (root / "plugin-started").write_text("started")
    attempts = root / "attempts.jsonl"
    earlier = attempts.read_text().splitlines() if attempts.exists() else []
    with attempts.open("a") as output:
        output.write(json.dumps({"idempotency_key": notice["idempotency_key"], "at_ms": time.time_ns() // 1_000_000}) + "\n")
        output.flush()
        os.fsync(output.fileno())
    if mode == "always-fail" or mode == "fail-once" and not earlier:
        sys.exit(75)
    while (root / "hold-plugin").exists():
        time.sleep(0.01)
    received = root / "received.jsonl"
    previous = [json.loads(line) for line in received.read_text().splitlines()] if received.exists() else []
    if mode == "no-idempotency" or not any(row["idempotency_key"] == notice["idempotency_key"] for row in previous):
        with received.open("a") as output:
            output.write(json.dumps(notice) + "\n")
            output.flush()
            os.fsync(output.fileno())
    (root / "plugin-sent").write_text("sent")
    while (root / "after-send").exists():
        time.sleep(0.01)
    result = {"schema_version": 1,
              "external_message_id": "fixture-" + notice["notification_id"]}
print(json.dumps(result))
