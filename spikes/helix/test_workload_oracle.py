"""Independent current-read receipts must match the pinned public corpus."""
import copy
import hashlib
import json
import pathlib
import tempfile
import unittest

from run_case import digest, verify_requests


class CorpusCurrentOracle(unittest.TestCase):
    def test_pinned_content_and_identity_cannot_be_replaced_by_self_report(self):
        with tempfile.TemporaryDirectory(prefix="akasha-current-oracle-") as temporary:
            root = pathlib.Path(temporary)
            (root / "runtime").mkdir()
            run, base = "corpus-oracle-fixture", 1_000_000_000
            heads = []
            for i in range(50):
                prefix = f"Synthetic workload; excluded from corpus. Run {run}; client {i}; request 0. "
                heads.append(dict(memory=base+i, seq=i+1, scope=i % 8, active=True,
                                  body_sha256=hashlib.sha256((prefix + "x" * (4096-len(prefix))).encode()).hexdigest()))
            (root / "runtime/baseline.json").write_text(json.dumps(dict(base=base, run_id=run, heads=heads)))
            query = dict(memory=1_000_000, key="public:point", partition="p3", body_sha256=hashlib.sha256(b"public original").hexdigest())
            fixture = root / "queries.json"
            fixture.write_text(json.dumps(dict(queries=[query])))
            (root / "request.json").write_text(json.dumps(dict(fixture=str(fixture))))
            (root / "runtime/admission.json").write_text(json.dumps(dict(fixture_sha256=digest(fixture))))
            corpus = dict(at=50, query_index=0, key=query["key"], permission_revision=1,
                          entry=dict(memory=query["memory"], seq=0, scope=3, active=True, body_sha256=query["body_sha256"]))
            row = dict(client=60, ordinal=1, operation="current", status="success",
                       receipt=dict(at=50, memory=base, entry=heads[0], corpus=corpus))
            requests = root / "runtime/requests.jsonl"
            for case in ["missing", "body", "memory", "scope", "key", "future", "source"]:
                changed = copy.deepcopy(row)
                if case == "missing":del changed["receipt"]["corpus"]
                if case == "body":changed["receipt"]["corpus"]["entry"]["body_sha256"] = "0" * 64
                if case == "memory":changed["receipt"]["corpus"]["entry"]["memory"] += 1
                if case == "scope":changed["receipt"]["corpus"]["entry"]["scope"] = 4
                if case == "key":changed["receipt"]["corpus"]["key"] = "forged"
                if case == "future":changed["receipt"]["corpus"]["at"] = 51
                if case == "source":fixture.write_text(fixture.read_text() + "\n")
                requests.write_text(json.dumps(changed) + "\n")
                with self.subTest(case=case), self.assertRaises(ValueError):
                    verify_requests(root)
                fixture.write_text(json.dumps(dict(queries=[query])))
            requests.write_text(json.dumps(row) + "\n")
            result = verify_requests(root)
            self.assertEqual(result["corpus_current_checks"], 1)
            self.assertEqual(result["current_checks"], 1)
            (root / "request.json").write_text(json.dumps(dict(fixture=str(fixture), generation=1)))
            derived = dict(progress=dict(generation=1, committed=50, text=50, vector=50, remaining=0), indexed_jobs=100, text=heads, vectors=[])
            derived_path = root / "runtime/derived-oracle.json"
            for case in ["body", "jobs", "generation", "watermark", "commit", "vectors", "missing", "duplicate"]:
                changed = copy.deepcopy(derived)
                if case == "body":changed["text"][0]["body_sha256"] = "0" * 64
                if case == "jobs":changed["indexed_jobs"] -= 1
                if case == "generation":changed["progress"]["generation"] = 2
                if case == "watermark":changed["progress"]["text"] -= 1
                if case == "commit":changed["progress"]["committed"] += 1
                if case == "vectors":changed["vectors"] = [dict(memory=base)]
                if case == "missing":changed["text"].pop()
                if case == "duplicate":changed["text"].append(changed["text"][0])
                derived_path.write_text(json.dumps(changed))
                with self.subTest(derived=case), self.assertRaises(ValueError):
                    verify_requests(root)
            derived_path.write_text(json.dumps(derived))
            self.assertEqual(verify_requests(root)["derived_current_checks"], 50)


if __name__ == "__main__":
    unittest.main()
