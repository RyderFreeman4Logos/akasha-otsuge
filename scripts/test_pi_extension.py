"""Run the dependency-free Pi bridge regressions with Node's built-in test runner."""
import pathlib
import subprocess
import unittest


class PiExtension(unittest.TestCase):
    def test_native_bridge_contracts(self):
        script = pathlib.Path(__file__).with_suffix('.cjs')
        result = subprocess.run(['node', '--test', str(script)], capture_output=True,
                                text=True, timeout=30, check=False)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
