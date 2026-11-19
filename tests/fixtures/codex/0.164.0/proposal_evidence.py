"""Deterministic worker for the bounded Consult proposal regression."""
import json


def investigate(root, tool, send, supports_proposals):
    query = (root / "proposal-query").read_text()
    observations = {"supports_proposals": supports_proposals}

    def search(phase):
        if not supports_proposals:
            observations[phase] = {"handles": []}
            return []
        response = tool("memory_search", {"query": query, "proposals": True})
        proposals = response["value"].get("proposals", [])
        observations[phase] = {
            "kind": response["kind"],
            "error": response["value"].get("code"),
            "handles": proposals,
        }
        return proposals

    regular = tool("memory_search", {"query": query})
    observations["adopted"] = [item["memory"]["revision"]["key"]["id"]
                               for item in regular["value"]["items"]]
    search("unread")
    matches = tool("context_search", {"query": query, "limit": 16})["value"]["events"]
    (original,) = [event for event in matches if event["visibility"] == "tool"]
    handle = original["handle"]
    first = tool("context_read", {"handle": handle, "start": 0, "max_bytes": 1})["value"]
    search("partial")
    rest = tool("context_read", {"handle": handle, "start": 1, "max_bytes": 4096})["value"]
    observations["original"] = {"sha256": rest["sha256"], "end": rest["end"],
                                "total_bytes": rest["total_bytes"]}
    assert first["end"] == rest["start"] == 1 and rest["end"] == rest["total_bytes"]
    hits = search("complete")
    whole = tool("context_read", {"handle": handle, "start": 0, "max_bytes": 4096})["value"]
    memories, conditions = [], []
    answer = "The original is available."
    for item in hits:
        reference = {k: item[k] for k in ("memory_id", "revision")}
        proposal = tool("memory_get", reference)["value"]["proposal"]
        assert proposal["authority"] == proposal["lifecycle"] == "candidate"
        observations["expanded_origin"] = proposal["origin"]
        assert proposal["sources"] == whole["sources"]
        memories.append(reference)
        conditions.extend(proposal["conditions"])
        answer = proposal["claim"]
    (root / "proposal-observed.json").write_text(json.dumps(observations))
    conclusion = dict(status="found", answer=answer, sources=whole["sources"],
                      memories=memories, conditions=conditions, conflicts=[], gaps=[])
    send({"method": "turn/completed", "params": {"threadId": "thread", "turn": {
        "id": "turn", "status": "completed", "error": None, "items": [{
            "type": "agentMessage", "id": "answer", "phase": "final_answer",
            "text": json.dumps(conclusion)}]}}})
