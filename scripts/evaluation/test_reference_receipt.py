"""Offline checks for registered model-arm inputs, receipt ownership and native terminal reporting."""

import asyncio
import contextlib
import gzip
import hashlib
import io
import json
import os
import sys
import tempfile
import unittest
from multiprocessing import get_context
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import reference_direct as driver


class ReceiptTests(unittest.TestCase):
    def setUp(self):
        area = tempfile.TemporaryDirectory()
        self.addCleanup(area.cleanup)
        previous_umask = os.umask(0o077)
        self.addCleanup(os.umask, previous_umask)
        self.root = Path(area.name)
        self.output = self.root / "result.json"

    def args(self):
        names = ("codex", "observer", "observer-build", "auth", "catalog", "scratch")
        flags = [part for name in names for part in ("--" + name, str(self.root))]
        return ["reference_direct", *flags, "--output", str(self.output)]

    def test_unregistered_consult_argument_stops_before_service_submission(self):
        for registered, supplied in (
            (None, {"hint": "extra"}),
            ("exact", {}),
            ("exact", {"hint": "other"}),
        ):
            with self.assertRaisesRegex(ValueError, "registered hint"):
                asyncio.run(
                    driver.reference_consult.submit(
                        None,
                        None,
                        {"hint": registered},
                        {},
                        "thread",
                        "turn",
                        "tool",
                        supplied,
                    )
                )

    def test_hint_never_changes_registered_task_or_other_unavailable_inputs(self):
        tasks = {t["id"]: t for t in driver.read_reference()[1]}
        results = driver.ROOT / "docs/benchmarks/results/mvp027"
        prior = json.loads(
            gzip.decompress((results / "registered-arms-r1.json.gz").read_bytes())
        )
        others = [
            x
            for x in prior["remaining_registered_inputs"]
            if x["task"] != "C03" and not x["task"].endswith("-hint")
        ]
        self.assertEqual(len(others), 36)
        for old in others:
            with (
                tempfile.TemporaryDirectory(dir=self.root) as d,
                self.assertRaises(
                    driver.reference_episode.EpisodeUnavailable
                ) as caught,
            ):
                driver.reference_episode.prepare(tasks[old["task"]], None, Path(d), {})
            self.assertEqual(str(caught.exception), old["reason"])
        for i in range(1, 11):
            none, hinted = [tasks[f"I{i:02}-{v}"] for v in ("none", "hint")]
            left, right = [
                driver.main_input(driver.reference_episode.input_only(t))
                for t in (none, hinted)
            ]
            self.assertEqual(left["input"], right["input"])
            self.assertEqual(
                right["input"], [{"type": "text", "text": hinted["current_task"]}]
            )
            self.assertTrue(
                right["developerInstructions"].endswith(
                    json.dumps(hinted["hint"], ensure_ascii=False)
                )
            )
            self.assertEqual(
                driver.reference_consult.tool(hinted["hint"])["inputSchema"][
                    "required"
                ],
                ["hint"],
            )
            self.assertEqual(
                driver.reference_consult.tool(None)["inputSchema"]["properties"], {}
            )

    def test_all_native_hint_pairs_share_main_input_and_bind_only_scoped_originals(
        self,
    ):
        results = driver.ROOT / "docs/benchmarks/results/mvp027"
        archive = json.loads(
            gzip.decompress(
                (results / "registered-hint-preflights-r1.json.gz").read_bytes()
            )
        )
        tasks = {t["id"]: t for t in driver.read_reference()[1]}
        seen = set()
        for entry in archive["runs"]:
            r = entry["receipt"]
            key = r["task_id"], r["strategy"]
            self.assertNotIn(key, seen)
            seen.add(key)
            task = tasks[key[0]]
            self.assertEqual(entry["exit_code"], 0)
            self.assertFalse(r["model_turn_submitted"] or r["model_turn_accepted"])
            self.assertNotIn("auth", r)
            self.assertEqual(
                r["main_input_sha256"],
                driver.digest(driver.encoded(driver.main_input(task))),
            )
            self.assertEqual(
                r["task_utf8_sha256"], driver.digest(task["current_task"].encode())
            )
            self.assertEqual(
                r["hint_sha256"],
                None if task["hint"] is None else driver.digest(task["hint"].encode()),
            )
            self.assertEqual(r["binding"]["scope"], task["caller"]["scope"])
            if key[1] == "consult":
                self.assertEqual(r["queued"]["persisted_hint"], task["hint"])
                self.assertEqual(
                    r["invocation"]["current_request"]["utf8_sha256"],
                    r["task_utf8_sha256"],
                )
                self.assertEqual(
                    r["invocation"]["historical_ordinals"],
                    [s["sequence"] for s in task["sources"]],
                )
                ready = r["preparation"]["ready_search"]
                self.assertEqual(
                    ready["permission_revision"], r["binding"]["grant"]["commit_seq"]
                )
                self.assertEqual(ready["model_calls"], [])
            elif task["hint"]:
                control = r["hint_control_read"]
                self.assertEqual(control["scope"], task["caller"]["scope"])
                sources = {s["id"]: s for s in task["sources"]}
                for item in control["items"]:
                    self.assertEqual(item["scope"], control["scope"])
                    for src in item["sources"]:
                        self.assertIn(src["id"], task["caller"]["allowed_event_ids"])
                        self.assertEqual(src["text"], sources[src["id"]]["text"])
                        self.assertEqual(src["sha256"], sources[src["id"]]["sha256"])
                        self.assertTrue(src["receipt"])
        expected = [
            "C03",
            *(f"I{i:02}-{v}" for i in range(1, 11) for v in ("none", "hint")),
        ]
        self.assertEqual(
            seen, {(tid, arm) for tid in expected for arm in ("direct", "consult")}
        )

    def test_withdrawal_requires_matching_native_rejection_and_frozen_original(self):
        task = next(t for t in driver.read_reference()[1] if t["id"] == "C03")
        case = driver.reference_episode.input_only(task)
        root = driver.ROOT / "docs/benchmarks/results/mvp027"
        archive = json.loads(
            gzip.decompress((root / "offline-transitions-r1.json.gz").read_bytes())
        )
        native = next(row["native"] for row in archive["rows"] if row["id"] == "C03")
        observed = driver.reference_episode.withdrawn_revisions
        self.assertEqual(observed(case, native), {("C03-memory", 1): "C03-withdrawal"})
        for changed in (
            {"action": "approve"},
            {"original_sha256": "wrong"},
            {"previous_commit": 0},
        ):
            transition = dict(native["transitions"][0], **changed)
            self.assertEqual(
                observed(case, {**native, "transitions": [transition]}), {}
            )
        pair = json.loads(
            gzip.decompress(
                (root / "registered-withdrawal-preflights-r1.json.gz").read_bytes()
            )
        )
        for item in pair["runs"]:
            r = item["receipt"]
            self.assertFalse(r["model_turn_submitted"] or r["model_turn_accepted"])
            if r["task_id"] != "C03":
                self.assertEqual(r["status"], "REGISTERED_INPUT_UNAVAILABLE")
                continue
            self.assertEqual(r["native_import"]["active_revisions"], 0)
            self.assertEqual(r["native_import"]["withdrawn_revisions"], 1)
            if r["strategy"] == "direct":
                self.assertEqual(r["control_read"]["items"], [])
            else:
                self.assertEqual(r["preparation"]["ready_search"]["status"], "no_hit")
                self.assertEqual(r["invocation"]["historical_ordinals"], [1, 2])

    def test_unavailable_registered_episode_writes_receipt_before_account_access(self):
        with (
            patch.object(sys, "argv", [*self.args(), "--task", "V02"]),
            patch.object(driver, "sandbox") as sandbox,
            self.assertRaises(SystemExit) as result,
        ):
            driver.main()
        self.assertEqual(result.exception.code, 1)
        sandbox.assert_not_called()
        record = json.loads(self.output.read_bytes())
        self.assertEqual(record["status"], "REGISTERED_INPUT_UNAVAILABLE")
        self.assertEqual(record["task_id"], "V02")
        self.assertEqual(len(record["prior_import"]["refused_revisions"]), 2)
        self.assertFalse(record["model_turn_submitted"])

    def test_registered_arm_preflights_and_original_receipt_hashes(self):
        archive = (
            driver.ROOT / "docs/benchmarks/results/mvp027/registered-arms-r1.json.gz"
        )
        data = json.loads(gzip.decompress(archive.read_bytes()))
        tasks = {t["id"]: t for t in driver.read_reference()[1]}
        observed = {}
        for entry in data["runs"]:
            r = entry["receipt"]
            payload = (json.dumps(r, ensure_ascii=False, indent=2) + "\n").encode()
            self.assertEqual(
                hashlib.sha256(payload).hexdigest(), entry["private_result_sha256"]
            )
            t = tasks[r["task_id"]]
            key = (t["id"], r["strategy"])
            self.assertNotIn(key, observed)
            observed[key] = r
            case = driver.reference_episode.input_only(t)
            self.assertNotIn("oracle", case)
            self.assertEqual(
                r["registered_input_sha256"],
                driver.reference_episode.sha(driver.reference_episode.canonical(case)),
            )
            self.assertFalse(r["model_turn_submitted"] or r["model_turn_accepted"])
            self.assertNotIn("auth", r)
            if r["status"] == "REGISTERED_INPUT_UNAVAILABLE":
                self.assertEqual(entry["exit_code"], 1)
                continue
            self.assertEqual(entry["exit_code"], 0)
            self.assertEqual(r["binding"]["scope"], case["caller"]["scope"])
            self.assertEqual(
                r["binding"]["logical_fence"], case["caller"]["fence_sequence"]
            )
            if r["strategy"] == "consult":
                self.assertEqual(r["runner_policy"]["global_auth"], "absent")
                self.assertEqual(
                    r["invocation"]["historical_ordinals"],
                    sorted(s["sequence"] for s in case["sources"]),
                )
                search = r["preparation"]["ready_search"]
                self.assertEqual(
                    search["permission_revision"], r["binding"]["grant"]["commit_seq"]
                )
                self.assertEqual(search["model_calls"], [])
            elif case["memories"]:
                self.assertEqual(r["unmatched_query_read"]["items"], [])
        registered = {
            tid
            for (tid, arm), r in observed.items()
            if r["status"] != "REGISTERED_INPUT_UNAVAILABLE"
        }
        self.assertEqual(
            registered, {"H01", "A01", "A03", *(f"I{i:02d}-none" for i in range(1, 11))}
        )
        pending = {row["task"] for row in data["remaining_registered_inputs"]}
        self.assertEqual(pending, set(tasks) - registered)
        self.assertTrue(
            all(row["reason"] for row in data["remaining_registered_inputs"])
        )
        self.assertEqual(data["native_arm_preflights"], 26)
        self.assertEqual(data["model_turns"], 0)
        human = observed["I01-none", "direct"]["control_read"]["items"][0]
        self.assertEqual(
            (human["id"], human["authority"]), ("I01-none-second", "human_confirmed")
        )
        self.assertEqual(
            observed["I01-none", "consult"]["invocation"]["historical_ordinals"], [1, 2]
        )
        for tid in registered:
            self.assertIn((tid, "direct"), observed)
            self.assertIn((tid, "consult"), observed)
        a = observed["A01", "direct"]["control_read"]["items"][0]
        c = observed["A01", "consult"]["preparation"]["ready_search"]["items"][0][
            "memory"
        ]
        self.assertEqual(
            (a["id"], a["authority"]),
            (c["revision"]["key"]["id"], "delegated_approved"),
        )
        self.assertEqual(a["authority"], c["record"]["authority"])
        self.assertEqual(
            a["sources"][0]["sha256"], tasks["A01"]["sources"][0]["sha256"]
        )
        self.assertTrue(a["sources"][0]["receipt"])
        self.assertEqual(observed["H01", "direct"]["control_read"]["items"], [])
        self.assertEqual(
            observed["H01", "consult"]["preparation"]["ready_search"]["status"],
            "no_hit",
        )

    def test_shutdown_reads_native_frames_after_terminal(self):
        async def check():
            child = await asyncio.create_subprocess_exec(
                sys.executable,
                "-c",
                "import sys;sys.stdin.read();sys.stdout.buffer.write(b'x'*1048576)",
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                limit=1024,
                start_new_session=True,
            )
            await driver.stop(child, grace=2)
            self.assertEqual(child.returncode, 0)

        asyncio.run(check())

    def test_already_exited_group_at_final_shutdown_signal(self):
        async def check():
            stdout = asyncio.StreamReader()
            child = SimpleNamespace(
                pid=42, stdin=io.BytesIO(), stdout=stdout, wait=lambda: asyncio.sleep(0)
            )

            def disappeared(pid, sig):
                self.assertEqual(pid, 42)
                if sig == driver.signal.SIGKILL:
                    stdout.feed_eof()
                raise ProcessLookupError

            with patch.object(
                driver.os, "killpg", side_effect=disappeared
            ) as signal_group:
                await driver.stop(child, grace=0.01)
            self.assertEqual(signal_group.call_count, 2)

        asyncio.run(check())

    def test_invalid_destination_rejects_before_start(self):
        self.output = self.root / "missing" / "result.json"
        with patch.object(sys, "argv", self.args()), patch.object(driver, "run") as run:
            with self.assertRaises((OSError, SystemExit)):
                driver.main()
            run.assert_not_called()

    def test_changed_reference_inputs_reject_before_reserving_output(self):
        for path in driver.FROZEN_INPUTS:
            with (
                self.subTest(path=path),
                patch.object(sys, "argv", self.args()),
                patch.dict(driver.FROZEN_INPUTS, {path: "0" * 64}),
                contextlib.redirect_stderr(io.StringIO()),
                self.assertRaises(SystemExit),
            ):
                driver.main()
            self.assertFalse(self.output.exists())

    def test_same_destination_only_starts_once(self):
        processes = get_context("fork")
        entered = processes.Event()
        release = processes.Event()
        calls = self.root / "starts"

        async def simulate(args, private, result):
            with calls.open("a") as log:
                log.write("start\n")
            entered.set()
            self.assertTrue(release.wait(5))
            result["status"] = "NATIVE_CONSULT_TERMINAL_COMPLETED"
            result["terminal"] = {"status": "completed"}
            result["consult_terminal"] = {"publication": {"status": "incomplete"}}

        with (
            patch.object(sys, "argv", [*self.args(), "--strategy", "consult"]),
            patch.object(driver, "run", simulate),
        ):
            first = processes.Process(target=driver.main)
            first.start()
            try:
                self.assertTrue(entered.wait(5))
                marker = json.loads(self.output.read_bytes())
                self.assertEqual(marker["status"], "IN_PROGRESS")
                self.assertEqual(self.output.stat().st_mode & 0o777, 0o600)
                with self.assertRaises(SystemExit) as refused:
                    driver.main()
                self.assertEqual(refused.exception.code, 2)
            finally:
                release.set()
                first.join(5)
            self.assertEqual(first.exitcode, 0)
        receipt = json.loads(self.output.read_bytes())
        self.assertEqual(calls.read_text(), "start\n")
        self.assertEqual(receipt["utc"], marker["utc"])
        self.assertEqual(receipt["terminal"]["status"], "completed")
        self.assertEqual(
            receipt["consult_terminal"]["publication"]["status"], "incomplete"
        )
        self.assertGreaterEqual(receipt["timing"]["invocation_wall_s"], 0)
        self.assertEqual(set(self.root.iterdir()), {self.output, calls})

    def test_preparation_and_inner_elapsed_have_distinct_boundaries(self):
        observer = self.root / "observer"
        observer.write_bytes(b"synthetic")
        build = self.root / "build.json"
        driver.dump(
            build,
            {
                "base": driver.BASE,
                "compiled_library_commit": driver.BASE,
                "binary_sha256": driver.sha(observer),
                "source_sha256": driver.sha(
                    driver.ROOT / "scripts/evaluation/reference_read.rs"
                ),
            },
        )
        args = SimpleNamespace(
            strategy="direct", observer_build=build, observer=observer, codex=observer
        )
        result = {}
        with (
            patch.object(driver, "time") as clock,
            patch.object(
                driver, "sandbox", return_value=([], "fixture", "config", "catalog")
            ),
            patch.object(
                asyncio,
                "create_subprocess_exec",
                side_effect=RuntimeError("offline stop"),
            ),
        ):
            clock.monotonic.side_effect = [100, 103, 105]
            with self.assertRaisesRegex(RuntimeError, "offline stop"):
                asyncio.run(driver.run(args, self.root, result))
        self.assertEqual(result["timing"], {"preparation_s": 3, "total_s": 2})

    def test_completed_turn_is_kept_when_cleanup_fails(self):
        async def completed(args, private, result):
            result.update(
                status="NATIVE_DIRECT_TERMINAL_COMPLETED",
                terminal={"status": "completed"},
                cleanup_failure={
                    "class": "TimeoutError",
                    "message": "observer cleanup",
                },
            )

        with (
            patch.object(sys, "argv", self.args()),
            patch.object(driver, "run", completed),
        ):
            with self.assertRaises(SystemExit) as exit_status:
                driver.main()
        self.assertEqual(exit_status.exception.code, 1)
        receipt = json.loads(self.output.read_bytes())
        self.assertEqual(receipt["terminal"]["status"], "completed")
        self.assertEqual(receipt["cleanup_failure"]["class"], "TimeoutError")
        self.assertEqual(receipt["status"], "NATIVE_DIRECT_TERMINAL_COMPLETED")

    def test_failed_and_interrupted_main_turns_retain_raw_terminal_and_exit_unsuccessfully(
        self,
    ):
        for status in ("failed", "interrupted"):
            with self.subTest(status=status):
                self.output = self.root / (status + ".json")

                async def simulate(args, private, result, status=status):
                    result["status"] = "NATIVE_DIRECT_TERMINAL_" + status.upper()
                    result["terminal"] = {
                        "status": status,
                        "message": "synthetic failure",
                    }
                    result["model_turn_accepted"] = True

                with (
                    patch.object(sys, "argv", self.args()),
                    patch.object(driver, "run", side_effect=simulate),
                    self.assertRaises(SystemExit) as exc,
                ):
                    driver.main()
                self.assertNotEqual(exc.exception.code, 0)
                receipt = json.loads(self.output.read_bytes())
                self.assertEqual(
                    receipt["terminal"],
                    {"status": status, "message": "synthetic failure"},
                )
                self.assertTrue(receipt["model_turn_accepted"])


if __name__ == "__main__":
    with contextlib.redirect_stdout(io.StringIO()):
        unittest.main()
