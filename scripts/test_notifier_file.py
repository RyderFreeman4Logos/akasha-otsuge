import concurrent.futures
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


PLUGIN = Path(__file__).resolve().parents[1] / "examples/notifier-file.py"


class FileNotifierTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="akasha-notifier-example-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.state = self.root / "inbox.json"
        self.notice = dict(notification_id="notice-1", idempotency_key="notice-1:1:route",
                           review_id="review-1", priority="normal", summary="review is open (1 reports)",
                           evidence_locator="akasha-otsuge review show review-1", action="create",
                           external_message_id=None)

    def invoke(self, request, *args):
        data = request if isinstance(request, bytes) else json.dumps(request).encode()
        return subprocess.run([sys.executable, str(PLUGIN), "--state", str(self.state), *args],
                              input=data, capture_output=True, timeout=10)

    def deliver(self, notice=None, *args):
        return self.invoke(dict(schema_version=1, op="deliver", notification=notice or self.notice), *args)

    def test_real_file_delivery_retry_dedup_and_update(self):
        caps = self.invoke(dict(schema_version=1, op="capabilities"))
        self.assertEqual(caps.returncode, 0, caps.stderr)
        self.assertTrue(json.loads(caps.stdout)["idempotency"])
        self.assertFalse(self.state.exists())
        self.assertEqual(self.deliver(None, "--fail-first").returncode, 75)
        self.assertEqual(json.loads(self.state.read_text())["messages"], {})
        sent = self.deliver(None, "--fail-first")
        self.assertEqual(sent.returncode, 0, sent.stderr)
        message_id = json.loads(sent.stdout)["external_message_id"]
        before = self.state.read_bytes()
        self.assertEqual(self.deliver().stdout, sent.stdout)
        self.assertEqual(self.state.read_bytes(), before)
        changed = self.notice | dict(idempotency_key="notice-1:2:route", priority="urgent",
                                     action="update", external_message_id=message_id)
        updated = self.deliver(changed)
        self.assertEqual(updated.returncode, 0, updated.stderr)
        self.assertEqual(json.loads(updated.stdout)["external_message_id"], message_id)
        state = json.loads(self.state.read_text())
        self.assertEqual(state["messages"], {message_id: changed})
        self.assertEqual(len(state["receipts"]), 2)
        self.assertEqual(os.stat(self.state).st_mode & 0o777, 0o600)
        after_update = self.state.read_bytes()
        self.assertEqual(self.deliver().stdout, sent.stdout)
        self.assertEqual(self.state.read_bytes(), after_update)

    def test_conflicts_invalid_input_and_symlinks_preserve_previous_state(self):
        first = self.deliver()
        self.assertEqual(first.returncode, 0, first.stderr)
        before = self.state.read_bytes()
        self.assertNotEqual(self.deliver(self.notice | {"summary": "different"}).returncode, 0)
        for invalid in [b"x" * 8193, b"[]", b'{"schema_version":2,"op":"capabilities"}',
                        dict(schema_version=1, op="deliver", notification=self.notice | {"unexpected": "DO_NOT_ECHO"})]:
            result = self.invoke(invalid)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn(b"DO_NOT_ECHO", result.stdout + result.stderr)
            self.assertEqual(self.state.read_bytes(), before)
        saved = self.root / "saved.json"
        self.state.rename(saved)
        self.state.symlink_to(saved)
        self.assertNotEqual(self.deliver().returncode, 0)
        self.assertEqual(saved.read_bytes(), before)

    def test_concurrent_same_key_has_one_durable_effect(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(lambda _: self.deliver(), range(4)))
        self.assertTrue(all(r.returncode == 0 for r in results), [r.stderr for r in results])
        self.assertEqual(len({r.stdout for r in results}), 1)
        state = json.loads(self.state.read_text())
        self.assertEqual(len(state["messages"]), 1)
        self.assertEqual(len(state["receipts"]), 1)

    def test_nested_corrupt_state_and_dangling_receipts_are_rejected_unchanged(self):
        self.assertEqual(self.deliver().returncode, 0)
        valid = json.loads(self.state.read_text())
        fingerprint = hashlib.sha256(json.dumps(self.notice, ensure_ascii=False, sort_keys=True,
                                                separators=(",", ":")).encode()).hexdigest()
        malformed = valid | {"messages": {"invalid-message": False}}
        dangling = valid | {"messages": {}, "receipts": {self.notice["idempotency_key"]:
                    dict(sha256=fingerprint, external_message_id="missing-message")}}
        wrong_hash = copy.deepcopy(valid)
        wrong_hash["receipts"][self.notice["idempotency_key"]]["sha256"] = "bad"
        missing_receipt = valid | {"receipts": {}}
        for bad in [malformed, dangling, wrong_hash, missing_receipt]:
            with self.subTest(state=bad):
                self.state.write_text(json.dumps(bad))
                before = self.state.read_bytes()
                result = self.deliver()
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertEqual(self.state.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
