"""Real IPC workload operations; callers own durable admission and scheduling."""

import asyncio
import hashlib
import json
import os
import time

from .ipc import TransportError, call


def digest(value):
    return hashlib.sha256(value).hexdigest()


def rule_applies(rule_scope, query_scope):
    """Match the native Scope::contains constraints for a rule and its query."""
    fields = ("project", "branch_or_version", "session")
    return (
        rule_scope["user"] == query_scope["user"]
        and (rule_scope.get("branch_or_version") is None or rule_scope.get("project") is not None)
        and all(rule_scope.get(k) is None or rule_scope[k] == query_scope.get(k) for k in fields)
    )


class OperationError(Exception):
    def __init__(self, code, detail=None):
        self.code, self.detail = code, detail
        super().__init__(code)


class NativeOperations:
    def __init__(self, socket_path, request_timeout_s=10, job_timeout_s=600):
        if request_timeout_s <= 0 or job_timeout_s <= 0:
            raise ValueError("positive timeouts required")
        self.socket = socket_path
        self.request_timeout_s = request_timeout_s
        self.job_timeout_s = job_timeout_s

    async def request(self, command, request_id):
        reply = await call(
            self.socket,
            command,
            request_id,
            int(time.time() * 1000 + self.request_timeout_s * 1000),
            self.request_timeout_s,
        )
        if "error" in reply:
            raise OperationError(reply["error"]["code"], reply["error"])
        return reply["data"]

    async def job(self, command, identity):
        if command["idempotency_key"] != identity:
            raise ValueError("job identity must equal admitted idempotency key")
        deadline = asyncio.get_running_loop().time() + self.job_timeout_s
        submitted = False
        try:
            async with asyncio.timeout_at(deadline):
                job = await self.request(command, identity + "/submit")
                submitted = True
                polls = 0
                while True:
                    if job.get("id") != identity:
                        raise OperationError("JOB_ID_MISMATCH")
                    if job["state"] in ("succeeded", "failed", "cancelled"):
                        break
                    await asyncio.sleep(0.05)
                    polls += 1
                    job = await self.request(
                        dict(op="job_show", id=identity), identity + "/observe/" + str(polls)
                    )
        except (OperationError, TransportError) as error:
            if not submitted and not getattr(error, "effect_may_have_started", False):
                raise
            code = (
                error.code
                if isinstance(error, OperationError)
                else "TRANSPORT_" + error.phase.upper()
            )
            raise OperationError(
                code,
                dict(
                    job_id=identity,
                    effect_may_have_started=True,
                    cause=getattr(error, "detail", str(error)),
                ),
            ) from error
        except TimeoutError as error:
            raise OperationError(
                "JOB_COMPLETION_TIMEOUT", dict(job_id=identity, effect_may_have_started=True)
            ) from error
        # A delayed event loop can deliver an already-ready reply after its deadline.
        if asyncio.get_running_loop().time() >= deadline:
            raise OperationError(
                "JOB_COMPLETION_TIMEOUT", dict(job_id=identity, effect_may_have_started=True)
            )
        if job["state"] != "succeeded":
            raise OperationError(
                "JOB_" + job["state"].upper(), dict(job_id=identity, error=job.get("error"))
            )
        return job

    async def original_write(self, path, binding, identity):
        with path.open("rb") as source:
            before = os.fstat(source.fileno())
            first = source.read(4096)
            source.seek(max(0, before.st_size - 4096))
            last = source.read(4096)
        if before.st_size == 0:
            raise ValueError("capture file is empty")
        job = await self.job(
            dict(op="reconcile", binding=binding, path=str(path), idempotency_key=identity),
            identity,
        )
        after = path.stat()
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) != (
            after.st_dev,
            after.st_ino,
            after.st_size,
            after.st_mtime_ns,
        ):
            raise OperationError("CAPTURE_INPUT_CHANGED")
        checkpoint = job["result"]["checkpoint"]["checkpoint"]
        cursor = json.loads(checkpoint["cursor"])
        expected = dict(
            offset=before.st_size, prefix_sha256=digest(first), tail_sha256=digest(last)
        )
        if (
            checkpoint["pending"] is not None
            or checkpoint["loss"]
            or any(cursor.get(k) != v for k, v in expected.items())
        ):
            raise OperationError("CAPTURE_CURSOR_MISMATCH")
        return job

    async def current(self, expected, identity, at=None):
        value = await self.request(dict(op="memory_show", id=expected["id"], at=at), identity)
        state = value["data"]
        if state["state"] != "active":
            raise OperationError("CURRENT_STATE_MISMATCH")
        self.check_memory(state["revision"]["memory"], expected)
        return dict(read_seq=value["read_seq"], revision=expected["revision"])

    @staticmethod
    def check_memory(memory, expected):
        revision, draft = memory["revision"], memory["record"]["draft"]
        if (
            revision["key"]["id"] != expected["id"]
            or revision["commit_seq"] != expected["revision"]
            or draft["scope"] != expected["scope"]
            or digest(draft["claim"].encode()) != expected["claim_sha256"]
        ):
            raise OperationError("MEMORY_ORACLE_MISMATCH", dict(id=expected["id"]))
        if (
            "draft_sha256" in expected
            and digest(
                json.dumps(
                    draft, sort_keys=True, ensure_ascii=False, separators=(",", ":")
                ).encode()
            )
            != expected["draft_sha256"]
        ):
            raise OperationError("MEMORY_DRAFT_MISMATCH", dict(id=expected["id"]))

    async def scoped_history(self, expected, identity):
        members = expected["members"]
        if len(members) != 100 or any(
            key != row["id"] or row["scope"] != expected["scope"] for key, row in members.items()
        ):
            raise ValueError("history requires exactly 100 admitted members of one scope")
        cursor, seen, cursors, lengths = None, set(), set(), []
        loop = asyncio.get_running_loop()
        deadline = loop.time() + self.job_timeout_s
        while True:
            try:
                async with asyncio.timeout_at(deadline):
                    value = await self.request(
                        dict(
                            op="state_show",
                            scope=expected["scope"],
                            at=expected["at"],
                            cursor=cursor,
                        ),
                        identity + "/page/" + str(len(lengths)),
                    )
            except TimeoutError as error:
                raise OperationError("HISTORY_COMPLETION_TIMEOUT") from error
            page = value["data"]
            if value["read_seq"] != expected["at"] or value["retrieval_mode"] != "historical_state":
                raise OperationError("HISTORY_SNAPSHOT_MISMATCH")
            items, examined = page["items"], page["examined"]
            if (
                not isinstance(items, list)
                or type(examined) is not int
                or not len(items) <= examined <= 32
                or (examined == 0 and page["next"] is not None)
            ):
                raise OperationError("HISTORY_PAGE_BOUND")
            for item in items:
                memory = item["memory"]
                key = memory["revision"]["key"]["id"]
                if key not in members or key in seen:
                    raise OperationError("HISTORY_MEMBERSHIP_MISMATCH")
                self.check_memory(memory, members[key])
                seen.add(key)
            lengths.append(len(items))
            if loop.time() >= deadline:
                raise OperationError("HISTORY_COMPLETION_TIMEOUT")
            cursor = page["next"]
            if cursor is None:
                break
            token = json.dumps(cursor, sort_keys=True)
            if token in cursors:
                raise OperationError("HISTORY_CURSOR_LOOP")
            cursors.add(token)
        if seen != set(members):
            raise OperationError("HISTORY_MEMBERSHIP_MISMATCH")
        return dict(read_seq=expected["at"], members=len(seen), page_lengths=lengths)

    async def search(self, expected, identity):
        if expected["mode"] not in ("keyword", "hybrid") or expected["top_k"] != 20:
            raise ValueError("admitted workload requires native top-20 lexical or hybrid retrieval")
        if not 1 <= len(expected["required_refs"]) <= 20:
            raise ValueError("search requires admitted positive references")
        required = set(map(tuple, expected["required_refs"]))
        forbidden = set(expected.get("excluded_ids", []))
        rule_oracle = {}
        for rule in expected.get("expected_rules", []):
            key = rule["id"]
            if (
                key in rule_oracle
                or key in forbidden
                or type(rule["revision"]) is not int
                or rule["revision"] <= 0
                or not rule_applies(rule["scope"], expected["scope"])
            ):
                raise ValueError("invalid search rule oracle: " + key)
            rule_oracle[key] = (rule["revision"], rule["scope"])
        job = await self.job(
            dict(
                op="retrieval_run",
                plan=dict(kind="search", query=expected["query"], scope=expected["scope"]),
                idempotency_key=identity,
            ),
            identity,
        )
        output = json.loads(job["result"]["output"])
        if output["mode"] != expected["mode"]:
            raise OperationError("SEARCH_MODE_MISMATCH", dict(actual=output["mode"]))
        if len(output["items"]) + len(output.get("rules", [])) > expected["top_k"]:
            raise OperationError("SEARCH_TOP_K_MISMATCH")
        if output["scope"] != expected["scope"] or output["query"] != expected["query"]:
            raise OperationError("SEARCH_AUTHORITY_OR_SCOPE_MISMATCH")
        seen_ids, scored, rules = set(), set(), set()
        for group, references in (("items", scored), ("rules", rules)):
            for item in output[group]:
                memory = item["memory"]
                revision, record = memory["revision"], memory["record"]
                key, seq = revision["key"]["id"], revision["commit_seq"]
                scope = record["draft"]["scope"]
                if key in seen_ids or key in forbidden or type(seq) is not int or seq <= 0:
                    raise OperationError("SEARCH_AUTHORITY_OR_SCOPE_MISMATCH")
                seen_ids.add(key)
                if group == "items":
                    allowed = scope == expected["scope"] and key not in rule_oracle
                else:
                    if "expected_rules" in expected:
                        allowed = rule_oracle.get(key) == (seq, scope)
                    else:
                        allowed = scope == expected["scope"] and (key, seq) in required
                    allowed = (
                        allowed
                        and record["authority"] == "human_confirmed"
                        and record["draft"]["kind"] in ("decision", "preference", "procedure")
                    )
                if not allowed:
                    raise OperationError("SEARCH_AUTHORITY_OR_SCOPE_MISMATCH")
                references.add((key, seq))
        if "expected_rules" in expected and {key for key, _ in rules} != rule_oracle.keys():
            raise OperationError("SEARCH_REQUIRED_RULE_MISSING")
        positives = scored if "expected_rules" in expected else scored | rules
        if not required.issubset(positives):
            raise OperationError("SEARCH_REQUIRED_HIT_MISSING")
        return dict(
            job_id=identity,
            receipt_id=job["result"]["receipt_id"],
            mode=output["mode"],
            refs=sorted(scored | rules),
            coverage=output["coverage"],
            ann_relevance_measured=False,
        )

    async def correction(self, command, identity):
        if command["op"] != "patch_apply" or command["idempotency_key"] != identity:
            raise ValueError("patch must use its admitted identity")
        return await self.request(command, identity + "/apply")
