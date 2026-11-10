#!/usr/bin/env python3
"""Independent stdio notifier: one private JSON inbox, atomic effects and receipts."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tempfile


MAX_STATE = 2 * 1024 * 1024
FIELDS = {"notification_id", "idempotency_key", "review_id", "priority", "summary",
          "evidence_locator", "action", "external_message_id"}


def check(condition):
    if not condition:
        raise ValueError("invalid notifier input or state")


def encode(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def private_file(fd):
    info = os.fstat(fd)
    check(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
          and info.st_mode & 0o077 == 0 and info.st_nlink == 1)


def validate_notice(notice):
    check(isinstance(notice, dict) and set(notice) == FIELDS)
    for name in FIELDS - {"external_message_id"}:
        value = notice[name]
        check(isinstance(value, str) and value.strip() and len(value.encode()) <= 1024
              and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value))
    check(notice["priority"] in {"normal", "high", "urgent"})
    check(notice["action"] in {"create", "update"})
    external = notice["external_message_id"]
    check((notice["action"] == "create" and external is None)
          or (notice["action"] == "update" and isinstance(external, str) and 0 < len(external.encode()) <= 256))


def message_id(notice):
    return "file-" + hashlib.sha256(notice["notification_id"].encode()).hexdigest()


def read_state(path):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except FileNotFoundError:
        return dict(schema_version=1, messages={}, receipts={}, failed_once=False)
    with os.fdopen(fd, "rb") as source:
        private_file(source.fileno())
        raw = source.read(MAX_STATE + 1)
    check(len(raw) <= MAX_STATE)
    state = json.loads(raw)
    check(isinstance(state, dict) and set(state) == {"schema_version", "messages", "receipts", "failed_once"}
          and type(state["schema_version"]) is int and state["schema_version"] == 1
          and isinstance(state["messages"], dict) and isinstance(state["receipts"], dict)
          and type(state["failed_once"]) is bool)
    for key, receipt in state["receipts"].items():
        check(key.strip() and len(key.encode()) <= 1024 and isinstance(receipt, dict)
              and set(receipt) == {"sha256", "external_message_id"})
        digest = receipt["sha256"]
        check(isinstance(digest, str) and len(digest) == 64 and all(c in "0123456789abcdef" for c in digest))
        target = receipt["external_message_id"]
        check(isinstance(target, str) and target in state["messages"])
    for target, notice in state["messages"].items():
        validate_notice(notice)
        check(target == message_id(notice))
        check(notice["action"] != "update" or notice["external_message_id"] == target)
        receipt = state["receipts"].get(notice["idempotency_key"])
        check(receipt == dict(sha256=hashlib.sha256(encode(notice)).hexdigest(), external_message_id=target))
    return state


def save_state(path, state):
    data = encode(state) + b"\n"
    check(len(data) <= MAX_STATE)
    fd, temporary = tempfile.mkstemp(prefix=".notifier-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        Path(temporary).unlink(missing_ok=True)


def deliver(path, notice, fail_first):
    validate_notice(notice)
    external = notice["external_message_id"]
    check(path.is_absolute())
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.parent.lstat()
    check(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid() and info.st_mode & 0o077 == 0)
    lock = os.open(str(path) + ".lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        private_file(lock)
        fcntl.flock(lock, fcntl.LOCK_EX)
        state = read_state(path)
        fingerprint = hashlib.sha256(encode(notice)).hexdigest()
        key = notice["idempotency_key"]
        if key in state["receipts"]:
            receipt = state["receipts"][key]
            check(receipt["sha256"] == fingerprint and receipt["external_message_id"] == message_id(notice))
            return receipt["external_message_id"]
        if fail_first and not state["failed_once"]:
            state["failed_once"] = True
            save_state(path, state)
            raise OSError("demonstration transient failure")
        target = message_id(notice)
        if notice["action"] == "update":
            check(external == target and external in state["messages"])
        state["messages"][target] = notice
        state["receipts"][key] = dict(sha256=fingerprint, external_message_id=target)
        save_state(path, state)
        return target
    finally:
        os.close(lock)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state", required=True, type=Path, help="absolute private inbox JSON path")
    parser.add_argument("--fail-first", action="store_true", help="persist one failure before the first effect")
    args = parser.parse_args()
    raw = sys.stdin.buffer.read(8193)
    check(len(raw) <= 8192)
    request = json.loads(raw)
    check(isinstance(request, dict) and type(request.get("schema_version")) is int
          and request["schema_version"] == 1)
    if request.get("op") == "capabilities":
        check(set(request) == {"schema_version", "op"})
        result = dict(schema_version=1, send=True, update=True, idempotency=True,
                      ack_callback=False, rich_text=False)
    else:
        check(request.get("op") == "deliver" and set(request) == {"schema_version", "op", "notification"})
        result = dict(schema_version=1, external_message_id=deliver(args.state, request["notification"], args.fail_first))
    sys.stdout.buffer.write(encode(result) + b"\n")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, TypeError, OSError):
        print("NOTIFIER_FILE_FAILED", file=sys.stderr)
        sys.exit(75)
