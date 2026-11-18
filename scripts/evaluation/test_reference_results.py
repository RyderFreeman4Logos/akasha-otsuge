"""Structural reporting must preserve a model terminal and omit unrestricted text."""

import json
import unittest

from reference_results import direct_reads, structural, terminal_fields
from reference_suite import read_reference


class PublicResults(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        _, tasks = read_reference()
        cls.task = next(t for t in tasks if t["id"] == "A01")

    def test_private_gap_and_answers_stay_out(self):
        pub = {
            "status": "incomplete",
            "gaps": ["private phrase", "INVALID_ARGUMENT"],
            "answer": "private phrase",
        }
        summary = structural(self.task, pub, True)
        self.assertEqual(summary["extra_known_codes"], ["INVALID_ARGUMENT"])
        self.assertEqual(summary["other_extra_gaps"], 1)
        self.assertFalse(summary["gap_set_matches"])
        self.assertNotIn("private phrase", json.dumps(summary))
        error = structural(
            self.task, {"status": "error", "error": {"code": "PROTOCOL_INVALID"}}, False
        )
        self.assertIsNone(error["gap_set_matches"])
        self.assertFalse(error["receipt_present"])

    def test_insufficient_and_unknown_reads_do_not_credit_required_item(self):
        read = {
            "response": {"items": [{"id": "unregistered-private-id", "revision": 1}]}
        }
        summary = direct_reads(self.task, [read])
        self.assertEqual(summary["required_revisions_unseen"], [("A01-memory", 1)])
        self.assertEqual(summary["other_distinct_revisions"], 1)
        self.assertNotIn("unregistered-private-id", json.dumps(summary))

    def test_completed_turn_can_have_failed_observer(self):
        row = {
            "terminal": {"status": "completed"},
            "status": "HARNESS_OR_HOST_FAILURE",
            "failure": {"class": "ProcessLookupError", "message": "private message"},
        }
        out = terminal_fields(row, "direct")
        self.assertEqual(out["model_terminal"], "completed")
        self.assertEqual(out["driver_status"], "HARNESS_OR_HOST_FAILURE")
        self.assertEqual(out["observer_exception"], "ProcessLookupError")
        self.assertNotIn("private message", json.dumps(out))
        row["status"] = "NATIVE_DIRECT_TERMINAL_FAILED"
        with self.assertRaisesRegex(ValueError, "observer status"):
            terminal_fields(row, "direct")


if __name__ == "__main__":
    unittest.main()
