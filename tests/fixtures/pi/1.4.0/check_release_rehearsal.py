"""Linux candidate workflow through the downloaded archive, one Pi host and one private store."""

import argparse
import hashlib
import json
import os
import queue
import re
import select
import signal
import sqlite3
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

sys.path.append(str(Path(__file__).resolve().parent / "../../../../scripts"))
from check_linux_package import MESSAGE, Provider, exercise, sha, write_json
from check_linux_upgrade import read_archive
from release_rehearsal_provider import RehearsalProvider


def digest(obj):
    return hashlib.sha256(
        json.dumps(obj, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def after_compaction(args, root, env, workspace, history, run, cli, finished, wait):
    profile = Path(env["PI_CODING_AGENT_DIR"])
    extension = profile / "extensions/akasha-otsuge/index.ts"
    hook = root / "compact.ts"
    hook.write_text(
        "export default function(pi){pi.on('session_before_compact', e=>({compaction:{summary:'Controlled lookup rehearsal summary',firstKeptEntryId:e.preparation.firstKeptEntryId,tokensBefore:e.preparation.tokensBefore}}));}"
    )
    common = [
        "--session",
        history,
        "--no-builtin-tools",
        "--no-extensions",
        "--no-mcp",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
        "--no-approve",
        "-e",
        extension,
        "--tools",
        "akasha_consult",
    ]
    replies = queue.Queue()
    proc = subprocess.Popen(
        [args.pi, "--mode", "rpc", *common, "-e", hook],
        cwd=workspace,
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )

    def collect():
        for line in proc.stdout:
            replies.put(json.loads(line))
        replies.put(None)

    thread = threading.Thread(target=collect, daemon=True)
    thread.start()
    try:
        proc.stdin.write(
            json.dumps({"id": "compact-release", "type": "compact"}) + "\n"
        )
        proc.stdin.flush()
        while True:
            item = replies.get(timeout=25)
            if item is None:
                raise RuntimeError("Pi closed the compaction stream")
            if item.get("id") == "compact-release" and item["type"] == "response":
                assert item["success"], item
                break
    finally:
        proc.terminate()
        proc.wait(timeout=5)
        thread.join(timeout=5)
    events = [json.loads(line) for line in history.read_text().splitlines()]
    compact = [x for x in events if x["type"] == "compaction"]
    assert (
        len(compact) == 1
        and compact[0]["summary"] == "Controlled lookup rehearsal summary"
    )
    pairs = []

    for tag, hint in (("plain", None), ("narrow", "offline lookup")):
        offset = history.stat().st_size
        run(
            args.pi,
            "--mode",
            "json",
            "--print",
            *common,
            f"AFTER-COMPACTION {tag}: recall the offline lookup condition.",
        )
        with history.open("rb") as src:
            src.seek(offset)
            entries = map(json.loads, src)
            tool_results = [
                entry["message"]
                for entry in entries
                if entry.get("message", {}).get("role") == "toolResult"
            ]
        (tool,) = [m for m in tool_results if m["toolName"] == "akasha_consult"]
        result = json.loads(tool["content"][0]["text"])
        assert "data" in result, result
        job = finished(result["data"]["job_id"])
        assert job["consult_plan"]["args"]["hint"] == hint
        answer = cli("consult", "show", job["id"])
        assert job["run_trace"]["runs"][0]["host"] == "pi"
        assert answer["status"] == "found" and answer["evidence"]
        pairs.append(
            {
                "hint": hint,
                "job": job["id"],
                "receipt": answer["receipt_id"],
                "status": answer["status"],
                "evidence": answer["evidence"],
            }
        )
    return {"native_compaction": compact[0]["id"], "after": pairs}


def rehearsal(
    args,
    root,
    env,
    workspace,
    cfg,
    app,
    fixture,
    run,
    cli,
    finished,
    wait,
    stop,
    job_id,
    job,
    public,
    original,
    scope_file,
    opened,
    review,
    history,
):
    log = fixture.receipt
    scope = original["scope"]
    receipt = cli("consult", "receipt", job_id)
    receipt_sha = digest(receipt["data"])
    source = public["evidence"][0]
    inv = job["consult_plan"]["invocation_id"]
    cfg0 = cfg.read_text()
    profile = Path(env["PI_CODING_AGENT_DIR"])
    native_cfg = (
        '\n[executors.only_pi]\nkind="pi"\nbinary='
        + json.dumps(str(args.pi))
        + "\nnative_home="
        + json.dumps(str(profile))
        + "\n"
    )
    inbox = root / "delivery/inbox.json"
    channel = (
        '\n[notifiers.external]\nkind="stdio"\nenabled=true\ncommand='
        + json.dumps(
            [str(root / "notifier-file.py"), "--state", str(inbox), "--fail-first"]
        )
        + "\n"
    )
    draft = review["proposal"]["record"]["draft"]
    assert (
        draft["source_spans"] == [source]
        and review["proposal"]["record"]["approval"] is None
    )
    (mode,) = re.findall(r"use (\w+) search when offline", draft["claim"])
    assert (
        "when offline" in draft["conditions"]
        and draft["claim"] == original["output"] == MESSAGE
    )
    log["source"] = {
        "pi_history_sha256": sha(history),
        "source": source,
        "consult_job": job_id,
        "receipt_id": public["receipt_id"],
        "receipt_sha256": receipt_sha,
        "invocation": inv,
        "scope": scope,
        "memory_id": opened["memory_id"],
        "authority": review["proposal"]["record"]["authority"],
    }
    profile = Path(env["PI_CODING_AGENT_DIR"])
    extra_cfg = (
        '\n[executors.only_pi]\nkind="pi"\nbinary='
        + json.dumps(str(args.pi))
        + "\nnative_home="
        + json.dumps(str(profile))
        + '\n[grill]\ninterviewer="only_pi"\nexpert="only_pi"\nreviewer="only_pi"\nmax_questions=1\nmax_rounds=1\nmax_runner_starts=3\nmax_tokens=32000\n'
    )
    fixture.hold.set()
    old = cli(
        "extract",
        "run",
        root / "extraction.json",
        "--idempotency-key",
        "extract-before-reload",
    )
    try:
        assert fixture.arrived.wait(12)
        cfg.write_text(cfg0.replace('mode="manual"', 'mode="automatic"') + extra_cfg)
        applied = cli("config", "reload")
    finally:
        fixture.hold.clear()
        fixture.release.set()
    try:
        previous = finished(old["id"])
        later = finished(
            cli(
                "extract",
                "run",
                root / "extraction.json",
                "--idempotency-key",
                "extract-after-reload",
            )["id"]
        )
        assert (
            previous["extraction_review_mode"] == "manual"
            and later["extraction_review_mode"] == "automatic"
        )
        assert previous["config_generation"] < later["config_generation"]
        log["hot_reload"] = {
            "old": {
                k: previous[k]
                for k in ("id", "config_generation", "extraction_review_mode", "result")
            },
            "new": {
                k: later[k]
                for k in ("id", "config_generation", "extraction_review_mode", "result")
            },
            "request_in_flight": True,
            "reload": applied,
        }
        plan = write_json(
            root / "grill.json",
            {
                "scope": scope,
                "sources": [source],
                "question": "What fallback does the captured user request specify for offline lookup?",
            },
        )
        grill = finished(
            cli("grill", "run", plan, "--idempotency-key", "release-grill")["id"]
        )
        assert (
            fixture.roles == ["interviewer", "expert", "reviewer"]
            and grill["result"]["steps"] == 3
        )
        log["grill"] = {
            "id": grill["id"],
            "roles": fixture.roles,
            "discussion": grill["result"]["discussion"],
            "usage": grill["result"]["usage"],
            "stop_reason": grill["result"]["stop_reason"],
        }
    finally:
        cfg.write_text(cfg0)
        cli("config", "reload")
    (oldrow,) = previous["result"]["candidates"]
    (newrow,) = later["result"]["candidates"]
    assert (
        oldrow["memory_id"] == newrow["memory_id"] == opened["memory_id"]
        and newrow["status"] == "active"
    )
    change = cli(
        "state", "diff", oldrow["revision"], newrow["revision"], "--scope", scope_file
    )["data"]
    (added,) = change["items"]
    active = added["after"]["memory"]
    assert (
        added["before"] is None
        and active["revision"]["commit_seq"] == newrow["revision"]
    )
    assert (
        active["record"]["authority"] == "delegated_approved"
        and active["record"]["approval"] is None
    )
    assert active["record"]["draft"]["source_spans"] == [source]
    log["adoption_history"] = {
        "from": change["from"],
        "to": change["to"],
        "memory": active["revision"],
        "authority": active["record"]["authority"],
        "source": source,
        "diff_sha256": digest(change),
    }
    finished(
        cli("index", "sync", "--idempotency-key", "rehearsal-current-memory")["id"]
    )
    fixture.recall_adopted = True
    log["compaction"] = after_compaction(
        args, root, env, workspace, history, run, cli, finished, wait
    )
    fixture.recall_adopted = False
    used_job_id = log["compaction"]["after"][-1]["job"]
    used_inv = cli("job", "show", used_job_id)["consult_plan"]["invocation_id"]
    used_receipt = cli("consult", "receipt", used_job_id)["data"]
    assert active["revision"] in used_receipt["record"]["selected"]
    recalled = cli("consult", "show", used_job_id)
    (mode,) = re.findall(r"use (\w+) search when offline", recalled["answer"])
    assert source in recalled["evidence"]
    run("git", "init", "-q")
    run("git", "config", "user.name", "Rehearsal")
    run("git", "config", "user.email", "fixture@localhost")
    (workspace / ".gitignore").write_text("app\n")
    code = workspace / "lookup.c"
    original_code = '#include <stdio.h>\n#include <string.h>\nstatic const char *lookup(int offline){(void)offline; return "network";}\nint main(void){puts(lookup(1));return strcmp(lookup(1), "keyword") || strcmp(lookup(0), "network");}\n'
    code.write_text(original_code)
    run("git", "add", ".")
    run("git", "commit", "-qm", "Fixture before offline lookup change")

    def outcome(name, argv, expected=0, artifacts=(), incident=None):
        plan = {
            "event_id": name,
            "invocation_id": used_inv,
            "artifacts": list(artifacts),
        }
        if incident is not None:
            plan["incident"] = incident
        path = write_json(root / (name + ".json"), plan)
        raw = run(
            app, "--config", cfg, "outcome", "run", path, "--", *argv, expected=expected
        )
        out = json.loads(raw)["data"]
        assert out == cli("outcome", "show", name)
        assert used_job_id in out["event"]["receipt_jobs"]
        assert not out["event"]["execution"]["before"]["gaps"]
        return out

    cc = ["/usr/bin/gcc", "-std=c11", "-Wall", "-Werror", "lookup.c", "-o", "app"]
    outcome("before-build", cc, artifacts=["app"])
    red = outcome("before-test", ["./app"], expected=1)
    code.write_text(
        original_code.replace(
            '(void)offline; return "network";',
            f'return offline ? "{mode}" : "network";',
        )
    )
    good = outcome("after-build", cc, artifacts=["app"])
    green = outcome("after-test", ["./app"])
    assert sha(workspace / "app") == good["event"]["artifacts"][0]["sha256"]
    log["software"] = {
        "used_recall": used_receipt["record"]["selected"],
        "used_consult_job": used_job_id,
        "used_invocation": used_inv,
        "compiler": run("gcc", "--version").splitlines()[0],
        "verified_condition": draft["conditions"],
        "red_outcome": red["event"]["event_id"],
        "green_outcome": green["event"]["event_id"],
        "events": {
            name: cli("outcome", "show", name)["event"]["execution"]
            for name in ("before-test", "after-test")
        },
        "artifact": good["event"]["artifacts"][0],
        "source_sha256": sha(code),
        "git_base": run("git", "rev-parse", "HEAD").strip(),
    }
    patch = run("git", "diff", "--", "lookup.c")
    run("git", "add", "lookup.c")
    run("git", "commit", "-qm", "Use the recalled keyword fallback while offline")
    bundle = args.output.with_suffix(".bundle").resolve()
    if bundle.exists():
        raise FileExistsError(bundle)
    run("git", "bundle", "create", bundle, "--all")
    run("git", "bundle", "verify", bundle)
    log["software"].update(
        git_result=run("git", "rev-parse", "HEAD").strip(),
        worktree_patch=patch,
        git_bundle_sha256=sha(bundle),
    )
    good_code = code.read_text()
    code.write_text(good_code.replace("puts(lookup(1))", "puts(obsolete_lookup(1))"))
    try:
        cfg.write_text(
            cfg0
            + channel
            + native_cfg
            + '\n[incidents]\nexecutor="only_pi"\nmax_runner_starts=1\ntriggers=["reproducible_failure"]\ndeduplicate=true\nescalate_after=2\n[incidents.priorities]\nreproducible_failure="high"\n'
        )
        cli("config", "reload")
        signal_data = {
            "kind": "reproducible_failure",
            "key": "rehearsal-obsolete-lookup",
            "reason": "Compare the compiler diagnostic and src state with the recalled offline lookup constraint.",
            "targets": [],
        }
        outputs = [
            outcome(name, cc, expected=1, incident=signal_data)
            for name in ("broken-build-1", "broken-build-2")
        ]
        incident = outputs[0]["incident"]
        assert incident and outputs[1]["incident"] == incident
        issue = cli("incident", "show", incident)["record"]
        assert issue["occurrences"] == 2 and issue["priority"] in ("high", "urgent")
        issue = wait(
            lambda: cli("incident", "show", incident)["record"],
            lambda v: v["investigation"] == "finished",
        )
        assert (
            issue["job_id"]
            and fixture.incident_handles
            and issue["classification"] == "insufficient_evidence"
        ), issue
        investigation = finished(issue["job_id"])
        log["external_investigation"] = {
            "job": investigation["id"],
            "classification": issue["classification"],
            "handles": fixture.incident_handles,
            "state": issue["state"],
        }
        raw = cli("outcome", "log", "broken-build-1", 1, "--start", "0")
        assert "obsolete_lookup" in bytes.fromhex(raw["raw_hex"]).decode()
        notice = wait(
            lambda: next(
                (
                    n
                    for n in cli("notify", "list", "--scope", scope_file)["items"]
                    if n["review_id"] == incident
                    and n["occurrences"] == 2
                    and n["deliveries"].get("inbox", {}).get("status") == "sent"
                ),
                None,
            ),
            bool,
        )
        delivered = wait(
            lambda: cli("notify", "show", notice["notification_id"]),
            lambda v: v["deliveries"].get("external", {}).get("status") == "sent",
        )
        state = wait(
            lambda: json.loads(inbox.read_text()) if inbox.exists() else {},
            lambda v: v.get("failed_once") and v.get("messages"),
        )
        ext = delivered["deliveries"]["external"]["external_message_id"]
        assert state["messages"][ext]["review_id"] == incident

        def retried_delivery():
            for notification in cli("notify", "list", "--scope", scope_file)["items"]:
                d = notification["deliveries"].get("external", {})
                a = d.get("attempt") or {}
                if (
                    d.get("status") == "sent"
                    and a.get("transient_failures", 0) > 0
                    and a["number"] >= 2
                ):
                    return {
                        "notification_id": notification["notification_id"],
                        "generation": d["generation"],
                        "external_id": d["external_message_id"],
                        "claims": a["number"],
                        "transient_failures": a["transient_failures"],
                    }

        retry = wait(retried_delivery, bool)
        assert retry["external_id"] in json.loads(inbox.read_text())["messages"]
        log["stdio"] = {
            "verified_retry": retry,
            "failed_once": state["failed_once"],
            "external_id": ext,
            "incident_notification": notice["notification_id"],
            "delivery": delivered["deliveries"]["external"],
            "recorded_receipts": len(state["receipts"]),
        }
        ack = (
            "notify",
            "ack",
            notice["notification_id"],
            "--generation",
            notice["generation"],
            "--idempotency-key",
            "incident-rehearsal-ack",
        )
        assert cli(*ack) == cli(*ack)
        log["incident"] = {
            "id": incident,
            "priority": issue["priority"],
            "occurrences": issue["occurrences"],
            "investigation": issue["investigation"],
            "stderr_sha256": hashlib.sha256(bytes.fromhex(raw["raw_hex"])).hexdigest(),
            "notification_id": notice["notification_id"],
            "ack_generation": notice["generation"],
        }
    finally:
        cfg.write_text(cfg0)
        cli("config", "reload")
        code.write_text(good_code)
    rid = opened["id"]
    before = cli("review", "show", rid, "--at", opened["revision"])
    assert (
        before["item"]["state"] == "open" and review["item"]["state"] == "investigating"
    )
    diff = cli(
        "state",
        "diff",
        opened["revision"],
        review["item"]["revision"],
        "--scope",
        scope_file,
    )
    assert digest(cli("consult", "receipt", job_id)["data"]) == receipt_sha
    used_hash = digest(used_receipt)
    assert digest(cli("consult", "receipt", used_job_id)["data"]) == used_hash
    again = cli(
        "show",
        source["event_id"],
        "--revision",
        source["revision"],
        "--range",
        source["start"],
        source["end"],
    )
    assert again["output"] == original["output"]
    log["history"] = {
        "review_before": opened["revision"],
        "review_after": review["item"]["revision"],
        "state_diff_sha256": digest(diff),
        "original_sha256": source["content_sha256"],
        "recall_before_and_after_sha256": receipt_sha,
        "used_recall_before_and_after_sha256": used_hash,
    }
    stop()
    backup = root / "neutral-backup"
    cli("archive", "export", backup)
    cli("daemon", "start")
    pid = cli("daemon", "status")["pid"]
    assert os.path.samefile("/proc/" + str(pid) + "/exe", app)
    fd = os.pidfd_open(pid)
    try:
        poll = select.poll()
        poll.register(fd, select.POLLIN)
        signal.pidfd_send_signal(fd, signal.SIGKILL)
        assert poll.poll(10000)
    finally:
        os.close(fd)
    cli("daemon", "start")
    assert (
        digest(cli("consult", "receipt", job_id)["data"]) == receipt_sha
        and cli("incident", "show", incident)["record"]["occurrences"] == 2
    )

    assert digest(cli("consult", "receipt", used_job_id)["data"]) == used_hash

    def restore_roundtrip(archive, name):
        store = root / (name + "-store")
        restored = root / (name + "-export")
        conf = root / (name + ".toml")
        conf.write_text(
            f"[paths]\ndata_dir={json.dumps(str(store))}\nsocket_path={json.dumps(str(root / (name + '.sock')))}\n"
        )
        run(app, "--config", conf, "archive", "import", archive)
        run(app, "--config", conf, "archive", "export", restored)
        files = {p.name: sha(p) for p in archive.iterdir() if p.is_file()}
        native_files = {p.name: sha(p) for p in restored.iterdir() if p.is_file()}
        original_rows = read_archive(archive)
        assert original_rows == read_archive(restored)
        assert all(files[k] == value for k, value in native_files.items())
        if name == "restored":
            assert files == native_files
        else:
            assert any(MESSAGE.encode() in value for value in original_rows[1].values())
        return {
            "original_files": files,
            "exported_files": native_files,
            "commits": len(original_rows[0]),
            "revisions": len(original_rows[1]),
        }

    log["crash_restore"] = {
        "signal": "SIGKILL",
        "reopened_receipt_sha256": receipt_sha,
        "backup_roundtrip": restore_roundtrip(backup, "restored"),
    }
    mempal = root / "fixture.sqlite"
    db = sqlite3.connect(mempal)
    try:
        db.executescript(args.mempal_schema.read_text())
        db.execute(
            "insert into conversation_turns(id,session_id,tool,turn_index,role,content,timestamp_epoch) values(?,?,?,?,?,?,?)",
            ("source-1", "fixture-session", "pi", 0, "user", MESSAGE, 1),
        )
        db.commit()
    finally:
        db.close()
    migration = root / "mempal"
    cli("migrate", "mempal", mempal, migration)
    log["mempal"] = {
        "sqlite_sha256": sha(mempal),
        "registered_rows": 1,
        "neutral_roundtrip": restore_roundtrip(migration, "mempal-roundtrip"),
    }


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    for k in ("scratch", "archive", "pi", "mempal-schema", "output"):
        p.add_argument("--" + k, type=Path, required=True)
    for k in ("sha256", "commit"):
        p.add_argument("--" + k, required=True)
    args = p.parse_args()
    os.umask(0o077)
    fixture = Provider()
    fixture.server.RequestHandlerClass = RehearsalProvider
    fixture.server.fixture = fixture
    fixture.calls = []
    fixture.receipt = {}
    fixture.hold = threading.Event()
    fixture.arrived = threading.Event()
    fixture.release = threading.Event()
    fixture.roles = []
    fixture.incident_handles = []
    fixture.recall_adopted = False
    try:
        with tempfile.TemporaryDirectory(prefix="rc-", dir=args.scratch) as td:
            baseline = exercise(args, Path(td), fixture, rehearsal)
        fixture.receipt.update(package=baseline, scratch_removed=not Path(td).exists())
    finally:
        fixture.close()
        with args.output.open("x") as fp:
            json.dump(fixture.receipt, fp, indent=2)
