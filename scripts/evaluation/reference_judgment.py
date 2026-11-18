"""Aggregate one anonymized arm of human reference adjudications against the entire frozen suite."""

import argparse
import hashlib
import json
from pathlib import Path

from reference_suite import plan, read_reference


def required_key(item):
    return item["memory_id"], item["revision"]


def task_score(task, record):
    """Keep failures, missing evidence and missing tasks distinct from zero denominators."""
    result = {
        "task": task["id"],
        "state": "missing",
        "credited": 0,
        "credited_items": [],
        "irrelevant_propositions": [],
        "required": len(task["oracle"]["required_items"]),
        "propositions": 0,
        "irrelevant": 0,
        "critical": task["critical"],
        "reasons": [],
    }
    if record is None:
        return result
    required = (
        "anonymous_answer_id",
        "public_answer",
        "public_answer_sha256",
        "subject_sha256",
        "typed_status",
        "gaps",
        "terminal_received",
        "episode_import_verified",
        "all_returned_items_checked",
        "invariant_violations",
        "factual_memory_uses",
        "credited_items",
        "propositions",
        "reason",
    )
    if (
        any(key not in record for key in required)
        or record["terminal_received"] is not True
        or record["episode_import_verified"] is not True
        or record["all_returned_items_checked"] is not True
        or not isinstance(record["reason"], str)
        or not record["reason"].strip()
    ):
        result.update(
            state="incomplete",
            reasons=[
                "missing verified import, terminal or complete human adjudication"
            ],
        )
        return result
    if (
        not isinstance(record["anonymous_answer_id"], str)
        or not record["anonymous_answer_id"]
        or any(k in record for k in ["arm", "strategy"])
    ):
        raise ValueError(
            "blind adjudications require an opaque answer ID without strategy labels"
        )
    answer = record["public_answer"]
    if (
        not isinstance(answer, str)
        or record["public_answer_sha256"] != hashlib.sha256(answer.encode()).hexdigest()
        or record["subject_sha256"] != plan(task)["subject_sha256"]
    ):
        raise ValueError("public answer or frozen subject identity mismatch")
    for field in [
        "gaps",
        "invariant_violations",
        "factual_memory_uses",
        "credited_items",
        "propositions",
    ]:
        if not isinstance(record[field], list):
            raise TypeError(f"expected a list for {field}")
    expected = task["oracle"]
    reasons = list(record["invariant_violations"])
    if record["typed_status"] != expected["status"] or set(record["gaps"]) != set(
        expected["gaps"]
    ):
        reasons.append("status_or_gap_mismatch")
    if expected["status"] == "error" and (
        record.get("error_code") != expected["error_code"]
        or record.get("successful_receipt") is not False
    ):
        reasons.append("denial_or_receipt_mismatch")
    forbidden = set(expected["forbidden_memory_ids"])
    revisions = {required_key(x) for x in expected.get("forbidden_revisions", [])}
    memories = {(m["id"], m["revision"]): m for m in task["memories"]}
    allowed_sources = set(task["caller"]["allowed_event_ids"])
    factual = set()
    for item in record["factual_memory_uses"]:
        key = required_key(item)
        if key in factual:
            raise ValueError("duplicate factual memory use")
        factual.add(key)
        if item["memory_id"] in forbidden or key in revisions:
            reasons.append("prohibited_factual_memory_use")
        memory = memories.get(key)
        if memory is None:
            reasons.append("unrecognized_memory_revision")
        elif (
            item.get("authority") != memory["authority"]
            or item.get("scope") != memory["scope"]
            or set(item.get("source_ids", [])) != set(memory["source_ids"])
            or not set(memory["source_ids"]) <= allowed_sources
            or (
                task["retrieval_mode"] == "current_state"
                and memory["status"] == "retired"
            )
        ):
            reasons.append("factual_memory_provenance_mismatch")
    items = {required_key(item): item for item in expected["required_items"]}
    seen = set()
    for item in record["credited_items"]:
        key = required_key(item)
        if key in seen or key not in items:
            raise ValueError("duplicate or unknown required item")
        seen.add(key)
        if (
            not item.get("reason")
            or not item.get("excerpt")
            or item["excerpt"] not in answer
        ):
            raise ValueError(
                "every item needs a human reason and a literal published-answer excerpt"
            )
        reference = items[key]
        if (
            item.get("claim") == reference["claim"]
            and item.get("authority") == reference["authority"]
            and set(item.get("conditions", [])) == set(reference["conditions"])
            and set(item.get("source_ids", [])) == set(reference["source_ids"])
        ):
            result["credited"] += 1
            result["credited_items"].append({"memory_id": key[0], "revision": key[1]})
    credited = {required_key(k) for k in result["credited_items"]}
    if not credited <= factual:
        raise ValueError("credited item missing from factual use audit")
    covered = set()
    seen = set()
    for prop in record["propositions"]:
        if (
            not prop.get("text")
            or not prop.get("reason")
            or not prop.get("excerpt")
            or prop["excerpt"] not in answer
            or type(prop.get("relevant")) is not bool
        ):
            raise ValueError(
                "each substantive proposition needs one human classification and a published-answer excerpt"
            )
        ident = prop["text"].strip().casefold()
        if ident in seen:
            raise ValueError("duplicate atomic proposition")
        seen.add(ident)
        result["propositions"] += 1
        matches = {required_key(k) for k in prop.get("satisfies", [])}
        if not matches <= credited or bool(matches) != prop["relevant"]:
            raise ValueError(
                "proposition relevance must agree with supported required items"
            )
        covered.update(matches)
        if not prop["relevant"]:
            result["irrelevant"] += 1
            result["irrelevant_propositions"].append(prop["text"])
    if covered != credited:
        raise ValueError("credited items lack adjudicated substantive propositions")
    result.update(state="adjudicated", reasons=reasons, human_reason=record["reason"])
    return result


def score(tasks, records):
    expected = {t["id"] for t in tasks}
    if set(records) - expected:
        raise ValueError("unknown task")
    ids = [
        r.get("anonymous_answer_id")
        for r in records.values()
        if r.get("anonymous_answer_id")
    ]
    if len(ids) != len(set(ids)):
        raise ValueError("duplicate blind answer ID")
    rows = [task_score(task, records.get(task["id"])) for task in tasks]
    missing = [r["task"] for r in rows if r["state"] != "adjudicated"]
    violations = [
        r["task"] for r in rows if r["reasons"] and r["state"] == "adjudicated"
    ]
    crit = [
        r["task"]
        for r in rows
        if r["critical"]
        and (
            r["state"] != "adjudicated"
            or r["credited"] != r["required"]
            or r["reasons"]
        )
    ]
    non = [r for r in rows if not r["critical"]]
    earned = sum(r["credited"] for r in non)
    required = sum(r["required"] for r in non)
    dubious = sum(r["irrelevant"] for r in rows)
    total = sum(r["propositions"] for r in rows)
    acceptance = (
        "incomplete"
        if missing
        else "failed"
        if violations or crit or earned * 10 < required * 9 or dubious * 10 > total
        else "passed"
    )
    pair_reports = {}
    for task, row in zip(tasks, rows, strict=True):
        if pair := task["intent_pair"]:
            src = records.get(task["id"]) or {}
            pair_reports.setdefault(pair, []).append(
                {
                    "task": task["id"],
                    "variant": task["variant"],
                    "state": row["state"],
                    "required": [
                        {"memory_id": item["memory_id"], "revision": item["revision"]}
                        for item in task["oracle"]["required_items"]
                    ],
                    "credited": row["credited_items"],
                    "irrelevant_propositions": row["irrelevant_propositions"],
                    "failures": row["reasons"],
                    "public_answer": src.get("public_answer"),
                }
            )
    return {
        "acceptance": acceptance,
        "scope": "Human-adjudicated quality of one anonymous arm with operator-verified imports and terminals. Host acceptance and cost evidence are separate.",
        "missing_or_incomplete": missing,
        "invariant_failures": violations,
        "critical_not_satisfied": crit,
        "noncritical_items": {"earned": earned, "frozen_denominator": required},
        "irrelevance": {
            "irrelevant": dubious,
            "substantive_propositions": total,
            "zero_denominator_fraction": 0 if total == 0 else None,
        },
        "intent_pairs": pair_reports,
        "items": rows,
    }


def worksheet(tasks):
    """Private coordinator pairs actual terminal artifacts to opaque slots before blind review."""
    return {
        t["id"]: {
            "anonymous_answer_id": None,
            "subject_sha256": plan(t)["subject_sha256"],
            "public_answer": None,
            "public_answer_sha256": None,
            "typed_status": None,
            "gaps": [],
            "error_code": None,
            "successful_receipt": None,
            "terminal_received": False,
            "episode_import_verified": False,
            "all_returned_items_checked": False,
            "invariant_violations": [],
            "factual_memory_uses": [],
            "credited_items": [],
            "propositions": [],
            "reason": None,
        }
        for t in tasks
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "judgments",
        nargs="?",
        type=Path,
        help="trusted blind mappings keyed by frozen task ID for one anonymous cohort",
    )
    parser.add_argument(
        "--worksheet",
        action="store_true",
        help="prepare blanks without strategy labels for a coordinator",
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    _, tasks = read_reference()
    if args.worksheet == bool(args.judgments):
        parser.error("choose a judgments file or --worksheet")
    combined = (
        worksheet(tasks)
        if args.worksheet
        else score(tasks, json.loads(args.judgments.read_bytes()))
    )
    with args.output.open("x") as stream:
        json.dump(combined, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    if args.worksheet:
        return 0
    print(combined["acceptance"])
    return int(combined["acceptance"] != "passed")


if __name__ == "__main__":
    raise SystemExit(main())
