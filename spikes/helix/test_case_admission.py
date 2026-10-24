"""Full-generation disk admission must fail before any database access."""
import pathlib
import subprocess
import sys
import tempfile
import unittest


class CaseAdmission(unittest.TestCase):
    def test_all_generations_retain_rebuild_space(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            for generation in (1, 2, 3):
                with self.subTest(generation=generation):
                    config = root / 'case.toml'
                    config.write_text(f'''[run]
id = "insufficient-rebuild-space"
profile = "low"
phase = "steady"
seconds = 1800
diagnostic = false
generation = {generation}
[paths]
binary = "{root}/absent-probe"
database = "{root}/absent-database"
fixture = "{root}/absent-fixture"
output = "{root}/absent-output"
corpus_manifest = "{root}/absent-manifest"
[disk]
start_bytes = {30 * 1024**3 + 4_000_000_000}
reserve_bytes = {30 * 1024**3 + 4_000_000_000}
[reliability]
max_attempts = 1
''')
                    child = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name('run_case.py')), str(config)],
                                           capture_output=True, text=True, timeout=10)
                    self.assertNotEqual(child.returncode, 0)
                    self.assertIn('current/next index disk reservation', child.stderr)
                    self.assertEqual(sorted(p.name for p in root.iterdir()), ['case.toml'])
            config.write_text(config.read_text().replace('diagnostic = false', 'diagnostic = false\ncold_cache = 1'))
            child = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name('run_case.py')), str(config)],
                                   capture_output=True, text=True, timeout=10)
            self.assertNotEqual(child.returncode, 0)
            self.assertIn('cold_cache must be a boolean', child.stderr)

    def test_rebuild_requires_a_different_positive_generation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            for rebuild in ('0', '1', 'true', str(2**64)):
                config = root / 'case.toml'
                config.write_text(f'''[run]
id="invalid-rebuild"
profile="low"
phase="steady"
seconds=60
diagnostic=true
generation=1
rebuild_generation={rebuild}
[paths]
binary="absent"
database="absent"
fixture="absent"
output="absent"
corpus_manifest="absent"
[reliability]
max_attempts=1
''')
                child = subprocess.run([sys.executable, str(pathlib.Path(__file__).with_name('run_case.py')), str(config)],
                                       capture_output=True, text=True, timeout=10)
                self.assertNotEqual(child.returncode, 0)
                self.assertIn('rebuild must target a different positive generation', child.stderr)
                self.assertEqual(sorted(p.name for p in root.iterdir()), ['case.toml'])


if __name__ == '__main__':
    unittest.main()
