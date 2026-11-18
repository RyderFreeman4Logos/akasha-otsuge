"""Exercise low-space admission on two private Linux tmpfs mounts.

Run after building akasha-otsuge and the corpus_seed example. Requires bwrap.
"""

import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path


def cli(*args, error=None):
    result = subprocess.run(
        ["/app", "--config", "/work/config.toml", *map(str, args)],
        capture_output=True,
        check=False,
        timeout=20,
    )
    if error is None:
        result.check_returncode()
        return json.loads(result.stdout)["data"]
    assert result.returncode and not result.stdout, result
    observed = json.loads(result.stderr)["error"]
    assert observed["code"] == error, observed
    if error == "RESOURCE_BUSY":
        assert result.returncode == 4 and observed["retry_when"] == "capacity_available"


def job(verb, key):
    cli("index", verb, "--idempotency-key", key)
    end = time.monotonic() + 20
    while time.monotonic() < end:
        record = cli("job", "show", key)
        if record["state"] in ("failed", "succeeded", "cancelled"):
            assert record["state"] == "succeeded", record
            return
        time.sleep(0.05)
    raise TimeoutError(key)


def fill_to_one_mib(path):
    stats = os.statvfs(path)
    filler = path / "owned-filler"
    with filler.open("xb") as f:
        os.posix_fallocate(f.fileno(), 0, stats.f_bavail * stats.f_frsize - 1024**2)
    assert os.statvfs(path).f_bavail * stats.f_frsize == 1024**2
    return filler


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

    filler = fill_to_one_mib(Path(volume))

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
