#!/usr/bin/env python3
"""Verify a stopped-store upgrade using two distinct Linux executable releases."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import tempfile
import time


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_hash(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def read_archive(directory):
    """Index this small test export by exact immutable revision."""
    revisions = {}
    commits = list(map(json.loads, (directory / "commits.jsonl").read_text().splitlines()))
    with (directory / "blobs.bin").open("rb") as blobs:
        for commit in commits:
            for row in commit["revisions"]:
                blobs.seek(row["blob_offset"])
                chunk = blobs.read(row["blob_bytes"])
                assert digest(chunk) == row["content_sha256"]
                key = (row["key"]["kind"], row["key"]["id"], row["commit_seq"])
                assert key not in revisions
                revisions[key] = chunk
    # Export order follows request keys; later requests can change physical blob offsets.
    by_sequence = {}
    for commit in commits:
        for row in commit["revisions"]:
            del row["blob_offset"]
        seq = commit["receipt"]["commit_seq"]
        assert seq not in by_sequence
        by_sequence[seq] = commit
    return by_sequence, revisions


def job_record(revisions, key):
    return json.loads(max((seq, content) for (kind, _, seq), content in revisions.items()
                          if kind == "job" and json.loads(content)["id"] == key)[1])


def write_capture(root, key, count):
    identity = dict(adapter="upgrade-fixture", profile="test", session=key, thread=key, agent="local")
    scope = dict(user=f"uid:{os.getuid()}", project=None, branch_or_version=None, session=None)
    binding = dict(source=identity, scope=scope, stream=key, coverage="available_payloads")
    (root / f"{key}.bind").write_text(json.dumps(binding))
    expected = {}
    with (root / f"{key}.jsonl").open("w") as capture:
        for index in range(count):
            ident = f"{key}-{index:03d}"
            raw = f"{ident}|".ljust(4096, "r").encode()
            text = f"{ident}|".ljust(4096, "t")
            event = dict(event_id=ident, source_version="1", runtime_instance="binary-upgrade",
                         branch="main", parent=None, fork_cutoff=None, event_time_ms=None,
                         observed_time_ms=1, visibility="user", kind={"kind": "resume"}, text=text)
            print(json.dumps(dict(schema_version=1, event=event, raw_hex=raw.hex())), file=capture)
            expected[ident] = (identity, scope, raw, text.encode())
    return expected


def verify_originals(revisions, expected):
    seen = set()
    for (kind, key, _), value in revisions.items():
        if kind != "evidence" or not key.startswith("capture-event/"):
            continue
        event = json.loads(value)
        ident = event["reference"]["event_id"]
        identity, scope, raw, text = expected[ident]
        assert ident not in seen and event["reference"]["source"] == identity and event["scope"] == scope
        parts = []
        for part in event["raw"]:
            revision = part["revision"]
            actual = bytes.fromhex(revisions[(revision["key"]["kind"], revision["key"]["id"],
                                              revision["commit_seq"])].decode())
            assert len(actual) == part["bytes"] and digest(actual) == part["sha256"]
            parts.append(actual)
        assert b"".join(parts) == raw and event["raw_sha256"] == digest(raw)
        parts = []
        for span in event["normalized"]:
            source_key = "source/" + digest(span["event_id"].encode())
            source = json.loads(revisions[("evidence", source_key, span["revision"])])
            payload = source["payload"]
            assert payload["event_id"] == span["event_id"] and payload["scope"] == scope
            original = payload["text"].encode()
            assert digest(original) == span["content_sha256"] == source["content_sha256"]
            parts.append(original[span["start"]:span["end"]])
        assert b"".join(parts) == text and event["text_sha256"] == digest(text)
        seen.add(ident)
    assert seen == expected.keys()


def exercise(args, root):
    home, runtime = root / "home", root / "run"
    home.mkdir(mode=0o700)
    runtime.mkdir(mode=0o700)
    app = home / ".local/bin/akasha-otsuge"
    app.parent.mkdir(mode=0o700, parents=True)
    socket, store, config = runtime / "service.sock", root / "store", root / "config.toml"
    env = dict(HOME=str(home), PATH="/usr/bin:/bin", XDG_RUNTIME_DIR=str(runtime), TMPDIR=str(root), LANG="C.UTF-8")
    config.write_text(f'[paths]\ndata_dir={json.dumps(str(store))}\nsocket_path={json.dumps(str(socket))}\n'
                      f'[access]\nread_roots=[{json.dumps(str(root))}]\n[timeouts]\ntools="180s"\n')
    config_hash = file_hash(config)

    def run(*cmd, cwd=root):
        p = subprocess.run([str(x) for x in cmd], cwd=cwd, env=env, capture_output=True,
                           stdin=subprocess.DEVNULL, text=True, timeout=180)
        if p.returncode:
            raise RuntimeError((str(cmd[0]), p.returncode, p.stderr[-1000:]))
        return p.stdout

    def cli(*cmd):
        return json.loads(run(app, "--config", config, *cmd))["data"]

    def stop():
        pid = cli("daemon", "status")["pid"]
        assert os.path.samefile(app, Path("/proc") / str(pid) / "exe")
        fd = os.pidfd_open(pid)
        try:
            watcher = select.poll()
            watcher.register(fd, select.POLLIN)
            assert cli("daemon", "stop")["state"] == "draining"
            assert watcher.poll(120000), "daemon did not close storage"
        finally:
            os.close(fd)
        assert not socket.exists()

    def capture(key):
        return cli("capture", "reconcile", root / f"{key}.bind", root / f"{key}.jsonl",
                   "--idempotency-key", key)

    def finished(key):
        deadline = time.monotonic() + 150
        while True:
            job = cli("job", "show", key)
            if job["state"] == "succeeded":
                return job
            assert job["state"] in ("queued", "running") and time.monotonic() < deadline, job["state"]
            time.sleep(0.2)

    assert file_hash(args.baseline) == args.baseline_sha256
    checksum = args.archive.with_name(args.archive.name + ".sha256")
    assert checksum.read_text().split() == [args.sha256, args.archive.name]
    run("sha256sum", "-c", checksum, cwd=args.archive.parent)
    run("tar", "-xzf", args.archive)
    package = root / args.archive.name.removesuffix(".tar.gz")
    assert (package / "SOURCE_COMMIT").read_text().strip() == args.commit
    new = package / "bin/akasha-otsuge"
    assert file_hash(new) != args.baseline_sha256
    inventory = json.loads((package / "THIRD-PARTY.json").read_bytes())
    bundled = {}
    assert inventory and any(p["name"] == "lancedb" for p in inventory)
    for dependency in inventory:
        assert dependency["files"], dependency["name"]
        for name in dependency["files"].values():
            path = package / name
            assert path.parent == package / "licenses"
            if name not in bundled:
                bundled[name] = file_hash(path)
            assert bundled[name] == path.stem
    assert len(bundled) == len(list((package / "licenses").iterdir()))
    expected = {**write_capture(root, "before", 1), **write_capture(root, "pending", 128)}

    run("install", "-m", "0755", args.baseline, app)
    cli("daemon", "start")
    try:
        cli("doctor", "--json")
        capture("before")
        before = finished("before")
        accepted = capture("pending")
        assert accepted["state"] in ("queued", "running")
    finally:
        stop()

    cli("archive", "export", root / "pre")
    pre_commits, pre = read_archive(root / "pre")
    paused = job_record(pre, "pending")
    assert paused["state"] in ("queued", "running")
    run("install", "-m", "0755", new, app.with_suffix(".new"))
    os.replace(app.with_suffix(".new"), app)
    binary_hash = file_hash(app)
    assert binary_hash == file_hash(new) and file_hash(config) == config_hash
    shutil.rmtree(package)
    cli("daemon", "start")
    try:
        cli("doctor", "--json")
        done = finished("pending")
        assert cli("job", "show", "before") == before and capture("pending") == done
        for field in ("id", "request_sha256", "config_digest", "config_generation", "queue_deadline_ms",
                      "execution_budget_ms", "accepted_at_ms"):
            assert accepted[field] == paused[field] == done[field], field
        for field in ("started_at_ms", "execution_deadline_ms"):
            if paused[field] is not None:
                assert paused[field] == done[field], field
        checkpoint = done["result"]["checkpoint"]["checkpoint"]
        assert not checkpoint["loss"] and checkpoint["pending"] is None
    finally:
        stop()
    cli("archive", "export", root / "post")
    post_commits, post = read_archive(root / "post")
    assert all(post_commits[seq] == commit for seq, commit in pre_commits.items())
    assert all(post[ref] == value for ref, value in pre.items())
    assert job_record(post, "pending") == done
    verify_originals(post, expected)
    app.unlink()
    assert not app.exists() and (store / "authority").is_dir() and file_hash(config) == config_hash
    return dict(source_commit=args.commit, archive_sha256=args.sha256, baseline_sha256=args.baseline_sha256,
                upgraded_executable_sha256=binary_hash, third_party_packages=len(inventory), notice_texts=len(bundled),
                pending_before=paused["state"], pending_after=done["state"], idempotent_replay=True,
                unchanged_prior_commits=len(pre_commits), final_commits=len(post_commits), original_events=len(expected),
                raw_bytes=sum(len(v[2]) for v in expected.values()), normalized_bytes=sum(len(v[3]) for v in expected.values()),
                config_preserved=True, history_after_uninstall=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--baseline-sha256", required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--scratch", type=Path, required=True)
    args = parser.parse_args()
    args.baseline = args.baseline.resolve(strict=True)
    args.archive = args.archive.resolve(strict=True)
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix="u26-", dir=args.scratch) as directory:
        result = exercise(args, Path(directory))
    print(json.dumps(result, sort_keys=True))
