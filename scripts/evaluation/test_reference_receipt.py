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
