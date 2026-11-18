import asyncio
import tempfile
import unittest
from pathlib import Path
from unittest.mock import AsyncMock

from scripts.native_workload import __main__ as runner
from scripts.native_workload.journal import Journal, records
from scripts.native_workload.operations import NativeOperations, OperationError
from scripts.native_workload.schedule import run_schedule, summarize


class WorkloadBounds(unittest.IsolatedAsyncioTestCase):
    async def test_timeout_includes_pending_terminal_poll(self):
        client = NativeOperations("unused", job_timeout_s=0.1)
        calls = []

        async def request(command, _identity):
            calls.append(command["op"])
            if command["op"] == "submit":
                return {"state": "running", "id": "job"}
            await asyncio.Event().wait()

        client.request = request
        with self.assertRaises(OperationError) as raised:
            await asyncio.wait_for(client.job({"op": "submit", "idempotency_key": "job"}, "job"), 1)
        self.assertEqual(raised.exception.code, "JOB_COMPLETION_TIMEOUT")
        self.assertTrue(raised.exception.detail["effect_may_have_started"])
        self.assertEqual(calls.count("submit"), 1)

    async def test_admission_write_cannot_authorize_expired_offer(self):
        events = []

        async def emit(row):
            events.append(row)
            if row["event"] == "admitted":
                await asyncio.sleep(0.2)

        execute = AsyncMock()
        selected = [dict(client=1, kind="original_write", period_s=1, offset_s=0)]
        await run_schedule("qualify", 0.15, execute, emit, selected)
        execute.assert_not_awaited()
        report = summarize(events, 0.15)
        self.assertTrue(report["complete"])
        self.assertEqual(report["kinds"]["original_write"]["counts"]["admission_expired"], 1)

    async def test_partial_journal_refuses_replay(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "requests.jsonl"
            journal = Journal(path)
            await journal.append({"event": "admitted"})
            journal.close()
            path.write_bytes(path.read_bytes()[:-1])
            with self.assertRaisesRegex(ValueError, "partial"):
                list(records(path))

    def test_group_peak_includes_children(self):
        report = {
            "complete": True,
            "kinds": {
                k: {
                    "counts": {"success_within_interval": 100},
                    "success_offered_latency_s": {"95": 0.1},
                }
                for k in runner.MINIMUMS
            },
        }
        sample = {
            "number": 0,
            "observed_monotonic_s": 0.2,
            "daemon": {"VmHWM": 60 * 2**20, "VmRSS": 50 * 2**20},
            "events": {"oom_kill": 0},
            "resources": {
                "measurement": {"complete": True, "daemon_group": {"rss_bytes": 641 * 2**20}}
            },
        }
        gaps = runner.assess(report, [sample], 1, 512, 640, {"started_monotonic_s": 0})["gaps"]
        self.assertEqual(set(gaps), {"RSS maximum", "RSS p99"})
        sample["resources"]["measurement"]["complete"] = False
        with self.assertRaisesRegex(ValueError, "incomplete"):
            runner.assess(report, [sample], 1, 512, 640, {"started_monotonic_s": 0})
