import hashlib
import json
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, Mock

from scripts.native_workload.index_sync import completed_writes, observe
from scripts.native_workload.ipc import TransportError
from scripts.native_workload.operations import OperationError


class ConcurrentIndex(unittest.IsolatedAsyncioTestCase):
    async def run_sync(
        self, *, physical=True, terminal_attempt=1, final_epoch=8, lost_submit=False
    ):
        key = "mixed/concurrent-index-sync"
        job = {"id": key, "retrieval_plan": {"kind": "sync"}, "retrieval_target_seq": 8}
        running = dict(job, state="running", attempt=1, started_at_ms=100)
        complete = dict(running, state="succeeded", attempt=terminal_attempt)
        snapshot = {"build_epoch": 7 if physical else 6, "building": physical}
        submit = [TransportError("response", EOFError())] if lost_submit else []
        ops = Mock(job_timeout_s=2)
        ops.request = AsyncMock(
            side_effect=[
                OperationError("JOB_NOT_FOUND"),
                {
                    "generation": "lex",
                    "lexical_watermark": 3,
                    "build_epoch": 6,
                    "queue": {"storage_seq": 8},
                },
                *submit,
                dict(job, state="queued"),
                running,
                snapshot,
                running,
                snapshot,
                complete,
                {
                    "generation": "lex",
                    "lexical_watermark": 8,
                    "build_epoch": final_epoch,
                    "building": False,
                },
            ]
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            observed = await observe(
                ops, "mixed", root, time.monotonic() - 1.51, 3, Mock()
            )
            terminal = (root / "concurrent-index-terminal.json").read_bytes()
            self.assertEqual(json.loads(terminal), complete)
            self.assertEqual(
                observed["terminal_sha256"], hashlib.sha256(terminal).hexdigest()
            )
        ids = [call.args[1] for call in ops.request.await_args_list]
        self.assertEqual(len(set(ids)), len(ids))
        self.assertEqual(ids.count(key + "/submit"), 1)
        return observed

    async def test_write_falls_within_one_native_build_after_lost_submit_reply(self):
        observed = await self.run_sync(lost_submit=True)
        self.assertNotIn("error", observed)
        self.assertIn("submit_transport_error", observed)
        self.assertEqual(observed["attempt"], [1, 100])
        begin = observed["first_build_sample"]["received_s"]
        end = observed["last_build_sample"]["sent_s"]
        self.assertLess(begin, end)
        valid = {
            "event": "outcome",
            "kind": "correction",
            "outcome": {"status": "success"},
            "within_interval": True,
            "dispatch_s": begin,
            "finished_s": end,
            "identity": "update",
        }
        outcomes = [
            valid,
            dict(valid, dispatch_s=begin - 0.01),
            dict(valid, finished_s=end + 0.01),
            dict(valid, outcome={"status": "conflict"}),
            dict(valid, kind="current"),
            dict(valid, within_interval=False),
        ]
        writes = completed_writes(observed, outcomes)
        self.assertEqual(writes["counts"], {"original_write": 0, "correction": 1})
        self.assertEqual(writes["first_request_by_kind"], {"correction": "update"})
        observed["last_build_sample"] = None
        self.assertFalse(any(completed_writes(observed, outcomes)["counts"].values()))

    async def test_running_without_a_physical_build_gives_no_witness(self):
        observed = await self.run_sync(physical=False)
        self.assertNotIn("error", observed)
        self.assertIsNone(observed["first_build_sample"])
        self.assertFalse(any(completed_writes(observed, [])["counts"].values()))

    async def test_another_attempt_or_build_cannot_supply_the_witness(self):
        for kwargs, error in [
            ({"terminal_attempt": 2}, "multiple index attempts"),
            ({"final_epoch": 10}, "one completed native build"),
        ]:
            with self.subTest(**kwargs):
                observed = await self.run_sync(**kwargs)
                self.assertIn(error, observed["error"]["message"])
                self.assertFalse(any(completed_writes(observed, [])["counts"].values()))

    async def test_existing_job_is_preserved(self):
        ops = Mock(request=AsyncMock(return_value={"id": "used"}))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = await observe(ops, "mixed", root, time.monotonic() - 1, 1, Mock())
            self.assertIn("already exists", result["error"]["message"])
            self.assertFalse((root / "concurrent-index-intent.json").exists())
        self.assertEqual(ops.request.await_count, 1)
