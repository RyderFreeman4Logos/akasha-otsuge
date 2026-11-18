"""Extract public, structural observations from existing pairs without opening a model."""

import argparse
import gzip
import json
from collections import Counter
from pathlib import Path

from reference_suite import (
    BASE,
    ERRATA_MANIFEST_SHA256,
    FROZEN_MANIFEST_SHA256,
    ROOT,
    canonical,
    file_hash,
    plan,
    read_reference,
    sha,
)

PUBLIC = ROOT / "docs/benchmarks/results/mvp027"
TIMERS = ("preparation_s", "setup_s", "model_turn_s", "total_s", "invocation_wall_s")
TOKENS = (
    "inputTokens",
    "cachedInputTokens",
    "outputTokens",
    "reasoningOutputTokens",
    "totalTokens",
)
TRACE = ("starts", "reads", "read_bytes", "output_bytes", "tokens")
STATUSES = {"found", "no_hit", "incomplete", "error"}
# Only registered gap labels and these published codes can enter the output.
CODES = {
    "INVALID_ARGUMENT",
    "INDEX_NOT_READY",
    "DELEGATION_UNAVAILABLE",
    "PROTOCOL_INVALID",
    "SOURCE_UNAVAILABLE",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def token_counts(raw):
    values = {k: raw[k] for k in TOKENS}
    require(
        all(type(n) is int and n >= 0 for n in values.values()), "invalid token count"
    )
    require(
        values["totalTokens"] == values["inputTokens"] + values["outputTokens"],
        "token total",
    )
    return values


def structural(task, publication, has_receipt):
    oracle = task["oracle"]
    expected_gaps = set(oracle["gaps"])
    gaps = set(publication.get("gaps", []))
    memories = publication.get("memories", [])
    ids = [m["reference"]["key"]["id"] for m in memories]
    required = {m["memory_id"]: m for m in oracle["required_items"]}
    source_ids = {s for m in required.values() for s in m["source_ids"]}
    # Native event IDs/revisions and frozen fixture IDs use distinct namespaces.
    # Only compare full registered bytes, never equate IDs on their content alone.
    required_spans = Counter(
        (s["sha256"], 0, len(s["text"].encode()))
        for s in task["sources"]
        if s["id"] in source_ids
    )
    actual_spans = Counter(
        (s["content_sha256"], s["start"], s["end"])
        for s in publication.get("evidence", [])
    )
    fields_differ = []
    for m in memories:
        mid = m["reference"]["key"]["id"]
        ref = required.get(mid)
        if ref and (
            m["authority"] != ref["authority"]
            or set(m["conditions"]) != set(ref["conditions"])
            or m["scope"] != task["caller"]["scope"]
        ):
            fields_differ.append(mid)
    status = publication["status"]
    require(status in STATUSES, "unrecognized publication status")
    error = (publication.get("error") or {}).get("code")
    return {
        "expected_status": oracle["status"],
        "published_status": status,
        "status_matches": status == oracle["status"],
        "gap_set_matches": gaps == expected_gaps if "gaps" in publication else None,
        "missing_registered_gaps": sorted(expected_gaps - gaps),
        "extra_known_codes": sorted((gaps - expected_gaps) & CODES),
        "other_extra_gaps": len(gaps - expected_gaps - CODES),
        "error": error
        if error is None or error in CODES or error == oracle.get("error_code")
        else "other",
        "error_matches": error == oracle.get("error_code"),
        "receipt_present": has_receipt,
        "receipt_matches": has_receipt == oracle["successful_receipt"]
        if "successful_receipt" in oracle
        else None,
        "missing_required_ids": sorted(required.keys() - set(ids)),
        "other_returned_ids": len(set(ids) - required.keys()),
        "forbidden_ids_returned": sorted(
            set(oracle["forbidden_memory_ids"]) & set(ids)
        ),
        "duplicate_ids": len(ids) - len(set(ids)),
        "authority_conditions_or_scope_differ": sorted(set(fields_differ)),
        "missing_full_source_spans": (required_spans - actual_spans).total(),
        "other_source_spans": (actual_spans - required_spans).total(),
    }


def direct_reads(task, reads):
    required = {
        (m["memory_id"], m["revision"]): m for m in task["oracle"]["required_items"]
    }
    sources = {s["id"]: s["sha256"] for s in task["sources"]}
    found, mismatches = set(), set()
    for read in reads:
        for item in read["response"]["items"]:
            key = (item["id"], item["revision"])
            found.add(key)
            ref = required.get(key)
            if ref and (
                item["authority"] != ref["authority"]
                or set(item["conditions"]) != set(ref["conditions"])
                or item["scope"] != task["caller"]["scope"]
                or {(s["id"], s["sha256"]) for s in item["sources"]}
                != {(i, sources[i]) for i in ref["source_ids"]}
            ):
                mismatches.add(key)
    return {
        "returned_per_query": [len(read["response"]["items"]) for read in reads],
        "required_revisions_unseen": sorted(required.keys() - found),
        "required_revision_metadata_differ": sorted(mismatches),
        "other_distinct_revisions": len(found - required.keys()),
    }


def terminal_fields(row, name):
    native = row["terminal"]["status"]
    require(native in {"completed", "failed", "interrupted"}, "unknown model terminal")
    expected = "NATIVE_" + name.upper() + "_TERMINAL_" + native.upper()
    require(
        row["status"] in {expected, "HARNESS_OR_HOST_FAILURE"},
        "unknown observer status",
    )
    exception = row.get("failure") or row.get("cleanup_failure")
    exception_class = exception.get("class") if exception else None
    return {
        "model_terminal": native,
        "driver_status": row["status"],
        "observer_exception": exception_class
        if exception_class in {None, "ProcessLookupError"}
        else "other",
    }


def arm(task, name, path, profile, profile_hash):
    raw = path.read_bytes()
    row = json.loads(raw)
    frozen = plan(task)
    require(
        row["task_id"] == task["id"]
        and row["strategy"] == name
        and row["attempt_kind"] == "model_arm"
        and row["model_turn_submitted"] is True
        and row["model_turn_accepted"] is True
        and row["git_source"] == BASE
        and row["reference_manifest_sha256"] == FROZEN_MANIFEST_SHA256
        and row["errata_manifest_sha256"] == ERRATA_MANIFEST_SHA256
        and row["profile_sha256"] == profile_hash
        and row["registered_input_sha256"]
        == row["episode_input_sha256"]
        == frozen["subject_sha256"]
        and row["task_utf8_sha256"] == frozen["main_request_sha256"]
        and row["binding"]["scope"] == frozen["scope"]
        and row["binding"]["thread"] == frozen["thread"]
        and row["host_policy"]["sandbox"] == "readOnly"
        and row["host_policy"]["tool_network"] is False
        and row["binding"]["logical_fence"] == frozen["logical_fence"]
        and row["codex_sha256"] == profile["installation_observed"]["native_sha256"]
        and row["resolved_model"]
        == {
            "model": profile["model"],
            "model_provider": "openai",
            "model_reasoning_effort": profile["reasoning_effort"],
        }
        and row["auth"]["type"] == "chatgpt"
        and row["incremental_billed_cost"] is None,
        "receipt does not match registered model arm",
    )
    out = {
        **terminal_fields(row, name),
        "private_receipt_sha256": sha(raw),
        "main_tokens": token_counts(row["reported_token_usage"]["total"]),
        "seconds": {
            k: round(row["timing"][k], 3) if k in row["timing"] else None
            for k in TIMERS
        },
    }
    if name == "direct":
        require(row["tools"] == ["memory_search"], "unexpected direct tool")
        out.update(
            main_tool_calls=len(row["reads"]), reads=direct_reads(task, row["reads"])
        )
    else:
        require(row["tools"] == ["akasha_consult"], "unexpected consult tool")
        t = row["consult_terminal"]
        require(
            row["queued"]["id"] == t["id"] == t["publication"]["job_id"]
            and row["invocation"]["binding"]["tool_call_id"]
            and row["invocation"]["logical_fence"] == frozen["logical_fence"],
            "unbound consult publication",
        )
        require(
            row["runner_policy"]["model"] == profile["model"]
            and row["runner_policy"]["reasoning"] == profile["reasoning_effort"]
            and row["runner_policy"]["provider"] == "openai",
            "external profile mismatch",
        )
        trace = t["run_trace"]
        require(
            t["job_state"] in {"succeeded", "failed", "cancelled"}, "unknown job state"
        )
        require(
            all(
                type(trace["usage"][k]) is int and trace["usage"][k] >= 0 for k in TRACE
            ),
            "invalid trace totals",
        )
        states = Counter(r["state"] for r in trace["runs"])
        require(
            set(states) <= {"succeeded", "failed", "cancelled"}
            and sum(states.values()) == trace["usage"]["starts"],
            "inconsistent runner states",
        )
        out.update(
            main_tool_calls=1,
            job_state=t["job_state"],
            configured_service_budget_mib=row["preparation"][
                "service_total_budget_mib"
            ],
            configured_job_limit=row["preparation"]["max_concurrent_jobs"],
            external_states=states,
            external_trace={k: trace["usage"][k] for k in TRACE},
            max_observed_depth=max((r["depth"] for r in trace["runs"]), default=None),
            result=structural(task, t["publication"], t["receipt"] is not None),
        )
    return row, out


def historical(task, profile_hash):
    path = PUBLIC / "h01-subscription-r1.json"
    x = json.loads(path.read_bytes())
    frozen = plan(task)
    shared = x["shared_input"]
    require(
        file_hash(path)
        == "eadfbb918a2cd861b1097f850f3829b198a6b30e3a8a7905c08cb214eee6ab61"
        and x["reference"]["profile_sha256"] == profile_hash
        and x["product_source_commit"] == BASE
        and shared["task_sha256"] == frozen["main_request_sha256"]
        and shared["hint"] == task["hint"]
        and shared["scope"] == frozen["scope"]
        and shared["thread"] == frozen["thread"]
        and shared["logical_fence"] == frozen["logical_fence"]
        and shared["retained_sources"] == frozen["source_count"]
        and shared["memories"] == frozen["memory_revision_count"],
        "published H01 does not match reference",
    )
    out = {
        "id": "H01",
        "public_receipt_sha256": file_hash(path),
        "source": "h01-subscription-r1.json",
    }
    for name in ("direct", "consult"):
        a = x[name]
        out[name] = {
            "private_receipt_sha256": a["private_result_sha256"],
            "model_terminal": a["main_turn_status"],
            "driver_status": None,
            "observer_exception": None,
            "seconds": {k: a["elapsed_seconds"].get(k) for k in TIMERS},
            "supervisor_wall_s": x["historical_accounting"][
                "model_arm_supervisor_wall"
            ][name]["seconds"],
            "main_tokens": token_counts(a["main_token_usage"]),
        }
    counts = x["direct"]["per_query_returned_item_counts"]
    require(
        not any(counts) and not task["oracle"]["required_items"],
        "H01 historical direct coverage changed",
    )
    out["direct"].update(
        main_tool_calls=len(counts),
        reads={
            "returned_per_query": counts,
            "required_revisions_unseen": [],
            "required_revision_metadata_differ": [],
            "other_distinct_revisions": 0,
        },
    )
    consult = x["consult"]
    out["consult"].update(
        main_tool_calls=1,
        job_state=None,
        external_states={consult["native_external"]["status"]: 1},
        external_trace={
            k: consult["native_external"]["run_trace_usage"][k] for k in TRACE
        },
        max_observed_depth=None,
        result=structural(task, consult["publication"], consult["receipt"] is not None),
    )
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("receipt_dir", type=Path)
    parser.add_argument("episodes", nargs="+")
    parser.add_argument("--with-public-h01", action="store_true")
    parser.add_argument(
        "--output", type=Path, required=True, help="new .json or .json.gz file"
    )
    args = parser.parse_args()
    manifest, tasks = read_reference()
    catalog = {t["id"]: t for t in tasks}
    require(
        set(args.episodes) <= catalog.keys()
        and len(set(args.episodes)) == len(args.episodes),
        "unknown or repeated task",
    )
    profile_hash = manifest["files"]["profile.json"]
    profile = json.loads(
        (ROOT / "docs/evaluation/reference-v1/profile.json").read_bytes()
    )
    rows = []
    if args.with_public_h01:
        require("H01" not in args.episodes, "duplicate H01")
        rows.append(historical(catalog["H01"], profile_hash))
    for task_id in sorted(args.episodes):
        task = catalog[task_id]
        row = {"id": task_id}
        inputs = []
        for name in ("direct", "consult"):
            path = args.receipt_dir / f"mvp027-real-{task_id}-{name}-v1.json"
            private, row[name] = arm(task, name, path, profile, profile_hash)
            if task["hint"] is not None:
                require(
                    private["hint_sha256"] == plan(task)["hint_sha256"]
                    and private["main_input_sha256"],
                    "registered hint not bound to main turn",
                )
                if name == "consult":
                    require(
                        private["queued"]["persisted_hint"] == task["hint"],
                        "registered hint missing from consult",
                    )
            inputs.append(
                tuple(
                    private.get(k)
                    for k in (
                        "episode_input_sha256",
                        "prompt_sha256",
                        "main_input_sha256",
                        "hint_sha256",
                        "config_sha256",
                        "sanitized_model_catalog_sha256",
                    )
                )
            )
        require(
            inputs[0] == inputs[1],
            "arms do not share registered input and main model settings",
        )
        rows.append(row)
    for row in rows:
        direct = row["direct"]["main_tokens"]
        consult = row["consult"]["main_tokens"]
        row["consult_minus_direct_main_tokens"] = {
            k: consult[k] - direct[k]
            for k in ("inputTokens", "outputTokens", "totalTokens")
        }
    report = {
        "schema_version": 1,
        "product_source": BASE,
        "profile_sha256": profile_hash,
        "frozen_manifest_sha256": FROZEN_MANIFEST_SHA256,
        "errata_manifest_sha256": ERRATA_MANIFEST_SHA256,
        "registered_budget": profile["budget"],
        "pairs": rows,
        "frozen_tasks": len(tasks),
        "blind_semantic_adjudication": "pending",
        "monetary_charge": None,
        "account_quota_delta": None,
    }
    data = canonical(report) + b"\n"
    if args.output.name.endswith(".json.gz"):
        data = gzip.compress(data, mtime=0)
    with args.output.open("xb") as f:
        f.write(data)


if __name__ == "__main__":
    main()
