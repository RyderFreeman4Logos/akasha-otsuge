"""Offline checks for H01 receipt ownership and native terminal reporting."""

import asyncio
import contextlib
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
