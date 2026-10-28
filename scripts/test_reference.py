import copy
import json
import unittest
import tempfile
import shutil
from pathlib import Path

from check_reference import ROOT, validate_tasks, validate_snapshot


class ReferenceTests(unittest.TestCase):
    def setUp(self):
        self.tasks = [json.loads(line) for line in (ROOT / "tasks.jsonl").read_text().splitlines()]

    def test_current_sources_and_oracles(self):
        validate_tasks(self.tasks)
        self.assertEqual(validate_snapshot()["tasks"], 60)

    def test_frozen_bytes_cannot_be_silently_changed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "reference"
            shutil.copytree(ROOT, root)
            with (root / "tasks.jsonl").open("a") as output:
                output.write("\n")
            with self.assertRaisesRegex(ValueError, "frozen hash mismatch"):
                validate_snapshot(root)

    def test_source_change_and_invented_revision_are_rejected(self):
        for mutation in ("source", "revision", "conditions"):
            with self.subTest(mutation=mutation):
                tasks = copy.deepcopy(self.tasks)
                if mutation == "source":
                    tasks[0]["sources"][0]["text"] += " changed"
                elif mutation == "revision":
                    tasks[0]["oracle"]["required_items"][0]["revision"] = 999
                else:
                    tasks[0]["oracle"]["required_items"][0]["conditions"] = []
                with self.assertRaises(ValueError):
                    validate_tasks(tasks)

    def test_scope_and_fork_cannot_be_widened(self):
        for task_id in ("S01", "B01", "B03"):
            with self.subTest(task=task_id):
                tasks = copy.deepcopy(self.tasks)
                task = next(t for t in tasks if t["id"] == task_id)
                task["caller"]["allowed_event_ids"].append(task["sources"][-1]["id"])
                with self.assertRaises(ValueError):
                    validate_tasks(tasks)

    def test_critical_cases_and_pairs_cannot_be_removed_or_granted_by_hint(self):
        for mutation in ("count", "critical", "pair"):
            with self.subTest(mutation=mutation):
                tasks = copy.deepcopy(self.tasks)
                if mutation == "count":
                    tasks.pop()
                elif mutation == "critical":
                    tasks[0]["critical"] = False
                else:
                    tasks[-1]["caller"]["scope"]["project"] = "Borealis"
                with self.assertRaises(ValueError):
                    validate_tasks(tasks)


if __name__ == "__main__":
    unittest.main()
