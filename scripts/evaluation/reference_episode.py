"""Prepare one registered current-state episode before opening an account or model turn."""

import gzip
import json
import subprocess
from pathlib import Path

from reference_suite import BASE, canonical, file_hash, input_only, sha


class EpisodeUnavailable(ValueError):
    """A registered episode cannot yet be supplied through both equivalent native tools."""


def withdrawn_revisions(case, native):
    sources = {s["id"]: s for s in case["sources"]}
    revisions = {(m["id"], m.get("commit")): m for m in native["memories"]}
    withdrawn = {}
    for change in native["transitions"]:
        source = sources.get(change.get("original_id"), {})
        prior = revisions.get(
            (change.get("memory_id"), change.get("previous_commit")), {}
        )
        if (
            change.get("operation") == source.get("kind") == "withdrawal"
            and source.get("channel") == "verified_human"
            and change.get("original_sha256") == source["sha256"]
            and prior.get("frozen_status") == "retired"
            and change.get("action") == "reject"
            and change.get("revision_after") in {"rejected", "retired"}
            and change["current_after"]["state"] in {"absent", "retired"}
        ):
            withdrawn[(prior["id"], prior["ordinal"])] = source["id"]
    return withdrawn


def prepare(task, args, private, result):
    case = input_only(task)
    caller = case["caller"]
    result.update(task_id=case["id"], registered_input_sha256=sha(canonical(case)))
    area = private / "authority"
    area.mkdir(mode=0o700)
    path = area / "episode.json"
    path.write_bytes(canonical(case))
    archive = (
        Path(__file__).resolve().parents[2]
        / "docs/benchmarks/results/mvp027/offline-transitions-r1.json.gz"
    )
    archive_bytes = archive.read_bytes()
    archive_hash = sha(archive_bytes)
    if (
        archive_hash
        != "6f7b587eeba1862ed7670d856baf9d3484f40a6cbaadca31718b248202ea826f"
    ):
        raise ValueError("frozen native import archive hash mismatch")
    rows = json.loads(gzip.decompress(archive_bytes))["rows"]
    row = next(r for r in rows if r["id"] == case["id"])
    if row["subject_sha256"] != result["registered_input_sha256"]:
        raise ValueError("frozen registered episode hash mismatch")
    native = row["native"]
    refused = [
        {"id": m["id"], "revision": m["ordinal"], "error": m["error"]}
        for m in native["memories"]
        if "error" in m
    ]
    result["prior_import"] = {
        "archive_sha256": archive_hash,
        "observer_stdout_sha256": native["observer_stdout_sha256"],
        "refused_revisions": refused,
    }
    flags = ["unavailable registered Memory revisions"] if refused else []
    if case["retrieval_mode"] != "current_state" or case.get("as_of") is not None:
        flags.append("historical snapshot binding")
    if case.get("changes_during_run"):
        flags.append("live change and publication barrier")
    missing = case.get("missing_event_ids", [])
    if missing and (
        len(missing) != 1
        or case["sources"]
        or case["memories"]
        or native["missing"] != missing[0]
    ):
        flags.append("unavailable historical originals")
    withdrawals = withdrawn_revisions(case, native)
    sources = case["sources"]
    if (
        caller["thread"] != "root"
        or caller.get("ancestry")
        or {s["id"] for s in sources} != set(caller["allowed_event_ids"])
        or any(
            s["thread"] != "root"
            or (s.get("kind") and s["id"] not in withdrawals.values())
            or not s["available"]
            or s["scope"] != caller["scope"]
            or s["sequence"] > caller["fence_sequence"]
            for s in sources
        )
    ):
        flags.append("caller lineage or source fence")
    if any(
        m["scope"] != caller["scope"]
        or (
            m["status"] != "active"
            and not (
                m["status"] == "retired" and (m["id"], m["revision"]) in withdrawals
            )
        )
        for m in case["memories"]
    ):
        flags.append("non-current or cross-scope Memory")
    if flags:
        raise EpisodeUnavailable(", ".join(flags))
    if not (sources or case["memories"] or missing):
        return case, area
    if args.importer is None or args.import_build is None:
        raise EpisodeUnavailable(
            "a pinned reference-import binary and its build receipt are required"
        )
    build = json.loads(args.import_build.read_bytes())
    if not (
        build["base"] == build["compiled_library_commit"] == BASE
        and build["source_sha256"]
        == file_hash(Path(__file__).with_name("reference_import.rs"))
        and build["binary_sha256"] == file_hash(args.importer)
    ):
        raise ValueError("native importer provenance mismatch")
    proc = subprocess.run(
        [
            "unshare",
            "--user",
            "--map-root-user",
            "--net",
            "--",
            str(args.importer.resolve()),
            str(path),
            str(area),
        ],
        capture_output=True,
        check=False,
        timeout=120,
        env={
            "HOME": str(private),
            "TMPDIR": str(private),
            "PATH": "/usr/bin:/bin",
            "RAYON_NUM_THREADS": "2",
            "TOKIO_WORKER_THREADS": "2",
        },
    )
    result["native_import"] = {
        "binary_sha256": build["binary_sha256"],
        "source_sha256": build["source_sha256"],
        "stdout_sha256": sha(proc.stdout),
        "stderr_sha256": sha(proc.stderr),
        "exit_code": proc.returncode,
    }
    if proc.returncode:
        raise RuntimeError("native importer exited unsuccessfully")
    observed = json.loads(proc.stdout)
    if (
        observed["id"] != case["id"]
        or observed["scope"] != caller["scope"]
        or observed["logical_fence"] != caller["fence_sequence"]
        or set(observed["native_visible_ids"]) != set(caller["allowed_event_ids"])
        or observed["missing"] != (missing[0] if missing else None)
    ):
        raise EpisodeUnavailable("native scope or capture fence did not match")
    failures = [
        {"id": m["id"], "revision": m["ordinal"], "error": m["error"]}
        for m in observed["memories"]
        if "error" in m
    ]
    result["native_import"]["refused_revisions"] = failures
    if failures:
        raise EpisodeUnavailable("native Memory import refused registered revisions")
    registered = {(m["id"], m["revision"], m["authority"]) for m in case["memories"]}
    materialized = {
        (m["id"], m["ordinal"], m["authority"]) for m in observed["memories"]
    }
    if (
        registered != materialized
        or sorted(native["native_state_ids"]) != sorted(observed["native_state_ids"])
        or set(observed["native_state_ids"])
        != {m["id"] for m in case["memories"] if m["status"] == "active"}
        or withdrawn_revisions(case, observed) != withdrawals
        or any(s["scope_read"] != "allowed" for s in observed["sources"])
    ):
        raise EpisodeUnavailable("native authority or visible state did not match")
    seed = {
        k: observed[k]
        for k in (
            "id",
            "grant",
            "sources",
            "memories",
            "native_visible_ids",
            "logical_fence",
            "capture_seq",
            "missing",
        )
    }
    (area / "seed.json").write_bytes(canonical(seed))
    result["native_import"].update(
        grant_commit=seed["grant"],
        capture_commit=seed["capture_seq"],
        retained_originals=len(seed["sources"]),
        missing_parent=seed["missing"],
        active_revisions=len(observed["native_state_ids"]),
        withdrawn_revisions=len(withdrawals),
    )
    return case, area
