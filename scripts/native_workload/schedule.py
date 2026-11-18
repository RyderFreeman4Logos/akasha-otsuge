"""Fixed-rate client offers with durable admission; no replay of uncertain effects."""

import asyncio
import collections
import math
import time

ROLES = (
    (60, "original_write", 5),
    (12, "current", 5),
    (12, "lexical", 10),
    (4, "hybrid_vector", 16),
    (4, "scoped_history", 16),
    (8, "correction", 20),
)


def clients():
    result = []
    for count, kind, period in ROLES:
        for index in range(count):
            group, groups = (index // 2, count // 2) if kind == "correction" else (index, count)
            result.append(
                dict(
                    client=len(result), kind=kind, period_s=period, offset_s=period * group / groups
                )
            )
    return result


def offers(client, seconds):
    ordinal = 0
    while client["offset_s"] + ordinal * client["period_s"] < seconds:
        yield ordinal, client["offset_s"] + ordinal * client["period_s"]
        ordinal += 1


async def run_schedule(run_id, seconds, execute, emit, selected=None):
    """emit must fsync admission before returning; execute performs one logical operation.

    The caller owns immutable source/config/input admission and resource sampling.
    An interrupted continuous interval cannot be resumed into a passing interval.
    """
    selected = clients() if selected is None else selected
    if not run_id or seconds <= 0 or not selected:
        raise ValueError("nonempty run and positive duration required")
    if len({c["client"] for c in selected}) != len(selected):
        raise ValueError("duplicate client namespace")
    for client in selected:
        if client["period_s"] <= 0 or not 0 <= client["offset_s"] < client["period_s"]:
            raise ValueError("invalid offered schedule")
    loop = asyncio.get_running_loop()
    start = loop.time() + 0.05
    end = start + seconds
    await emit(
        dict(
            event="interval",
            run_id=run_id,
            started_unix_s=time.time() + 0.05,
            started_monotonic_s=start,
            duration_s=seconds,
            clients=selected,
            planned=sum(sum(1 for _ in offers(c, seconds)) for c in selected),
        )
    )

    async def client_loop(client):
        active = None
        quarantined = False

        async def perform(identity, ordinal, due):
            nonlocal quarantined
            dispatched = loop.time()
            if dispatched >= min(end, due + client["period_s"]):
                await emit(
                    dict(event="not_dispatched", identity=identity, reason="admission_expired")
                )
                return
            try:
                outcome = await execute(client, ordinal, identity)
                if outcome.get("status") not in ("success", "conflict", "error"):
                    raise ValueError("invalid operation outcome")
            except Exception as error:
                outcome = dict(
                    status="error",
                    code=type(error).__name__,
                    detail=str(error)[:2000],
                    quarantine=True,
                    effect_may_have_started=True,
                )
            finished = loop.time()
            quarantined |= bool(outcome.get("quarantine"))
            await emit(
                dict(
                    event="outcome",
                    identity=identity,
                    client=client["client"],
                    kind=client["kind"],
                    ordinal=ordinal,
                    outcome=outcome,
                    due_s=due - start,
                    dispatch_s=dispatched - start,
                    finished_s=finished - start,
                    latency_s=finished - dispatched,
                    offered_latency_s=finished - due,
                    within_interval=finished < end,
                )
            )

        async with asyncio.TaskGroup() as running:
            for ordinal, offset in offers(client, seconds):
                due = start + offset
                await asyncio.sleep(max(0, due - loop.time()))
                identity = f"{run_id}/client/{client['client']}/request/{ordinal}"
                await emit(
                    dict(
                        event="offered",
                        identity=identity,
                        client=client["client"],
                        kind=client["kind"],
                        ordinal=ordinal,
                        due_s=offset,
                    )
                )
                if active is not None and active.done():
                    active.result()
                reason = (
                    "quarantined"
                    if quarantined
                    else "client_busy"
                    if active is not None and not active.done()
                    else "missed_slot"
                    if loop.time() >= min(end, due + client["period_s"])
                    else None
                )
                if reason is not None:
                    await emit(dict(event="not_dispatched", identity=identity, reason=reason))
                    continue
                await emit(
                    dict(
                        event="admitted",
                        identity=identity,
                        client=client["client"],
                        kind=client["kind"],
                        ordinal=ordinal,
                    )
                )
                active = running.create_task(perform(identity, ordinal, due))
        return client["client"]

    async with asyncio.TaskGroup() as group:
        for client in selected:
            group.create_task(client_loop(client))
        await asyncio.sleep(max(0, end - loop.time()))
    await emit(
        dict(
            event="drained",
            elapsed_s=loop.time() - start,
            continuous_interval_s=seconds,
            native_acceptance=False,
        )
    )


def summarize(events, seconds):
    """Fail closed on missing, duplicated or reordered logical request records."""
    if seconds <= 0:
        raise ValueError("positive duration required")
    pending, tallies = {}, collections.defaultdict(collections.Counter)
    interval, drained = None, False
    latencies, offered_latencies = collections.defaultdict(list), collections.defaultdict(list)
    for row in events:
        event = row["event"]
        if event == "interval":
            if interval is not None or pending or row["duration_s"] != seconds:
                raise ValueError("invalid interval identity")
            interval = row
            continue
        if interval is None or drained:
            raise ValueError("request outside its interval journal")
        if event == "drained":
            if row["continuous_interval_s"] != seconds or row["elapsed_s"] < seconds:
                raise ValueError("interval ended early")
            drained = True
            continue
        identity = row["identity"]
        if event == "offered":
            if identity in pending:
                raise ValueError("duplicate offer")
            pending[identity] = dict(kind=row["kind"], state="offered")
            tallies[row["kind"]]["offered"] += 1
            continue
        if identity not in pending:
            raise ValueError("outcome without offer")
        item = pending[identity]
        kind, state = item["kind"], item["state"]
        if event == "admitted" and state == "offered":
            item["state"] = "admitted"
            tallies[kind]["admitted"] += 1
        elif event == "not_dispatched" and state == "offered":
            if row["reason"] not in ("quarantined", "client_busy", "missed_slot"):
                raise ValueError("unknown skipped-offer reason")
            item["state"] = "terminal"
            tallies[kind][row["reason"]] += 1
        elif (
            event == "not_dispatched"
            and state == "admitted"
            and row["reason"] == "admission_expired"
        ):
            item["state"] = "terminal"
            tallies[kind]["admission_expired"] += 1
        elif event == "outcome" and state == "admitted":
            tallies[kind]["dispatched"] += 1
            item["state"] = "terminal"
            outcome = row["outcome"]
            status = outcome["status"]
            if status not in ("success", "conflict", "error"):
                raise ValueError("invalid outcome status")
            due, dispatched, finished = (row[k] for k in ("due_s", "dispatch_s", "finished_s"))
            if not (
                0 <= due <= dispatched <= finished
                and math.isclose(row["latency_s"], finished - dispatched, abs_tol=1e-8)
                and math.isclose(row["offered_latency_s"], finished - due, abs_tol=1e-8)
            ):
                raise ValueError("invalid latency observation")
            tallies[kind][status] += 1
            if status == "success":
                latencies[kind].append(row["latency_s"])
                offered_latencies[kind].append(row["offered_latency_s"])
                # Recompute the window boundary; never trust the supplied flag alone.
                within = 0 <= row["finished_s"] < seconds
                if within != row["within_interval"]:
                    raise ValueError("completion window mismatch")
                tallies[kind]["success_within_interval" if within else "late_success"] += 1
        else:
            raise ValueError("duplicate or reordered request event")
    unfinished = {identity: row for identity, row in pending.items() if row["state"] != "terminal"}
    result = {}
    for kind, counts in tallies.items():

        def percentiles(values):
            values = sorted(values)
            return {
                str(p): values[math.ceil(len(values) * p / 100) - 1] if values else None
                for p in (50, 95, 99)
            }

        result[kind] = dict(
            counts=counts,
            successful_per_s=counts["success_within_interval"] / seconds,
            success_latency_s=percentiles(latencies[kind]),
            success_offered_latency_s=percentiles(offered_latencies[kind]),
        )
    planned = None if interval is None else interval["planned"]
    return dict(
        kinds=result,
        unfinished=unfinished,
        complete=bool(interval) and drained and not unfinished and len(pending) == planned,
        full_native_acceptance=False,
    )
