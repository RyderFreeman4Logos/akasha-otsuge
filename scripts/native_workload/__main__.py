"""Run one native 100-client interval; scale and offline byte oracles stay separate."""

import argparse
import asyncio
import collections
import datetime
import json
import math
import os
import pathlib
import resource
import shutil
import time
import tomllib

from spikes.helix.run_case import digest as sha
from spikes.helix.run_case import process_identity, publish

from .journal import Journal, records
from .operations import NativeOperations
from .operations_by_role import NativeWorkload
from .schedule import clients, offers, run_schedule, summarize

MODULES = pathlib.Path(__file__).resolve().parent
REPO = MODULES.parents[1]
MINIMUMS = {
    "original_write": 10,
    "current": 2,
    "lexical": 1,
    "hybrid_vector": 0.2,
    "scoped_history": 0.2,
    "correction": 1 / 6,
}
P95 = {"original_write": 5, "current": 5, "lexical": 60, "hybrid_vector": 60, "scoped_history": 10}


def exact(identity):
    observed = process_identity(identity["pid"])
    if (
        not observed
        or observed["state"] == "Z"
        or str(observed["start_ticks"]) != str(identity["start_ticks"])
    ):
        raise RuntimeError("admitted native process exited or changed")


def process_sample(identity):
    exact(identity)
    root = pathlib.Path("/proc") / str(identity["pid"])
    values = {}
    for name, keys in [
        ("status", ("VmRSS", "VmHWM", "RssAnon", "RssFile", "RssShmem", "VmSwap")),
        ("smaps_rollup", ("Pss", "Private_Clean", "Private_Dirty")),
    ]:
        for line in (root / name).read_text().splitlines():
            key, _, raw = line.partition(":")
            if key in keys:
                values[key] = int(raw.split()[0]) * 1024
    values["io"] = {
        key.rstrip(":"): int(value)
        for key, value in (line.split() for line in (root / "io").read_text().splitlines())
    }
    return values


def validate_config(config):
    if set(config) != {"execution", "reliability", "resources"}:
        raise ValueError("unknown run settings")
    if any(type(v) is not int for group in config.values() for v in group.values()):
        raise ValueError("integer settings required")
    if set(config["execution"]) != {"seconds"} or set(config["resources"]) != {
        "target_mib",
        "peak_mib",
        "service_total_mib",
    }:
        raise ValueError("unknown execution or resource setting")
    seconds = config["execution"]["seconds"]
    if type(seconds) is not int or not 1 <= seconds <= 1800:
        raise ValueError("interval must be 1..1800 seconds")
    if config["reliability"] != dict(max_attempts=1, request_timeout_s=10, job_timeout_s=600):
        raise ValueError("unqualified reliability policy")
    target, peak = config["resources"]["target_mib"], config["resources"]["peak_mib"]
    if [target, peak] not in ([512, 640], [1024, 1280], [2048, 2048]):
        raise ValueError("unknown memory profile")
    if config["resources"]["service_total_mib"] != 2048:
        raise ValueError("service budget changed")
    return seconds, target, peak


def daemon_rss(resources):
    measurement = resources["measurement"]
    if measurement["complete"] is not True:
        raise ValueError("incomplete native process tree measurement")
    rss = measurement["daemon_group"]["rss_bytes"]
    if type(rss) is not int or rss <= 0:
        raise ValueError("invalid daemon group RSS")
    return rss


def assess(report, samples, seconds, target, peak, interval):
    gaps = []
    if not report["complete"]:
        gaps.append("incomplete request journal")
    for kind, minimum in MINIMUMS.items():
        counts = report["kinds"].get(kind, {}).get("counts", {})
        if counts.get("success_within_interval", 0) < math.ceil(seconds * minimum):
            gaps.append(kind + " throughput")
        if counts.get("error", 0):
            gaps.append(kind + " errors")
        latency = report["kinds"].get(kind, {}).get("success_offered_latency_s", {}).get("95")
        if kind in P95 and (latency is None or latency > P95[kind]):
            gaps.append(kind + " p95")
    start = interval["started_monotonic_s"]
    end = start + seconds
    valid = [r for r in samples if "daemon" in r and start <= r["observed_monotonic_s"] < end]
    ticks = {
        r["number"]
        for r in valid
        if 0 <= r["number"] < seconds
        and start + r["number"] <= r["observed_monotonic_s"] < start + r["number"] + 1
    }
    if len(ticks) != seconds or any("error" in r for r in samples):
        gaps.append("resource sample coverage")
    rss = sorted(daemon_rss(r["resources"]) for r in valid)
    if not rss or rss[math.ceil(0.99 * len(rss)) - 1] > target * 1024**2:
        gaps.append("RSS p99")
    if not rss or max(rss) > peak * 1024**2:
        gaps.append("RSS maximum")
    if any(r["daemon"]["VmHWM"] > peak * 1024**2 or r["events"].get("oom_kill", 0) for r in valid):
        gaps.append("native peak or OOM")
    return dict(
        gaps=gaps,
        interval_thresholds_pass=not gaps,
        resource_samples=len(valid),
        sampled_rss_p99_bytes=rss[math.ceil(0.99 * len(rss)) - 1] if rss else None,
        sampled_rss_max_bytes=max(rss) if rss else None,
        required_duration_executed=seconds >= 1800,
        full_native_acceptance=False,
    )


async def run(plan_path, config_path, root):
    plan = json.loads(plan_path.read_text())
    config = tomllib.loads(config_path.read_text())
    seconds, target, peak = validate_config(config)
    if not plan["run_id"] or any(
        c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_"
        for c in plan["run_id"]
    ):
        raise ValueError("invalid run namespace")
    if len(plan["run_id"]) > 64:
        raise ValueError("run namespace too long")
    exact(plan["daemon"])
    exact(plan["lookup"])
    daemon_exe = pathlib.Path("/proc") / str(plan["daemon"]["pid"]) / "exe"
    if sha(daemon_exe) != plan["daemon_sha256"]:
        raise ValueError("native executable mismatch")
    if sha(pathlib.Path(plan["daemon_config"])) != plan["daemon_config_sha256"]:
        raise ValueError("daemon configuration mismatch")
    if sha(pathlib.Path(plan["scale_receipt"])) != plan["scale_receipt_sha256"]:
        raise ValueError("scale evidence changed")
    if root.parent != (REPO / ".spike-work").resolve() or not root.name.startswith(
        "native-workload-"
    ):
        raise ValueError("output must be an owned SSD workload directory")
    if shutil.disk_usage(root.parent).free < 30 * 1024**3:
        raise ValueError("free reserve")
    root.mkdir(mode=0o700)
    (root / "OWNER").write_text("akasha-otsuge:native-workload-interval\n")
    with (root / "OWNER").open("rb") as owner:
        os.fsync(owner.fileno())
    directory = os.open(root.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
    source_names = [
        "__main__.py",
        "journal.py",
        "operations_by_role.py",
        "operations.py",
        "ipc.py",
        "schedule.py",
    ]
    admission = dict(
        utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        process=process_identity(os.getpid()),
        plan_sha256=sha(plan_path),
        config_sha256=sha(config_path),
        source_sha256={name: sha(MODULES / name) for name in source_names},
        plan=plan,
        config=config,
        planned=sum(sum(1 for _ in offers(c, seconds)) for c in clients()),
        recovery="Existing interval roots are refused. Adopt the recorded live process or reconcile uncertain effects; never replay an interrupted continuous interval.",
        acceptance="Native interval only. Offline accepted-event, full corpus, ANN recall and recovery oracles are separate.",
    )
    publish(root / "admission.json", admission)
    requests, preparations, metrics = (
        Journal(root / name) for name in ("requests.jsonl", "preparations.jsonl", "metrics.jsonl")
    )
    ops = NativeOperations(plan["socket"], 10, 600)
    work = NativeWorkload(root, plan["run_id"], plan["oracles"], ops, preparations.append)
    counts = collections.Counter()
    done = asyncio.Event()
    ready = asyncio.Event()
    started = time.monotonic()
    failure = None
    bounds = {}

    async def emit(row):
        await requests.append(row)
        counts[row["event"]] += 1
        if row["event"] == "interval":
            bounds.update(row)
            ready.set()
        if row["event"] == "outcome":
            counts[row["outcome"]["status"]] += 1

    async def interval():
        try:
            await run_schedule(plan["run_id"], seconds, work, emit)
        finally:
            done.set()

    async def sample(number):
        status = await ops.request(dict(op="status"), f"{plan['run_id']}/metrics/{number}")
        if status["pid"] != plan["daemon"]["pid"]:
            raise RuntimeError("status daemon identity changed")
        group_rss = daemon_rss(status["resources"])
        native = status["resources"]["cgroup"]
        group = native["daemon_group"]
        daemon, lookup = await asyncio.gather(
            asyncio.to_thread(process_sample, plan["daemon"]),
            asyncio.to_thread(process_sample, plan["lookup"]),
        )
        events = {
            key: int(value)
            for key, value in (
                line.split()
                for line in (pathlib.Path(group["path"]) / "memory.events").read_text().splitlines()
            )
        }
        row = dict(
            event="sample",
            number=number,
            elapsed_s=time.monotonic() - started,
            observed_monotonic_s=time.monotonic(),
            utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
            daemon=daemon,
            lookup=lookup,
            resources=status["resources"],
            events=events,
            client_peak_bytes=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * 1024,
            filesystem_free_bytes=shutil.disk_usage(root).free,
        )
        await metrics.append(row)
        if not native["enforced"] or not native["complete"]:
            raise RuntimeError("missing owned kernel limits")
        if (group["high_bytes"], group["max_bytes"], native["service_total"]["max_bytes"]) != (
            target * 1024**2,
            peak * 1024**2,
            2048 * 1024**2,
        ):
            raise RuntimeError("native resource profile changed")
        if events.get("oom_kill", 0) or max(daemon["VmHWM"], group_rss) > peak * 1024**2:
            raise RuntimeError("native peak or OOM")
        if row["client_peak_bytes"] > 512 * 1024**2 or lookup["VmHWM"] > 512 * 1024**2:
            raise RuntimeError("external helper ceiling")
        if row["filesystem_free_bytes"] < 30 * 1024**3:
            raise RuntimeError("free reserve crossed")

    try:
        await sample(-1)
        async with asyncio.TaskGroup() as running:
            running.create_task(interval())
            await ready.wait()
            number = 0
            while not done.is_set():
                due = bounds["started_monotonic_s"] + number
                try:
                    await asyncio.wait_for(done.wait(), timeout=max(0.001, due - time.monotonic()))
                except TimeoutError:
                    pass
                if done.is_set():
                    break
                await sample(number)
                elapsed = time.monotonic() - started
                completed = counts["outcome"] + counts["not_dispatched"]
                publish(
                    root / "progress.json",
                    dict(
                        phase="interval_or_drain",
                        completed=completed,
                        total=admission["planned"],
                        elapsed_s=elapsed,
                        rate_per_s=completed / elapsed if completed else None,
                        eta_s=max(0, bounds["started_monotonic_s"] + seconds - time.monotonic()),
                        counts=counts,
                        process=admission["process"],
                        config_epoch=admission["config_sha256"],
                    ),
                )
                number = max(
                    number + 1, math.ceil(time.monotonic() - bounds["started_monotonic_s"])
                )
        if (
            sha(plan_path) != admission["plan_sha256"]
            or sha(config_path) != admission["config_sha256"]
        ):
            raise RuntimeError("interval input changed")
        if sha(plan["scale_receipt"]) != plan["scale_receipt_sha256"]:
            raise RuntimeError("scale receipt changed during workload")
        if (
            sha(daemon_exe) != plan["daemon_sha256"]
            or sha(pathlib.Path(plan["daemon_config"])) != plan["daemon_config_sha256"]
        ):
            raise RuntimeError("native runtime identity changed")
        for name, digest in admission["source_sha256"].items():
            if sha(MODULES / name) != digest:
                raise RuntimeError("executing workload source changed")
    except BaseException as error:
        failure = dict(type=type(error).__name__, message=str(error)[:4000])
        publish(root / "failure.json", failure)
        raise
    finally:
        requests.close()
        preparations.close()
        metrics.close()
        publish(
            root / "state.json",
            dict(
                phase="failed" if failure else "awaiting_oracle",
                failure=failure,
                counts=counts,
                process=admission["process"],
                elapsed_s=time.monotonic() - started,
                admission_sha256=sha(root / "admission.json"),
            ),
        )
    report = summarize(records(root / "requests.jsonl"), seconds)
    assessed = assess(report, list(records(root / "metrics.jsonl")), seconds, target, peak, bounds)
    result = dict(
        status="INTERVAL_EXECUTED_PENDING_OFFLINE_ORACLES"
        if assessed["interval_thresholds_pass"]
        else "INTERVAL_THRESHOLDS_FAILED",
        requests=report,
        assessment=assessed,
        elapsed_s=time.monotonic() - started,
        journal_sha256={
            name: sha(root / name)
            for name in ("requests.jsonl", "preparations.jsonl", "metrics.jsonl")
        },
        full_native_acceptance=False,
    )
    publish(root / "result.json", result)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", type=pathlib.Path)
    parser.add_argument("config", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    args = parser.parse_args()
    result = asyncio.run(
        run(args.plan.resolve(strict=True), args.config.resolve(strict=True), args.output.resolve())
    )
    print(json.dumps(dict(status=result["status"], assessment=result["assessment"])))
    raise SystemExit(0 if result["assessment"]["interval_thresholds_pass"] else 1)
