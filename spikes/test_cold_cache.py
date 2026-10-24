"""Cold preparation must evict owned file pages without changing their bytes."""
import hashlib
import json
import pathlib
import tempfile
import unittest

from cold_cache import prepare


class ColdCache(unittest.TestCase):
    def test_real_residency_and_owned_path_boundary(self):
        with tempfile.TemporaryDirectory() as temporary:
            parent = pathlib.Path(temporary)
            (parent / 'OWNER').write_text('akasha-otsuge MVP-003 cold fixture\n')
            root = parent / 'database'
            root.mkdir()
            lock = self.enterContext((root / '.akasha-case-owner.lock').open('a'))
            payload = bytes(range(256)) * 16384 + b'partial last page'
            file = root / 'data.bin'
            file.write_bytes(payload)
            self.assertEqual(file.read_bytes(), payload)
            output = parent / 'receipts'
            output.mkdir()
            result = prepare(root, output, lock)
            self.assertGreater(result['resident_pages_before'], 0)
            self.assertEqual(result['resident_pages_after'], 0)
            self.assertEqual(result['files'], 1)
            self.assertEqual(hashlib.sha256(file.read_bytes()).digest(), hashlib.sha256(payload).digest())
            self.assertEqual(json.loads((output / 'cold-cache.json').read_text())['status'], 'passed')
            with (parent / 'wrong.lock').open('a') as wrong:
                with self.assertRaisesRegex(ValueError, 'does not belong'):
                    prepare(root, parent / 'wrong-receipts', wrong)
            with (root / '.akasha-case-owner.lock').open('a') as competing:
                with self.assertRaises(BlockingIOError):
                    prepare(root, parent / 'busy-receipts', competing)
            (root / 'foreign').symlink_to(parent / 'OWNER')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                prepare(root, parent / 'bad-receipts', lock)
            (root / 'foreign').unlink()
            (parent / 'OWNER').unlink()
            with self.assertRaisesRegex(ValueError, 'owned'):
                prepare(root, parent / 'unowned-receipts', lock)


if __name__ == '__main__':
    unittest.main()
