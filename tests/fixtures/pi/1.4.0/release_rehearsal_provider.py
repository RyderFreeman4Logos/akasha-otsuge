"""Deterministic Pi protocol peer for the candidate rehearsal, without account access."""

import io
import json
import uuid

from check_linux_package import MESSAGE, Endpoint


class RehearsalProvider(Endpoint):
    def completion(self, answer=None, tool=None, arguments=None):
        ident = uuid.uuid4().hex
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()

        def send(delta, finish=None):
            data = {
                "id": ident,
                "object": "chat.completion.chunk",
                "model": "fixture",
                "created": 1,
                "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
            }
            self.wfile.write(b"data: " + json.dumps(data).encode() + b"\n\n")

        send({"role": "assistant"})
        if tool:
            send(
                {
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": ident,
                            "type": "function",
                            "function": {
                                "name": tool,
                                "arguments": json.dumps(arguments),
                            },
                        }
                    ]
                }
            )
        else:
            send({"content": answer})
        send({}, "tool_calls" if tool else "stop")
        self.wfile.write(b"data: [DONE]\n\n")
        self.close_connection = True

    def do_POST(self):
        fixture = self.server.fixture
        raw = self.rfile.read(int(self.headers["Content-Length"]))
        req = json.loads(raw)
        messages = req["messages"]
        self.rfile = io.BytesIO(raw)
        if req["model"] == "fixture-extractor":
            if fixture.hold.is_set():
                fixture.arrived.set()
                if not fixture.release.wait(30):
                    raise TimeoutError("extractor reload barrier")
            return super().do_POST()
        texts = []
        for m in messages:
            content = m.get("content")
            if isinstance(content, str):
                texts.append(content)
            elif isinstance(content, list):
                texts.extend(b["text"] for b in content if "text" in b)
        grill = next(
            (t for t in texts if "auto-grill-v1:" in t and "\nInput: " in t), None
        )
        if grill:
            data, _ = json.JSONDecoder().raw_decode(grill.split("\nInput: ", 1)[1])
            assert data["evidence"][0]["text"] == MESSAGE
            role = data["stage"]["role"]
            result = {"schema_version": 1, "questions": [], "answers": []}
            if role == "interviewer":
                result["questions"] = [
                    {
                        "id": "offline",
                        "text": "How should lookup work offline?",
                        "depends_on": [],
                    }
                ]
            else:
                assert (
                    role in ("expert", "reviewer")
                    and data["questions"][0]["id"] == "offline"
                )
                result["answers"] = [
                    {
                        "question": "offline",
                        "kind": "procedure",
                        "claim": MESSAGE,
                        "rationale": "The retained user message gives the offline fallback.",
                        "strongest_opposition": "The message defines only the offline case.",
                        "conditions": ["when offline"],
                        "assumptions": [],
                        "unresolved": [],
                        "sources": [{"source": 0, "start": 0, "end": len(MESSAGE)}],
                        "resolved": True,
                    }
                ]
            fixture.roles.append(role)
            return self.completion(answer=json.dumps(result))
        names = {t["function"]["name"] for t in req.get("tools", [])}
        if any("context_describe.investigation" in text for text in texts):
            seen = [json.loads(m["content"]) for m in messages if m["role"] == "tool"]
            if not seen:
                return self.completion(tool="context_describe", arguments={})
            desc = seen[0]["value"]["investigation"]
            handles = [desc["outcome"]] + [lg["handle"] for lg in desc["logs"]]
            reads = seen[1:]
            if len(reads) < len(handles):
                return self.completion(
                    tool="context_read",
                    arguments={
                        "handle": handles[len(reads)],
                        "start": 0,
                        "max_bytes": 16384,
                    },
                )
            pages = [v["value"] for v in reads]
            assert all(v["end"] == v["total_bytes"] for v in pages)
            assert any("obsolete_lookup" in v["text"] for v in pages)
            fixture.incident_handles = handles
            sources = [s for v in pages for s in v["sources"]]
            report = {
                "classification": "insufficient_evidence",
                "reason": "Compiler stderr names obsolete_lookup. The logs alone do not establish that the recalled memory caused the failure.",
                "patch": None,
            }
            return self.completion(
                answer=json.dumps(
                    {
                        "status": "incomplete",
                        "answer": json.dumps(report),
                        "sources": sources,
                        "memories": [],
                        "conditions": [],
                        "conflicts": [],
                        "gaps": ["memory_causality_unverified"],
                    }
                )
            )
        if fixture.recall_adopted and "memory_search" in names:
            seen = [
                json.loads(m["content"])["value"]
                for m in messages
                if m["role"] == "tool"
            ]
            if not seen:
                return self.completion(
                    tool="memory_search", arguments={"query": "keyword"}
                )
            # Use the returned extraction revision. A Grill conclusion may also match.
            item = next(
                x["memory"]
                for x in seen[0]["items"]
                if x["memory"]["revision"]["key"]["id"].startswith("extracted/")
            )
            ref = {
                "memory_id": item["revision"]["key"]["id"],
                "revision": item["revision"]["commit_seq"],
            }
            if len(seen) == 1:
                return self.completion(tool="memory_get", arguments=ref)
            assert len(seen) == 2 and not seen[-1].get("truncated")
            draft = item["record"]["draft"]
            assert draft["claim"] == MESSAGE
            return self.completion(
                answer=json.dumps(
                    {
                        "status": "found",
                        "answer": draft["claim"],
                        "sources": draft["source_spans"],
                        "memories": [ref],
                        "conditions": draft["conditions"],
                        "conflicts": [],
                        "gaps": [],
                    }
                )
            )
        if "akasha_consult" in names:
            last = max(i for i, m in enumerate(messages) if m["role"] == "user")
            user = json.dumps(messages[last]["content"])
            if "AFTER-COMPACTION" in user:
                if any(m["role"] == "tool" for m in messages[last + 1 :]):
                    return self.completion(answer="Consult submitted.")
                return self.completion(
                    tool="akasha_consult",
                    arguments={"hint": "offline lookup"} if "narrow" in user else {},
                )
        return super().do_POST()
