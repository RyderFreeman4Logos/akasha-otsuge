import hashlib
import json
import math
import pathlib
import tempfile
import unittest
from unittest.mock import patch

import pyarrow
import pyarrow.parquet as parquet

from prepare_corpus import CHUNK_BYTES, chunks, prepare, vector_fingerprint


class CorpusBoundaries(unittest.TestCase):
    def test_real_parquet_deduplication_excludes_added_separator_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            vectors = [{"_id": "one", "title": "Title", "text": "A",
                        "text-embedding-3-large-1536-embedding": [1.0] + [0.0] * 1535}]
            sources = []
            for name, rows in [("vectors", vectors), ("text", [{"text": "Title\nA"}, {"text": "same"}, {"text": "same"}])]:
                path = root / (name + "-00000.parquet")
                parquet.write_table(pyarrow.Table.from_pylist(rows), path)
                content = path.read_bytes()
                sources.append({"repository": "test/" + name, "revision": "fixture", "license": "CC0",
                                "files": [{"path": path.name, "size": len(content),
                                           "sha256": hashlib.sha256(content).hexdigest()}]})
            with patch.object(pathlib.Path, "read_text", return_value=json.dumps({"sources": sources})):
                result = prepare(root, [1, 1])
            self.assertEqual(result["documents"], 2)
            self.assertEqual(result["unique_content_utf8_bytes"], 10)
            self.assertEqual(result["indexed_body_utf8_bytes"], 11)
            self.assertEqual(result["duplicate_content_utf8_bytes"], 11)
            self.assertEqual(result["unique_vectors"], 1)
            self.assertEqual(result["unvectorized_unique_utf8_bytes"], 4)

    def test_utf8_chunks_preserve_all_bytes_without_overlap(self):
        for text in ("", "a" * CHUNK_BYTES, "a" * (CHUNK_BYTES - 1) + "你好🦀" * 50_000):
            parts = list(chunks(text))
            self.assertEqual("".join(parts), text)
            self.assertTrue(all(0 < len(part.encode()) <= CHUNK_BYTES for part in parts))

    def test_real_vector_validation_rejects_padding_and_nonfinite_values(self):
        for vector in ([0.0] * 1536, [1.0] * 3, [math.nan] * 1536, [math.inf] * 1536,
                       [1e-100] * 1536, [1e100] * 1536):
            with self.assertRaises(ValueError):
                vector_fingerprint(vector)
        self.assertNotEqual(vector_fingerprint([1.0] + [0.0] * 1535),
                            vector_fingerprint([0.0, 1.0] + [0.0] * 1534))


if __name__ == "__main__":
    unittest.main()
