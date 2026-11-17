import copy
import hashlib
import http.client
import json
import pathlib
import tempfile
import threading
import unittest

import pyarrow
import pyarrow.parquet as parquet

from precomputed import Lookup, VECTOR, server


class PrecomputedInputs(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="akasha-precomputed-test-")
        self.root = pathlib.Path(self.scratch.name)
        self.vectors = [[1.0] + [0.0] * 1535, [0.0, 1.0] + [0.0] * 1534]
        self.rows = [{"title": "First", "text": "原文", VECTOR: self.vectors[0]},
                     {"title": "Second", "text": "exact text", VECTOR: self.vectors[1]},
                     {"title": "Too long", "text": "x" * 4096, VECTOR: self.vectors[0]}]
        self.manifest = {"model": "pinned-precomputed-fixture", "dimensions": 1536,
                         "sources": [self.write("base.parquet", self.rows, "dbpedia"),
                                     self.write("extra.parquet", [{"word": "supplement", "embedding": self.vectors[1]}], "dictionary")]}

    def tearDown(self):
        self.scratch.cleanup()

    def write(self, name, rows, layout):
        path = self.root / name
        parquet.write_table(pyarrow.Table.from_pylist(rows), path, row_group_size=1)
        return {"path": str(path), "layout": layout, "size": path.stat().st_size,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

    def request(self, texts, **extra):
        return dict(model=self.manifest["model"], input=texts, encoding_format="float", **extra)

    def test_exact_http_round_trip_and_unavailable_inputs(self):
        progress = []
        lookup = Lookup(self.manifest, lambda n, total, state: progress.append((n, total)))
        httpd = server(lookup)
        thread = threading.Thread(target=httpd.serve_forever)
        thread.start()
        try:
            self.assertEqual(progress, [(1, 2), (2, 2)])
            self.assertEqual(lookup.snapshot()["eligible_inputs"], 3)
            self.assertEqual(lookup.snapshot()["oversized_inputs"], 1)
            def post(value):
                connection = http.client.HTTPConnection(*httpd.server_address, timeout=5)
                try:
                    connection.request("POST", "/v1/embeddings", json.dumps(value))
                    response = connection.getresponse()
                    return response.status, json.loads(response.read())
                finally:
                    connection.close()
            code, result = post(self.request(["First\n原文", "Second\nexact text", "supplement"]))
            self.assertEqual(code, 200)
            self.assertEqual([r["index"] for r in result["data"]], [0, 1, 2])
            self.assertEqual([r["embedding"] for r in result["data"]], [*self.vectors, self.vectors[1]])
            self.assertEqual(post(self.request(["unknown"]))[0], 400)
            self.assertEqual(post(self.request(["First\n原文 " ]))[0], 400)
            bad = self.request(["First\n原文"]);bad["model"] = "other"
            self.assertEqual(post(bad)[0], 400)
            self.assertEqual(post(self.request(["x" * 4097]))[0], 400)
            self.assertEqual(post(self.request(["First\n原文"], dimensions=1536))[0], 400)
            self.assertEqual(lookup.snapshot()["returned_vectors"], 3)
            self.assertEqual(lookup.snapshot()["failed_requests"], 5)
            self.assertFalse(lookup.snapshot()["live_model_inference"])
        finally:
            httpd.shutdown();thread.join();httpd.server_close();lookup.close()

    def test_identity_and_cached_source_mutation_are_refused(self):
        bad = copy.deepcopy(self.manifest);bad["sources"][0]["sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            Lookup(bad)
        duplicate = copy.deepcopy(self.manifest)
        duplicate["sources"].append(duplicate["sources"][0])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            Lookup(duplicate)
        lookup = Lookup(self.manifest)
        try:
            self.assertEqual(lookup.vector("First\n原文"), self.vectors[0])
            with pathlib.Path(self.manifest["sources"][0]["path"]).open("ab") as stream:
                stream.write(b"changed")
            with self.assertRaisesRegex(ValueError, "changed after admission"):
                lookup.vector("First\n原文")
        finally:
            lookup.close()

    def test_invalid_vector_is_not_replaced_with_a_synthetic_result(self):
        invalid = {"word": "zero", "embedding": [0.0] * 1536}
        self.manifest["sources"] = [self.write("zero.parquet", [invalid], "dictionary")]
        lookup = Lookup(self.manifest)
        try:
            with self.assertRaisesRegex(ValueError, "invalid real embedding"):
                lookup.respond(self.request(["zero"]))
            self.assertEqual(lookup.snapshot()["returned_vectors"], 0)
            self.assertEqual(lookup.snapshot()["failed_requests"], 1)
        finally:
            lookup.close()


if __name__ == "__main__":
    unittest.main()
