import copy
import json
from pathlib import Path
import shutil
import tempfile
import unittest

from check_plan import validate_index, validate_snapshot

ROOT = Path(__file__).resolve().parents[1]


class PlanValidationTests(unittest.TestCase):
    def setUp(self):
        self.data = json.loads((ROOT / "docs/plan/issues-index.json").read_text())

    def test_current_snapshot_and_mapping(self):
        validate_snapshot(ROOT)
        validate_index(ROOT, self.data)

    def test_duplicate_id_is_rejected(self):
        self.data["issues"][-1] = copy.deepcopy(self.data["issues"][0])
        with self.assertRaisesRegex(ValueError, "unique plan IDs"):
            validate_index(ROOT, self.data)

    def test_dependency_cannot_be_erased_or_cyclic(self):
        for deps in ([], ["MVP-002"], ["MVP-029"]):
            self.data["issues"][1]["dependencies"] = deps
            with self.assertRaisesRegex(ValueError, "dependency"):
                validate_index(ROOT, self.data)

    def test_requirements_cannot_only_be_covered_by_final_acceptance(self):
        self.data["issues"][0]["requirements"] = []
        with self.assertRaisesRegex(ValueError, "requirement"):
            validate_index(ROOT, self.data)

    def test_remote_missing_dependency_is_rejected(self):
        by_id = {i["plan_id"]: i for i in self.data["issues"]}
        remote = [
            {"number": i["number"], "html_url": i["url"], "title": i["title"],
             "milestone": {"number": self.data["milestone"]},
             "body": "## Blocked by\n" + "\n".join(by_id[d]["url"] for d in i["dependencies"])}
            for i in self.data["issues"]
        ]
        validate_index(ROOT, self.data, remote)
        remote[1]["body"] = "## Blocked by\n"
        with self.assertRaisesRegex(ValueError, "remote dependency"):
            validate_index(ROOT, self.data, remote)

    def test_modified_snapshot_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            shutil.copytree(ROOT / "docs/spec", root / "docs/spec")
            (root / "docs/spec/00.md").write_text("altered requirement\n")
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                validate_snapshot(root)


if __name__ == "__main__":
    unittest.main()
