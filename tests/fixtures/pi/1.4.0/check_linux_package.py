"""Exercise an installed Linux archive with Pi 1.4.0 and an offline protocol fixture.

Run in a fresh Linux home with only Pi available. The receipt prints synthetic
identifiers and hashes. All work occurs in a temporary directory on --scratch.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import tempfile
import time
import uuid
from http.server import BaseHTTPRequestHandler

from native_provider import Provider

MESSAGE = "For the package fixture, use keyword search when offline."


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value))
    return path


class Endpoint(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        fixture = self.server.fixture
        try:
            assert self.path == "/v1/chat/completions"
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            if request["model"] == "fixture-extractor":
                assert "extraction-v1" in str(request["messages"]) and MESSAGE in str(request["messages"])
                candidate = dict(kind="decision", scope=fixture.scope, claim=MESSAGE,
                                 rationale="", conditions=["when offline"], assumptions=[],
                                 sources=[dict(source=0, start=0, end=len(MESSAGE))],
                                 valid_from_ms=None, valid_until_ms=None)
                content = json.dumps(dict(schema_version=1, candidates=[candidate]))
                value = {"choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}]}
                data = json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                fixture.calls.append("extractor")
                return
            tools = {t["function"]["name"] for t in request["tools"]}
            results = [m["content"] for m in request["messages"] if m["role"] == "tool"]
            tool, args, answer = None, {}, None
            if "akasha_consult" in tools:
                assert MESSAGE in str(request["messages"])
                if not results:
                    tool = "akasha_consult"
                else:
                    assert len(results) == 1
                    answer = "Consult submitted."
            elif not results:
                tool, args = "context_search", {"query": MESSAGE, "limit": 8}
            elif len(results) == 1:
                events = json.loads(results[0])["value"]["events"]
                assert events
                fixture.selected_event = events[0]["reference"]
                tool, args = "context_read", {"handle": events[0]["handle"], "start": 0, "max_bytes": 4096}
            else:
                assert len(results) == 2
                read = json.loads(results[1])["value"]
                assert read["text"] == MESSAGE and not read["truncated"]
                answer = json.dumps(dict(status="found", answer=MESSAGE, sources=read["sources"],
                                         memories=[], conditions=["when offline"], conflicts=[], gaps=[]))
            assert tool is None or tool in tools
            fixture.calls.append(tool or "native_final")
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            ident = uuid.uuid4().hex

            def chunk(delta, finish=None):
                value = {"id": ident, "object": "chat.completion.chunk", "created": 1, "model": "fixture",
                         "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
                self.wfile.write(b"data: " + json.dumps(value).encode() + b"\n\n")

            chunk({"role": "assistant"})
            if tool:
                chunk({"tool_calls": [{"index": 0, "id": ident, "type": "function",
                                       "function": {"name": tool, "arguments": json.dumps(args)}}]})
            else:
                chunk({"content": answer})
            chunk({}, "tool_calls" if tool else "stop")
            self.wfile.write(b"data: [DONE]\n\n")
            self.close_connection = True
        except Exception as exc:
            fixture.errors.append(repr(exc))
            self.close_connection = True


def exercise(args, root, fixture, rehearsal=None):
    package = args.archive.name.removesuffix(".tar.gz")
    checksum = args.archive.with_name(args.archive.name + ".sha256")
    assert checksum.read_text().split() == [args.sha256, args.archive.name]
    home, profile, workspace = root / "home", root / "agent", root / "project"
    runtime, config_dir, data = home / "run", home / "config", home / "data"
    for path in (home, profile, workspace, runtime, config_dir, data):
        path.mkdir(mode=0o700)
    app = home / ".local/bin/akasha-otsuge"
    cfg = config_dir / "fixture.toml"
    env = dict(HOME=str(home), TMPDIR=str(root), XDG_RUNTIME_DIR=str(runtime),
               XDG_CONFIG_HOME=str(config_dir), XDG_DATA_HOME=str(data), XDG_CACHE_HOME=str(home / "cache"),
               PI_CODING_AGENT_DIR=str(profile), PI_SKIP_VERSION_CHECK="1", PATH=os.environ["PATH"], LANG="C.UTF-8")
    history = profile / "sessions/fixture.jsonl"
    history.parent.mkdir()
    fixture.configure(profile)
    user_file = write_json(profile / "local-user-settings.json", {"theme": "fixture-owner"})
    cfg.write_text(f'[access]\nread_roots=[{json.dumps(str(root))}]\n'
                   '[review]\nmode="manual"\n[notifiers.inbox]\nkind="local"\nenabled=true\n'
                   '[retrieval]\nextraction="fixture"\n[models.fixture]\nkind="text"\nlocal=true\n'
                   f'endpoint="http://127.0.0.1:{fixture.server.server_port}/v1"\nmodel="fixture-extractor"\n')

    def run(*cmd, cwd=workspace, expected=0):
        p = subprocess.run([str(a) for a in cmd], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                           capture_output=True, text=True, timeout=120)
        if p.returncode != expected:
            raise RuntimeError((str(cmd[0]), str(cmd[1]) if len(cmd) > 1 else "", p.returncode, p.stderr[-800:]))
        return p.stdout

    def cli(*cmd):
        return json.loads(run(app, "--config", cfg, *cmd))["data"]

    def wait(read, ready):
        end = time.monotonic() + 45
        while True:
            result = read()
            if ready(result):
                return result
            if time.monotonic() >= end:
                raise AssertionError(("timed out", str(result)[:900], fixture.errors))
            time.sleep(0.1)

    def finished(job):
        item = wait(lambda: cli("job", "show", job), lambda x: x["state"] not in ("queued", "running"))
        assert item["state"] == "succeeded", (item.get("error"), fixture.errors)
        return item

    def tree(path):
        return {str(f.relative_to(path)): sha(f) for f in sorted(path.rglob("*")) if f.is_file()}

    def stop():
        pid = cli("daemon", "status")["pid"]
        fd = os.pidfd_open(pid)
        try:
            watcher = select.poll()
            watcher.register(fd, select.POLLIN)
            cli("daemon", "stop")
            assert watcher.poll(120000), "daemon did not finish draining"
        finally:
            os.close(fd)

    run("sha256sum", "-c", checksum, cwd=args.archive.parent)
    run("tar", "-xzf", args.archive, cwd=root)
    extracted = root / package
    assert (extracted / "SOURCE_COMMIT").read_text().strip() == args.commit
    required = ["docs/linux-install.md", "docs/adapters/pi-1.4.0/README.md", "docs/review.md", "docs/notifications.md"]
    assert all((extracted / path).is_file() for path in required)
    run("install", "-d", app.parent)
    run("install", "-m", "0755", extracted / "bin/akasha-otsuge", app)
    binary_hash = sha(app)
    assert binary_hash == sha(extracted / "bin/akasha-otsuge")
    if rehearsal:
        shutil.copy2(extracted / "examples/notifier-file.py", root / "notifier-file.py")
    shutil.rmtree(extracted)
    assert run(app, "--version").strip().startswith("akasha-otsuge ")
    pi_version = run(args.pi, "--version").strip()
    assert pi_version == "1.4.0"
    assert all(shutil.which(other, path=env["PATH"]) is None for other in ("codex", "claude", "hermes"))
    kept_paths = [cfg, profile / "models.json", profile / "settings.json", user_file]
    before = [sha(p) for p in kept_paths]
    cli("daemon", "start")
    try:
        status = cli("daemon", "status")
        assert os.path.samefile(Path("/proc") / str(status["pid"]) / "exe", app)
        cli("doctor", "--json")
        cli("doctor", "--host", "pi", "--binary", args.pi)
        cli("adapter", "install", "pi", "--binary", args.pi, "--agent-dir", profile)
        extension = profile / "extensions/akasha-otsuge/index.ts"
        run(args.pi, "--mode", "json", "--print", "--session", history, "--no-builtin-tools", "--no-extensions",
            "--no-mcp", "--no-skills", "--no-prompt-templates", "--no-themes", "--no-context-files", "--no-approve",
            "-e", extension, "--tools", "akasha_consult", MESSAGE)
        jobs = wait(lambda: [j["id"] for j in cli("job", "list")["items"] if j["id"].startswith("consult/")], bool)
        assert len(jobs) == 1
        job = finished(jobs[0])
        assert job["run_trace"]["runs"][0]["host"] == "pi"
        public = cli("consult", "show", jobs[0])
        assert json.loads(cli("consult", "receipt", jobs[0])["data"]["output"]) == public
        assert public["status"] == "found" and public["answer"] == MESSAGE
        source, = public["evidence"]
        original = cli("show", source["event_id"], "--revision", source["revision"], "--range", source["start"], source["end"])
        assert original["output"] == MESSAGE and source["content_sha256"] == hashlib.sha256(MESSAGE.encode()).hexdigest()
        native = list(map(json.loads, history.read_text().splitlines()))
        matching = [event for event in native if event.get("type") == "message" and
                    event["message"]["role"] == "user" and
                    (event["message"]["content"] == MESSAGE or
                     event["message"]["content"] == [{"type": "text", "text": MESSAGE}])]
        assert len(matching) == 1 and fixture.selected_event["event_id"] == "entry:" + matching[0]["id"]
        assert fixture.selected_event["source"]["adapter"] == "pi"
        fixture.scope = original["scope"]
        assert fixture.scope["user"] == f"uid:{os.geteuid()}"
        plan = write_json(root / "extraction.json", {"scope": fixture.scope, "sources": [source]})
        extracted_job = finished(cli("extract", "run", plan, "--idempotency-key", "package-proposal")["id"])
        candidate, = extracted_job["result"]["candidates"]
        scope = write_json(root / "scope.json", fixture.scope)
        item, = cli("review", "list", "--scope", scope)["items"]
        assert item["state"] == "open" and item["memory_id"] == candidate["memory_id"]
        review_id = item["id"]
        update = write_json(root / "review.json", dict(id=review_id, expected_revision=item["revision"],
                                                        state="investigating", reason="Verify the offline condition."))
        cli("review", "state", update, "--idempotency-key", "package-investigate")
        review = cli("review", "show", review_id)
        assert review["item"]["state"] == "investigating"
        assert review["proposal"]["record"]["authority"] == "candidate"
        assert review["proposal"]["record"]["draft"]["source_spans"] == [source]
        assert review["proposal"]["record"]["approval"] is None

        def delivered():
            items = cli("notify", "list", "--scope", scope)["items"]
            return next((n for n in items if n["review_id"] == review_id and
                         n["source_revision"] == review["item"]["revision"] and
                         n["deliveries"].get("inbox", {}).get("status") == "sent" and
                         n["deliveries"]["inbox"]["generation"] == n["generation"]), None)

        notice = wait(delivered, bool)
        ident, gen = notice["notification_id"], notice["generation"]
        ack = ("notify", "ack", ident, "--generation", gen, "--idempotency-key", "package-ack")
        assert cli(*ack) == cli(*ack)
        assert cli("notify", "show", ident)["acknowledged_generation"] == gen
        assert cli("review", "show", review_id) == review
        if rehearsal:
            rehearsal(args, root, env, workspace, cfg, app, fixture, run, cli, finished, wait, stop,
                      jobs[0], job, public, original, scope, item, review, history)
        stored_history_sha = sha(history)
        cli("adapter", "uninstall", "pi", "--agent-dir", profile)
        assert not extension.parent.exists() and before == [sha(p) for p in kept_paths]
    finally:
        stop()
    records = tree(data)
    assert records
    run("rm", "--", app)
    assert not app.exists() and records == tree(data) and before == [sha(p) for p in kept_paths]
    assert history.exists() and sha(history) == stored_history_sha and not fixture.errors
    return dict(package_sha256=args.sha256, source_commit=args.commit, installed_exe_sha256=binary_hash,
                pi_version=pi_version, other_hosts_absent_on_path=True, scope_user_bound=True,
                native_message_id=matching[0]["id"], native_capture_event_id=fixture.selected_event["event_id"],
                captured_event_id=source["event_id"], captured_revision=source["revision"], source_sha256=source["content_sha256"],
                consult_receipt_id=public["receipt_id"], consult_status=public["status"], review_state=review["item"]["state"],
                review_id=review_id, proposal_authority="candidate", notification_id=ident,
                notification_generation=gen, delivery="sent", ack_replayed=True,
                native_history_sha256=stored_history_sha, retained_data_files=len(records), user_settings_preserved=True,
                native_calls=fixture.calls, provider="deterministic loopback", real_model_calls=0, billed_usd=0)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--pi", required=True, type=Path)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--scratch", required=True, type=Path)
    args = parser.parse_args()
    args.archive = args.archive.resolve(strict=True)
    args.pi = args.pi.resolve(strict=True)
    os.umask(0o077)
    fixture = Provider()
    fixture.server.RequestHandlerClass = Endpoint
    fixture.server.fixture = fixture
    fixture.calls = []
    try:
        with tempfile.TemporaryDirectory(prefix="p26-", dir=args.scratch) as tmp:
            result = exercise(args, Path(tmp), fixture)
        result["scratch_removed"] = not Path(tmp).exists()
        print(json.dumps(result, sort_keys=True))
    finally:
        fixture.close()
