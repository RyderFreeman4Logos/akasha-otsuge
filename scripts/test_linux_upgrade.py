import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("linux_upgrade", Path(__file__).with_name("linux-upgrade.py"))
upgrade = importlib.util.module_from_spec(spec)
spec.loader.exec_module(upgrade)


class RollbackTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.backup = self.root / "backup"
        self.store = self.root / "live"
        for path, label in ((self.backup / "store", "saved"), (self.store, "candidate")):
            (path / "authority/kv").mkdir(parents=True)
            (path / "authority/kv/LOCK").touch()
            (path / "data").write_text(label)
        (self.backup / "binary").write_text("old")
        (self.backup / "snapshot.json").write_text(json.dumps(dict(
            app=str(self.root / "binary"), store=str(self.store), config=str(self.root / "config"),
            config_present=False, socket=str(self.root / "absent-socket"),
        )))

    def test_active_writer_on_another_socket_preserves_both_stores(self):
        with upgrade.exclusive(self.store / "authority/kv/LOCK"), self.assertRaises(SystemExit):
            upgrade.restore(self.backup)
        self.assertEqual((self.store / "data").read_text(), "candidate")
        self.assertFalse((self.backup / "failed-store").exists())

    def test_retry_after_directory_exchange_keeps_the_first_candidate(self):
        with patch.object(upgrade, "install", side_effect=OSError("interrupted")), self.assertRaises(OSError):
            upgrade.restore(self.backup)
        self.assertEqual((self.store / "data").read_text(), "saved")
        upgrade.restore(self.backup)
        self.assertEqual((self.backup / "failed-store/data").read_text(), "candidate")
        self.assertEqual((self.root / "binary").read_text(), "old")

    def test_missing_candidate(self):
        self.store.rename(self.root / "removed")
        upgrade.restore(self.backup)
        self.assertEqual((self.store / "data").read_text(), "saved")
        self.assertEqual((self.root / "removed/data").read_text(), "candidate")

    def test_daemon_default_ignores_relative_xdg_and_uses_its_own_home(self):
        (self.root / "cmdline").write_bytes(b"akasha\0daemon\0run\0")
        (self.root / "environ").write_bytes(b"XDG_CONFIG_HOME=relative\0HOME=" + os.fsencode(self.root))
        self.assertEqual(upgrade.daemon_config(self.root), self.root / ".config/akasha-otsuge/config.toml")
