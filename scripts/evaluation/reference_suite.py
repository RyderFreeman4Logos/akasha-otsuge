"""Plan and inspect the frozen 60 reference episodes without starting a model."""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from check_reference import (
    ERRATA_MANIFEST_SHA256,
    FROZEN_MANIFEST_SHA256,
    read_reference,
)

BASE = "a177d6e23568b627ec9eef593d88289bc8634041"
EPISODE_INPUT = (
    "id",
    "current_task",
    "hint",
    "caller",
    "sources",
    "memories",
    "retrieval_mode",
    "as_of",
    "changes_during_run",
    "missing_event_ids",
)
COST_FIELDS = (
    "main_input_tokens",
    "main_output_tokens",
    "main_cached_input_tokens",
    "main_added_input_tokens",
    "main_added_output_tokens",
    "external_input_tokens",
    "external_output_tokens",
    "external_cached_input_tokens",
    "external_reported_total_tokens",
    "model_calls",
    "further_reads",
    "tool_calls",
    "setup_seconds",
    "wall_seconds",
    "failed_attempt_seconds",
    "failed_attempt_tokens",
    "billed_amount",
    "currency",
)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def file_hash(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def canonical(data):
    return json.dumps(
        data, sort_keys=True, ensure_ascii=False, separators=(",", ":")
    ).encode()


def input_only(task):
    """Only these fields may travel to the trusted native import process."""
    return {key: task[key] for key in EPISODE_INPUT if key in task}


def plan(task):
    episode = input_only(task)
    oracle = task["oracle"]
    return {
        "id": task["id"],
        "subject_sha256": sha(canonical(episode)),
        "main_request_sha256": sha(episode["current_task"].encode()),
        "intent_shared_input_sha256": sha(
            canonical({k: v for k, v in episode.items() if k not in {"id", "hint"}})
        ),
        "hint_sha256": None
        if episode["hint"] is None
        else sha(episode["hint"].encode()),
        "scope": episode["caller"]["scope"],
        "thread": episode["caller"]["thread"],
        "logical_fence": episode["caller"]["fence_sequence"],
        "source_count": len(episode["sources"]),
        "memory_revision_count": len(episode["memories"]),
        "critical": task["critical"],
        "intent_pair": task["intent_pair"],
        "scoring": {
            "required_items": len(oracle["required_items"]),
            "expected_status": oracle["status"],
            "error_code": oracle.get("error_code"),
            "successful_receipt": oracle.get("successful_receipt"),
            "forbidden_memory_count": len(oracle["forbidden_memory_ids"]),
            "forbidden_revision_count": len(oracle.get("forbidden_revisions", [])),
        },
    }


def native_case(observer, scratch, episode):
    with tempfile.TemporaryDirectory(dir=scratch, prefix="reference-episode-") as temp:
        root = Path(temp)
        input_file = root / "episode.json"
        input_file.write_bytes(canonical(episode))
        try:
            p = subprocess.run(
                [str(observer), str(input_file), str(root)],
                capture_output=True,
                timeout=120,
                check=False,
                env={
                    "PATH": "/usr/bin:/bin",
                    "TMPDIR": temp,
                    "HOME": temp,
                    "RAYON_NUM_THREADS": "2",
                    "TOKIO_WORKER_THREADS": "2",
                },
            )
        except subprocess.TimeoutExpired as err:
            return {
                "native_error": {
                    "class": "timeout",
                    "deadline_seconds": err.timeout,
                    "stderr_sha256": sha(err.stderr or b""),
                    "message": "native case exceeded its deadline",
                }
            }
        if p.returncode:
            # Complete stderr is not used as reference data. Preserve a diagnostic for failed native imports.
            return {
                "native_error": {
                    "exit_code": p.returncode,
                    "stderr_sha256": sha(p.stderr),
                    "message": p.stderr.decode(errors="replace")[-1600:],
                }
            }
        observed = json.loads(p.stdout)
        if (
            observed["id"] != episode["id"]
            or observed["scope"] != episode["caller"]["scope"]
            or observed["current_task_sha256"] != sha(episode["current_task"].encode())
        ):
            raise ValueError("native task identity mismatch")
        return observed


def assess(task, actual):
    if "native_error" in actual:
        return ["native_import_failed"]
    pending = []
    errors = Counter(m["error"] for m in actual["memories"] if "error" in m)
    if errors:
        pending.append("native_authority_or_predecessor")
    if any(m["status"] == "retired" for m in task["memories"]):
        pending.append("retirement_or_withdrawal_replay")
    if task["retrieval_mode"] == "historical_state":
        pending.append("historical_as_of_commit")
    if task.get("changes_during_run"):
        if actual["grant_barrier"]:
            pending.append("model_tool_read_and_publication_barrier")
        else:
            pending.append("live_rescope_and_publication_barrier")
    if task.get("missing_event_ids"):
        pending.append("native_live_runner_history_gap")
    if actual["outside_lineage_keyword_candidates"]:
        pending.append("candidate_source_fence_at_publication")
    if task["memories"]:
        pending.append("frozen_memory_kind_mapping")
    if actual["id"] != "H01":
        pending.append("frozen_model_pair_adapter")
    return pending


def run(observer, build, scratch, tasks, keep_native):
    rows = []
    for task in tasks:
        row = plan(task)
        if observer:
            actual = native_case(observer, scratch, input_only(task))
            if "native_error" not in actual:
                visible = set(actual["native_visible_ids"])
                logical = {
                    (m["id"], m["ordinal"]): m
                    for m in actual["memories"]
                    if "commit" in m
                }
                outside = {
                    (m["id"], logical[(m["id"], m["revision"])]["commit"])
                    for m in task["memories"]
                    if (m["id"], m["revision"]) in logical
                    and not set(m["source_ids"]) <= visible
                }
                actual["outside_lineage_keyword_candidates"] = [
                    p for p in actual["index"]["candidate_ids"] if tuple(p) in outside
                ]
            row["native"] = (
                actual
                if keep_native or "native_error" in actual
                else {
                    "sources_reloaded": len(actual["sources"]),
                    "scope_reads": dict(
                        Counter(s["scope_read"] for s in actual["sources"])
                    ),
                    "captured_source_sha256": sha(canonical(actual["sources"])),
                    "native_visible_ids": actual["native_visible_ids"],
                    "missing": actual["missing"],
                    "granted_at": actual["grant"],
                    "capture_seq": actual["capture_seq"],
                    "memory_ordinals": actual["memories"],
                    "index": actual["index"],
                    "outside_lineage_keyword_candidates": actual[
                        "outside_lineage_keyword_candidates"
                    ],
                    "grant_barrier": actual["grant_barrier"],
                    "native_state_ids": actual["native_state_ids"],
                }
            )
            row["pending"] = assess(task, actual)
        rows.append(row)
        if observer:
            print(
                task["id"],
                "native_failed" if "native_error" in actual else "native_complete",
                flush=True,
            )
    summary = {
        "tasks": len(rows),
        "critical_tasks": sum(r["critical"] for r in rows),
        "critical_required_items": sum(
            r["scoring"]["required_items"] for r in rows if r["critical"]
        ),
        "noncritical_required_items": sum(
            r["scoring"]["required_items"] for r in rows if not r["critical"]
        ),
        "paired_intents": len({r["intent_pair"] for r in rows if r["intent_pair"]}),
        "status_histogram": dict(
            Counter(r["scoring"]["expected_status"] for r in rows)
        ),
        "sources": sum(r["source_count"] for r in rows),
        "memory_revisions": sum(r["memory_revision_count"] for r in rows),
        "native_cases": sum(
            "native" in r and "native_error" not in r["native"] for r in rows
        ),
        "native_failures": sum("native_error" in r.get("native", {}) for r in rows),
        "model_turns_in_this_command": 0,
    }
    return {
        "schema_version": 1,
        "reference": {
            "base_manifest_sha256": FROZEN_MANIFEST_SHA256,
            "errata_manifest_sha256": ERRATA_MANIFEST_SHA256,
            "native_product_commit": BASE,
        },
        "native_build": build,
        "data_path_scope": {
            "memory_kind_in_probe": "fact",
            "memory_kind_supplied_by_freeze": False,
            "initial_authority": "For a verified-human source backing an explicitly human-confirmed frozen Memory, the fixture supplies an explicit-statement ingress. Only production adoption or proposal can produce the stored authority.",
            "index": "An authorized fixture administrator prepares every imported row. Candidate IDs precede publication and may refer to partial state.",
            "lineage": "Native Capture lineage starts at the live request and uses explicit branch ancestry and cutoffs. The expected allowed IDs serve only as an assertion.",
            "model_arms": "This command runs no model. Both strategies must share subject_sha256 and use the original current_task; only a frozen consult hint may reach its tool.",
        },
        "summary": summary,
        "quality_contract": {
            "paired_identity": "Use one frozen subject hash and the same UTF-8 request in both arms. Keep caller, hint, ancestry and grants unchanged.",
            "blind_adjudication": "For every atomic proposition keep the opaque answer ID, required memory ID/revision, source IDs, conditions, authority, mapper reasoning and pass/fail. Ambiguous mappings receive no credit.",
            "invariants": "Each task and arm must satisfy scope, ancestry, live grant, source authority, current/historical validity and typed status. Any violation fails the gate.",
            "critical": "Every critical task must satisfy all items and denial oracles.",
            "noncritical": "Award one point only for a fully supported required item. Use the fixed noncritical denominator.",
            "irrelevance": "Pool distinct irrelevant substantive propositions over all substantive propositions, excluding status/gap diagnostics and greetings. Record each task's integer counts. Zero of zero is zero.",
            "thresholds": {
                "invariant_violations": 0,
                "critical_missing": 0,
                "noncritical_min_fraction": [9, 10],
                "irrelevant_max_fraction": [1, 10],
            },
            "missing_or_failed": "Preserve logical episode and both arms, every attempt and failure. A missing terminal or unresolved import prevents full-suite acceptance.",
            "attempt_template": {
                "arm": None,
                "subject_sha256": None,
                "attempt": None,
                "terminal": None,
                "failure_reason": None,
                "main_output": None,
                "permission_and_result_receipt": None,
                "cost": dict.fromkeys(COST_FIELDS),
                "cost_unavailable_reason": None,
            },
        },
        "rows": rows,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--task", action="append", help="optional exact task ID, repeatable"
    )
    parser.add_argument(
        "--observer", type=Path, help="optional native reference-import executable"
    )
    parser.add_argument("--build", type=Path, help="native observer build receipt")
    parser.add_argument(
        "--scratch",
        type=Path,
        help="private writable SSD parent for one disposable store at a time",
    )
    parser.add_argument(
        "--full-native",
        action="store_true",
        help="retain all logical-to-native source mappings",
    )
    args = parser.parse_args()
    if bool(args.observer) != bool(args.build) or bool(args.observer) != bool(
        args.scratch
    ):
        parser.error("native mode requires --observer, --build and --scratch together")
    if not __debug__:
        parser.error("integrity checks require Python without -O")
    os.umask(0o077)
    manifest, tasks = read_reference()
    if [t["id"] for t in tasks] != manifest["task_ids"]:
        raise ValueError("frozen task order mismatch")
    if args.task:
        selected = set(args.task)
        tasks = [t for t in tasks if t["id"] in selected]
        if len(tasks) != len(selected):
            parser.error("unknown frozen task ID")
    build = None
    if args.observer:
        build = json.loads(args.build.read_bytes())
        if (
            build["base"] != BASE
            or build["compiled_library_commit"] != BASE
            or build["binary_sha256"] != file_hash(args.observer)
            or build["source_sha256"]
            != file_hash(Path(__file__).with_name("reference_import.rs"))
        ):
            raise ValueError("native binary or source provenance mismatch")
        args.observer = args.observer.resolve(strict=True)
    if args.output.exists():
        parser.error("result already exists")
    result = run(args.observer, build, args.scratch, tasks, args.full_native)
    with args.output.open("x") as stream:
        json.dump(result, stream, indent=2, ensure_ascii=False)
        stream.write("\n")
    print(json.dumps(result["summary"], sort_keys=True))
    return int(result["summary"]["native_failures"] != 0)


if __name__ == "__main__":
    raise SystemExit(main())
