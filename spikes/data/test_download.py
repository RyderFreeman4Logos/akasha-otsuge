import hashlib
import pathlib
import tempfile
import unittest
from unittest.mock import patch

from download import fetch, retryable, verify


class Checksums(unittest.TestCase):
    def test_only_transient_download_errors_are_retried(self):
        for code, status in [(92, 0), (28, 0), (22, 429), (22, 503)]:
            self.assertTrue(retryable(code, status))
        for code, status in [(22, 403), (22, 404), (60, 0), (0, 200)]:
            self.assertFalse(retryable(code, status))

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

    def test_complete_partial_is_verified_without_repeating_http_request(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            part = root / "vectors-00000.parquet.part"
            part.write_bytes(b"valid fixture")
            item = {"size": 13, "sha256": hashlib.sha256(b"valid fixture").hexdigest(), "path": "data.parquet"}
            with patch("download.subprocess.run") as network:
                result = fetch(root, {"repository": "Qdrant/fixture", "revision": "pinned"}, 0, item)
                network.assert_not_called()
            self.assertEqual(result["status"], "download_verified")
            self.assertEqual((root / "vectors-00000.parquet").read_bytes(), b"valid fixture")
            self.assertFalse(part.exists())


if __name__ == "__main__":
    unittest.main()
