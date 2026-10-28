"""Validate the frozen reference inputs; this is not a model quality evaluation."""
import json
import hashlib
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "docs/evaluation/reference-v1"
# First freeze commit 7356d17f0a373ade3520e6e1a1249fc52e17ba8d. Add errata in a new set; never reseal v1.
FROZEN_MANIFEST_SHA256 = "6b6dffd7882dc703e393686d327f8aaae6becad1f5566e761b8cae8f4fad6ac7"


def validate_tasks(tasks):
    require(len(tasks) == 60 and len({t["id"] for t in tasks}) == 60, "60 unique frozen tasks required")
    pairs = defaultdict(list)
    categories = {"decision", "taste", "negation", "scope", "version", "branch", "no_hit", "correction", "intent"}
    require(categories <= {t["category"] for t in tasks}, "missing required category")
    for task in tasks:
        label = task["id"]
        require(task["schema_version"] == 1 and bool(task["current_task"].strip()), label + ": task schema")
        require(type(task["critical"]) is bool, label + ": critical flag")
        if task["category"] in {"decision", "negation"}:
            require(task["critical"], label + ": decision/negation cannot be downgraded")
        hint = task["hint"]
        require(hint is None or isinstance(hint, str) and 0 < len(hint.strip()) <= 512, label + ": hint")
        require((task["variant"] == "no_hint" and hint is None) or
                (task["variant"] == "short_hint" and hint is not None), label + ": hint variant")
        sources = {s["id"]: s for s in task["sources"]}
        require(len(sources) == len(task["sources"]), label + ": duplicate source")
        for source in sources.values():
            require(source["sha256"] == digest(source["text"].encode()), label + ": source hash")
            require(type(source["sequence"]) is int and source["sequence"] > 0, label + ": source sequence")
            require(source["channel"] in {"verified_human", "tool"}, label + ": channel")
            require(set(source.get("original_event_ids", [])) <= sources.keys(), label + ": missing original")
            require(source.get("parent_event_id") in (None, *sources), label + ": missing parent")
        caller = task["caller"]
        allowed = set(caller["allowed_event_ids"])
        require(allowed <= sources.keys(), label + ": unknown allowed source")
        cutoffs = {a["thread"]: a["cutoff_sequence"] for a in caller.get("ancestry", [])}
        for key in allowed:
            source = sources[key]
            require(source["available"] and source["sequence"] <= caller["fence_sequence"], label + ": unavailable/future source")
            require(covers(source["scope"], caller["scope"]), label + ": source scope widened")
            require(source["thread"] == caller["thread"] or source["sequence"] <= cutoffs.get(source["thread"], 0),
                    label + ": branch cutoff widened")
        memories = {(m["id"], m["revision"]): m for m in task["memories"]}
        require(len(memories) == len(task["memories"]), label + ": duplicate memory revision")
        for memory in memories.values():
            require(memory["source_ids"] and set(memory["source_ids"]) <= sources.keys(), label + ": unsourced memory")
        oracle = task["oracle"]
        require(oracle["status"] in {"found", "no_hit", "incomplete"}, label + ": result status")
        require(bool(oracle["required_items"]) == (oracle["status"] == "found"), label + ": result count")
        require(bool(oracle["gaps"]) == (oracle["status"] == "incomplete"), label + ": history gap")
        require(task["retrieval_mode"] in {"current_state", "historical_state"}, label + ": retrieval mode")
        historical = task["retrieval_mode"] == "historical_state"
        require((type(task["as_of"]) is int and task["as_of"] > 0) if historical else task["as_of"] is None,
                label + ": historical point")
        forbidden = set(oracle["forbidden_memory_ids"])
        irrelevant = set(oracle.get("irrelevant_memory_ids", []))
        require(forbidden | irrelevant <= {m[0] for m in memories}, label + ": unknown excluded memory")
        forbidden_revisions = {(m["memory_id"], m["revision"]) for m in oracle.get("forbidden_revisions", [])}
        require(forbidden_revisions <= memories.keys(), label + ": unknown excluded revision")
        required_keys = set()
        for item in oracle["required_items"]:
            key = (item["memory_id"], item["revision"])
            require(key in memories and key not in required_keys, label + ": required revision missing/duplicate")
            required_keys.add(key)
            memory = memories[key]
            require(key[0] not in forbidden | irrelevant and key not in forbidden_revisions, label + ": contradictory oracle")
            for field in ("claim", "conditions", "authority", "source_ids"):
                require(item[field] == memory[field], label + ": changed required " + field)
            require(set(item["source_ids"]) <= allowed and covers(memory["scope"], caller["scope"]), label + ": required scope")
            text = "\n".join(sources[s]["text"] for s in item["source_ids"])
            require(all(part in text for part in [item["claim"], *item["conditions"]]), label + ": unsupported oracle text")
            if item["authority"] == "human_confirmed":
                require(all(sources[s]["channel"] == "verified_human" for s in item["source_ids"]), label + ": false human approval")
            if historical:
                require(all(sources[s]["sequence"] <= task["as_of"] for s in item["source_ids"]), label + ": future historical source")
            else:
                require(memory["status"] in {"active", "candidate"}, label + ": retired current memory")
        for change in task.get("changes_during_run", []):
            require(change["at"] == "after_first_source_read_before_output" and
                    change["operation"] in {"revoke_read_grant", "rescope_memory"} and change["memory_id"] in forbidden,
                    label + ": live change must forbid output")
        if task["intent_pair"]:
            pairs[task["intent_pair"]].append(task)
    require(len(pairs) == 10, "ten intent pairs required")
    for pair in pairs.values():
        require(len(pair) == 2 and {t["variant"] for t in pair} == {"no_hint", "short_hint"}, "intent pair variants")
        for field in ("caller", "sources", "memories", "current_task", "retrieval_mode", "as_of"):
            require(pair[0][field] == pair[1][field], "hint cannot change identity, sources or permission")
        require(pair[0]["oracle"]["required_items"] != pair[1]["oracle"]["required_items"], "hint must distinguish intent")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def covers(scope, target):
    return all(scope.get(key) is None or scope[key] == target.get(key)
               for key in ("user", "project", "branch_or_version", "session"))


def validate_snapshot(root=ROOT):
    raw = (root / "manifest.json").read_bytes()
    require(digest(raw) == FROZEN_MANIFEST_SHA256, "reference-v1 differs from its committed freeze; use a separate regression/errata set")
    manifest = json.loads(raw)
    require(manifest["schema_version"] == 1 and set(manifest["files"]) == {"tasks.jsonl", "profile.json", "README.md"}, "manifest schema")
    for name, expected in manifest["files"].items():
        require(digest((root / name).read_bytes()) == expected, "frozen hash mismatch: " + name)
    tasks = [json.loads(line) for line in (root / "tasks.jsonl").read_text().splitlines()]
    validate_tasks(tasks)
    require(manifest["task_ids"] == [t["id"] for t in tasks], "frozen task membership")
    require(manifest["critical_ids"] == [t["id"] for t in tasks if t["critical"]], "frozen critical membership")
    require(manifest["categories"] == dict(Counter(t["category"] for t in tasks)), "category counts")
    return {"tasks": len(tasks), "critical_tasks": len(manifest["critical_ids"]), "intent_pairs": 10,
            "manifest_sha256": digest((root / "manifest.json").read_bytes()), "model_evaluation": "not_run"}


if __name__ == "__main__":
    print(json.dumps(validate_snapshot(), sort_keys=True))
