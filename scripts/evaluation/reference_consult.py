"""Run the H01 scoped subscription comparison through the frozen daemon's consult service."""

import json
import re
import shutil
import subprocess
from pathlib import Path

TOOL = {
    "name": "akasha_consult",
    "description": "Research the current request from this caller's authorized context and memories.",
    "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
}


def service_command(args, private, case, profile, sandbox, dump, sha):
    from_dir = private / "service"
    from_dir.mkdir(mode=0o700)
    command, version, config_sha, catalog_sha = sandbox(args, from_dir, profile)
    del command[-3:]  # Keep the sterile OS namespace through the '--' separator below.
    assert command[-1] == "--"
    command.pop()
    shutil.copyfile(
        from_dir / "catalog.json", from_dir / "home/.codex/models_cache.json"
    )
    area = private / "authority"
    area.mkdir(mode=0o700)
    dump(area / "episode.json", case)
    # Only the evaluator's dynamic loader and its three runtime libraries are shared.
    deps = subprocess.check_output(["ldd", str(args.observer)], text=True)
    library_paths = re.findall(
        r"(?:=>\s+|^\s*)(/[^\s]+)\s+\(0x", deps, flags=re.MULTILINE
    )
    assert {Path(p).name for p in library_paths} == {
        "ld-linux-x86-64.so.2",
        "libm.so.6",
        "libc.so.6",
        "libgcc_s.so.1",
    }
    for name in library_paths:
        command += ["--ro-bind", str(Path(name).resolve(strict=True)), name]
    command += [
        "--bind",
        str(area),
        "/fixture",
        "--ro-bind",
        str(args.observer),
        "/host/reference-service",
        "--",
        "/host/reference-service",
    ]
    return command, {
        "version": version,
        "config_sha256": config_sha,
        "catalog_sha256": catalog_sha,
        "binary_sha256": sha(args.observer),
        "model": profile["model"],
        "reasoning": profile["reasoning_effort"],
        "provider": "openai",
        "route_validation": "isolated config, exclusive catalog; native runner checks returned model against catalog",
        "global_auth": "absent"
        if args.preflight
        else "read-only subscription credential mounted only for the native client",
        "workspace": "/work",
        "history_count": len(case["sources"]),
    }


async def submit(state, send, case, result, tid, turnid, callid, preflight=False):
    assert not result.get("invocation"), "duplicate consult"
    assert all(isinstance(x, str) and 0 < len(x) <= 256 for x in (tid, turnid, callid))
    await send(
        state,
        {
            "thread_id": tid,
            "turn_id": turnid,
            "call_id": callid,
            "current_task": case["current_task"],
            "hint": None,
            "preflight": preflight,
        },
    )

    async def next_line():
        data = await state.stdout.readline()
        if not data:
            raise RuntimeError("Scoped service exited before the next receipt")
        return json.loads(data)

    inv = await next_line()
    assert (
        inv["type"] == "invocation"
        and inv["binding"]["scope"] == case["caller"]["scope"]
    )
    assert (
        inv["binding"]["turn_id"] == turnid and inv["binding"]["tool_call_id"] == callid
    )
    assert (
        inv["binding"]["caller"]["thread"] == tid
        and inv["current_request"]["utf8_sha256"] == result["task_utf8_sha256"]
    )
    assert (
        inv["historical_ordinals"] == []
        and inv["logical_fence"] == case["caller"]["fence_sequence"]
    )
    result["invocation"] = inv
    queued = await next_line()
    assert queued["type"] == "queued" and queued["state"] == "queued"
    result["queued"] = queued
    prepared = await next_line()
    assert (
        prepared["type"] == "prepared"
        and prepared["ready_search"]["scope"] == case["caller"]["scope"]
    )
    result["preparation"] = prepared
    if preflight:
        assert (
            prepared["cold_search_error"] == "INDEX_NOT_READY"
            and prepared["concurrent_reservations_accepted"] == 2
        )
        assert await state.wait() == 0
        return None
    terminal = await next_line()
    assert terminal["type"] == "terminal" and queued["id"] == terminal["id"]
    result["consult_terminal"] = terminal
    assert await state.wait() == 0
    return terminal["publication"]
