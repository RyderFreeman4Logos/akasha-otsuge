#!/usr/bin/env python3
"""Run one immutable workload case; reconcile completion, never replay ambiguity."""

import argparse
import fcntl
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import tomllib
import uuid


def digest(path):
    with pathlib.Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def publish(path, value):
    temporary = path.with_suffix(".tmp")
    with temporary.open("w") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def process_identity(pid):
    try:
        fields = pathlib.Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return {"pid": pid, "start_ticks": fields[19], "state": fields[0]}
    except FileNotFoundError:
        return None


def verify_requests(directory):
    baseline = json.loads((directory / "runtime/baseline.json").read_text())
    request = json.loads((directory / "request.json").read_text())
    admission = json.loads((directory / "runtime/admission.json").read_text())
    fixture = pathlib.Path(request["fixture"])
    with fixture.open("rb") as source:
        fixture_bytes = source.read(4 * 1024**2 + 1)
    if len(fixture_bytes) > 4 * 1024**2 or hashlib.sha256(fixture_bytes).hexdigest() != admission["fixture_sha256"]:
        raise ValueError("current corpus oracle source differs from admission")
    queries = json.loads(fixture_bytes)["queries"]
    base, run = baseline["base"], baseline["run_id"]
    def expected_body(client, ordinal):
        prefix = f"Synthetic workload; excluded from corpus. Run {run}; client {client}; request {ordinal}. "
        return hashlib.sha256((prefix + "x" * (4096 - len(prefix))).encode()).hexdigest()
    events, sequences = [], set()
    for i, value in enumerate(baseline["heads"]):
        expected = dict(memory=base + i, seq=value["seq"], scope=i % 8, active=True, body_sha256=expected_body(i, 0))
        if value != expected or value["seq"] in sequences:
            raise ValueError("baseline content or sequence mismatch")
        sequences.add(value["seq"])
        events.append((value["seq"], 0, value))
    records = current_checks = 0
    corpus_points = []
    corpus_memories, corpus_partitions = set(), {}
    search_points, search_checks = [], dict(bm25=0, ann=0)
    text_current, text_bm25 = [], []
    with (directory / "runtime/requests.jsonl").open() as source:
        for line in source:
            row = json.loads(line)
            records += 1
            if row["status"] != "success":
                continue
            receipt, client, ordinal = row["receipt"], row["client"], row["ordinal"]
            if row["operation"] in {"write", "correction"}:
                write = row["operation"] == "write"
                expected = dict(memory=base + (client // 2 if write else client % 30), seq=receipt["commit_seq"],
                                scope=(client + ordinal) % 8, active=write or ordinal % 2 == 0,
                                body_sha256=expected_body(client, ordinal))
                if receipt["oracle"] != expected or expected["seq"] in sequences:
                    raise ValueError("acknowledged write content or sequence mismatch")
                sequences.add(expected["seq"])
                events.append((expected["seq"], 0, expected))
            elif row["operation"] == "current":
                corpus = receipt.get("corpus")
                if not isinstance(corpus, dict) or type(corpus.get("query_index")) is not int or not 0 <= corpus["query_index"] < len(queries):
                    raise ValueError("missing or invalid corpus current receipt")
                query = queries[corpus["query_index"]]
                expected = dict(memory=query["memory"], seq=0, scope=int(query["partition"][1]), active=True, body_sha256=query["body_sha256"])
                if (corpus.get("entry") != expected or corpus.get("key") != query["key"]
                        or type(corpus.get("at")) is not int or corpus["at"] < receipt["at"]
                        or corpus.get("permission_revision") != 1):
                    raise ValueError("current corpus receipt differs from pinned source")
                corpus_points.append(corpus["at"])
                corpus_memories.add(query["memory"])
                corpus_partitions[query["partition"]] = corpus_partitions.get(query["partition"], 0) + 1
                if query.get('text_only'):
                    text_current.append(query['partition'])
                events.append((receipt["at"], 1, receipt))
            elif row['operation'] in search_checks:
                index = receipt.get('query_index')
                if type(index) is not int or not 0 <= index < len(queries):
                    raise ValueError('invalid search query identity')
                query, hits, validation = queries[index], receipt.get('hits'), receipt.get('validation', {})
                read, validated = validation.get('read_seq'), validation.get('validated_at_seq')
                if (receipt.get('query_key') != query['key'] or receipt.get('partition') != query['partition']
                        or (row['operation'] == 'ann' and query.get('text_only'))
                        or not isinstance(hits, list) or not 1 <= len(hits) <= 20
                        or validation.get('hits') != hits or validation.get('permission_revision') != 1
                        or type(read) is not int or type(validated) is not int or not 0 <= read <= validated
                        or any(hit.get('partition') != query['partition'] or type(hit.get('memory')) is not int
                               or not 0 <= hit['memory'] <= 2**63-1
                               or (hit.get('seq') is not None and (type(hit['seq']) is not int or not 0 <= hit['seq'] <= validated)) for hit in hits)):
                    raise ValueError('search receipt differs from pinned query or validated candidates')
                search_points.append(validated)
                search_checks[row['operation']] += 1
                if query.get('text_only'):
                    text_bm25.append(query['partition'])
    heads = {}
    for _, kind, value in sorted(events, key=lambda event: event[:2]):
        if kind == 0:
            heads[value["memory"]] = value
        else:
            expected = heads.get(value["memory"])
            if expected is None:
                raise ValueError("current read precedes its baseline")
            if value["entry"] != (expected if expected["active"] else None):
                raise ValueError("current snapshot differs from acknowledged writes at its commit point")
            current_checks += 1
    if len(baseline["heads"]) != 50 or len(sequences) != max(sequences) - min(sequences) + 1:
        raise ValueError("missing baseline identity or unexplained commit-sequence gap")
    if any(point > max(sequences) for point in corpus_points + search_points):
        raise ValueError("corpus read claims an uncommitted point")
    if request.get('diagnostic') is False and request.get('phase') == 'steady' and (
            set(text_current) != {f'p{i}' for i in range(8)} or set(text_bm25) != {f'p{i}' for i in range(8)}):
        raise ValueError('full steady case did not exercise all unvectorized partitions')
    derived_checks = 0
    if generation := request.get("generation"):
        derived = json.loads((directory / "runtime/derived-oracle.json").read_text())
        committed = max(sequences)
        expected_progress = dict(generation=generation, committed=committed, text=committed, vector=committed, remaining=0)
        expected_text = sorted((row for row in heads.values() if row["active"]), key=lambda row: row["memory"])
        if (derived.get("progress") != expected_progress or derived.get("indexed_jobs") != committed * 2
                or derived.get("vectors") != [] or derived.get("text") != expected_text):
            raise ValueError("derived generation differs from acknowledged source revisions")
        derived_checks = len(expected_text)
    return {"requests": records, "current_checks": current_checks, "corpus_current_checks": len(corpus_points),
            "corpus_current_distinct": len(corpus_memories), "corpus_current_partitions": corpus_partitions,
            "text_only_current_checks": len(text_current), "text_only_bm25_checks": len(text_bm25),
            "text_only_current_partitions": sorted(set(text_current)), "text_only_bm25_partitions": sorted(set(text_bm25)),
            "search_checks": search_checks,
            "distinct_commits": len(sequences), "derived_current_checks": derived_checks}


def summarize(directory, profile, binary_sha256):
    samples = [json.loads(line) for line in (directory / "memory.jsonl").read_text().splitlines()]
    exits = [row for row in samples if row["event"] == "exit"]
    if len(exits) != 1:
        raise ValueError("observer has no unique terminal receipt; effects remain unreconciled")
    executables = [row for row in samples if row["event"] == "executable"]
    if len(executables) != 1 or executables[0]["sha256"] != binary_sha256:
        raise ValueError("executed binary differs from admitted binary")
    limit = str((640 if profile == "low" else 2048) * 1024**2)
    if samples[0]["limits"]["memory.max"] != limit or samples[0]["limits"]["memory.swap.max"] != "0":
        raise ValueError("effective cgroup limits differ from admitted profile")
    runtime = directory / "runtime" / "result.json"
    result = json.loads(runtime.read_text()) if runtime.exists() else {"status": "failed", "reason": "engine has no terminal result"}
    oracle = verify_requests(directory) if runtime.exists() else None
    if oracle and oracle["requests"] != result["snapshot"]["completed"]:
        raise ValueError("raw request count differs from summary")
    rss = sorted(row["engine_rss_bytes"] for row in samples if row["event"] == "sample" and row["engine_rss_bytes"] > 0)
    p99 = rss[(99 * len(rss) + 99) // 100 - 1] if rss else None
    maximum = max(rss) if rss else None
    memory_ok = bool(rss) and p99 <= (512 if profile == "low" else 2048) * 1024**2 and maximum <= (640 if profile == "low" else 2048) * 1024**2
    status = "passed" if exits[0]["code"] == 0 and result["status"] == "passed" and memory_ok else "failed"
    paths = ["request.json", "memory.jsonl", "engine.log"]
    paths += [name for name in ["runtime/result.json", "runtime/requests.jsonl", "runtime/metrics.jsonl", "runtime/admission.json", "runtime/baseline.json", "runtime/derived-oracle.json"] if (directory / name).exists()]
    return {"status": status, "effects": "measured" if runtime.exists() else "unknown", "content_oracle": oracle, "engine": result, "exit": exits[0], "rss_p99_bytes": p99,
            "rss_max_bytes": maximum, "memory_thresholds_passed": memory_ok,
            "artifact_sha256": {name: digest(directory / name) for name in paths}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("config", type=pathlib.Path)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--abandon", metavar="EXACT_RUN_ID")
    args = parser.parse_args()
    if args.config.stat().st_size > 65536:
        raise ValueError("case TOML exceeds 64 KiB")
    config = tomllib.loads(args.config.read_text())
    if set(config) - {"disk"} != {"run", "paths", "reliability"} or config["reliability"] != {"max_attempts": 1}:
        raise ValueError("case has no network retries; exactly one measured attempt is required")
    run, paths = config["run"], config["paths"]
    if set(run) - {"generation"} != {"id", "profile", "phase", "seconds", "diagnostic"} or set(paths) != {"binary", "database", "fixture", "output", "corpus_manifest"}:
        raise ValueError("unknown or missing case setting")
    if run["profile"] not in {"low", "high"} or run["phase"] not in {"steady", "burst", "overload"}:
        raise ValueError("unknown profile or phase")
    if not isinstance(run["seconds"], int) or isinstance(run["seconds"], bool) or not 1 <= run["seconds"] <= 7200:
        raise ValueError("invalid duration")
    if not isinstance(run["diagnostic"], bool):
        raise ValueError("diagnostic must be a boolean")
    if "generation" in run and (type(run["generation"]) is not int or run["generation"] not in {1, 2}):
        raise ValueError("generation must be an explicit retained identity")
    disk = config.get("disk")
    minimum = 30 * 1024**3 + 4_000_000_000 + (45_000_000_000 if not run["diagnostic"] and run.get("generation") == 1 else 0)
    if (not run["diagnostic"] and disk is None) or (disk is not None and (
            set(disk) != {"start_bytes", "reserve_bytes"} or any(type(v) is not int for v in disk.values())
            or not disk["start_bytes"] >= disk["reserve_bytes"] >= minimum)):
        raise ValueError("full case requires its current/next index disk reservation")
    if args.abandon is not None and args.abandon != run["id"]:
        raise ValueError("abandonment requires the exact run ID")
    directory = pathlib.Path(paths["output"]).resolve()
    root = pathlib.Path(paths["database"]).resolve(strict=True)
    binary = pathlib.Path(paths["binary"]).resolve(strict=True)
    fixture = pathlib.Path(paths["fixture"]).resolve(strict=True)
    if fixture.stat().st_size > 4 * 1024**2:
        raise ValueError("query metadata exceeds 4 MiB")
    metadata = json.loads(fixture.read_text())
    vector_sha256 = None
    if vector := metadata.get("vector_file"):
        if pathlib.Path(vector["name"]).name != vector["name"] or not 0 < vector["bytes"] <= 4096 * 1536 * 4:
            raise ValueError("invalid query vector sidecar")
        sidecar = fixture.parent / vector["name"]
        vector_sha256 = digest(sidecar)
        if sidecar.stat().st_size != vector["bytes"] or vector_sha256 != vector["sha256"]:
            raise ValueError("query vector sidecar changed")
    manifest = pathlib.Path(paths["corpus_manifest"]).resolve(strict=True) if paths["corpus_manifest"] else None
    # One controller owns this database; the durable pointer survives its death.
    owner_lock = (root / ".akasha-case-owner.lock").open("a")
    fcntl.flock(owner_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    generation_owner = root / ".akasha-generation-owner.json"
    if generation_owner.exists():
        prior = json.loads((pathlib.Path(json.loads(generation_owner.read_text())["output"]) / "state.json").read_text())
        if prior["phase"] != "completed":
            raise ValueError("generation operation must be reconciled before starting a workload")
    owner_path = root / ".akasha-case-owner.json"
    if owner_path.exists():
        owner = json.loads(owner_path.read_text())
        if owner["output"] != str(directory):
            prior = json.loads((pathlib.Path(owner["output"]) / "state.json").read_text())
            if prior["phase"] not in {"completed", "abandoned"}:
                raise ValueError("previous case must be reconciled or explicitly abandoned: " + owner["run_id"])
    observer = pathlib.Path(__file__).resolve().parents[1] / "measure.py"
    identity = {"config": config, "binary_sha256": digest(binary), "fixture_sha256": digest(fixture),
                "fixture_vector_sha256": vector_sha256,
                "manifest_sha256": digest(manifest) if manifest else None, "database": str(root),
                "controller_sha256": digest(__file__), "observer_sha256": digest(observer)}
    state_path = directory / "state.json"
    if directory.exists():
        if not args.resume and not args.abandon:
            raise ValueError("case already exists; use explicit --resume to reconcile")
        state = json.loads(state_path.read_text())
        if state["identity"] != identity:
            raise ValueError("immutable case identity changed")
        if state["phase"] in {"completed", "abandoned"}:
            for name, expected in state["result"].get("artifact_sha256", {}).items():
                if digest(directory / name) != expected:
                    raise ValueError("completed case artifact changed: " + name)
            print(json.dumps(state["result"]))
            return 0 if state["result"]["status"] == "passed" else 1
        actual = process_identity(state["controller"]["pid"])
        if actual and actual["state"] != "Z" and actual["start_ticks"] == state["controller"]["start_ticks"]:
            raise ValueError("case controller is still alive; no duplicate owner")
        unit = subprocess.run(["systemctl", "--user", "show", state["unit"], "-p", "ActiveState", "--value"], check=True, capture_output=True, text=True).stdout.strip()
        if unit not in {"inactive", "failed"}:
            raise ValueError("owned unit has not terminated: " + unit)
        try:
            result = summarize(directory, run["profile"], identity["binary_sha256"])
            if result["effects"] == "unknown":
                raise ValueError("engine outcome incomplete")
        except (FileNotFoundError, ValueError):
            if args.abandon != run["id"]:
                raise ValueError("interrupted case is ambiguous; explicitly --abandon its exact run ID before later cases") from None
            state.update(phase="abandoned", result=state.get("result", {}) | {"status": "failed", "effects": "unknown", "reason": "explicit interrupted-case abandonment"})
            publish(state_path, state)
            return 1
    else:
        if args.resume or args.abandon:
            raise ValueError("cannot reconcile a nonexistent case")
        if disk is not None and shutil.disk_usage(root).free < disk["start_bytes"]:
            raise ValueError("case disk reservation cannot be admitted")
        directory.mkdir(mode=0o700)
        case_id = uuid.uuid4().hex
        state = {"phase": "admitted", "runtime_id": case_id, "identity": identity,
                 "controller": process_identity(os.getpid()), "unit": "akasha-case-" + case_id,
                 "config_epoch": 1, "admitted_unix_s": time.time()}
        publish(state_path, state)
        publish(owner_path, {"run_id": run["id"], "output": str(directory), "runtime_id": case_id})
        request = {"run_id": run["id"], "phase": run["phase"], "seconds": run["seconds"], "diagnostic": run["diagnostic"],
                   "fixture": str(fixture), "corpus_manifest": str(manifest) if manifest else None,
                   "output": str(directory / "runtime")}
        if "generation" in run:
            request["generation"] = run["generation"]
        publish(directory / "request.json", request)
        limit = (640 if run["profile"] == "low" else 2048) * 1024**2
        command = ["systemd-run", "--user", "--wait", "--pipe", "--collect", "--unit=" + state["unit"],
                   "-p", f"MemoryMax={limit}", "-p", "MemorySwapMax=0", "-p", f"RuntimeMaxSec={run['seconds'] + 900}",
                   "-p", "Environment=AKASHA_SPIKE_PROFILE=" + run["profile"],
                   *(["-p", "Environment=AKASHA_SPIKE_DISK_RESERVE_BYTES="+str(disk["reserve_bytes"])] if disk is not None else []),
                   sys.executable, str(observer), str(directory / "memory.jsonl"),
                   str(binary), str(root), "--workload", str(directory / "request.json")]
        state["command"] = command
        publish(state_path, state)
        with (directory / "engine.log").open("x") as output:
            terminal = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT)
        try:
            result = summarize(directory, run["profile"], identity["binary_sha256"])
        except (FileNotFoundError, ValueError) as error:
            result = {"status": "failed", "reason": str(error), "effects": "unknown"}
        result["unit_exit_code"] = terminal.returncode
        if terminal.returncode:
            result["status"] = "failed"
    state.update(phase="interrupted" if result.get("effects") == "unknown" else "completed", result=result)
    publish(state_path, state)
    print(json.dumps(result))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
