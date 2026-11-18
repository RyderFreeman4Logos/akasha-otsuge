"""Native operation bindings for the fixed offered schedule; no model simulation."""

import asyncio
import copy
import hashlib
import json
import os
import pathlib
import time

from .ipc import TransportError
from .operations import OperationError


def encoded(value):
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode()


def body(identity):
    prefix = "Synthetic workload source, excluded from corpus size: " + identity + "\n"
    if len(prefix.encode()) >= 4096:
        raise ValueError("request identity too long")
    remaining = 4096 - len(prefix.encode())
    return prefix + hashlib.shake_256(identity.encode()).hexdigest(2048)[:remaining]


class NativeWorkload:
    def __init__(self, root, run_id, plan, operations, prepare):
        self.root, self.run_id, self.plan = pathlib.Path(root), run_id, plan
        self.operations, self.prepare = operations, prepare
        self.pairs, self.failed_pairs = {}, set()
        if len(plan["corrections"]) != 4 or len({p["id"] for p in plan["corrections"]}) != 4:
            raise ValueError("four distinct correction targets required")
        for kind in ("current", "lexical", "hybrid_vector", "scoped_history"):
            if not plan[kind]:
                raise ValueError("missing native operation oracle: " + kind)
        self.transcripts = self.root / "transcripts"
        self.transcripts.mkdir(mode=0o700)
        directory = os.open(self.root, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)

    async def original(self, client, ordinal, identity):
        source = dict(
            adapter="fixture",
            profile="native-workload",
            session=self.run_id,
            thread=f"client-{client}",
            agent="benchmark",
        )
        scope = dict(
            user=f"uid:{os.geteuid()}",
            project=f"workload/{self.run_id}/{client}",
            branch_or_version=None,
            session=None,
        )
        binding = dict(source=source, scope=scope, stream="events", coverage="available_payloads")
        text = body(identity)
        raw = text.encode()
        event = dict(
            event_id=f"original-{ordinal}",
            source_version="native-workload-v1",
            runtime_instance=self.run_id,
            branch="main",
            parent=None,
            fork_cutoff=None,
            event_time_ms=None,
            observed_time_ms=int(time.time() * 1000),
            visibility="tool",
            kind=dict(kind="execution"),
            text=text,
        )
        frame = encoded(dict(schema_version=1, event=event, raw_hex=raw.hex())) + b"\n"
        path = self.transcripts / f"client-{client:03d}.jsonl"
        oracle = dict(
            source=source,
            scope=scope,
            stream="events",
            event_id=event["event_id"],
            raw_sha256=hashlib.sha256(raw).hexdigest(),
            text_sha256=hashlib.sha256(raw).hexdigest(),
        )
        await self.prepare(
            dict(
                identity=identity,
                kind="original_write",
                oracle=oracle,
                frame_sha256=hashlib.sha256(frame).hexdigest(),
                path=str(path),
            )
        )

        def append():
            fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "ab") as out:
                out.write(frame)
                out.flush()
                os.fsync(out.fileno())
            directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)

        await asyncio.to_thread(append)
        job = await self.operations.original_write(path, binding, identity)
        duplicate = False
        if ordinal % 10 == 9:
            replay = await self.operations.original_write(path, binding, identity)
            if replay != job:
                raise OperationError("CAPTURE_DUPLICATE_CHANGED")
            duplicate = True
        return dict(
            status="success",
            receipt=job,
            original_oracle=oracle,
            unique_original_bytes=4096,
            duplicate_reused_exact_receipt=duplicate,
        )

    async def correction(self, client, ordinal, identity):
        pair = (client - 92) // 2
        if pair in self.failed_pairs:
            return dict(status="error", code="PAIR_QUARANTINED", quarantine=True)
        key = (pair, ordinal)
        if key not in self.pairs:

            async def snapshot():
                target = self.plan["corrections"][pair]
                value = await self.operations.request(
                    dict(op="memory_show", id=target["id"], at=None),
                    f"{self.run_id}/pair/{pair}/round/{ordinal}/current",
                )
                state = value["data"]
                if state["state"] != "active":
                    raise OperationError("CORRECTION_TARGET_INACTIVE")
                memory = state["revision"]["memory"]
                if (
                    memory["revision"]["key"]["id"] != target["id"]
                    or memory["record"]["draft"]["scope"] != target["scope"]
                ):
                    raise OperationError("CORRECTION_TARGET_MISMATCH")
                return memory

            self.pairs[key] = asyncio.create_task(snapshot())
        memory = await self.pairs[key]
        draft = copy.deepcopy(memory["record"]["draft"])
        draft["claim"] = body(identity)
        command = dict(
            op="patch_apply",
            idempotency_key=identity,
            patch=dict(
                scope=draft["scope"],
                changes=[
                    dict(
                        memory_id=memory["revision"]["key"]["id"],
                        expected_revision=memory["revision"]["commit_seq"],
                        draft=draft,
                    )
                ],
                reason="Synthetic workload correction; no human approval asserted.",
                evidence=draft["source_spans"],
                expires_at_ms=int(time.time() * 1000) + 900000,
            ),
        )
        await self.prepare(
            dict(identity=identity, kind="correction", pair=pair, ordinal=ordinal, command=command)
        )
        receipt = await self.operations.correction(command, identity)
        if receipt["commit_seq"] <= memory["revision"]["commit_seq"]:
            raise OperationError("CORRECTION_SEQUENCE_MISMATCH")
        expected = dict(
            id=memory["revision"]["key"]["id"],
            revision=receipt["commit_seq"],
            scope=draft["scope"],
            claim_sha256=hashlib.sha256(draft["claim"].encode()).hexdigest(),
            draft_sha256=hashlib.sha256(encoded(draft)).hexdigest(),
        )
        verified = await self.operations.current(
            expected, identity + "/verify", at=receipt["commit_seq"]
        )
        return dict(
            status="success", receipt=receipt, expected=expected, verified=verified, pair=pair
        )

    async def __call__(self, client, ordinal, identity):
        kind = client["kind"]
        number = client["client"]
        try:
            if kind == "original_write":
                return await self.original(number, ordinal, identity)
            if kind == "correction":
                return await self.correction(number, ordinal, identity)
            options = self.plan[kind]
            expected = options[(number + ordinal) % len(options)]
            await self.prepare(
                dict(
                    identity=identity,
                    kind=kind,
                    oracle_sha256=hashlib.sha256(encoded(expected)).hexdigest(),
                )
            )
            if kind == "current":
                result = await self.operations.current(expected, identity)
            elif kind == "scoped_history":
                result = await self.operations.scoped_history(expected, identity)
            elif kind in ("lexical", "hybrid_vector"):
                expected_mode = "keyword" if kind == "lexical" else "hybrid"
                if expected["mode"] != expected_mode:
                    raise ValueError("retrieval mode differs from admitted role")
                result = await self.operations.search(expected, identity)
            else:
                raise ValueError("unknown native operation")
            return dict(status="success", receipt=result)
        except OperationError as error:
            if kind == "correction" and error.code == "REVISION_CONFLICT":
                return dict(status="conflict", code=error.code, detail=error.detail)
            # A failed write can have an accepted event or an applied revision.
            quarantine = kind in ("original_write", "correction")
            if kind == "correction":
                self.failed_pairs.add((number - 92) // 2)
            return dict(
                status="error",
                code=error.code,
                detail=error.detail,
                quarantine=quarantine,
                effect_may_have_started=quarantine,
            )
        except TransportError as error:
            if kind == "correction":
                self.failed_pairs.add((number - 92) // 2)
            return dict(
                status="error",
                code="TRANSPORT_" + error.phase.upper(),
                cause=error.cause,
                quarantine=True,
                effect_may_have_started=error.effect_may_have_started,
            )
        except Exception:
            if kind == "correction":
                self.failed_pairs.add((number - 92) // 2)
            raise
