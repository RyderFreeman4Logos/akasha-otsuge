"""Exercise low-space admission on two private Linux tmpfs mounts.

Run after building akasha-otsuge and the corpus_seed example. Requires bwrap.
"""

import hashlib
import json
import os
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def cli(*args, error=None):
    result = subprocess.run(
        ["/app", "--config", "/work/config.toml", *map(str, args)],
        capture_output=True,
        check=False,
        timeout=20,
    )
    if error is None:
        assert result.returncode == 0, (args, result.returncode, result.stderr)
        return json.loads(result.stdout)["data"]
    assert result.returncode and not result.stdout, result
    observed = json.loads(result.stderr)["error"]
    assert observed["code"] == error, observed
    if error == "RESOURCE_BUSY":
        assert result.returncode == 4 and observed["retry_when"] == "capacity_available"


def terminal_job(key):
    end = time.monotonic() + 20
    while time.monotonic() < end:
        record = cli("job", "show", key)
        if record["state"] in ("failed", "succeeded", "cancelled"):
            return record
        time.sleep(0.05)
    raise TimeoutError(key)


def job(verb, key):
    cli("index", verb, "--idempotency-key", key)
    record = terminal_job(key)
    assert record["state"] == "succeeded", record


def fill_to_one_mib(path):
    stats = os.statvfs(path)
    filler = Path(path) / "owned-filler"
    with filler.open("xb") as f:
        os.posix_fallocate(f.fileno(), 0, stats.f_bavail * stats.f_frsize - 1024**2)
    assert os.statvfs(path).f_bavail * stats.f_frsize == 1024**2
    return filler


def model_jobs_under_pressure(read_history):
    entered, release, large_started = (threading.Event() for _ in range(3))
    calls = []
    large = "x" * 16000
    cancelled = "y" * 16000

    class Provider(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            prompt = request["messages"][0]["content"]
            calls.append(prompt)
            if prompt == "short":
                entered.set()
                assert release.wait(20)
                content = "ordinary output " * 1536  # Exactly 24 KiB of valid text.
            else:
                large_started.set()
                content = "done"
            data = json.dumps(
                {
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": content},
                            "finish_reason": "stop",
                        }
                    ]
                }
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    def submit(key, prompt):
        path = Path("/work") / key
        path.write_text(
            json.dumps(
                {
                    "calls": [
                        {
                            "profile": "local",
                            "input": {
                                "kind": "text",
                                "prompt": prompt,
                                "max_output_tokens": 8192,
                            },
                        }
                    ]
                }
            )
        )
        assert path.stat().st_size < 16 * 1024
        record = cli("model", "run", path, "--idempotency-key", key)
        assert (
            record["id"] == key
            and record["state"] == "queued"
            and record["attempt"] == 0
        )
        return record

    with ThreadingHTTPServer(("127.0.0.1", 0), Provider) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with Path("/work/config.toml").open("a") as config:
                config.write(
                    '[runtime]\nmax_concurrent_jobs=1\n[models.local]\nkind="text"\nlocal=true\nmodel="disk-fixture"\n'
                    f'endpoint="http://127.0.0.1:{server.server_port}/v1"\n'
                )
            cli("config", "reload")
            daemon_pid = cli("daemon", "status")["pid"]
            submit("large-result", "short")
            assert (
                entered.wait(8)
                and cli("job", "show", "large-result")["state"] == "running"
            )
            queued = submit("resume-large", large)
            submit("cancel-large", cancelled)
            prior = read_history()
            filler = fill_to_one_mib("/data")
            try:
                release.set()
                failed = terminal_job("large-result")
                assert (failed["state"], failed["error"], failed["result"]) == (
                    "failed",
                    "RESOURCE_BUSY",
                    None,
                ), (failed["state"], failed["error"])
                assert (
                    read_history() == prior
                    and cli("daemon", "status")["pid"] == daemon_pid
                )
                stopped = cli("job", "cancel", "cancel-large")
                assert (stopped["state"], stopped["error"], stopped["attempt"]) == (
                    "cancelled",
                    "CANCELLED",
                    0,
                )
                # Allow the scheduler to tick after the running slot becomes free.
                assert not large_started.wait(1.2)
                waiting = cli("job", "show", "resume-large")
                assert waiting == queued and waiting["started_at_ms"] is None
            finally:
                filler.unlink()
            resumed = terminal_job("resume-large")
            assert (
                resumed["state"],
                resumed["attempt"],
                resumed["accepted_at_ms"],
            ) == ("succeeded", 1, queued["accepted_at_ms"])
            # The identical provider output can be persisted with free disk space.
            submit("roomy-result", "short")
            assert terminal_job("roomy-result")["state"] == "succeeded"
            assert calls == ["short", large, "short"]
            assert (
                read_history() == prior and cli("daemon", "status")["pid"] == daemon_pid
            )
        finally:
            release.set()
            server.shutdown()
            thread.join()


def check_delivery(key, revision):
    work = Path("/work")
    plugin = work / "notifier.py"
    plugin.write_bytes(Path("/plugin-source").read_bytes())
    plugin.chmod(0o700)
    (work / "plugin-mode").write_text("no-idempotency")
    hold = work / "after-send"
    hold.touch()
    # Ordinary local histories make the full persisted notification exceed 16 KiB.
    with (work / "config.toml").open("a") as config:
        for n in range(55):
            name = f"local{n:02}" + "a" * 110
            config.write(f"[notifiers.{name}]\nkind='local'\n")
        config.write("[notifiers.remote]\nkind='stdio'\ncommand=['/work/notifier.py']\n")
    target = work / "review.json"
    target.write_text(
        json.dumps(
            {"memory_id": key, "expected_revision": revision, "reason": "Check the fixture."}
        )
    )
    cli("daemon", "start")
    cli("review", "open", target, "--idempotency-key", "pressure-review")
    stop = time.monotonic() + 15
    while not (work / "plugin-sent").exists():
        if time.monotonic() >= stop:
            raise TimeoutError("waiting for external send")
        time.sleep(0.05)
    effects = work / "received.jsonl"
    notice_id = json.loads(effects.read_text())["notification_id"]
    assert cli("notify", "show", notice_id)["deliveries"]["remote"]["status"] == "sending"
    filler = fill_to_one_mib(Path("/data"))
    hold.unlink()
    while cli("notify", "show", notice_id)["deliveries"]["remote"]["status"] != "sent":
        if time.monotonic() >= stop:
            raise TimeoutError("waiting for durable receipt")
        time.sleep(0.05)
    cli("daemon", "stop")
    while (work / "socket").exists():
        if time.monotonic() >= stop:
            raise TimeoutError("daemon stop under disk pressure")
        time.sleep(0.05)
    cli("daemon", "start")
    saved = cli("notify", "show", notice_id)["deliveries"]["remote"]
    assert saved["status"] == "sent" and saved["attempt"]["number"] == 1
    assert not saved["attempt"]["duplicate_risk"]
    assert len(effects.read_text().splitlines()) == 1
    assert len((work / "attempts.jsonl").read_text().splitlines()) == 1
    filler.unlink()
    cli("daemon", "stop")


def check(volume):
    os.umask(0o077)
    mounts = Path("/proc/self/mountinfo").read_text().splitlines()
    for dest, mib in (("/data", 32), ("/data/search", 32), ("/work", 8)):
        assert any(line.split()[4] == dest and " - tmpfs " in line for line in mounts)
        fs = os.statvfs(dest)
        assert fs.f_blocks * fs.f_frsize == mib * 1024**2
    for path in (Path("/data"), Path("/data/search"), Path("/work")):
        path.chmod(0o700)
    Path("/work/config.toml").write_text(
        '[paths]\ndata_dir="/data"\nsocket_path="/work/socket"\n'
    )
    row = {
        "key": "low-disk",
        "body": "previous confirmed fact",
        "partition": "p0",
        "vector_cohort": False,
    }
    frame = json.dumps(row).encode() + b"\n"
    Path("/work/binding").write_text(
        json.dumps(
            {
                "observed_time_ms": 1793000000000,
                "source_manifest_sha256": hashlib.sha256(frame).hexdigest(),
            }
        )
    )
    seeded = subprocess.run(
        ["/seed", "/data", "/work/binding"],
        input=frame,
        capture_output=True,
        check=True,
        timeout=20,
    )
    key, original = json.loads(seeded.stdout)["memories"][0]
    if volume == "delivery":
        check_delivery(key, original)
        return
    cli("daemon", "start")

    def memory(*args):
        return cli("memory", "show", key, *args)["data"]["revision"]["memory"]

    draft = memory()["record"]["draft"]
    assert draft["claim"] == row["body"]

    def patch_file(tag, previous):
        changed = {**draft, "claim": hashlib.shake_256(tag.encode()).hexdigest(10000)}
        patch = {
            "scope": draft["scope"],
            "reason": "Disk admission fixture.",
            "evidence": draft["source_spans"],
            "expires_at_ms": int(time.time() * 1000) + 180000,
            "changes": [
                {"memory_id": key, "expected_revision": previous, "draft": changed}
            ],
        }
        path = Path("/work") / tag
        path.write_text(json.dumps(patch))
        return ["memory", "patch", str(path), "--idempotency-key", tag]

    approved = patch_file("approved", original)
    receipt = cli(*approved)
    fresh = patch_file("new-write", receipt["commit_seq"])
    job("sync", "original-index")
    before = cli("index", "status")

    # Check the exact mount type and capacity before creating the filler.
    filler = fill_to_one_mib(volume)

    assert cli(*approved) == receipt
    assert (
        cli("index", "sync", "--idempotency-key", "original-index")["state"]
        == "succeeded"
    )
    if volume == "/data":
        cli(*fresh, error="RESOURCE_BUSY")
    for verb in ("rebuild", "sync"):
        cli("index", verb, "--idempotency-key", verb, error="RESOURCE_BUSY")
        cli("job", "show", verb, error="JOB_NOT_FOUND")
    assert memory()["revision"]["commit_seq"] == receipt["commit_seq"]
    assert memory("--at", original)["record"]["draft"] == draft
    after = cli("index", "status")
    assert (before["generation"], before["lexical_watermark"]) == (
        after["generation"],
        after["lexical_watermark"],
    )

    filler.unlink()
    accepted = cli(*fresh)
    for verb in ("rebuild", "sync"):
        job(verb, verb)
    cli("daemon", "stop")
    end = time.monotonic() + 20
    while Path("/work/socket").exists():
        if time.monotonic() >= end:
            raise TimeoutError("daemon stop")
        time.sleep(0.05)
    cli("daemon", "start")
    assert (
        cli(*fresh) == accepted
        and memory()["revision"]["commit_seq"] == accepted["commit_seq"]
    )
    assert memory("--at", original)["record"]["draft"] == draft
    if volume == "/data":
        model_jobs_under_pressure(lambda: memory("--at", original))
    cli("daemon", "stop")


if __name__ == "__main__":
    if Path(__file__) == Path("/check.py"):
        check(sys.argv[1])
    else:
        binary, seeder = (Path(arg).resolve(strict=True) for arg in sys.argv[1:])
        for mount in ("/data", "/data/search", "delivery"):
            cmd = [
                "bwrap",
                "--unshare-all",
                "--die-with-parent",
                "--new-session",
                "--ro-bind",
                "/usr",
                "/usr",
                "--symlink",
                "usr/bin",
                "/bin",
                "--symlink",
                "usr/lib",
                "/lib",
                "--symlink",
                "usr/lib64",
                "/lib64",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
            ]
            for dest, mib in (
                ("/data", 32),
                ("/data/search", 32),
                ("/work", 8),
                ("/tmp", 8),
            ):
                cmd += ["--size", str(mib * 1024**2), "--tmpfs", dest]
            for source, dest in (
                (binary, "/app"),
                (seeder, "/seed"),
                (Path(__file__).resolve(), "/check.py"),
                (
                    Path(__file__).resolve().parents[1] / "tests/fixtures/notifier/oneshot.py",
                    "/plugin-source",
                ),
            ):
                cmd += ["--ro-bind", str(source), dest]
            cmd += [
                "--chdir",
                "/work",
                "--clearenv",
                "--setenv",
                "PATH",
                "/usr/bin:/bin",
                "--setenv",
                "HOME",
                "/work",
                "--setenv",
                "XDG_RUNTIME_DIR",
                "/work",
                "/usr/bin/python3",
                "-B",
                "/check.py",
                mount,
            ]
            subprocess.run(cmd, check=True, timeout=120)
            print("Passed on", mount, flush=True)
