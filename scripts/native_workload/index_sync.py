"""Witness a write wholly within one native incremental index build."""

import asyncio
import hashlib
import time

from spikes.helix.run_case import publish

from .ipc import TransportError
from .operations import OperationError


async def observe(ops, run_id, root, start, seconds, check_daemon):
    job_id = run_id + "/concurrent-index-sync"
    show = {"op": "job_show", "id": job_id}
    status = {"op": "index_status", "include_queue": False}
    result = {"job_id": job_id, "first_build_sample": None, "last_build_sample": None}
    try:
        await asyncio.sleep(max(0, start + seconds / 2 - time.monotonic()))
        check_daemon()
        try:
            await ops.request(show, job_id + "/collision")
        except OperationError as error:
            if error.code != "JOB_NOT_FOUND":
                raise
        else:
            raise ValueError("concurrent index job ID already exists")
        before = await ops.request(dict(status, include_queue=True), job_id + "/before")
        epoch = before["build_epoch"]
        if type(epoch) is not int or epoch < 0 or epoch % 2:
            raise ValueError("concurrent index requires an idle builder at admission")
        result.update(
            previous_watermark=before["lexical_watermark"],
            previous_epoch=epoch,
            generation=before["generation"],
            offered_s=time.monotonic() - start,
        )
        if result["offered_s"] >= seconds:
            raise TimeoutError("interval ended before index admission")
        publish(root / "concurrent-index-intent.json", result)
        try:
            job = await ops.request(
                {
                    "op": "retrieval_run",
                    "plan": {"kind": "sync"},
                    "idempotency_key": job_id,
                },
                job_id + "/submit",
            )
        except TransportError as error:
            if not error.effect_may_have_started:
                raise
            result["submit_transport_error"] = str(error)
            job = await ops.request(show, job_id + "/recover")
        target = job["retrieval_target_seq"]
        if type(target) is not int or target < before["queue"]["storage_seq"]:
            raise ValueError("index target does not cover the observed authority head")
        result["target"] = target
        polls = 0
        async with asyncio.timeout_at(start + seconds + ops.job_timeout_s):
            while True:
                if (job["id"], job["retrieval_plan"], job["retrieval_target_seq"]) != (
                    job_id,
                    {"kind": "sync"},
                    target,
                ):
                    raise ValueError("concurrent index job changed")
                state = job["state"]
                if state in ("succeeded", "failed", "cancelled"):
                    terminal = root / "concurrent-index-terminal.json"
                    publish(terminal, job)
                    result.update(
                        terminal_state=state,
                        terminal_sha256=hashlib.sha256(
                            terminal.read_bytes()
                        ).hexdigest(),
                    )
                    if state != "succeeded":
                        raise RuntimeError(job.get("error") or state)
                if state in ("running", "succeeded"):
                    witness = [job["attempt"], job["started_at_ms"]]
                    if any(type(v) is not int or v < 1 for v in witness):
                        raise ValueError("invalid native index attempt")
                    if result.setdefault("attempt", witness) != witness:
                        raise ValueError("observed multiple index attempts")
                if state == "succeeded":
                    break
                if state == "running" and time.monotonic() < start + seconds:
                    sent = time.monotonic() - start
                    progress = await ops.request(status, f"{job_id}/build/{polls}")
                    sample = {
                        "sent_s": sent,
                        "received_s": time.monotonic() - start,
                        "epoch": progress["build_epoch"],
                    }
                    if sample["epoch"] == epoch + 1 and progress["building"] is True:
                        if result["first_build_sample"] is None:
                            result["first_build_sample"] = sample
                        else:
                            result["last_build_sample"] = sample
                await asyncio.sleep(0.25)
                polls += 1
                job = await ops.request(show, f"{job_id}/observe/{polls}")
        check_daemon()
        after = await ops.request(status, job_id + "/after")
        result.update(
            final_epoch=after["build_epoch"], final_watermark=after["lexical_watermark"]
        )
        if after["build_epoch"] != epoch + 2 or after["building"] is not False:
            raise ValueError("expected one completed native build on the same daemon")
        if (
            before["generation"] != after["generation"]
            or not before["lexical_watermark"] < target <= after["lexical_watermark"]
        ):
            raise ValueError(
                "sync did not advance the lexical watermark through its target"
            )
    except (
        OperationError,
        TransportError,
        OSError,
        KeyError,
        TypeError,
        ValueError,
        RuntimeError,
    ) as error:
        result["error"] = {"type": type(error).__name__, "message": str(error)[:500]}
    publish(root / "concurrent-index-observation.json", result)
    return result


def completed_writes(observation, events):
    counts = {"original_write": 0, "correction": 0}
    first = {}
    begin, end = observation["first_build_sample"], observation["last_build_sample"]
    if begin and end and "error" not in observation:
        for row in events:
            if (
                row["event"] == "outcome"
                and row["kind"] in counts
                and row["outcome"]["status"] == "success"
                and row["within_interval"]
                and begin["received_s"]
                <= row["dispatch_s"]
                <= row["finished_s"]
                <= end["sent_s"]
            ):
                counts[row["kind"]] += 1
                first.setdefault(row["kind"], row["identity"])
    return {"counts": counts, "first_request_by_kind": first}
