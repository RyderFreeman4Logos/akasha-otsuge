import hashlib
import pathlib
import tempfile
import unittest

from download import fetch, verify


class Checksums(unittest.TestCase):
    def test_existing_file_is_revalidated_before_reuse(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            path = root / "vectors-00000.parquet"
            path.write_bytes(b"valid fixture")
            item = {"size": 13, "sha256": hashlib.sha256(b"valid fixture").hexdigest()}
            self.assertEqual(fetch(root, {"repository": "Qdrant/fixture"}, 0, item)["status"],
                             "verified_existing")
            path.write_bytes(b"wrong fixture")
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                verify(path, item)
            path.write_bytes(b"short")
            with self.assertRaisesRegex(ValueError, "size mismatch"):
                verify(path, item)


if __name__ == "__main__":
    unittest.main()
