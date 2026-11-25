import asyncio
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from scripts.native_workload import __main__ as runner
from scripts.native_workload.journal import records
from scripts.native_workload.operations import OperationError


class WorkloadMonitor(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        work = self.root / ".spike-work"
        work.mkdir()
        self.output = work / "native-workload-test"
        self.data = self.root / "daemon data"
        (self.data / "authority" / "kv").mkdir(parents=True)
        (self.data / "search").mkdir()
        self.config = self.root / "settings.toml"
        self.config.write_text(
            "[execution]\nseconds=2\n[resources]\ntarget_mib=512\npeak_mib=640\n"
            "service_total_mib=2048\n[reliability]\nmax_attempts=1\nrequest_timeout_s=10\njob_timeout_s=600\n"
        )
        # Sample a small process even when the surrounding Python test suite has a large HWM.
        observer = self.enterContext(subprocess.Popen(["sleep", "30"]))
        self.addCleanup(observer.terminate)
        self.pid = observer.pid
        identity = runner.process_identity(self.pid)
        self.plan = self.root / "plan.json"
        self.plan.write_text(
            json.dumps(
                {
                    "run_id": "test",
                    "daemon": identity,
                    "lookup": identity,
                    "socket": "unused",
                    "daemon_sha256": runner.sha(Path(f"/proc/{self.pid}/exe")),
                    "daemon_config": str(self.config),
                    "scale_receipt": str(self.config),
                    "daemon_config_sha256": runner.sha(self.config),
                    "scale_receipt_sha256": runner.sha(self.config),
                    "oracles": {"corrections": []},
                }
            )
        )
        self.events = self.root / "memory.events"
        self.events.write_text("oom_kill 0\nhigh 3\n")
        self.execute = AsyncMock(return_value={"status": "error", "code": "RESOURCE_BUSY"})

    def test_slow_sample_skips_only_closed_windows(self):
        for previous, elapsed, expected in [
            (0, 0.95, 1),
            (0, 1.25, 1),
            (0, 3.25, 3),
            (3, 3.25, 4),
        ]:
            with self.subTest(previous=previous, elapsed=elapsed):
                self.assertEqual(runner.next_sample_tick(previous, elapsed), expected)

    async def run_fixture(self, code, *, tick=0, oom=0, available=None):
        seen = set()

        async def request(_, command, request_id):
            self.assertNotIn(request_id, seen)
            seen.add(request_id)
            if command["op"] == "config_show":
                return {"config": {"paths": {"data_dir": str(self.data)}}}
            if command["op"] == "status":
                if request_id == f"test/metrics/{tick}":
                    self.events.write_text(f"oom_kill {oom}\nhigh 7\n")
                    raise OperationError(code)
                return {
                    "pid": self.pid,
                    "storage_seq": 10,
                    "resources": {
                        "measurement": {
                            "complete": True,
                            "daemon_group": {"rss_bytes": 80 * 2**20},
                        },
                        "cgroup": {
                            "enforced": True,
                            "complete": True,
                            "daemon_group": {
                                "path": str(self.root),
                                "high_bytes": 512 * 2**20,
                                "max_bytes": 640 * 2**20,
                            },
                            "service_total": {"max_bytes": 2048 * 2**20},
                        },
                    },
                }
            if command["op"] == "job_show":
                self.assertEqual(command["id"], "test/concurrent-index-sync")
                return {"id": command["id"]}
            self.assertEqual(command, {"op": "index_status", "include_queue": True})
            queued = 2 if request_id == "test/metrics/0/indexes" else 0
            return {
                "generation": "g",
                "lexical_watermark": 10,
                "vector": {"generation": "v", "watermark": 9},
                "active_vector": None,
                "queue": {
                    "storage_seq": 10,
                    "queued": queued,
                    "running": 1,
                    "queued_json_bytes": queued * 97,
                },
            }

        with (
            patch.object(runner, "REPO", self.root),
            patch.object(runner.NativeOperations, "request", request),
            patch.object(runner, "NativeWorkload", return_value=self.execute),
            patch.object(runner.shutil, "disk_usage") as disk,
            patch.object(runner.resource, "getrusage") as usage,
        ):
            usage.return_value.ru_maxrss = 1024
            disk.return_value.free = 32 * 1024**3
            if available:
                disk.side_effect = lambda path: SimpleNamespace(free=available(Path(path)))
            return await runner.run(self.plan, self.config, self.output)

    async def test_busy_status_preserves_offers_independent_samples_and_workload_errors(
        self,
    ):
        append = runner.Journal.append
        cutoff = None
        delayed = None

        async def delay_one_admission(journal, row):
            nonlocal cutoff, delayed
            await append(journal, row)
            if row.get("event") == "interval":
                cutoff = row["started_monotonic_s"] + row["duration_s"]
            elif row.get("event") == "admitted" and delayed is None:
                delayed = row["identity"]
                loop = asyncio.get_running_loop()
                while loop.time() < cutoff:
                    await asyncio.sleep(cutoff - loop.time())

        with patch.object(runner.Journal, "append", delay_one_admission):
            result = await self.run_fixture("RESOURCE_BUSY")
        journal = list(records(self.output / "requests.jsonl"))
        self.assertEqual(journal[-1]["event"], "drained")
        offered = [r["identity"] for r in journal if r["event"] == "offered"]
        self.assertEqual(len(set(offered)), journal[0]["planned"])
        admitted = [r["identity"] for r in journal if r["event"] == "admitted"]
        expired = [
            r["identity"]
            for r in journal
            if r["event"] == "not_dispatched" and r["reason"] == "admission_expired"
        ]
        outcomes = [r for r in journal if r["event"] == "outcome"]
        completed = [r["identity"] for r in outcomes]
        self.assertIn(delayed, expired)
        self.assertTrue(outcomes)
        self.assertCountEqual(admitted, completed + expired)
        self.assertCountEqual([c.args[2] for c in self.execute.await_args_list], completed)
        for row in outcomes:
            self.assertEqual(row["outcome"], {"status": "error", "code": "RESOURCE_BUSY"})
        ticks = {r["number"]: r for r in records(self.output / "metrics.jsonl")}
        busy = ticks[0]
        self.assertEqual(busy["status_error"]["code"], "RESOURCE_BUSY")
        self.assertLessEqual(
            busy["status_error"]["observed_monotonic_s"], busy["observed_monotonic_s"]
        )
        self.assertIsNone(busy["resources"])
        self.assertIsNone(busy["status_storage_seq"])
        self.assertEqual(busy["events"]["high"], 7)
        self.assertGreater(busy["daemon"]["VmHWM"], 0)
        self.assertGreater(busy["lookup"]["Pss"], 0)
        self.assertEqual(busy["backlog"]["queued"], 2)
        self.assertEqual(
            ticks[1]["resources"]["measurement"]["daemon_group"]["rss_bytes"],
            80 * 2**20,
        )
        assessment = result["assessment"]
        self.assertFalse(assessment["interval_thresholds_pass"])
        self.assertIn("resource sample coverage", assessment["gaps"])
        self.assertIn("queue and index sample coverage", assessment["gaps"])
        self.assertIn("original_write errors", assessment["gaps"])
        self.assertEqual(assessment["kernel_samples"], 2)
        self.assertEqual(assessment["resource_samples"], 1)
        self.assertEqual(assessment["backlog_samples"], 2)
        self.assertEqual(assessment["queue_high_water"]["queued"], 2)

    async def test_opt_in_sync_failure_is_recorded_without_losing_offers(self):
        plan = json.loads(self.plan.read_text())
        plan["concurrent_index"] = True
        self.plan.write_text(json.dumps(plan))
        result = await self.run_fixture("RESOURCE_BUSY")
        assessment = result["assessment"]
        self.assertIn("index build overlapping a confirmed write", assessment["gaps"])
        self.assertFalse(assessment["interval_thresholds_pass"])
        observation = assessment["concurrent_index"]["observation"]
        self.assertIn("already exists", observation["error"]["message"])
        admission = json.loads((self.output / "admission.json").read_text())
        self.assertIn("index_sync.py", admission["source_sha256"])
        offers = {
            r["identity"] for r in records(self.output / "requests.jsonl") if r["event"] == "offered"
        }
        self.assertEqual(len(offers), admission["planned"])

    async def test_busy_preflight_still_refuses_an_unadmitted_interval(self):
        with self.assertRaisesRegex(OperationError, "RESOURCE_BUSY"):
            await self.run_fixture("RESOURCE_BUSY", tick=-1)
        self.execute.assert_not_awaited()

    async def test_refuses_low_data_volume_before_opening_output(self):
        def available(path):
            return (1 if path.name == "search" else 32) * 1024**3

        with self.assertRaisesRegex(ValueError, "free reserve"):
            await self.run_fixture("RESOURCE_BUSY", available=available)
        self.assertFalse(self.output.exists())
        self.execute.assert_not_awaited()

    async def test_busy_status_still_stops_on_low_authority_volume(self):
        checks = 0

        def available(path):
            nonlocal checks
            if path.name == "kv":
                checks += 1
                if checks == 3:
                    return 1024**3
            return 32 * 1024**3

        with self.assertRaises(ExceptionGroup) as raised:
            await self.run_fixture("RESOURCE_BUSY", available=available)
        self.assertIn("free reserve", str(raised.exception.exceptions[0]))
        row = list(records(self.output / "metrics.jsonl"))[-1]
        self.assertEqual(row["status_error"]["code"], "RESOURCE_BUSY")
        self.assertEqual(row["filesystem_free_bytes"], 32 * 1024**3)
        self.assertEqual(row["storage_free_bytes"][str(self.data / "authority/kv")], 1024**3)

    def test_absolute_symlink_uses_daemon_namespace(self):
        alias = self.root / "storage"
        alias.symlink_to("/inside")
        (self.root / "inside").mkdir()
        (self.root / "inside" / "nested").symlink_to("../other")
        self.assertEqual(
            runner.daemon_path(self.root, Path("/storage/nested/kv")), Path("/other/kv")
        )

    def test_reserve_only_on_writable_storage_mounts(self):
        authority = self.data / "authority/kv"
        search = self.data / "search"
        table = search / "memory_lexical_v1.lance"
        nested = table / "_indices" / "index with spaces"
        mounts = b"".join(
            b"1 1 0:1 / " + str(path).encode().replace(b" ", rb"\040")
            + b" " + mode + b" - tmpfs none rw\n"
            for path, mode in [
                (nested, b"rw"), (table / "data", b"ro"), (search / "scratch", b"rw"),
                (table / "scratch", b"rw"), (authority / "backup", b"rw"),
                (authority / "wal", b"rw"), (self.data / "search-backup", b"rw"),
            ]
        )
        with (
            patch.object(Path, "read_bytes", return_value=mounts),
            patch.object(runner.shutil, "disk_usage", return_value=SimpleNamespace(free=7)),
        ):
            values = runner.storage_free_bytes(self.pid, self.data)
        self.assertEqual(set(values), {str(p) for p in (authority, authority / "wal", search, nested)})

    def test_missing_search_uses_data_volume_until_it_exists(self):
        (self.data / "search").rmdir()
        with patch.object(runner.shutil, "disk_usage", return_value=SimpleNamespace(free=7)):
            values = runner.storage_free_bytes(self.pid, self.data)
        self.assertEqual(set(values), {str(self.data), str(self.data / "authority/kv")})

    async def test_other_status_failure_remains_fatal(self):
        with self.assertRaises(ExceptionGroup) as raised:
            await self.run_fixture("READ_DENIED")
        self.assertEqual([e.code for e in raised.exception.exceptions], ["READ_DENIED"])

    async def test_busy_status_still_checks_kernel_oom(self):
        with self.assertRaises(ExceptionGroup) as raised:
            await self.run_fixture("RESOURCE_BUSY", oom=1)
        self.assertEqual([str(e) for e in raised.exception.exceptions], ["native peak or OOM"])
        self.assertEqual(list(records(self.output / "metrics.jsonl"))[-1]["events"]["oom_kill"], 1)
