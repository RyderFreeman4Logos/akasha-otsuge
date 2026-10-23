#!/usr/bin/env python3
"""Exercise directory-transfer crash windows without a second database writer."""

import json
import os
import pathlib
import tempfile
import unittest
from unittest.mock import patch

from import_corpus import OWNER, adopt_database
from run_case import digest, process_identity, publish


class Adoption(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="akasha-adoption-contract-")
        self.addCleanup(self.temporary.cleanup)
        root = pathlib.Path(self.temporary.name)
        self.source, self.run = root / "old", root / "new"
        for directory in (self.source, self.run):
            directory.mkdir()
            (directory / "OWNER").write_text(OWNER)
            (directory / "probe").write_bytes(directory.name.encode())
        (self.source / "database").mkdir()
        publish(self.source / "database/import-progress.json", {"completed": 32, "total": 1057})
        identity = dict(input="verified-input", manifest_sha256="manifest", preparer_sha256="source",
                        data_python="python", disk_plan={"reserve": 30 * 1024**3})
        self.old = dict(identity=identity, binary_sha256=digest(self.source / "probe"), attempts=[],
                        controller={"pid": 2147483647, "start_ticks": "0"}, phase="failed")
        publish(self.source / "state.json", self.old)
        self.state = dict(identity=identity, binary_sha256=digest(self.run / "probe"), attempts=[],
                          controller=process_identity(os.getpid()), phase="prepared")

    def verify_transfer(self):
        self.assertFalse((self.source / "database").exists())
        self.assertTrue((self.run / "database/import-progress.json").is_file())
        self.assertEqual(digest(self.source / "state.json"), digest(self.run / "origin-state.json"))
        self.assertTrue(json.loads((self.run / "state.json").read_text())["adoption"]["complete"])

    def test_transfer_preserves_origin_receipts(self):
        adopt_database(self.run, self.state, self.source)
        self.verify_transfer()

    def test_resume_before_and_after_atomic_rename(self):
        rename = os.rename
        for after in (False, True):
            with self.subTest(after_rename=after):
                def interrupted(old, new):
                    if after:
                        rename(old, new)
                    raise OSError("simulated controller interruption")
                with patch("import_corpus.os.rename", side_effect=interrupted):
                    with self.assertRaisesRegex(OSError, "interruption"):
                        adopt_database(self.run, self.state, self.source)
                saved = json.loads((self.run / "state.json").read_text())
                self.assertFalse(saved["adoption"]["complete"])
                adopt_database(self.run, saved)
                self.verify_transfer()
                if not after:
                    rename(self.run / "database", self.source / "database")
                    for path in self.run.iterdir():
                        if path.name not in {"OWNER", "probe"}:
                            path.unlink()
                    self.state.pop("adoption")

    def test_changed_origin_refused_after_interruption(self):
        with patch("import_corpus.os.rename", side_effect=OSError("interrupted")):
            with self.assertRaises(OSError):
                adopt_database(self.run, self.state, self.source)
        self.old["phase"] = "changed"
        publish(self.source / "state.json", self.old)
        with self.assertRaisesRegex(ValueError, "origin evidence changed"):
            adopt_database(self.run, self.state)
        self.assertTrue((self.source / "database").exists())

    def test_live_owner_and_binary_drift_refused_before_transfer(self):
        self.old["controller"] = process_identity(os.getpid())
        publish(self.source / "state.json", self.old)
        with self.assertRaisesRegex(ValueError, "controller is live"):
            adopt_database(self.run, self.state, self.source)
        self.old["controller"] = {"pid": 2147483647, "start_ticks": "0"}
        publish(self.source / "state.json", self.old)
        (self.source / "probe").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "binary changed"):
            adopt_database(self.run, self.state, self.source)
        self.assertFalse((self.run / "state.json").exists())

    def test_live_unit_and_duplicate_database_refused(self):
        self.old["attempts"] = [{"units": ["fixture-unit"]}]
        publish(self.source / "state.json", self.old)
        with patch("import_corpus.subprocess.run") as systemctl:
            systemctl.return_value.stdout = "active\n"
            with self.assertRaisesRegex(ValueError, "unit is still live"):
                adopt_database(self.run, self.state, self.source)
        self.old["attempts"] = []
        publish(self.source / "state.json", self.old)
        (self.run / "database").mkdir()
        with self.assertRaisesRegex(ValueError, "exactly one original"):
            adopt_database(self.run, self.state, self.source)


if __name__ == "__main__":
    unittest.main()
