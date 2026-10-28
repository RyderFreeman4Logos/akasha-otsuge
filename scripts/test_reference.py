import copy
import json
import unittest
import tempfile
import shutil
from pathlib import Path
from unittest.mock import patch

import check_reference

from check_reference import ROOT, digest, read_reference, validate_tasks, validate_snapshot


class ReferenceTests(unittest.TestCase):
    def setUp(self):
        self.tasks = read_reference()[1]

    def test_current_sources_and_oracles(self):
        validate_tasks(self.tasks)
        self.assertEqual(validate_snapshot()["tasks"], 60)

    def test_whole_grant_revocation_requires_a_permission_error(self):
        tasks = [json.loads(line) for line in (ROOT / "tasks.jsonl").read_text().splitlines()]
        with self.assertRaisesRegex(ValueError, "grant revocation"):
            validate_tasks(tasks)
        task = next(t for t in tasks if t["id"] == "S04")
        task["oracle"].update(status="error", error_code="READ_DENIED", successful_receipt=False)
        validate_tasks(tasks)
        for field, value in (("error_code", "RUNNER_FAILED"), ("successful_receipt", True)):
            with self.subTest(field=field):
                changed = copy.deepcopy(tasks)
                next(t for t in changed if t["id"] == "S04")["oracle"][field] = value
                with self.assertRaisesRegex(ValueError, "grant revocation"):
                    validate_tasks(changed)

    def test_erratum_cannot_be_coordinately_resealed(self):
        for name in ("corrections.json", "README.md"):
            with self.subTest(input=name), tempfile.TemporaryDirectory() as temp:
                root = Path(temp) / "erratum"
                shutil.copytree(ROOT.parent / "reference-v1-errata-1", root)
                with (root / name).open("a") as output:
                    output.write("\n")
                manifest = json.loads((root / "manifest.json").read_text())
                manifest["files"][name] = digest((root / name).read_bytes())
                (root / "manifest.json").write_text(json.dumps(manifest))
                with patch.object(check_reference, "ERRATA_ROOT", root, create=True):
                    with self.assertRaisesRegex(ValueError, "erratum.*freeze"):
                        validate_snapshot()

    def test_frozen_bytes_cannot_be_silently_changed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "reference"
            shutil.copytree(ROOT, root)
            with (root / "tasks.jsonl").open("a") as output:
                output.write("\n")
            with self.assertRaisesRegex(ValueError, "frozen hash mismatch"):
                validate_snapshot(root)

    def test_resealed_inputs_or_version_bumped_wipe_cannot_rewrite_original(self):
        for name in ("tasks.jsonl", "profile.json", "README.md"):
            with self.subTest(input=name), tempfile.TemporaryDirectory() as temp:
                root = Path(temp) / "reference"
                shutil.copytree(ROOT, root)
                manifest = json.loads((root / "manifest.json").read_text())
                (root / name).write_text("{}\n")
                manifest["files"][name] = digest((root / name).read_bytes())
                manifest["schema_version"] = 2
                (root / "ERRATA.md").write_text("Replace original with empty template.\n")
                (root / "manifest.json").write_text(json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, "committed freeze"):
                    validate_snapshot(root)

    def test_coordinated_reseal_does_not_change_fixed_reference(self):
        for name in ("tasks.jsonl", "profile.json", "README.md"):
            with self.subTest(input=name), tempfile.TemporaryDirectory() as temp:
                root = Path(temp) / "reference"
                shutil.copytree(ROOT, root)
                if name == "tasks.jsonl":
                    tasks = copy.deepcopy(self.tasks)
                    source = tasks[0]["sources"][0]
                    source["text"] += " Additional context."
                    source["sha256"] = digest(source["text"].encode())
                    (root / name).write_text("".join(json.dumps(t) + "\n" for t in tasks))
                elif name == "profile.json":
                    profile = json.loads((root / name).read_text())
                    profile["model"] = "different-reference-model"
                    (root / name).write_text(json.dumps(profile))
                else:
                    with (root / name).open("a") as output:
                        output.write("Changed scoring policy.\n")
                manifest = json.loads((root / "manifest.json").read_text())
                manifest["files"][name] = digest((root / name).read_bytes())
                (root / "manifest.json").write_text(json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, "committed freeze"):
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
